mod wasapi_audio;
mod fft;
mod loudness_calc;
mod draw_on_monitor;

use raylib::prelude::*;
use raylib_sys::{DrawTextEx, LoadFontFromMemory, SetWindowState};
use std::ffi::{c_uint, CString};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use hide_console::hide_console;
use serde_json::{json};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::RECT;
use crate::loudness_calc::{calculate_time_domain_loudness, calculate_weighted_loudness};
use crate::wasapi_audio::start_desktop_audio_capture;
use crate::draw_on_monitor::{erase_and_draw_rectangle, update_screen};

type SharedBuffer = Arc<Mutex<Vec<f32>>>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
struct Settings {
    fps_toggle: bool,
    circle_toggle: bool,
    ffi_enabled: bool,
    opacity: u8,
    size: (i32, i32),
    notif_closed: bool,
}

impl Settings {
    fn new(fps_toggle: bool, circle_toggle: bool, ffi_enabled: bool, opacity: u8, size: (i32, i32), notif_closed: bool) -> Self {
        Self {fps_toggle, circle_toggle, ffi_enabled, opacity, size, notif_closed}
    }
}

fn listen_to_other_keybindings(rl: &mut RaylibHandle, settings: &mut Settings, size_after_resize: (i32, i32)) {
    if rl.is_key_pressed(KeyboardKey::KEY_G) {
        settings.circle_toggle = !settings.circle_toggle;
    } else if rl.is_key_pressed(KeyboardKey::KEY_H) {
        settings.fps_toggle = !settings.fps_toggle;
    } else if rl.is_key_pressed(KeyboardKey::KEY_EQUAL) || rl.is_key_pressed(KeyboardKey::KEY_MINUS) {
        if rl.is_key_pressed(KeyboardKey::KEY_MINUS) {
            if settings.opacity >= 5 {
                settings.opacity -= 5;
            }
        } else {
            if settings.opacity <= 250 {
                settings.opacity += 5;
            }
        }
    } else if rl.is_key_pressed(KeyboardKey::KEY_BACKSPACE) {
        if !rl.is_window_minimized() {
            unsafe {
                SetWindowState(ConfigFlags::FLAG_WINDOW_MINIMIZED as c_uint);
            }
        } else {
            unsafe {
                SetWindowState(ConfigFlags::FLAG_WINDOW_MAXIMIZED as c_uint);
            }
        }
    } else if rl.is_key_pressed(KeyboardKey::KEY_F11) {
        let current_mon = get_current_monitor();
        let (mon_x, mon_y) = (get_monitor_width(current_mon), get_monitor_height(current_mon));
        if settings.size.0 < mon_x && settings.size.1 < mon_y {
            rl.set_window_size(mon_x, mon_y);
        } else {
            rl.set_window_size(size_after_resize.0, size_after_resize.1);
        }
    } else if rl.is_key_pressed(KeyboardKey::KEY_B) {
        settings.notif_closed = false;
    }
}

