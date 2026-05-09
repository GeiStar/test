use monitor_lib::{display, TopologyMode};
use native_windows_gui as nwg;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{
        GetMessagePos, PostMessageW, SetForegroundWindow, TrackPopupMenu, HMENU, TPM_BOTTOMALIGN,
        TPM_RIGHTALIGN, WM_NULL,
    },
};

// 引用日志宏（注意：log! 是 macro_rules!，不是函数）
use crate::log;

/// 启动托盘（旧接口保持兼容）
pub fn start_tray(initial_mode: TopologyMode, shutdown_tx: tokio::sync::oneshot::Sender<()>) {
    let state = Arc::new(TrayState {
        current_mode: Arc::new(Mutex::new(initial_mode.clone())),
        initial_mode: initial_mode.clone(),
        monitoring_enabled: Arc::new(AtomicBool::new(true)),
        force_refresh: Arc::new(AtomicBool::new(false)),
        hide_exit: false,
        web_addr: None,
    });
    std::thread::spawn(move || {
        if let Err(e) = run_tray(initial_mode, shutdown_tx, Some(state)) {
            eprintln!("Tray error: {}", e);
        }
    });
}

/// 启动托盘（新接口，支持外部状态共享）
pub fn start_tray_with_state(
    initial_mode: TopologyMode,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    current_mode: Arc<Mutex<TopologyMode>>,
    monitoring_enabled: Arc<AtomicBool>,
    force_refresh: Arc<AtomicBool>,
    hide_exit: bool,
    web_addr: Option<String>,
) {
    let state = Arc::new(TrayState {
        current_mode,
        initial_mode: initial_mode.clone(),
        monitoring_enabled,
        force_refresh,
        hide_exit,
        web_addr,
    });
    std::thread::spawn(move || {
        if let Err(e) = run_tray(initial_mode, shutdown_tx, Some(state)) {
            eprintln!("Tray error: {}", e);
        }
    });
}

struct TrayManager {
    window: nwg::Window,
    _icon: nwg::Icon,
    tray: nwg::TrayNotification,
    menu: nwg::Menu,
    timer: nwg::Timer,
    state: Arc<TrayState>,
    items: TrayItems,
    shutdown_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    last_sync: Mutex<Instant>,
}

struct TrayState {
    current_mode: Arc<Mutex<TopologyMode>>,
    initial_mode: TopologyMode,
    monitoring_enabled: Arc<AtomicBool>,
    /// 强制UI刷新标志 - 由外部（如快捷键、强制执行）设置，托盘线程消费
    force_refresh: Arc<AtomicBool>,
    /// 是否隐藏托盘菜单中的 Exit 按钮（默认显示）
    hide_exit: bool,
    /// Web 服务监听地址（如 "127.0.0.1:3000"），有值时托盘显示「Open Web Console」
    web_addr: Option<String>,
}

struct TrayItems {
    extend: nwg::MenuItem,
    clone: nwg::MenuItem,
    internal: nwg::MenuItem,
    external: nwg::MenuItem,
    monitoring: nwg::MenuItem,
    /// Web Console 按钮：仅在指定 --addr 时存在
    web_console: Option<nwg::MenuItem>,
    /// About 按钮
    about: nwg::MenuItem,
    /// Exit 按钮：仅在 --show-exit 时存在
    exit: Option<nwg::MenuItem>,
}

