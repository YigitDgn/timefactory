use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::OnceLock;

use winapi::shared::minwindef::{BOOL, DWORD, FARPROC, TRUE};
use winapi::shared::windef::HWND;
use winapi::um::dwmapi::DwmSetWindowAttribute;
use winapi::um::libloaderapi::{GetProcAddress, LoadLibraryW};
use winapi::um::winreg::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<ThemeMode> {
        match s {
            "system" => Some(ThemeMode::System),
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        }
    }

    pub fn is_dark(self) -> bool {
        match self {
            ThemeMode::System => system_prefers_dark(),
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
        }
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// Whether Windows is set to dark mode for apps (Settings > Personalization > Colors).
pub fn system_prefers_dark() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value = wide("AppsUseLightTheme");
    let mut data: DWORD = 1;
    let mut size = std::mem::size_of::<DWORD>() as DWORD;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            &mut data as *mut DWORD as _,
            &mut size,
        )
    };
    status == 0 && data == 0
}

// Undocumented uxtheme exports (Windows 10 1903+) that switch Win32 popup
// menus between dark and light.
struct UxThemeDarkMode {
    set_preferred_app_mode: Option<unsafe extern "system" fn(i32) -> i32>,
    flush_menu_themes: Option<unsafe extern "system" fn()>,
}

fn ux_theme() -> &'static UxThemeDarkMode {
    static API: OnceLock<UxThemeDarkMode> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let module = LoadLibraryW(wide("uxtheme.dll").as_ptr());
        if module.is_null() {
            return UxThemeDarkMode {
                set_preferred_app_mode: None,
                flush_menu_themes: None,
            };
        }
        let by_ordinal = |n: usize| -> FARPROC { GetProcAddress(module, n as *const i8) };
        let set_mode = by_ordinal(135);
        let flush = by_ordinal(136);
        UxThemeDarkMode {
            set_preferred_app_mode: (!set_mode.is_null()).then(|| std::mem::transmute(set_mode)),
            flush_menu_themes: (!flush.is_null()).then(|| std::mem::transmute(flush)),
        }
    })
}

/// Makes popup menus (the tray menu) follow `dark`.
pub fn set_app_dark_mode(dark: bool) {
    const FORCE_DARK: i32 = 2;
    const FORCE_LIGHT: i32 = 3;
    let api = ux_theme();
    unsafe {
        if let Some(set_mode) = api.set_preferred_app_mode {
            set_mode(if dark { FORCE_DARK } else { FORCE_LIGHT });
        }
        if let Some(flush) = api.flush_menu_themes {
            flush();
        }
    }
}

pub fn set_title_bar_dark(hwnd: HWND, dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: DWORD = 20;
    let value: BOOL = if dark { TRUE } else { 0 };
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const BOOL as _,
            std::mem::size_of::<BOOL>() as DWORD,
        );
    }
}
