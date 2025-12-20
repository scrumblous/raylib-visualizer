use std::ffi::CString;
use windows::core::PCSTR;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{CreatePen, DeleteObject, GetDC, GetStockObject, InvalidateRect, Rectangle, ReleaseDC, SelectObject, SetROP2, NULL_BRUSH, PS_SOLID, R2_COPYPEN, R2_XORPEN};
use windows::Win32::UI::WindowsAndMessaging::*;

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}
pub fn erase_and_draw_rectangle(
    old_rect: ((i32, i32), (i32, i32)),
    new_pos: (i32, i32),
    new_size: (i32, i32)
) -> ((i32, i32), (i32, i32)) {
    unsafe {
        let hdc = GetDC(None);
        SetROP2(hdc, R2_XORPEN);
        let pen = CreatePen(PS_SOLID, 1, COLORREF(rgb(255, 0, 0)));
        let old_pen = SelectObject(hdc, pen);
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));

        Rectangle(hdc, old_rect.0.0, old_rect.0.1, old_rect.1.0, old_rect.1.1); // XOR's the older rectangle

        Rectangle(hdc, new_pos.0, new_pos.1, new_pos.0 + new_size.0, new_pos.1 + new_size.1); // redraws it at a new pos

        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(pen);
        SetROP2(hdc, R2_COPYPEN);
        ReleaseDC(None, hdc);

        ((new_pos.0, new_pos.1), (new_pos.0 + new_size.0, new_pos.1 + new_size.1))
    }
}

pub fn update_screen(rect: RECT) {
    unsafe {
        let raw_string = CString::new("not a class").unwrap();
        let constu8_string = raw_string.as_ptr().cast::<u8>();
        let null_window = FindWindowA(PCSTR(constu8_string), PCSTR::null());
        let hdc = GetDC(null_window);
        InvalidateRect(
            null_window,
            Some(&rect),
            true
        );
        ReleaseDC(None, hdc);
    }
}