fn run_tray(
    initial_mode: TopologyMode,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    external_state: Option<Arc<TrayState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    nwg::init()?;

    // 从系统DLL提取图标（像PS1版本一样）
    let icon = crate::system_icon::extract_system_icon()
        .or_else(|| crate::system_icon::extract_fallback_icon())
        .unwrap_or_else(|| {
            log!("Failed to extract system icon, using fallback");
            create_fallback_icon()
        });

    // 使用外部状态或创建新的（必须在创建menu之前确定）
    let state = external_state.unwrap_or_else(|| {
        Arc::new(TrayState {
            current_mode: Arc::new(Mutex::new(initial_mode.clone())),
            initial_mode: initial_mode.clone(),
            monitoring_enabled: Arc::new(AtomicBool::new(true)),
            force_refresh: Arc::new(AtomicBool::new(false)),
            hide_exit: false,
            web_addr: None,
        })
    });

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .title("Display Manager Tray")
        .flags(nwg::WindowFlags::POPUP)
        .size((0, 0))
        .position((-32000, -32000))
        .build(&mut window)?;

    let mut tray = nwg::TrayNotification::default();
    let tooltip = tooltip_text(&state);
    nwg::TrayNotification::builder()
        .parent(&window)
        .icon(Some(&icon))
        .tip(Some(&tooltip))
        .build(&mut tray)?;

    let mut menu = nwg::Menu::default();
    nwg::Menu::builder()
        .popup(true)
        .parent(&window)
        .build(&mut menu)?;

    let items = build_tray_items(&menu, &state)?;

    let mut timer = nwg::Timer::default();
    nwg::Timer::builder()
        .parent(&window)
        .interval(500)
        .stopped(false)
        .build(&mut timer)?;

    let manager = Rc::new(TrayManager {
        window,
        _icon: icon,
        tray,
        menu,
        timer,
        state,
        items,
        shutdown_tx: Mutex::new(Some(shutdown_tx)),
        last_sync: Mutex::new(Instant::now()),
    });

    update_menu_checked(&manager, &initial_mode);
    update_monitoring_menu_checked(&manager);
    update_tooltip(&manager);

    let event_manager = Rc::downgrade(&manager);
    let handler =
        nwg::full_bind_event_handler(&manager.window.handle, move |event, _data, handle| {
            if let Some(manager) = event_manager.upgrade() {
                handle_tray_event(&manager, event, handle);
            }
        });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);

    Ok(())
}

fn build_tray_items(menu: &nwg::Menu, state: &TrayState) -> Result<TrayItems, nwg::NwgError> {
    let mut item_internal = nwg::MenuItem::default();
    let mut item_clone = nwg::MenuItem::default();
    let mut item_extend = nwg::MenuItem::default();
    let mut item_external = nwg::MenuItem::default();
    let mut item_monitor = nwg::MenuItem::default();
    let mut item_about = nwg::MenuItem::default();

    nwg::MenuItem::builder()
        .text("Internal (Alt+Shift+Q)")
        .parent(menu)
        .build(&mut item_internal)?;
    nwg::MenuItem::builder()
        .text("Clone (Alt+Shift+W)")
        .parent(menu)
        .build(&mut item_clone)?;
    nwg::MenuItem::builder()
        .text("Extend (Alt+Shift+E)")
        .parent(menu)
        .build(&mut item_extend)?;
    nwg::MenuItem::builder()
        .text("External (Alt+Shift+R)")
        .parent(menu)
        .build(&mut item_external)?;

    let mut sep1 = nwg::MenuSeparator::default();
    nwg::MenuSeparator::builder()
        .parent(menu)
        .build(&mut sep1)?;

    nwg::MenuItem::builder()
        .text("Runtime Monitoring (Alt+Shift+T)")
        .check(state.monitoring_enabled.load(Ordering::Relaxed))
        .parent(menu)
        .build(&mut item_monitor)?;

    let item_web_console = if state.web_addr.is_some() {
        let mut sep_web = nwg::MenuSeparator::default();
        nwg::MenuSeparator::builder()
            .parent(menu)
            .build(&mut sep_web)?;

        let mut item = nwg::MenuItem::default();
        nwg::MenuItem::builder()
            .text("Open Web Console")
            .parent(menu)
            .build(&mut item)?;
        Some(item)
    } else {
        None
    };

    let mut sep_about = nwg::MenuSeparator::default();
    nwg::MenuSeparator::builder()
        .parent(menu)
        .build(&mut sep_about)?;

    nwg::MenuItem::builder()
        .text("About")
        .parent(menu)
        .build(&mut item_about)?;

    let item_exit = if !state.hide_exit {
        let mut item = nwg::MenuItem::default();
        nwg::MenuItem::builder()
            .text("Exit")
            .parent(menu)
            .build(&mut item)?;
        Some(item)
    } else {
        None
    };

    Ok(TrayItems {
        extend: item_extend,
        clone: item_clone,
        internal: item_internal,
        external: item_external,
        monitoring: item_monitor,
        web_console: item_web_console,
        about: item_about,
        exit: item_exit,
    })
}

fn handle_tray_event(manager: &TrayManager, event: nwg::Event, handle: nwg::ControlHandle) {
    match event {
        nwg::Event::OnContextMenu if handle == manager.tray.handle => {
            show_tray_menu(manager);
        }
        nwg::Event::OnMenuItemSelected => {
            handle_menu_item(manager, handle);
        }
        nwg::Event::OnTimerTick if handle == manager.timer.handle => {
            sync_tray_state(manager);
        }
        _ => {}
    }
}

