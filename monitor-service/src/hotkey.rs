//! 全局热键模块 - Alt+Shift+Q/W/E/R/T
//!
//! 纯 Windows raw FFI，零外部依赖

use monitor_lib::TopologyMode;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotkeyEvent {
    ToInternal,
    ToClone,
    ToExtend,
    ToExternal,
    ToggleMonitoring,
}

pub struct HotkeyManager {
    _thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl HotkeyManager {
    pub fn new() -> anyhow::Result<(Self, UnboundedReceiver<HotkeyEvent>)> {
        let (tx, rx) = mpsc::unbounded_channel::<HotkeyEvent>();
        let handle = std::thread::Builder::new()
            .name("hotkey-loop".into())
            .spawn(move || hotkey_thread(tx))?;
        Ok((
            Self {
                _thread_handle: Some(handle),
            },
            rx,
        ))
    }
}

// =============== Pure Windows raw FFI (no windows crate) ===============

#[cfg(windows)]
mod raw {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    use super::HotkeyEvent;
    use tokio::sync::mpsc::UnboundedSender;

    // ===== Type aliases (matching Windows SDK) =====
    type BOOL = i32;
    type HANDLE = *mut c_void;
    type HINSTANCE = *mut c_void;
    type HWND = *mut c_void;
    type HCURSOR = *mut c_void;
    type HICON = *mut c_void;
    type HMENU = *mut c_void;
    type LRESULT = isize;
    type WPARAM = usize;
    type LPARAM = isize;
    type ATOM = u16;
    type UINT = u32;
    type DWORD = u32;
    type LONG = i32;
    type LPCTSTR = *const u16;
    type LPVOID = *mut c_void;

    #[allow(non_snake_case)]
    #[repr(C)]
    struct WNDCLASSEXW {
        cbSize: UINT,
        style: UINT,
        lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        cbClsExtra: i32,
        cbWndExtra: i32,
        hInstance: HINSTANCE,
        hIcon: HICON,
        hCursor: HCURSOR,
        hbrBackground: HANDLE,
        lpszMenuName: LPCTSTR,
        lpszClassName: LPCTSTR,
        hIconSm: HICON,
    }

    #[allow(non_snake_case)]
    #[repr(C)]
    struct MSG {
        hwnd: HWND,
        message: UINT,
        wParam: WPARAM,
        lParam: LPARAM,
        time: DWORD,
        pt_x: LONG,
        pt_y: LONG,
    }

    extern "system" {
        fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
        fn CreateWindowExW(
            dwExStyle: DWORD,
            lpClassName: LPCTSTR,
            lpWindowName: LPCTSTR,
            dwStyle: DWORD,
            X: i32,
            Y: i32,
            nWidth: i32,
            nHeight: i32,
            hWndParent: HWND,
            hMenu: HMENU,
            hInstance: HINSTANCE,
            lpParam: LPVOID,
        ) -> HWND;
        fn GetMessageW(
            lpMsg: *mut MSG,
            hWnd: HWND,
            wMsgFilterMin: UINT,
            wMsgFilterMax: UINT,
        ) -> BOOL;
        fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
        fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        fn RegisterHotKey(hWnd: HWND, id: i32, fsModifiers: UINT, vk: UINT) -> BOOL;
        fn UnregisterHotKey(hWnd: HWND, id: i32) -> BOOL;
        fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: LPCTSTR) -> HCURSOR;
        fn LoadIconW(hInstance: HINSTANCE, lpIconName: LPCTSTR) -> HICON;
    }

    const WS_OVERLAPPED: DWORD = 0;
    const CW_USEDEFAULT: i32 = 0x80000000u32 as i32;
    const HWND_MESSAGE: HWND = -3isize as HWND;
    const CS_HREDRAW: UINT = 2;
    const CS_VREDRAW: UINT = 1;
    const IDC_ARROW: *const u16 = 32512usize as *const u16;
    const IDI_APPLICATION: *const u16 = 32512usize as *const u16;

    const MOD_ALT: UINT = 0x0001;
    const MOD_SHIFT: UINT = 0x0004;
    const MOD_NOREPEAT: UINT = 0x4000;
    const HOTKEY_ID_Q: i32 = 1;
    const HOTKEY_ID_W: i32 = 2;
    const HOTKEY_ID_E: i32 = 3;
    const HOTKEY_ID_R: i32 = 4;
    const HOTKEY_ID_T: i32 = 5;
    const WM_HOTKEY: UINT = 0x0312;

    // Virtual-key codes
    const VK_Q: u32 = 0x51;
    const VK_W: u32 = 0x57;
    const VK_E: u32 = 0x45;
    const VK_R: u32 = 0x52;
    const VK_T: u32 = 0x54;

    static mut SENDER: Option<UnboundedSender<HotkeyEvent>> = None;

    unsafe extern "system" fn wndproc(
        _hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_HOTKEY {
            unsafe {
                if let Some(ref tx) = SENDER {
                    let event = match wparam as i32 {
                        HOTKEY_ID_Q => Some(HotkeyEvent::ToInternal),
                        HOTKEY_ID_W => Some(HotkeyEvent::ToClone),
                        HOTKEY_ID_E => Some(HotkeyEvent::ToExtend),
                        HOTKEY_ID_R => Some(HotkeyEvent::ToExternal),
                        HOTKEY_ID_T => Some(HotkeyEvent::ToggleMonitoring),
                        _ => None,
                    };
                    if let Some(evt) = event {
                        let _ = tx.send(evt);
                    }
                }
            }
            return 0;
        }
        DefWindowProcW(_hwnd, msg, wparam, _lparam)
    }

