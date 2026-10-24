use std::ptr;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

use eframe::egui;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HICON, HWND, POINT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::shellapi::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use winapi::um::winuser::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    GetSystemMetrics, GetWindowLongPtrW, IsIconic, IsWindowVisible, LoadImageW, PostMessageW,
    RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
    TrackPopupMenu, GWLP_USERDATA, IMAGE_ICON, MF_SEPARATOR, MF_STRING, SM_CXSMICON, SM_CYSMICON,
    SW_HIDE, SW_RESTORE, SW_SHOW, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};

use crate::theme::wide;
use crate::tracking::Shared;

const APP_TITLE: &str = "TimeFactory";
const TRAY_ID: UINT = 1;
// Icon resource id in resources/app.rc.in.
const APP_ICON_ID: u16 = 1;
const WM_TRAY: UINT = WM_APP + 1;

const CMD_SHOW: usize = 1;
const CMD_PAUSE: usize = 2;
const CMD_QUIT: usize = 3;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn set_main_window(hwnd: HWND) {
    MAIN_HWND.store(hwnd as isize, Ordering::Relaxed);
}

fn main_window() -> HWND {
    MAIN_HWND.load(Ordering::Relaxed) as HWND
}

pub fn show_main_window(ctx: &egui::Context) {
    let hwnd = main_window();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        ShowWindow(
            hwnd,
            if IsIconic(hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_SHOW
            },
        );
        SetForegroundWindow(hwnd);
    }
    ctx.request_repaint();
}

pub fn hide_main_window() {
    let hwnd = main_window();
    if !hwnd.is_null() {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

fn toggle_main_window(ctx: &egui::Context) {
    let hwnd = main_window();
    if !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) } != 0 {
        hide_main_window();
    } else {
        show_main_window(ctx);
    }
}

struct TrayState {
    ctx: egui::Context,
    shared: Arc<Shared>,
    show_message: UINT,
    taskbar_created: UINT,
    icon: HICON,
}

/// The notification-area icon. Owns a hidden top-level window that receives
/// the icon's mouse events and the "show yourself" broadcast sent by a second
/// instance. Must be created on the UI thread, whose event loop pumps its
/// messages.
pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    pub fn create(ctx: egui::Context, shared: Arc<Shared>, show_message: UINT) -> Option<Tray> {
        unsafe {
            let module = GetModuleHandleW(ptr::null());
            let class_name = wide("TimeFactoryTray");
            let class = WNDCLASSW {
                lpfnWndProc: Some(tray_proc),
                hInstance: module,
                lpszClassName: class_name.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&class);

            // Not HWND_MESSAGE: message-only windows don't get broadcasts.
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide(APP_TITLE).as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                module,
                ptr::null_mut(),
            );
            if hwnd.is_null() {
                return None;
            }

            let icon = LoadImageW(
                module,
                APP_ICON_ID as usize as *const u16,
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                0,
            ) as HICON;

            let state = Box::new(TrayState {
                ctx,
                shared,
                show_message,
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                icon,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

            add_icon(hwnd, icon);
            Some(Tray { hwnd })
        }
    }

    /// Shows a balloon / toast from the tray icon.
    pub fn notify(&self, title: &str, text: &str) {
        unsafe {
            let mut data = icon_data(self.hwnd);
            data.uFlags = NIF_INFO;
            data.dwInfoFlags = NIIF_INFO;
            copy_wide(&mut data.szInfoTitle, title);
            copy_wide(&mut data.szInfo, text);
            Shell_NotifyIconW(NIM_MODIFY, &mut data);
        }
    }
}

unsafe fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = std::mem::zeroed();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ID;
    data
}

fn copy_wide(dest: &mut [u16], text: &str) {
    let src: Vec<u16> = text.encode_utf16().take(dest.len() - 1).collect();
    dest[..src.len()].copy_from_slice(&src);
    dest[src.len()] = 0;
}

unsafe fn add_icon(hwnd: HWND, icon: HICON) {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    copy_wide(&mut data.szTip, APP_TITLE);
    Shell_NotifyIconW(NIM_ADD, &mut data);
}

unsafe fn show_menu(hwnd: HWND, state: &TrayState) {
    let paused = state.shared.paused.load(Ordering::Relaxed);
    let menu = CreatePopupMenu();
    AppendMenuW(menu, MF_STRING, CMD_SHOW, wide("Göster").as_ptr());
    let pause_text = if paused {
        "Takibe devam et"
    } else {
        "Takibi duraklat"
    };
    AppendMenuW(menu, MF_STRING, CMD_PAUSE, wide(pause_text).as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    AppendMenuW(menu, MF_STRING, CMD_QUIT, wide("Çıkış").as_ptr());

    let mut cursor = POINT { x: 0, y: 0 };
    GetCursorPos(&mut cursor);
    // Required so the menu closes when clicking elsewhere.
    SetForegroundWindow(hwnd);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        cursor.x,
        cursor.y,
        0,
        hwnd,
        ptr::null(),
    ) as usize;
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);

    match command {
        CMD_SHOW => show_main_window(&state.ctx),
        CMD_PAUSE => {
            state.shared.paused.store(!paused, Ordering::Relaxed);
            state.ctx.request_repaint();
        }
        CMD_QUIT => {
            let mut data = icon_data(hwnd);
            Shell_NotifyIconW(NIM_DELETE, &mut data);
            std::process::exit(0);
        }
        _ => {}
    }
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: UINT, w: WPARAM, l: LPARAM) -> LRESULT {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const TrayState;
    if let Some(state) = state.as_ref() {
        if msg == WM_TRAY {
            match l as UINT {
                WM_LBUTTONUP => toggle_main_window(&state.ctx),
                WM_RBUTTONUP => show_menu(hwnd, state),
                _ => {}
            }
            return 0;
        }
        if msg == state.show_message {
            // Another instance was started: bring this one to the front.
            show_main_window(&state.ctx);
            return 0;
        }
        if msg == state.taskbar_created {
            // Explorer restarted; the icon has to be added again.
            add_icon(hwnd, state.icon);
            return 0;
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}
