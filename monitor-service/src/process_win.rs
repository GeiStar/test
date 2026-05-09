use anyhow::{Context, Result};
use std::mem;
use std::ptr;

#[cfg(windows)]
use windows::{
    core::PWSTR,
    Win32::Foundation::{CloseHandle, BOOL, HANDLE, INVALID_HANDLE_VALUE},
    Win32::Security::{
        DuplicateTokenEx, SecurityIdentification, TokenPrimary, SECURITY_ATTRIBUTES,
        TOKEN_ALL_ACCESS, TOKEN_DUPLICATE, TOKEN_QUERY,
    },
    Win32::Storage::FileSystem::ReadFile,
    Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
    Win32::System::Pipes::CreatePipe,
    Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId},
    Win32::System::Threading::{
        CreateProcessAsUserW, CreateProcessW, OpenProcess, OpenProcessToken,
        SetProcessShutdownParameters, WaitForSingleObject, CREATE_NO_WINDOW, DETACHED_PROCESS,
        INFINITE, NORMAL_PRIORITY_CLASS, PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION,
        STARTF_USESTDHANDLES, STARTUPINFOW,
    },
};

#[cfg(not(windows))]
pub fn run_agent_in_active_session(_args: &[&str]) -> Result<String> {
    Ok("[]".to_string())
}

#[cfg(windows)]
fn get_session_user_token(session_id: u32) -> Result<(HANDLE, bool)> {
    unsafe {
        // Try to find a process in the session to steal token from
        // Priority: explorer.exe (User), winlogon.exe (System/Login)
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(anyhow::anyhow!("Failed to create process snapshot"));
        }

        let mut entry = PROCESSENTRY32W {
            dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut found_explorer = 0u32;
        let mut found_winlogon = 0u32;

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                let mut proc_session_id = 0;

                if ProcessIdToSessionId(pid, &mut proc_session_id).is_ok()
                    && proc_session_id == session_id
                {
                    let name = String::from_utf16_lossy(&entry.szExeFile)
                        .trim_matches(char::from(0))
                        .to_lowercase();

                    if name == "explorer.exe" {
                        found_explorer = pid;
                        // Don't break — continue to also find winlogon.exe
                    } else if name == "winlogon.exe" {
                        found_winlogon = pid;
                    }
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);

        // Query session lock state via WTS API
        let session_locked = {
            use windows::Win32::System::RemoteDesktop::{
                WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
                WTS_SESSIONSTATE_LOCK,
            };
            let mut ppbuffer = PWSTR(std::ptr::null_mut());
            let mut bytes_returned: u32 = 0;
            let result = WTSQuerySessionInformationW(
                HANDLE::default(),
                session_id,
                WTSSessionInfoEx,
                &mut ppbuffer,
                &mut bytes_returned,
            );
            if result.is_ok() && !ppbuffer.is_null() {
                let info = &*(ppbuffer.0 as *const WTSINFOEXW);
                let locked = info.Level == 1
                    && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_LOCK as i32;
                WTSFreeMemory(ppbuffer.0 as *mut _);
                locked
            } else {
                false
            }
        };

        // Token selection strategy (session-based active-desktop detection):
        // Win32k SetDisplayConfig requires the calling thread to be on the ACTIVE desktop (layer-2 check).
        //
        // When session_locked=true with both explorer and winlogon present, we cannot determine
        // which desktop is active purely from WTS state — Win11 Stage-1 (LockApp/spotlight) has
        // session_locked=true but active desktop = WinSta0\Default, while Stage-2 (password input)
        // also has session_locked=true but active desktop = WinSta0\Winlogon.
        //
        // Resolution: spawn a minimal "agent query-desktop" in the user's session using the explorer
        // token. From within the session, OpenInputDesktop() correctly returns the active desktop for
        // that session's WinSta0. No process-name inspection (no logonui.exe check) needed.
        //
        //   - Active desktop = Winlogon (Win11 Stage-2, Win10 Win+L, FUS credential):
        //     → SYSTEM token (winlogon.exe) + Winlogon desktop
        //
        //   - Active desktop = Default (Win11 Stage-1 LockApp, normal desktop, FUS-direct):
        //     → user token (explorer.exe) + Default desktop
        //
        //   - No explorer in session (pre-login / FUS new user without existing session):
        //     → SYSTEM token (winlogon.exe) + Winlogon desktop (only option)

        // Determine active desktop when the ambiguous case arises (session locked, both processes present).
        let active_is_winlogon = if session_locked && found_winlogon != 0 && found_explorer != 0 {
            let desktop = query_active_desktop_in_session(found_explorer);
            desktop.eq_ignore_ascii_case("Winlogon")
        } else {
            false
        };

        let (target_pid, is_winlogon) = if active_is_winlogon && found_winlogon != 0 {
            (found_winlogon, true)  // Active desktop: Winlogon (Win11 Stage-2, Win10 lock, FUS credential)
        } else if found_explorer != 0 {
            (found_explorer, false) // Active desktop: Default (normal, Win11 Stage-1 lockscreen, FUS-direct)
        } else if found_winlogon != 0 {
            (found_winlogon, true)  // Pre-login fallback: no explorer in session
        } else {
            return Err(anyhow::anyhow!(
                "No suitable process found in session {}",
                session_id
            ));
        };

        let h_process = OpenProcess(PROCESS_QUERY_INFORMATION, BOOL(0), target_pid)?;
        let mut h_token = HANDLE::default();

        if OpenProcessToken(h_process, TOKEN_DUPLICATE | TOKEN_QUERY, &mut h_token).is_err() {
            let _ = CloseHandle(h_process);
            return Err(anyhow::anyhow!("Failed to open process token"));
        }
        let _ = CloseHandle(h_process);

        let mut primary_token = HANDLE::default();
        let dup_res = DuplicateTokenEx(
            h_token,
            TOKEN_ALL_ACCESS,
            None,
            SecurityIdentification,
            TokenPrimary,
            &mut primary_token,
        );
        let _ = CloseHandle(h_token);

        dup_res.context("Failed to duplicate token")?;

        Ok((primary_token, is_winlogon))
    }
}

