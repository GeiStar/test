//! 关机/注销拦截模块
//!
//! 实现 WM_QUERYENDSESSION 消息处理，确保关机前恢复正确的显示模式。
//! 包含三种 CASE 处理：
//! - CASE 1: 模式正确，直接放行
//! - CASE 2: 模式错误但在用户桌面，修复后放行
//! - CASE 3: 在安全桌面，拒绝后等待并重新发起关机

use monitor_lib::{display, TopologyMode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
    Win32::System::Shutdown::{
        ExitWindowsEx, EWX_FORCEIFHUNG, EWX_LOGOFF, EWX_SHUTDOWN, SHUTDOWN_REASON,
    },
    Win32::System::Threading::SetProcessShutdownParameters,
    Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, WM_DESTROY,
        WM_QUERYENDSESSION, WNDCLASSW,
    },
};

use crate::log;

#[cfg(windows)]
static CURRENT_MODE_PTR: std::sync::atomic::AtomicPtr<std::sync::Mutex<TopologyMode>> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static INITIAL_MODE_PTR: std::sync::atomic::AtomicPtr<TopologyMode> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static IS_SCRIPT_INITIATED_PTR: std::sync::atomic::AtomicPtr<AtomicBool> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static TASK_RUNNING_PTR: std::sync::atomic::AtomicPtr<AtomicBool> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// 关机拦截器状态
pub struct ShutdownInterceptor {
    /// 由脚本发起的关机标志（防止循环）
    is_script_initiated: Arc<AtomicBool>,
    /// 后台任务运行标志（防止并发）
    task_running: Arc<AtomicBool>,
}