fn handle_menu_item(manager: &TrayManager, handle: nwg::ControlHandle) {
    if handle == manager.items.extend.handle {
        set_mode(manager, TopologyMode::Extend);
    } else if handle == manager.items.clone.handle {
        set_mode(manager, TopologyMode::Clone);
    } else if handle == manager.items.internal.handle {
        set_mode(manager, TopologyMode::Internal);
    } else if handle == manager.items.external.handle {
        set_mode(manager, TopologyMode::External);
    } else if handle == manager.items.monitoring.handle {
        toggle_monitoring(manager);
    } else if manager
        .items
        .web_console
        .as_ref()
        .map_or(false, |w| handle == w.handle)
    {
        open_web_console(&manager.state.web_addr);
    } else if handle == manager.items.about.handle {
        show_about();
    } else if manager
        .items
        .exit
        .as_ref()
        .map_or(false, |e| handle == e.handle)
    {
        if let Ok(mut tx) = manager.shutdown_tx.lock() {
            if let Some(tx) = tx.take() {
                let _ = tx.send(());
            }
        }
        nwg::stop_thread_dispatch();
    }
}

fn show_tray_menu(manager: &TrayManager) {
    // Capture the click message position before IME activation or UI refresh can disturb live cursor coordinates.
    let (x, y) = tray_menu_anchor_position();

    // 与 PS1 的 ContextMenu.Opening 一致：打开前先刷新实际拓扑勾选状态。
    if let Ok(actual) = display::get_topology() {
        update_menu_checked(manager, &actual);
    }
    update_monitoring_menu_checked(manager);
    update_tooltip(manager);

    popup_tray_menu(&manager.menu, x, y);
}

#[cfg(windows)]
fn tray_menu_anchor_position() -> (i32, i32) {
    unsafe {
        let pos = GetMessagePos();
        let x = (pos & 0xFFFF) as i16 as i32;
        let y = ((pos >> 16) & 0xFFFF) as i16 as i32;
        (x, y)
    }
}

#[cfg(not(windows))]
fn tray_menu_anchor_position() -> (i32, i32) {
    nwg::GlobalCursor::position()
}

#[cfg(windows)]
fn popup_tray_menu(menu: &nwg::Menu, x: i32, y: i32) {
    if let Some((parent, hmenu)) = menu.handle.pop_hmenu() {
        unsafe {
            let hwnd = HWND(parent as isize);
            let hmenu = HMENU(hmenu as isize);

            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(hmenu, TPM_BOTTOMALIGN | TPM_RIGHTALIGN, x, y, 0, hwnd, None);
            let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        }
    } else {
        menu.popup(x, y);
    }
}

#[cfg(not(windows))]
fn popup_tray_menu(menu: &nwg::Menu, x: i32, y: i32) {
    menu.popup(x, y);
}

fn sync_tray_state(manager: &TrayManager) {
    // Check force refresh flag (triggered by hotkey or enforcement)
    // 注意：只刷新 UI 显示，绝对不修改 current_mode（用户设定的目标模式）
    if manager.state.force_refresh.swap(false, Ordering::Relaxed) {
        let target = manager.state.current_mode.lock().unwrap().clone();
        update_menu_checked(manager, &target);
        update_monitoring_menu_checked(manager);
        update_tooltip(manager);
    }

    let mut last_sync = manager.last_sync.lock().unwrap();
    if last_sync.elapsed() < Duration::from_secs(2) {
        return;
    }

    *last_sync = Instant::now();
    // 使用系统实际拓扑来更新菜单勾选状态（视觉反馈）
    if let Ok(actual) = display::get_topology() {
        update_menu_checked(manager, &actual);
    }
    // tooltip 始终显示目标模式（用户设定的）
    update_tooltip(manager);
}

fn set_mode(manager: &TrayManager, mode: TopologyMode) {
    log!("User menu: {:?}", mode);

    if let Err(e) = display::set_topology(mode.clone()) {
        log!("Failed to set topology via menu: {}", e);
        return;
    }
    {
        let mut current = manager.state.current_mode.lock().unwrap();
        *current = mode.clone();
    }
    update_menu_checked(manager, &mode);
    update_tooltip(manager);
    log!("Topology set via menu: {:?}", mode);
}