/// Query the active input desktop name from within the user's session.
///
/// Spawns a minimal "agent query-desktop" child process using the explorer.exe token,
/// placed on winsta0\default. From WITHIN the user's session, OpenInputDesktop() correctly
/// returns the active input desktop for that session's WinSta0 — giving a session-based
/// way to distinguish Win11 Stage-1 lockscreen (Default active) from Stage-2 password input
/// (Winlogon active) WITHOUT scanning for logonui.exe or any other process name.
///
/// Returns "Default", "Winlogon", or "Default" on any error (safe fallback for Stage-1).
#[cfg(windows)]
fn query_active_desktop_in_session(explorer_pid: u32) -> String {
    unsafe {
        // Get primary token from explorer.exe in the target session
        let h_process = match OpenProcess(PROCESS_QUERY_INFORMATION, BOOL(0), explorer_pid) {
            Ok(h) => h,
            Err(_) => return "Default".to_string(),
        };
        let mut h_token = HANDLE::default();
        if OpenProcessToken(h_process, TOKEN_DUPLICATE | TOKEN_QUERY, &mut h_token).is_err() {
            let _ = CloseHandle(h_process);
            return "Default".to_string();
        }
        let _ = CloseHandle(h_process);

        let mut primary_token = HANDLE::default();
        let dup_res = DuplicateTokenEx(
            h_token,
            TOKEN_ALL_ACCESS,
            None,
            SecurityIdentification,
            TokenPrimary,
            &mut primary_token,
        );
        let _ = CloseHandle(h_token);
        if dup_res.is_err() {
            return "Default".to_string();
        }

        // Set up stdout pipe
        let mut h_read = HANDLE::default();
        let mut h_write = HANDLE::default();
        let sa = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: BOOL(1),
        };
        if CreatePipe(&mut h_read, &mut h_write, Some(&sa), 0).is_err() {
            let _ = CloseHandle(primary_token);
            return "Default".to_string();
        }

        let mut si: STARTUPINFOW = mem::zeroed();
        si.cb = mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdOutput = h_write;
        si.hStdError = h_write;

        // Run on Default desktop — the agent's OpenInputDesktop() returns the active desktop
        // for WinSta0 of the user's session regardless of which desktop the agent is assigned to.
        let mut desktop_u16: Vec<u16> = "winsta0\\default"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        si.lpDesktop = PWSTR(desktop_u16.as_mut_ptr());

        let exe_path = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => {
                let _ = CloseHandle(h_write);
                let _ = CloseHandle(h_read);
                let _ = CloseHandle(primary_token);
                return "Default".to_string();
            }
        };
        let cmd_line = format!(
            "\"{}\" agent query-desktop",
            exe_path.to_string_lossy()
        );
        let mut cmd_line_u16: Vec<u16> =
            cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

        let mut pi: PROCESS_INFORMATION = mem::zeroed();
        let result = CreateProcessAsUserW(
            primary_token,
            None,
            PWSTR(cmd_line_u16.as_mut_ptr()),
            None,
            None,
            BOOL(1),
            CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS,
            None,
            None,
            &si,
            &mut pi,
        );
        let _ = CloseHandle(h_write);
        let _ = CloseHandle(primary_token);

        if result.is_err() {
            let _ = CloseHandle(h_read);
            return "Default".to_string();
        }
        let _ = CloseHandle(pi.hThread);

        // Read output (agent exits immediately after printing the desktop name)
        let mut output = Vec::new();
        let mut buffer = [0u8; 256];
        let mut bytes_read = 0u32;
        loop {
            let ok = ReadFile(h_read, Some(&mut buffer), Some(&mut bytes_read), None);
            if ok.is_err() || bytes_read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..bytes_read as usize]);
        }
        WaitForSingleObject(pi.hProcess, 3000); // 3 s timeout; agent should finish in <100 ms
        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(h_read);

        let name = String::from_utf8_lossy(&output).trim().to_string();
        if name.is_empty() {
            "Default".to_string()
        } else {
            name
        }
    }
}