impl ShutdownInterceptor {
    pub fn new() -> Self {
        Self {
            is_script_initiated: Arc::new(AtomicBool::new(false)),
            task_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// 启动关机拦截器
///
/// 创建隐藏消息窗口并在独立线程中运行消息循环
pub fn start_shutdown_interceptor(
    initial_mode: TopologyMode,
    current_mode: Arc<std::sync::Mutex<TopologyMode>>,
) -> ShutdownInterceptor {
    let interceptor = ShutdownInterceptor::new();
    let is_script_initiated = interceptor.is_script_initiated.clone();
    let task_running = interceptor.task_running.clone();

    #[cfg(windows)]
    {
        // 设置进程关机优先级（0x3FF = 最后一个被关闭）
        unsafe {
            let _ = SetProcessShutdownParameters(0x3FF, 0);
        }

        thread::spawn(move || {
            run_message_loop(
                initial_mode,
                current_mode,
                is_script_initiated,
                task_running,
            );
        });
    }

    #[cfg(not(windows))]
    {
        log!("Shutdown interceptor not supported on non-Windows OS");
    }

    interceptor
}

#[cfg(windows)]
fn run_message_loop(
    initial_mode: TopologyMode,
    current_mode: Arc<std::sync::Mutex<TopologyMode>>,
    is_script_initiated: Arc<AtomicBool>,
    task_running: Arc<AtomicBool>,
) {
    use std::sync::atomic::Ordering as AtomicOrdering;

    // 存储状态指针
    CURRENT_MODE_PTR.store(
        Arc::into_raw(current_mode) as *mut _,
        AtomicOrdering::SeqCst,
    );
    INITIAL_MODE_PTR.store(
        Box::into_raw(Box::new(initial_mode)) as *mut _,
        AtomicOrdering::SeqCst,
    );
    IS_SCRIPT_INITIATED_PTR.store(
        Arc::into_raw(is_script_initiated) as *mut _,
        AtomicOrdering::SeqCst,
    );
    TASK_RUNNING_PTR.store(
        Arc::into_raw(task_running) as *mut _,
        AtomicOrdering::SeqCst,
    );

    unsafe {
        // 注册窗口类
        let class_name = wide_string("MonitorShutdownHandler");
        let class_name_ptr = PCWSTR(class_name.as_ptr());
        let hinstance: HINSTANCE = std::mem::transmute(
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default(),
        );
        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(shutdown_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name_ptr,
            style: CS_HREDRAW | CS_VREDRAW,
            ..Default::default()
        };

        let atom = RegisterClassW(&wnd_class);
        if atom == 0 {
            log!("Failed to register window class for shutdown handler");
            return;
        }

        // 创建隐藏的消息窗口
        let window_name = wide_string("MonitorShutdownWindow");
        let hwnd = CreateWindowExW(
            Default::default(),
            class_name_ptr,
            PCWSTR(window_name.as_ptr()),
            Default::default(),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            HWND(0), // 隐藏顶层窗口；message-only window 收不到系统广播
            None,
            hinstance,
            None,
        );

        if hwnd.0 == 0 {
            log!("Failed to create shutdown handler window");
            return;
        }

        log!("Shutdown interceptor started - monitoring WM_QUERYENDSESSION");

        // 消息循环
        let mut msg: MSG = Default::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn shutdown_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => handle_query_end_session(hwnd, wparam, lparam),
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(windows)]
unsafe fn handle_query_end_session(_hwnd: HWND, _wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use std::sync::atomic::Ordering as AtomicOrdering;

    // 恢复状态指针
    let current_mode = if let Some(ptr) = CURRENT_MODE_PTR.load(AtomicOrdering::SeqCst).as_ref() {
        ptr
    } else {
        return LRESULT(1); // 无法获取状态，放行
    };

    let initial_mode = if let Some(ptr) = INITIAL_MODE_PTR.load(AtomicOrdering::SeqCst).as_ref() {
        ptr
    } else {
        return LRESULT(1); // 无法获取状态，放行
    };

    let is_script_initiated = if let Some(ptr) = IS_SCRIPT_INITIATED_PTR
        .load(AtomicOrdering::SeqCst)
        .as_ref()
    {
        ptr
    } else {
        return LRESULT(1); // 无法获取状态，放行
    };

    let task_running = if let Some(ptr) = TASK_RUNNING_PTR.load(AtomicOrdering::SeqCst).as_ref() {
        ptr
    } else {
        return LRESULT(1); // 无法获取状态，放行
    };

    // ENDSESSION_LOGOFF = 0x80000000
    let is_logoff = (lparam.0 & 0x80000000) != 0;
    let reason = if is_logoff { "LOGOFF" } else { "SHUTDOWN" };

    log!(
        ">>> [COMPLETE] {} detected - Starting COMPLETE logic...",
        reason
    );

    // 检查 1: 是否由脚本发起的关机（防止循环）
    if is_script_initiated.load(Ordering::SeqCst) {
        log!(">>> [COMPLETE] Script initiated shutdown detected - ALLOWING immediately (preventing loop)");
        is_script_initiated.store(false, Ordering::SeqCst);
        return LRESULT(1); // TRUE = 允许关机
    }

    // 检查 2: 是否有后台任务在运行（防止并发）
    if task_running.load(Ordering::SeqCst) {
        log!(">>> [COMPLETE] Task already running - ALLOWING to prevent deadlock");
        return LRESULT(1); // TRUE = 允许关机
    }

    // STEP 1: 检查显示器数量
    log!(">>> [COMPLETE] STEP 1: Checking monitor count...");

    let monitor_count = match display::get_monitor_count() {
        Ok(count) => count,
        Err(e) => {
            log!(
                ">>> [COMPLETE] Failed to get monitor count: {} - ALLOWING",
                e
            );
            return LRESULT(1); // 无法获取显示器数量，放行
        }
    };

    log!(">>> [COMPLETE] Monitor count: {}", monitor_count);

    // 单显示器：直接放行
    if monitor_count <= 1 {
        log!(">>> [COMPLETE] Single monitor - No switching needed - ALLOWING immediately");
        return LRESULT(1); // TRUE = 允许关机
    }

    // STEP 2: 检测当前模式
    log!(">>> [COMPLETE] STEP 2: Detecting current mode...");

    let current = match display::get_topology() {
        Ok(mode) => mode,
        Err(e) => {
            // CASE 3: 安全桌面场景 - 无法检测模式
            log!(
                ">>> [COMPLETE] CASE 3: Cannot detect mode (Secure Desktop?): {} - CANCELLING",
                e
            );

            task_running.store(true, Ordering::SeqCst);

            // 保存需要的状态用于后台任务
            let initial_mode_clone = initial_mode.clone();
            // 将指针转换为 usize 以安全地跨线程传递
            let is_script_initiated_usize =
                IS_SCRIPT_INITIATED_PTR.load(AtomicOrdering::SeqCst) as usize;
            let task_running_usize = TASK_RUNNING_PTR.load(AtomicOrdering::SeqCst) as usize;

            // 启动后台任务等待用户桌面
            thread::spawn(move || {
                // 在任务内部转换回指针
                handle_secure_desktop_task(
                    initial_mode_clone,
                    is_logoff,
                    is_script_initiated_usize as *mut AtomicBool,
                    task_running_usize as *mut AtomicBool,
                );
            });

            // 返回 FALSE 拒绝关机，等待后台任务重新发起
            return LRESULT(0); // FALSE = 拒绝关机
        }
    };

    let current_name = format!("{:?}", current);
    let initial_name = format!("{:?}", initial_mode);

    log!(
        ">>> [COMPLETE] Current mode: {}, Initial: {}",
        current_name,
        initial_name
    );

    // 获取当前目标模式（考虑用户通过热键修改后的模式）
    let target_mode = {
        let mode_guard = current_mode.lock().unwrap();
        mode_guard.clone()
    };
    let target_name = format!("{:?}", target_mode);

    // CASE 1: 模式正确
    if &current == initial_mode {
        log!(">>> [COMPLETE] CASE 1: Mode CORRECT - ALLOWING (0 flash, 0 delay)");
        return LRESULT(1); // TRUE = 允许关机
    }

    // CASE 2: 模式错误，在用户桌面
    log!(
        ">>> [COMPLETE] CASE 2: Mode mismatch (Current: {} != Initial: {}; Runtime target: {}) - Fixing -> {}...",
        current_name,
        initial_name,
        target_name,
        initial_name
    );

    // 执行修复：恢复到初始模式
    match display::set_topology(initial_mode.clone()) {
        Ok(_) => {
            // 等待模式切换生效
            thread::sleep(Duration::from_millis(50));
            log!(">>> [COMPLETE] CASE 2: Fixed - ALLOWING (~200ms)");
            LRESULT(1) // TRUE = 允许关机
        }
        Err(e) => {
            log!(">>> [COMPLETE] CASE 2: Fix failed: {} - ALLOWING anyway", e);
            LRESULT(1) // 修复失败，但仍然放行
        }
    }
}

/// CASE 3 后台任务：等待返回用户桌面，然后修复并重新发起关机
#[cfg(windows)]
fn handle_secure_desktop_task(
    initial_mode: TopologyMode,
    is_logoff: bool,
    is_script_initiated_ptr: *mut AtomicBool,
    task_running_ptr: *mut AtomicBool,
) {
    use std::sync::atomic::Ordering;

    const MAX_RETRIES: i32 = 50;
    const RETRY_DELAY_MS: u64 = 100;

    log!(">>> [TASK] Waiting for user desktop...");

    // 等待回到用户桌面（最多 5 秒）
    for attempt in 0..MAX_RETRIES {
        thread::sleep(Duration::from_millis(RETRY_DELAY_MS));

        // 尝试获取显示器数量和模式
        let (monitors, mode_result) = (display::get_monitor_count(), display::get_topology());

        if let (Ok(count), Ok(mode)) = (&monitors, &mode_result) {
            // 成功获取模式，说明回到用户桌面了
            let mode_name = format!("{:?}", mode);
            log!(
                ">>> [TASK] Back to desktop! Monitors={}, Mode={}",
                count,
                mode_name
            );

            // 如果是多显示器且模式不正确，需要修复
            if *count > 1 && *mode != initial_mode {
                let initial_name = format!("{:?}", initial_mode);
                log!(">>> [TASK] Fixing: {} -> {}", mode_name, initial_name);

                match display::set_topology(initial_mode.clone()) {
                    Ok(_) => {
                        log!(">>> [TASK] Fix result: 0 (success)");
                    }
                    Err(e) => {
                        log!(">>> [TASK] Fix failed: {}", e);
                    }
                }
            } else {
                log!(">>> [TASK] No fix needed (single monitor or mode correct)");
            }

            // 设置标志并重新发起关机
            if !is_script_initiated_ptr.is_null() {
                unsafe {
                    (*is_script_initiated_ptr).store(true, Ordering::SeqCst);
                }
            }

            let action = if is_logoff { "LOGOFF" } else { "SHUTDOWN" };
            log!(">>> [TASK] Re-initiating {}...", action);

            unsafe {
                // 重新发起原始动作（注销或关机）
                let exit_flag = if is_logoff { EWX_LOGOFF } else { EWX_SHUTDOWN };
                let _ = ExitWindowsEx(exit_flag | EWX_FORCEIFHUNG, SHUTDOWN_REASON(0));
            }

            // 清理任务运行标志
            if !task_running_ptr.is_null() {
                unsafe {
                    (*task_running_ptr).store(false, Ordering::SeqCst);
                }
            }

            return;
        }

        // 每10次尝试记录一次日志
        if attempt % 10 == 9 {
            log!(
                ">>> [TASK] Still waiting... (attempt {}/{})",
                attempt + 1,
                MAX_RETRIES
            );
        }
    }

    // 超时
    log!(">>> [TASK] Timeout waiting for desktop - giving up");

    if !task_running_ptr.is_null() {
        unsafe {
            (*task_running_ptr).store(false, Ordering::SeqCst);
        }
    }
}

/// 将 Rust 字符串转换为宽字符字符串
#[cfg(windows)]
fn wide_string(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
