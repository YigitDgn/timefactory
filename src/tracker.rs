use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::ptr;

use winapi::shared::minwindef::{BOOL, DWORD, LPARAM, LPVOID, TRUE, UINT, WORD};
use winapi::shared::windef::HWND;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetCurrentProcessId, OpenProcess};
use winapi::um::sysinfoapi::GetTickCount;
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
use winapi::um::winuser::{
    EnumWindows, GetForegroundWindow, GetLastInputInfo, GetWindow, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
    LASTINPUTINFO, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use winapi::um::winver::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};

thread_local! {
    // Reading version resources is comparatively expensive and an exe's
    // FileDescription doesn't change while it runs, so cache it per path.
    static NAME_CACHE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

unsafe fn wide_ptr_to_string(p: *const u16) -> String {
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    OsString::from_wide(std::slice::from_raw_parts(p, len))
        .to_string_lossy()
        .into_owned()
}

#[repr(C)]
struct LangAndCodePage {
    language: WORD,
    code_page: WORD,
}

// "FileDescription" from the exe's version resource (e.g. "Google Chrome"),
// falling back to the file name without its extension.
fn friendly_name_from_path(full_path: &str) -> String {
    let fallback = Path::new(full_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let path_w = wide(full_path);
    unsafe {
        let mut handle: DWORD = 0;
        let size = GetFileVersionInfoSizeW(path_w.as_ptr(), &mut handle);
        if size == 0 {
            return fallback;
        }

        let mut buffer = vec![0u8; size as usize];
        if GetFileVersionInfoW(path_w.as_ptr(), 0, size, buffer.as_mut_ptr() as LPVOID) == 0 {
            return fallback;
        }

        let mut translations: LPVOID = ptr::null_mut();
        let mut translations_bytes: UINT = 0;
        let query = wide("\\VarFileInfo\\Translation");
        if VerQueryValueW(
            buffer.as_ptr() as LPVOID,
            query.as_ptr(),
            &mut translations,
            &mut translations_bytes,
        ) == 0
            || (translations_bytes as usize) < std::mem::size_of::<LangAndCodePage>()
        {
            return fallback;
        }

        let first = &*(translations as *const LangAndCodePage);
        let sub_block = wide(&format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            first.language, first.code_page
        ));

        let mut desc_ptr: LPVOID = ptr::null_mut();
        let mut desc_len: UINT = 0;
        if VerQueryValueW(
            buffer.as_ptr() as LPVOID,
            sub_block.as_ptr(),
            &mut desc_ptr,
            &mut desc_len,
        ) != 0
            && desc_len > 0
        {
            let description = wide_ptr_to_string(desc_ptr as *const u16);
            let description = description.trim();
            if !description.is_empty() {
                return description.to_string();
            }
        }
    }

    fallback
}

/// (display name, full exe path) of the process, unless it's this process.
fn app_for_pid(pid: DWORD) -> Option<(String, String)> {
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
        return None;
    }

    let full_path = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 32768];
        let mut size = buf.len() as DWORD;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        OsString::from_wide(&buf[..size as usize])
            .to_string_lossy()
            .into_owned()
    };

    let name = NAME_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(full_path.clone())
            .or_insert_with(|| friendly_name_from_path(&full_path))
            .clone()
    });
    (!name.is_empty()).then_some((name, full_path))
}

fn pid_of(hwnd: HWND) -> DWORD {
    let mut pid: DWORD = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// (display name, exe path) of the process owning the foreground window, e.g.
/// ("Google Chrome", "C:\...\chrome.exe"), or `None` if it can't be
/// determined or belongs to this process.
pub fn active_app() -> Option<(String, String)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    app_for_pid(pid_of(hwnd))
}

/// Seconds since the last keyboard or mouse input in this session.
pub fn idle_seconds() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as UINT,
        dwTime: 0,
    };
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            return 0;
        }
        // Both are 32-bit tick counts; wrapping_sub handles the ~49 day rollover.
        u64::from(GetTickCount().wrapping_sub(info.dwTime)) / 1000
    }
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let names = &mut *(lparam as *mut HashSet<String>);

    if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) == 0 {
        return TRUE;
    }
    if !GetWindow(hwnd, GW_OWNER).is_null() {
        return TRUE;
    }
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as DWORD;
    if ex_style & WS_EX_TOOLWINDOW != 0 && ex_style & WS_EX_APPWINDOW == 0 {
        return TRUE;
    }

    if let Some((name, _)) = app_for_pid(pid_of(hwnd)) {
        names.insert(name);
    }
    TRUE
}

/// Distinct display names of every process that currently has a visible
/// top-level window (every open application, not just the foreground one),
/// excluding this process.
pub fn open_display_names() -> HashSet<String> {
    let mut names = HashSet::new();
    unsafe { EnumWindows(Some(enum_windows_proc), &mut names as *mut _ as LPARAM) };
    names
}