fn show_about() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ABOUT_OPEN: AtomicBool = AtomicBool::new(false);
    // 已有窗口则忽略本次点击
    if ABOUT_OPEN.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| unsafe {
        about_raw::run();
        ABOUT_OPEN.store(false, Ordering::Relaxed);
    });
}

/// About 窗口实现（raw Win32 绑定，无新依赖，XP 兼容，文本可选中/复制）
#[cfg(windows)]
mod about_raw {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    type BOOL = i32;
    type HANDLE = *mut c_void;
    type HWND = *mut c_void;
    type HINSTANCE = *mut c_void;
    type HCURSOR = *mut c_void;
    type LRESULT = isize;
    type WPARAM = usize;
    type LPARAM = isize;
    type UINT = u32;
    type DWORD = u32;
    type LONG = i32;
    type ATOM = u16;
    type LPCTSTR = *const u16;

    #[repr(C)]
    struct WNDCLASSEXW {
        cbSize: UINT,
        style: UINT,
        lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        cbClsExtra: i32,
        cbWndExtra: i32,
        hInstance: HINSTANCE,
        hIcon: HANDLE,
        hCursor: HCURSOR,
        hbrBackground: HANDLE,
        lpszMenuName: LPCTSTR,
        lpszClassName: LPCTSTR,
        hIconSm: HANDLE,
    }

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
            hMenu: HWND,
            hInstance: HINSTANCE,
            lpParam: *mut c_void,
        ) -> HWND;
        fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, min: UINT, max: UINT) -> BOOL;
        fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
        fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        fn DefWindowProcW(hWnd: HWND, Msg: UINT, wP: WPARAM, lP: LPARAM) -> LRESULT;
        fn PostQuitMessage(nExitCode: i32);
        fn SetWindowPos(
            hWnd: HWND,
            after: HWND,
            X: i32,
            Y: i32,
            cx: i32,
            cy: i32,
            flags: UINT,
        ) -> BOOL;
        fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dw: isize) -> isize;
        fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
        fn LoadCursorW(hInstance: HINSTANCE, name: LPCTSTR) -> HCURSOR;
        fn GetSystemMetrics(nIndex: i32) -> i32;
    }

    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;

    // Window / Edit styles
    const WS_OVERLAPPED: DWORD = 0x00000000;
    const WS_CAPTION: DWORD = 0x00C00000;
    const WS_SYSMENU: DWORD = 0x00080000;
    const WS_MINIMIZEBOX: DWORD = 0x00020000;
    const WS_VISIBLE: DWORD = 0x10000000;
    const WS_CHILD: DWORD = 0x40000000;
    const WS_VSCROLL: DWORD = 0x00200000;
    const WS_EX_DLGMODALFRAME: DWORD = 0x00000001;
    const ES_MULTILINE: DWORD = 0x0004;
    const ES_READONLY: DWORD = 0x0800;
    const ES_AUTOVSCROLL: DWORD = 0x0040;
    const SWP_NOZORDER: UINT = 0x0004;
    const SWP_NOACTIVATE: UINT = 0x0010;
    const GWLP_USERDATA: i32 = -21;
    const WM_CREATE: UINT = 0x0001;
    const WM_SIZE: UINT = 0x0005;
    const WM_DESTROY: UINT = 0x0002;
    const IDC_ARROW: LPCTSTR = 32512usize as LPCTSTR;

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: UINT,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                let text: Vec<u16> = concat!(
                    "Windows Monitor Manager Auto  v0.1.0\r\n",
                    "\r\n",
                    "Project:\r\n",
                    "https://github.com/GeiStar/windows-monitor-manager-auto\r\n",
                    "\r\n",
                    "Automatically keeps your display topology at the\r\n",
                    "correct setting (Extend / Clone / Internal / External).\r\n",
                    "\r\n",
                    "Hotkeys:\r\n",
                    "  Alt+Shift+Q    Internal only\r\n",
                    "  Alt+Shift+W    Clone\r\n",
                    "  Alt+Shift+E    Extend\r\n",
                    "  Alt+Shift+R    External only\r\n",
                    "  Alt+Shift+T    Toggle monitoring\0",
                )
                .encode_utf16()
                .collect();

                let edit_cls: Vec<u16> = "EDIT\0".encode_utf16().collect();
                let edit = CreateWindowExW(
                    0,
                    edit_cls.as_ptr(),
                    text.as_ptr(),
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_VSCROLL
                        | ES_MULTILINE
                        | ES_READONLY
                        | ES_AUTOVSCROLL,
                    5,
                    5,
                    440,
                    295,
                    hwnd,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                );
                // Edit HWND 存入父窗口 userdata，供 WM_SIZE 取用
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, edit as isize);
                0
            }
            WM_SIZE => {
                let cx = (lparam as u32 & 0xFFFF) as i32;
                let cy = ((lparam as u32 >> 16) & 0xFFFF) as i32;
                let edit = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HWND;
                if !edit.is_null() {
                    SetWindowPos(
                        edit,
                        null_mut(),
                        5,
                        5,
                        cx - 10,
                        cy - 10,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, _wparam, lparam),
        }
    }

    pub unsafe fn run() {
        let hinstance: HINSTANCE = null_mut();
        let cls: Vec<u16> = "DisplayMgrAbout\0".encode_utf16().collect();
        let ttl: Vec<u16> = "About\0".encode_utf16().collect();

        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinstance;
        wc.lpszClassName = cls.as_ptr();
        wc.hbrBackground = 6 as HANDLE; // COLOR_WINDOW + 1
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        let _ = RegisterClassExW(&wc); // 重复注册时忽略

        const WIN_W: i32 = 460;
        const WIN_H: i32 = 350;
        let x = (GetSystemMetrics(SM_CXSCREEN) - WIN_W) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - WIN_H) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            cls.as_ptr(),
            ttl.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE | WS_MINIMIZEBOX,
            x,
            y,
            WIN_W,
            WIN_H,
            null_mut(),
            null_mut(),
            hinstance,
            null_mut(),
        );
        if hwnd.is_null() {
            return;
        }

        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = GetMessageW(&mut msg, null_mut(), 0, 0);
            if r <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn open_web_console(web_addr: &Option<String>) {
    if let Some(addr) = web_addr {
        // 0.0.0.0 → 127.0.0.1，使浏览器可访问
        let normalized = addr.replace("0.0.0.0", "127.0.0.1");
        let url = format!("http://{}", normalized);
        log!("Opening web console: {}", url);
        let _ = std::process::Command::new("explorer").arg(&url).spawn();
    }
}