#[cfg(windows)]
pub fn run_agent_in_active_session(args: &[&str]) -> Result<String> {
    unsafe {
        // 1. Get Active Console Session ID
        let session_id = WTSGetActiveConsoleSessionId();
        if session_id == 0xFFFFFFFF {
            return Err(anyhow::anyhow!("No active console session found"));
        }

        // 2. Get User Token (via Stealing)
        let (primary_token, is_winlogon) = get_session_user_token(session_id)?;

        // 3. Create Pipes for Stdout
        let mut h_read = HANDLE::default();
        let mut h_write = HANDLE::default();
        let sa = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: BOOL(1), // True
        };

        if CreatePipe(&mut h_read, &mut h_write, Some(&sa), 0).is_err() {
            let _ = CloseHandle(primary_token);
            return Err(anyhow::anyhow!("Failed to create pipe"));
        }

        // 4. Setup Startup Info
        let mut si: STARTUPINFOW = mem::zeroed();
        si.cb = mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdOutput = h_write;
        si.hStdError = h_write;

        // Explicitly set desktop for WinLogon session
        let desktop_name = if is_winlogon {
            "winsta0\\winlogon"
        } else {
            "winsta0\\default"
        };
        let mut desktop_u16: Vec<u16> = desktop_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        si.lpDesktop = PWSTR(desktop_u16.as_mut_ptr());

        let mut pi: PROCESS_INFORMATION = mem::zeroed();

        // 5. Build Command Line
        let exe_path = std::env::current_exe()?;
        // Use the same executable but with "agent" subcommand
        let mut cmd_line = format!("\"{}\" agent", exe_path.to_string_lossy());
        for arg in args {
            cmd_line.push_str(" ");
            // Quote argument if it contains spaces and not already quoted
            // But here args are simple strings like "list" or "set" or "internal", no spaces expected usually.
            // If ID contains colons, it's fine.
            cmd_line.push_str(arg);
        }

        let mut cmd_line_u16: Vec<u16> =
            cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

        // 6. Create Process As User
        let result = CreateProcessAsUserW(
            primary_token,
            None,
            PWSTR(cmd_line_u16.as_mut_ptr()),
            None,
            None,
            BOOL(1), // Inherit handles
            CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS,
            None,
            None, // Current directory
            &si,
            &mut pi,
        );

        let _ = CloseHandle(h_write);
        let _ = CloseHandle(primary_token);

        if result.is_err() {
            let _ = CloseHandle(h_read);
            return Err(anyhow::anyhow!("CreateProcessAsUserW failed: {:?}", result));
        }

        let _ = CloseHandle(pi.hThread);

        // 7. Read Output
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut bytes_read = 0;

        loop {
            let success = ReadFile(h_read, Some(&mut buffer), Some(&mut bytes_read), None);

            if success.is_err() || bytes_read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[0..bytes_read as usize]);
        }

        WaitForSingleObject(pi.hProcess, INFINITE);
        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(h_read);

        let output_str = String::from_utf8_lossy(&output).to_string();
        Ok(output_str)
    }
}