    pub fn run(tx: UnboundedSender<HotkeyEvent>) {
        unsafe {
            SENDER = Some(tx);
        }

        let hinstance: HINSTANCE = null_mut();

        // Wide class name
        const CLASS_NAME: &[u16] = &[
            'H' as u16, 'o' as u16, 't' as u16, 'k' as u16, 'e' as u16, 'y' as u16, 'W' as u16,
            'i' as u16, 'n' as u16, 'd' as u16, 'o' as u16, 'w' as u16, 'C' as u16, 'l' as u16,
            'a' as u16, 's' as u16, 's' as u16, 0,
        ];
        const WINDOW_NAME: &[u16] = &[
            'H' as u16, 'o' as u16, 't' as u16, 'k' as u16, 'e' as u16, 'y' as u16, 'W' as u16,
            'i' as u16, 'n' as u16, 'd' as u16, 'o' as u16, 'w' as u16, 0,
        ];

        let mut wc: WNDCLASSEXW = unsafe { std::mem::zeroed() };
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinstance;
        wc.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
        wc.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        wc.lpszClassName = CLASS_NAME.as_ptr();

        if unsafe { RegisterClassExW(&wc) } == 0 {
            return;
        }

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME.as_ptr(),
                WINDOW_NAME.as_ptr(),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                HWND_MESSAGE,
                null_mut(),
                hinstance,
                null_mut(),
            )
        };

        if hwnd.is_null() {
            return;
        }

        // Register hotkeys
        let mods = MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        unsafe {
            RegisterHotKey(hwnd, HOTKEY_ID_Q, mods, VK_Q);
            RegisterHotKey(hwnd, HOTKEY_ID_W, mods, VK_W);
            RegisterHotKey(hwnd, HOTKEY_ID_E, mods, VK_E);
            RegisterHotKey(hwnd, HOTKEY_ID_R, mods, VK_R);
            RegisterHotKey(hwnd, HOTKEY_ID_T, mods, VK_T);
        }

        // Message loop
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        loop {
            let ret = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
            if ret <= 0 {
                break;
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Cleanup
        unsafe {
            UnregisterHotKey(hwnd, HOTKEY_ID_Q);
            UnregisterHotKey(hwnd, HOTKEY_ID_W);
            UnregisterHotKey(hwnd, HOTKEY_ID_E);
            UnregisterHotKey(hwnd, HOTKEY_ID_R);
            UnregisterHotKey(hwnd, HOTKEY_ID_T);
        }
    }
}

fn hotkey_thread(tx: UnboundedSender<HotkeyEvent>) {
    #[cfg(windows)]
    raw::run(tx);
    #[cfg(not(windows))]
    drop(tx);
}

pub fn handle_hotkey_event(
    event: HotkeyEvent,
    current_mode: &std::sync::Mutex<TopologyMode>,
    monitoring_enabled: &std::sync::atomic::AtomicBool,
) -> Option<(Option<TopologyMode>, Option<bool>)> {
    use std::sync::atomic::Ordering;
    match event {
        HotkeyEvent::ToInternal => {
            if let Ok(mut mode) = current_mode.lock() {
                if *mode != TopologyMode::Internal {
                    *mode = TopologyMode::Internal;
                    return Some((Some(TopologyMode::Internal), None));
                }
            }
        }
        HotkeyEvent::ToClone => {
            if let Ok(mut mode) = current_mode.lock() {
                if *mode != TopologyMode::Clone {
                    *mode = TopologyMode::Clone;
                    return Some((Some(TopologyMode::Clone), None));
                }
            }
        }
        HotkeyEvent::ToExtend => {
            if let Ok(mut mode) = current_mode.lock() {
                if *mode != TopologyMode::Extend {
                    *mode = TopologyMode::Extend;
                    return Some((Some(TopologyMode::Extend), None));
                }
            }
        }
        HotkeyEvent::ToExternal => {
            if let Ok(mut mode) = current_mode.lock() {
                if *mode != TopologyMode::External {
                    *mode = TopologyMode::External;
                    return Some((Some(TopologyMode::External), None));
                }
            }
        }
        HotkeyEvent::ToggleMonitoring => {
            // fetch_xor 是原子 read-modify-write，避免并发时 load-flip-store 竞态
            let old = monitoring_enabled.fetch_xor(true, Ordering::Relaxed);
            return Some((None, Some(!old)));
        }
    }
    None
}

pub fn event_to_mode(event: HotkeyEvent) -> Option<TopologyMode> {
    match event {
        HotkeyEvent::ToInternal => Some(TopologyMode::Internal),
        HotkeyEvent::ToClone => Some(TopologyMode::Clone),
        HotkeyEvent::ToExtend => Some(TopologyMode::Extend),
        HotkeyEvent::ToExternal => Some(TopologyMode::External),
        _ => None,
    }
}
