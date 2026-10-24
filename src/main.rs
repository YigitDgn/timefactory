#![windows_subsystem = "windows"]

mod db;
mod theme;
mod tracker;
mod tracking;
mod tray;
mod ui;
mod winicon;

use std::ptr;

use eframe::egui;
use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::synchapi::CreateMutexW;
use winapi::um::winuser::{
    AllowSetForegroundWindow, MessageBoxW, PostMessageW, RegisterWindowMessageW, ASFW_ANY,
    HWND_BROADCAST, MB_ICONERROR, MB_OK,
};

use theme::wide;

const INSTANCE_KEY: &str = "TimeFactory-SingleInstance-a1e6c9";

fn main() -> eframe::Result {
    let key = wide(INSTANCE_KEY);
    let show_message = unsafe { RegisterWindowMessageW(key.as_ptr()) };

    // The mutex lives until the process exits.
    let _guard = unsafe { CreateMutexW(ptr::null_mut(), 0, key.as_ptr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // Another instance is already running: ask it to show itself and quit.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            PostMessageW(HWND_BROADCAST, show_message, 0, 0);
        }
        return Ok(());
    }

    let db = match db::UsageDatabase::open() {
        Ok(db) => db,
        Err(e) => {
            let text = wide(&format!("Veritabanı açılamadı: {e}"));
            unsafe {
                MessageBoxW(
                    ptr::null_mut(),
                    text.as_ptr(),
                    wide("TimeFactory").as_ptr(),
                    MB_OK | MB_ICONERROR,
                )
            };
            return Ok(());
        }
    };

    let idle_minutes = db
        .setting(ui::IDLE_SETTING)
        .and_then(|s| s.parse().ok())
        .unwrap_or(tracking::DEFAULT_IDLE_MINUTES);
    let shared = tracking::Shared::new(idle_minutes);
    tracking::spawn(shared.clone());

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("TimeFactory")
        .with_inner_size([1240.0, 740.0])
        .with_min_inner_size([760.0, 560.0]);
    if let Some(icon) = winicon::from_own_resource(1, 64) {
        viewport = viewport.with_icon(egui::IconData {
            rgba: icon.rgba,
            width: icon.width,
            height: icon.height,
        });
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "TimeFactory",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::TimeFactoryApp::new(
                cc,
                db,
                shared,
                show_message,
            )))
        }),
    )
}