// =============================================================================
// =============================================================================
// 进程控制功能 - 对应 PS1 中的 ShutdownHelper 和进程优先级设置
// =============================================================================

#[cfg(windows)]
use windows::{
    Win32::Foundation::HWND,
    Win32::System::Threading::{GetCurrentProcess, SetPriorityClass},
    Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE},
};

// Raw FFI: functions not (yet) in windows 0.52 safe wrappers
#[cfg(windows)]
extern "system" {
    fn GetConsoleWindow() -> HWND;
}

/// 进程优先级
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessPriority {
    /// 空转
    Idle,
    /// 低于正常
    BelowNormal,
    /// 正常
    Normal,
    /// 高于正常
    AboveNormal,
    /// 高
    High,
    /// 实时（需要管理员权限）
    Realtime,
}

impl ProcessPriority {
    /// 转换为 Windows 优先级常量
    #[cfg(windows)]
    fn to_windows_priority(&self) -> u32 {
        use windows::Win32::System::Threading::*;
        match self {
            ProcessPriority::Idle => IDLE_PRIORITY_CLASS.0,
            ProcessPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS.0,
            ProcessPriority::Normal => NORMAL_PRIORITY_CLASS.0,
            ProcessPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS.0,
            ProcessPriority::High => HIGH_PRIORITY_CLASS.0,
            ProcessPriority::Realtime => REALTIME_PRIORITY_CLASS.0,
        }
    }
}