fn main() {
    hide_console();
    let app_data_path = std::env::var("APPDATA".to_string()).unwrap();
    let config_path = String::from(app_data_path + "\\visualizer.json");
    let last_config_file = std::fs::File::open(config_path.clone());
    let mut config_file_handle;
    match last_config_file {
        Ok(contents) => {config_file_handle = contents;},
        Err(_) => {config_file_handle = std::fs::File::create(config_path.clone()).unwrap();}
    }
    let mut settings = Settings::new(
        true,
        true,
        false,
        255,
        (1024, 1000),
        false
    );

    if std::fs::metadata(config_path.clone()).unwrap().file_size() > 0 {
        let mut contents_string: String = "".to_string();
        config_file_handle.read_to_string(&mut contents_string).unwrap();
        let json_contents: Settings = serde_json::from_str(contents_string.as_str()).unwrap();
        settings = json_contents;
    } else {
        let json_new_config = json!(settings);
        let json_text_content = serde_json::to_string(&json_new_config).unwrap();
        config_file_handle.write_all(json_text_content.as_bytes()).unwrap();
    }
    drop(config_file_handle);

    let data: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(2048)));
    let clone = data.clone();
    let ffi_enabled = Arc::new(Mutex::new(settings.ffi_enabled));
    let ffi_enabled_clone_1 = ffi_enabled.clone();
    std::thread::spawn(move || {
        start_desktop_audio_capture(clone, ffi_enabled_clone_1).unwrap();
    });


    let (mut rl, thread) = init()
        .transparent()
        .resizable()
        .vsync()
        .size(settings.size.0,settings.size.1)
        .title("visualizer")
        .log_level(TraceLogLevel::LOG_ERROR)
        .undecorated()
        .build();
    let cat_icon = include_bytes!("cat.png");
    rl.set_window_icon(Image::load_image_from_mem(".png", cat_icon).unwrap());

    unsafe {
        SetWindowState(ConfigFlags::FLAG_WINDOW_TOPMOST as i32 as c_uint);
    }
    let font: ffi::Font;
    let papyrus_font = include_bytes!("confession.ttf");
    unsafe {
        let file_type = CString::new(".ttf").unwrap();
        let font_size = papyrus_font.len();
        font = LoadFontFromMemory(
            file_type.as_ptr(),
            papyrus_font.as_ptr(),
            font_size.try_into().unwrap(),
            256,
            null_mut(),
            100,
        )
    }

    let mut hue = 0.0;
    let mut hue_change = 0.3;
    let mut last_data = vec![0.0_f32; 1536];

    let mut last_mouse_pos = Vector2 { x: 0.0, y: 0.0 };
    let (min_width, min_height) = (200, 200);
    rl.set_exit_key(None);

    let mut size_after_resize = (500, 500);

    let mut first_frame = true;
    let mut frame_counter: i64 = 0;
    let mut required_to_be_in_pos = true;
    let mut required_to_be_in_pos_resize = true;
    let mut need_to_move = false;
    //let mut needs_to_resize = false;
    let mut old_rectangle = ((0, 0), (0, 0));
    while !rl.window_should_close() {
        frame_counter += 1;
        if frame_counter > 10000 {
            frame_counter = 0;
        }
        if frame_counter % 100 == 0 {
            let current_monitor = get_current_monitor();
            let target_fps = get_monitor_refresh_rate(current_monitor);
            rl.set_target_fps((target_fps as f32 * 0.9) as u32);
        }
        let ffi_enabled_clone_2 = ffi_enabled.clone();
        if let Ok(guard) = ffi_enabled_clone_2.try_lock() {
            settings.ffi_enabled = *guard;
            drop(guard);
        }

        let (screen_x, screen_y) = (rl.get_screen_width(), rl.get_screen_height());

        if rl.is_key_pressed(KeyboardKey::KEY_F) {
            if let Ok(mut guard) = ffi_enabled_clone_2.try_lock() {
                *guard = !settings.ffi_enabled;
                drop(guard);
            }
        } else if rl.is_key_pressed(KeyboardKey::KEY_ESCAPE) {
            break
        }

        listen_to_other_keybindings(&mut rl, &mut settings, size_after_resize);

        let (win_x, win_y) = (rl.get_window_position().x as i32, rl.get_window_position().y as i32);
        if rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_LEFT) {
            let is_allowed_width = screen_x > min_width;
            let is_allowed_height = screen_y > min_height;
            let is_mouse_on_top = rl.get_mouse_y() < ((screen_y as f32 * 0.1) as i32).max(50);
            let is_mouse_on_bottom = rl.get_mouse_y() > ((screen_y as f32 * 0.9) as i32).max(screen_y - 50);
            let is_mouse_on_left = rl.get_mouse_x() < (screen_x as f32 * 0.5) as i32;
            let is_mouse_in_corner = (rl.get_mouse_x() < ((screen_x as f32 * 0.1) as i32).max(50)) || (rl.get_mouse_x() > ((screen_x as f32 * 0.9) as i32).max(screen_x - 50));
            if (is_mouse_on_top && !is_mouse_in_corner && required_to_be_in_pos_resize) || (!required_to_be_in_pos) {
                required_to_be_in_pos = false;
                old_rectangle = erase_and_draw_rectangle(
                    old_rectangle,
                    (win_x + rl.get_mouse_x() - last_mouse_pos.x as i32,
                     win_y + rl.get_mouse_y() - last_mouse_pos.y as i32),
                    (screen_x, screen_y)
                );
                need_to_move = true;
            }
            if (is_mouse_in_corner && is_mouse_on_bottom && !need_to_move) || (!required_to_be_in_pos_resize) {
                if !is_mouse_on_left {
                    required_to_be_in_pos_resize = false;
                    let resized_x = screen_x + rl.get_mouse_delta().x as i32;
                    let resized_y = screen_y + rl.get_mouse_delta().y as i32;
                    rl.set_window_size(resized_x, resized_y);
                    size_after_resize = (resized_x, resized_y);
                    if !is_allowed_width {
                        rl.set_window_size(min_width + 50, screen_y);
                    }
                    if !is_allowed_height {
                        rl.set_window_size(screen_x, min_height + 50);
                    }
                }
             }
        } else {
            if need_to_move {
                rl.set_window_position(
                    win_x + rl.get_mouse_x() - last_mouse_pos.x as i32,
                    win_y + rl.get_mouse_y() - last_mouse_pos.y as i32
                );
                need_to_move = false;
                update_screen(RECT {
                    left: old_rectangle.0.0,
                    top: old_rectangle.0.1,
                    right: old_rectangle.1.0,
                    bottom: old_rectangle.1.1,
                })
            }
            required_to_be_in_pos = true;
            required_to_be_in_pos_resize = true;
            last_mouse_pos = rl.get_mouse_position();
            settings.size = (screen_x, screen_y);
            if first_frame {
                size_after_resize = (screen_x, screen_y);
            }
        }


        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::new(0, 0, 10, settings.opacity));


        if settings.fps_toggle {
            d.draw_fps(0, 0);
        }
        if hue > 360.0 || hue < 0.0 {
            hue_change *= -1.0
        }
        hue += hue_change;
        let vol: f32;
        if settings.ffi_enabled {
            vol = (calculate_weighted_loudness(&last_data, 48000) * 10.0).max(0.0);
        } else {
            vol = (calculate_time_domain_loudness(&last_data) * 100.0).max(0.0);
        }
        if settings.circle_toggle {
            d.draw_circle(
                (screen_x as f32 / 2.0) as i32,
                (screen_y as f32 * 0.1) as i32,
                vol.sqrt() * 8.0,
                Color::new(vol as u8, 0, 0, 255)
            );
        }
        if let Ok(guard) = data.try_lock() {
            last_data.clone_from(&*guard);
            drop(guard);
        }
        let mut x = 0.0;
        let mut last_pos = Vector2::new(0.0, screen_y as f32 / 2.0);
        for bin in last_data.iter().take(last_data.len()/1).step_by(2) {
            let x_change = screen_x as f32 / ((last_data.len() as f32) / 2.0);
            if settings.ffi_enabled {
                let height = (bin.sqrt().max(0.0)) * 20.0;
                let bar_color = Color::new(
                    height.min(255.0) as u8,
                    0,
                    (150 - (height as u8).min(150)).max(0),
                    255
                );
                d.draw_line_ex(Vector2::new(x, screen_y as f32), Vector2::new(x, screen_y as f32 - height), x_change, bar_color);
                x += x_change;
            } else {
                let height_change = bin * 250.0;
                let y = ((screen_y as f32 / 2.0) + height_change * (screen_y as f32 * 0.001)).max(0.0).min(screen_y as f32);
                x += x_change;
                let thickness = 2.0;
                let current_pos = Vector2::new(x - x_change - thickness - 2.0, y);
                let color = Color::new(
                    (height_change.abs() * 1.5) as u8,
                    0,
                    (150 - (height_change.abs() as u8).min(150)).max(0),
                    255
                );
                d.draw_line_ex(last_pos, current_pos, thickness, color);
                last_pos = current_pos;
            }
        }
        if !settings.notif_closed {
            let message_box = d.gui_message_box(
                Rectangle::new(50.0, 50.0, 500.0, 500.0),
                "hint",
                "",
                "ok"
            );
            unsafe {
                let instructions: Vec<String> = vec![
                    "press b to show this message again".to_string(),
                    "press f to switch modes".to_string(),
                    "press g to toggle the volume circle".to_string(),
                    "press h to toggle fps".to_string(),
                    "press +/- to change opacity".to_string(),
                    "bottom right corner stretches".to_string(),
                    "top middle part moves".to_string(),
                    "f11 for fullscreen".to_string(),
                    "backspace to hide".to_string(),
                    "ESC to close".to_string(),
                ];
                let text: String = instructions.join("\n");
                let c_str = CString::new(&*text).unwrap();
                DrawTextEx(
                    font, c_str.as_ptr(),
                    raylib_sys::Vector2::from(Vector2::new(55.0, 80.0)),
                    25.0, 1.0, Color::BLACK.into()
                )
            };
            if message_box == 1 || message_box == 0 {
                settings.notif_closed = true;
            }
        }
        first_frame = false;
    //print!("elements: {:?}\r", last_data.len());
    }
    let new_config_file = json!({
        "fps_toggle": settings.fps_toggle,
        "circle_toggle": settings.circle_toggle,
        "opacity": settings.opacity,
        "ffi_enabled": settings.ffi_enabled,
        "size": settings.size,
        "notif_closed": settings.notif_closed,
    });
    {
        let mut closing_config_file = std::fs::File::create(config_path.clone()).unwrap();
        closing_config_file.write_all(serde_json::to_string(&new_config_file).unwrap().as_bytes()).unwrap();
    }
}
