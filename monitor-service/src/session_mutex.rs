//! Session级互斥量管理
//!
//! 机制：托盘模式在启动时创建Session专属的Mutex，服务模式检测退让
//! - 托盘创建 `Global\MonitorTray_{SessionId}` Mutex（表示当前Session有托盘接管）
//! - 服务模式检测到此Mutex存在时，enforcement退让（不做任何事）
//! - Mutex随托盘进程销毁自动释放，服务模式自动恢复
//!
//! 单实例保障：
//! - 如果新实例发现Mutex已存在（前一个托盘正在关闭），等待最多5秒
//! - 等待成功（前一实例已退出）→ 接管Mutex，正常启动
//! - 等待超时（另一个托盘真的在运行）→ 返回 Err，调用方应退出

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenMutexW, ReleaseMutex, WaitForSingleObject,
    MUTEX_ALL_ACCESS,
};

const ERROR_ALREADY_EXISTS_CODE: u32 = 183;
/// WaitForSingleObject 返回值常量
const WAIT_OBJECT_0: u32 = 0x00000000;
/// WAIT_ABANDONED：前一个拥有者异常退出（进程崩溃/正常退出未ReleaseMutex）
const WAIT_ABANDONED_0: u32 = 0x00000080;
/// 等待前一个托盘退出的最大时间（毫秒）
const MUTEX_WAIT_TIMEOUT_MS: u32 = 5000;

extern "system" {
    fn GetLastError() -> u32;
    fn SetLastError(dw_err_code: u32);
}

/// SessionMutex守卫 - 托盘模式持有，进程退出时自动释放
pub struct SessionMutex {
    handle: windows::Win32::Foundation::HANDLE,
    session_id: u32,
}

impl SessionMutex {
    /// 托盘模式调用：创建并获取Session专属的Mutex所有权
    ///
    /// - 如果Mutex不存在：立即创建并获取所有权
    /// - 如果Mutex已存在（前一个托盘正在关闭）：等待最多5秒，待前一实例释放后接管
    /// - 如果5秒后仍无法获取：返回 Err（另一个托盘真的在运行）
    pub fn create() -> Result<Self, Box<dyn std::error::Error>> {
        // 使用进程自身的 session ID，而不是 WTSGetActiveConsoleSessionId()。
        // WTSGetActiveConsoleSessionId() 在 FUS 过渡期可能返回另一 session 的 ID，
        // 导致 mutex 名与 service 侧检查时使用的 active console session ID 不匹配，
        // 进而使 yield 机制在 FUS 后失效、引发打乒乓。
        // 进程自身 session ID 稳定且准确代表"这个 tray 属于哪个 session"。
        let session_id = unsafe {
            let pid = GetCurrentProcessId();
            let mut sid: u32 = 0;
            if ProcessIdToSessionId(pid, &mut sid).is_ok() {
                sid
            } else {
                // 兜底：回退到 active console session（应不会发生）
                WTSGetActiveConsoleSessionId()
            }
        };
        let name = format!("Global\\MonitorTray_{}", session_id);
        let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            // 创建Mutex，bInitialOwner=true 表示我们请求初始所有权
            // 若Mutex已存在，CreateMutexW 仍返回其句柄，但所有权归旧实例
            // GetLastError() == ERROR_ALREADY_EXISTS_CODE 标识此情况
            SetLastError(0);
            let handle = CreateMutexW(None, true, windows::core::PCWSTR(name_wide.as_ptr()));

            match handle {
                Ok(h) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS_CODE {
                        // Mutex已存在：等待旧实例释放（进程退出时OS自动放弃Mutex所有权）
                        // WAIT_OBJECT_0(0)    = 正常释放
                        // WAIT_ABANDONED_0(0x80) = 旧进程退出未ReleaseMutex（正常退出场景）
                        let wait_result = WaitForSingleObject(h, MUTEX_WAIT_TIMEOUT_MS);
                        match wait_result.0 {
                            WAIT_OBJECT_0 | WAIT_ABANDONED_0 => {
                                // 已获得所有权：可以安全接管
                                Ok(SessionMutex {
                                    handle: h,
                                    session_id,
                                })
                            }
                            _ => {
                                // 超时或错误：另一个托盘实例正在运行
                                let _ = CloseHandle(h);
                                Err(format!(
                                    "Another tray instance is running in Session {} (wait timeout {}ms)",
                                    session_id, MUTEX_WAIT_TIMEOUT_MS
                                )
                                .into())
                            }
                        }
                    } else {
                        // Mutex是新建的，且我们已持有所有权（bInitialOwner=true）
                        Ok(SessionMutex {
                            handle: h,
                            session_id,
                        })
                    }
                }
                Err(e) => Err(format!("Failed to create mutex: {}", e).into()),
            }
        }
    }

    pub fn session_id(&self) -> u32 {
        self.session_id
    }
}

impl Drop for SessionMutex {
    fn drop(&mut self) {
        unsafe {
            // 先释放所有权（让等待者能立即获取，而不是等 CloseHandle 触发 ABANDONED）
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

/// 检测当前Session是否有托盘在运行
/// 服务模式调用：返回true表示应该退让
pub fn is_tray_present_in_current_session() -> bool {
    unsafe {
        let session_id = WTSGetActiveConsoleSessionId();
        let name = format!("Global\\MonitorTray_{}", session_id);
        let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        // 尝试打开Mutex（不取所有权，只检测存在性）
        // 使用 MUTEX_ALL_ACCESS；对 ACCESS_DENIED 也视为「Mutex存在」，
        // 避免服务模式(SYSTEM)权限不足时误判为「托盘未运行」
        let handle = OpenMutexW(
            MUTEX_ALL_ACCESS,
            false,
            windows::core::PCWSTR(name_wide.as_ptr()),
        );

        match handle {
            Ok(h) => {
                // Mutex存在，关闭句柄（只是检测）
                let _ = CloseHandle(h);
                true
            }
            Err(e) => {
                // ACCESS_DENIED (0x80070005): Mutex存在但权限不足
                // 这也意味着托盘实例在运行，服务应退让
                const HRESULT_ACCESS_DENIED: i32 = 0x80070005_u32 as i32;
                if e.code().0 == HRESULT_ACCESS_DENIED {
                    true
                } else {
                    // Mutex不存在（ERROR_FILE_NOT_FOUND 等）→ 托盘未运行
                    false
                }
            }
        }
    }
}

/// 获取当前Session ID（用于日志记录）
pub fn get_current_session_id() -> u32 {
    unsafe { WTSGetActiveConsoleSessionId() }
}