/// 以 DETACHED 方式重新启动当前进程（无控制台窗口）
///
/// 用于修复 bat 黑窗问题：
/// - bat 启动 exe 后，cmd.exe 在 batch 模式下会等待子进程退出
/// - exe 调用此函数以 DETACHED_PROCESS | CREATE_NO_WINDOW 重新启动自身
/// - 原进程立即退出，cmd.exe 看到子进程退出后关闭 CMD 窗口
/// - 新进程以独立方式运行，正常显示托盘图标
///
/// 调用方应在此函数返回 Ok(()) 后立即 return 退出原进程。
/// 若返回 Err，则继续正常运行（不影响功能，只是黑窗问题未修复）。
#[cfg(windows)]
pub fn relaunch_as_detached() -> anyhow::Result<()> {
    use std::mem;

    let exe_path = std::env::current_exe()?;

    // 收集当前所有参数（跳过 argv[0]），追加 --relaunched 防止无限循环
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cmd_parts = vec![format!("\"{}\"", exe_path.to_string_lossy())];
    for a in &args {
        if a.contains(' ') {
            cmd_parts.push(format!("\"{}\"", a));
        } else {
            cmd_parts.push(a.clone());
        }
    }
    cmd_parts.push("--relaunched".to_string());

    let cmd_line = cmd_parts.join(" ");
    let mut cmd_wide: Vec<u16> = cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut si: STARTUPINFOW = mem::zeroed();
        si.cb = mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = mem::zeroed();

        CreateProcessW(
            None,                              // lpApplicationName (由 cmdline 决定)
            PWSTR(cmd_wide.as_mut_ptr()),       // lpCommandLine
            None,                              // lpProcessAttributes
            None,                              // lpThreadAttributes
            BOOL(0),                           // bInheritHandles = false
            DETACHED_PROCESS | CREATE_NO_WINDOW, // 完全脱离父控制台
            None,                              // lpEnvironment (继承父进程)
            None,                              // lpCurrentDirectory (使用当前目录)
            &si,
            &mut pi,
        )
        .context("CreateProcessW 重新启动失败")?;

        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(pi.hThread);
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn relaunch_as_detached() -> anyhow::Result<()> {
    Ok(())
}

/// 隐藏控制台窗口
///
/// 对应 PS1:
/// ```powershell
/// $SW_HIDE = 0
/// $consolePtr = $ShutdownHelper::GetConsoleWindow()
/// if ($consolePtr -ne [IntPtr]::Zero) {
///     $ShutdownHelper::ShowWindow($consolePtr, $SW_HIDE)
/// }
/// ```
#[cfg(windows)]
pub fn hide_console() {
    unsafe {
        let console_window = GetConsoleWindow();
        if console_window.0 != 0 {
            let _ = ShowWindow(console_window, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
pub fn hide_console() {
    // Non-Windows 平台无操作
}

/// 设置当前进程优先级
///
/// 对应 PS1:
/// ```powershell
/// try {
///     $process = Get-Process -Id $PID
///     $process.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::High
/// } catch {}
/// ```
///
/// # Arguments
/// * `priority` - 目标优先级，默认为 High
///
/// # Returns
/// * `bool` - 是否成功设置
#[cfg(windows)]
pub fn set_process_priority(priority: ProcessPriority) -> bool {
    unsafe {
        let process = GetCurrentProcess();
        SetPriorityClass(
            process,
            windows::Win32::System::Threading::PROCESS_CREATION_FLAGS(
                priority.to_windows_priority(),
            ),
        )
        .is_ok()
    }
}

#[cfg(not(windows))]
pub fn set_process_priority(_priority: ProcessPriority) -> bool {
    // Non-Windows 平台无操作
    true
}

/// 设置进程关机参数
///
/// 对应 PS1:
/// ```powershell
/// [ShutdownHelper]::SetProcessShutdownParameters(0x3FF, 0)
/// ```
///
/// 设置为最后一个关机通知级别（0x3FF），确保在其他应用之后执行
///
/// # Returns
/// * `bool` - 是否成功设置
#[cfg(windows)]
pub fn set_shutdown_parameters() -> bool {
    unsafe {
        // 0x3FF = 最后一个关机通知级别, 0 = 无标志
        // SetProcessShutdownParameters 返回 Result<(), Error>
        SetProcessShutdownParameters(0x3FF, 0).is_ok()
    }
}

#[cfg(not(windows))]
pub fn set_shutdown_parameters() -> bool {
    // Non-Windows 平台无操作
    true
}

/// 初始化进程设置
///
/// 组合调用以下功能（对应 PS1 启动时的初始化）：
/// - 设置关机参数
/// - 设置进程优先级为高
/// - 隐藏控制台窗口
///
/// # Arguments
/// * `hide_console_flag` - 是否隐藏控制台窗口
pub fn init_process_settings(hide_console_flag: bool) {
    use crate::log;

    // 设置关机参数
    if set_shutdown_parameters() {
        log!("Process: Shutdown parameters set to 0x3FF (last to terminate)");
    }

    // 设置高优先级
    if set_process_priority(ProcessPriority::High) {
        log!("Process: Priority set to High");
    } else {
        // 静默失败 - 仅记录但不阻断
        log!("Process: Failed to set priority (silently continuing)");
    }

    // 隐藏控制台窗口
    if hide_console_flag {
        hide_console();
        log!("Process: Console window hidden");
    }
}

/// 检测活跃控制台 Session 是否处于锁屏状态（Win+L）
///
/// 使用 WTSQuerySessionInformationW + WTSSessionInfoEx 查询 SessionFlags：
/// - WTS_SESSIONSTATE_LOCK (0)：session 已锁定 → true
/// - WTS_SESSIONSTATE_UNLOCK (1)：session 未锁定 → false
///
/// 适用：Windows Vista 及之后所有版本（含 Win7/Win8/Win10/Win11）。
/// 相比扫描进程列表检测 LogonUI.exe，此 API 更可靠、更高效。
/// 注：SessionId 0xFFFFFFFF 表示无活跃控制台 Session（如纯服务器环境）。
#[cfg(windows)]
pub fn is_active_console_session_locked() -> bool {
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
        WTS_SESSIONSTATE_LOCK,
    };

    unsafe {
        let session_id = WTSGetActiveConsoleSessionId();
        if session_id == 0xFFFFFFFF {
            return false;
        }

        let mut ppbuffer = PWSTR(std::ptr::null_mut());
        let mut bytes_returned: u32 = 0;

        let result = WTSQuerySessionInformationW(
            HANDLE::default(), // WTS_CURRENT_SERVER_HANDLE
            session_id,
            WTSSessionInfoEx,
            &mut ppbuffer,
            &mut bytes_returned,
        );

        if result.is_err() || ppbuffer.is_null() {
            return false;
        }

        let info = &*(ppbuffer.0 as *const WTSINFOEXW);
        let locked = info.Level == 1
            && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_LOCK as i32;

        WTSFreeMemory(ppbuffer.0 as *mut _);
        locked
    }
}

#[cfg(not(windows))]
pub fn is_active_console_session_locked() -> bool {
    false
}

/// 检测活跃控制台 Session 是否有关联用户（用户已登录桌面）
///
/// 使用 WTSQuerySessionInformationW(session_id, WTSUserName) 查询该 session 的用户名：
/// - 返回非空且非 SYSTEM 的用户名 → 有真实用户登录 → true（用户已登入桌面）
/// - 返回空 / 返回 SYSTEM / API 调用失败 → 无用户登录 → false（Winlogon/预登录/其他）
///
/// 为什么不用 SessionFlags（WTSSessionInfoEx）：
/// Winlogon 登录 screen 下 session 已创建且常处于 UNLOCK 状态，
/// SessionFlags == UNLOCK 无法区分"用户在 Winlogon"和"用户已登录桌面"。
/// 查询 WTSUserName 直接判断 session 是否关联真实用户账号，比 SessionFlags 更准确。
///
/// 用途：`--enforce-session0-only` 模式下，有用户登录桌面时服务跳过 enforcement。
/// 注意：Win+L 锁屏时用户仍登录 → WTSUserName 仍然非空 → 此函数返回 true（enforcement 不跳过）。
#[cfg(windows)]
pub fn is_user_on_desktop() -> bool {
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSUserName,
    };

    unsafe {
        let session_id = WTSGetActiveConsoleSessionId();
        if session_id == 0xFFFFFFFF {
            return false;
        }

        let mut ppbuffer = PWSTR(std::ptr::null_mut());
        let mut bytes_returned: u32 = 0;

        let result = WTSQuerySessionInformationW(
            HANDLE::default(), // WTS_CURRENT_SERVER_HANDLE
            session_id,
            WTSUserName,
            &mut ppbuffer,
            &mut bytes_returned,
        );

        if result.is_err() || ppbuffer.is_null() || bytes_returned == 0 {
            // API 调用失败或无数据 → 无用户
            return false;
        }

        let user_name = String::from_utf16_lossy(std::slice::from_raw_parts(
            ppbuffer.0,
            (bytes_returned / 2) as usize,
        ))
        .trim_matches(char::from(0))
        .trim()
        .to_string();

        WTSFreeMemory(ppbuffer.0 as *mut _);

        // 空字符串或 SYSTEM（SYSTEM 账号 = 无真实用户登录）
        !user_name.is_empty() && !user_name.eq_ignore_ascii_case("SYSTEM")
    }
}

#[cfg(not(windows))]
pub fn is_user_on_desktop() -> bool {
    false
}

/// 检测用户是否"活跃在桌面"：已登录且屏幕未锁定。
///
/// 等价于 `is_user_on_desktop() && !is_active_console_session_locked()`。
/// 封装为独立函数，供服务模式退让判断和 --enforce-session0-only skip 判断共同引用。
///
/// 返回 true：用户已登录且屏幕未锁（正在使用桌面）→ 服务退让 / 跳过 enforcement
/// 返回 false：无用户（Winlogon/预登录）、用户锁屏（Win+L）、API 调用失败 → 执行 enforcement
#[cfg(windows)]
pub fn is_user_actively_on_desktop() -> bool {
    is_user_on_desktop() && !is_active_console_session_locked()
}

#[cfg(not(windows))]
pub fn is_user_actively_on_desktop() -> bool {
    false
}