fn toggle_monitoring(manager: &TrayManager) {
    // fetch_xor 是原子 read-modify-write，避免并发时 load-flip-store 竞态
    let old = manager
        .state
        .monitoring_enabled
        .fetch_xor(true, Ordering::Relaxed);
    let new_enabled = !old;
    manager.items.monitoring.set_checked(new_enabled);
    update_tooltip(manager);

    if new_enabled {
        log!("Runtime monitoring ENABLED by user");
    } else {
        log!("Runtime monitoring DISABLED by user");
    }
}

fn update_menu_checked(manager: &TrayManager, mode: &TopologyMode) {
    manager
        .items
        .extend
        .set_checked(matches!(mode, TopologyMode::Extend));
    manager
        .items
        .clone
        .set_checked(matches!(mode, TopologyMode::Clone));
    manager
        .items
        .internal
        .set_checked(matches!(mode, TopologyMode::Internal));
    manager
        .items
        .external
        .set_checked(matches!(mode, TopologyMode::External));
}

/// 更新monitoring菜单项的checkbox状态，使其与当前状态同步
fn update_monitoring_menu_checked(manager: &TrayManager) {
    let enabled = manager.state.monitoring_enabled.load(Ordering::Relaxed);
    manager.items.monitoring.set_checked(enabled);
}

fn update_tooltip(manager: &TrayManager) {
    let tooltip = tooltip_text(&manager.state);
    manager.tray.set_tip(&tooltip);
}

fn tooltip_text(state: &TrayState) -> String {
    let target = state.current_mode.lock().unwrap().clone();
    let enabled = state.monitoring_enabled.load(Ordering::Relaxed);

    format!(
        "Display Manager: {:?} (Initial: {:?}) [{}]",
        target,
        state.initial_mode,
        if enabled {
            "Monitoring ON"
        } else {
            "Monitoring OFF"
        }
    )
}

/// 创建后备图标（当系统图标提取失败时使用）
fn create_fallback_icon() -> nwg::Icon {
    let mut icon = nwg::Icon::default();
    const APP_ICO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "\\assets\\app.ico");

    if nwg::Icon::builder()
        .source_file(Some(APP_ICO))
        .strict(false)
        .build(&mut icon)
        .is_ok()
    {
        return icon;
    }

    let _ = nwg::Icon::builder()
        .source_system(Some(nwg::OemIcon::Information))
        .build(&mut icon);
    icon
}
