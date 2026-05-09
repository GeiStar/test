//! 系统图标提取
//!
//! 从 compstui.dll 提取图标（与 PS1 版本行为一致：ExtractIconW index 16）。

use std::ffi::c_void;

use native_windows_gui as nwg;

#[link(name = "shell32")]
extern "system" {
    fn ExtractIconW(
        hInst: *mut c_void,
        lpszExeFileName: *const u16,
        nIconIndex: u32,
    ) -> *mut c_void;
}

/// 从 compstui.dll index 16 提取图标（与 PS1 版本完全一致）。
pub fn extract_system_icon() -> Option<nwg::Icon> {
    extract_dll_icon("compstui.dll", 16)
}

/// 备用图标由托盘模块统一处理。
pub fn extract_fallback_icon() -> Option<nwg::Icon> {
    None
}

fn extract_dll_icon(dll_name: &str, icon_index: u32) -> Option<nwg::Icon> {
    unsafe {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let dll_path = format!("{}\\system32\\{}", system_root, dll_name);
        let dll_wide: Vec<u16> = dll_path.encode_utf16().chain(std::iter::once(0)).collect();

        let hicon = ExtractIconW(std::ptr::null_mut(), dll_wide.as_ptr(), icon_index);
        if hicon.is_null() || hicon as usize == 1 {
            return None;
        }

        Some(nwg::Icon::from_raw_handle(hicon.cast(), true))
    }
}
