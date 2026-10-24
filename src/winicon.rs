use std::ptr;

use winapi::shared::minwindef::UINT;
use winapi::shared::windef::{HBITMAP, HICON};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{
    DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use winapi::um::winuser::{
    DestroyIcon, GetDC, GetIconInfo, LoadImageW, ReleaseDC, ICONINFO, IMAGE_ICON,
};

use crate::theme::wide;

/// An icon as straight (unpremultiplied) RGBA pixels.
pub struct RgbaIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[link(name = "user32")]
extern "system" {
    fn PrivateExtractIconsW(
        file: *const u16,
        index: i32,
        cx: i32,
        cy: i32,
        icons: *mut HICON,
        ids: *mut UINT,
        count: UINT,
        flags: UINT,
    ) -> UINT;
}

/// The main icon of an exe, rendered at `size` x `size`.
pub fn from_exe(path: &str, size: i32) -> Option<RgbaIcon> {
    let path = wide(path);
    let mut icon: HICON = ptr::null_mut();
    let count = unsafe {
        PrivateExtractIconsW(
            path.as_ptr(),
            0,
            size,
            size,
            &mut icon,
            ptr::null_mut(),
            1,
            0,
        )
    };
    if count == 0 || count == u32::MAX || icon.is_null() {
        return None;
    }
    let result = unsafe { to_rgba(icon) };
    unsafe { DestroyIcon(icon) };
    result
}

/// An icon embedded in this exe's resources (see resources/app.rc.in).
pub fn from_own_resource(id: u16, size: i32) -> Option<RgbaIcon> {
    unsafe {
        let module = GetModuleHandleW(ptr::null());
        let icon =
            LoadImageW(module, id as usize as *const u16, IMAGE_ICON, size, size, 0) as HICON;
        if icon.is_null() {
            return None;
        }
        let result = to_rgba(icon);
        DestroyIcon(icon);
        result
    }
}

unsafe fn to_rgba(icon: HICON) -> Option<RgbaIcon> {
    let mut info: ICONINFO = std::mem::zeroed();
    if GetIconInfo(icon, &mut info) == 0 {
        return None;
    }
    let result = if info.hbmColor.is_null() {
        None // monochrome icon; not worth supporting
    } else {
        read_color_bitmap(info.hbmColor, info.hbmMask)
    };
    if !info.hbmColor.is_null() {
        DeleteObject(info.hbmColor as _);
    }
    if !info.hbmMask.is_null() {
        DeleteObject(info.hbmMask as _);
    }
    result
}

unsafe fn read_bits(bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut header: BITMAPINFO = std::mem::zeroed();
    header.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // top-down rows
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..std::mem::zeroed()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let dc = GetDC(ptr::null_mut());
    let lines = GetDIBits(
        dc,
        bitmap,
        0,
        height as u32,
        pixels.as_mut_ptr() as _,
        &mut header,
        DIB_RGB_COLORS,
    );
    ReleaseDC(ptr::null_mut(), dc);
    (lines == height).then_some(pixels)
}

unsafe fn read_color_bitmap(color: HBITMAP, mask: HBITMAP) -> Option<RgbaIcon> {
    let mut bitmap: BITMAP = std::mem::zeroed();
    if GetObjectW(
        color as _,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bitmap as *mut _ as _,
    ) == 0
    {
        return None;
    }
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
    let mut pixels = read_bits(color, width, height)?;

    // BGRA -> RGBA
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }

    // Old-style icons carry no alpha channel; take transparency from the mask.
    if pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        let mask_bits = (!mask.is_null())
            .then(|| read_bits(mask, width, height))
            .flatten();
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let transparent = mask_bits.as_ref().is_some_and(|m| m[i * 4] != 0);
            px[3] = if transparent { 0 } else { 255 };
        }
    }

    Some(RgbaIcon {
        rgba: pixels,
        width: width as u32,
        height: height as u32,
    })
}
