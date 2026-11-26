//! ModernDualBoot UEFI Boot Manager (`modernboot-efi`).

#![no_std]
#![no_main]

extern crate alloc;

mod canvas;
mod chainload;
mod config;
mod font;
mod icons;
mod scanner;
mod ui;

use uefi::boot::{self, SearchType};
use uefi::proto::console::pointer::Pointer;
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::{entry, Identify, Status};

use crate::ui::BootUi;

const TICK_US: usize = 25_000; // 25 ms per poll tick (40 Hz)
const TICK_MS: u64 = 25;

#[entry]
fn main() -> Status {
    let config = config::load_config();
    let entries = scanner::discover_entries(&config);

    // Determine initial selection from `config.default_entry_id`.
    let mut selected: usize = config
        .default_entry_id
        .as_deref()
        .and_then(|id| entries.iter().position(|e| e.entry.id == id))
        .unwrap_or(0);

    let mut ui = match BootUi::new() {
        Ok(ui) => ui,
        Err(_) => {
            // Headless / no GOP fallback: boot default entry directly if available.
            if let Some(entry) = entries.get(selected) {
                let _ = chainload::boot_entry(entry);
            }
            return Status::LOAD_ERROR;
        }
    };

    let (screen_w, screen_h) = ui.resolution();

    // Open keyboard input protocol.
    let mut input = boot::get_handle_for_protocol::<Input>()
        .ok()
        .and_then(|h| boot::open_protocol_exclusive::<Input>(h).ok());

    // Open optional SimplePointer mouse protocol.
    let mut pointer = boot::locate_handle_buffer(SearchType::ByProtocol(&Pointer::GUID))
        .ok()
        .and_then(|buf| buf.first().copied())
        .and_then(|h| boot::open_protocol_exclusive::<Pointer>(h).ok());

    let total_timeout_ms = u64::from(config.timeout_seconds) * 1000;
    let mut countdown_ms: Option<u64> = if total_timeout_ms > 0 && !entries.is_empty() {
        Some(total_timeout_ms)
    } else {
        None
    };

    let mut error_msg: Option<&'static str> = None;
    let mut mouse_pos: Option<(isize, isize)> = None;
    let mut mouse_x: isize = (screen_w / 2) as isize;
    let mut mouse_y: isize = (screen_h / 2) as isize;
    let mut prev_left_btn = false;
    let mut dirty = true;
    let mut last_displayed_sec: Option<u64> = countdown_ms.map(|ms| (ms + 999) / 1000);

    loop {
        if dirty {
            ui.render(
                &config,
                &entries,
                selected,
                countdown_ms,
                total_timeout_ms,
                error_msg,
                mouse_pos,
            );
            dirty = false;
        }

        // 1. Poll keyboard
        if let Some(inp) = input.as_mut() {
            if let Ok(Some(key)) = inp.read_key() {
                // Any keypress cancels the auto-boot countdown and clears errors.
                countdown_ms = None;
                error_msg = None;
                dirty = true;

                match key {
                    Key::Special(ScanCode::LEFT) | Key::Special(ScanCode::UP) => {
                        if !entries.is_empty() {
                            selected = if selected == 0 {
                                entries.len() - 1
                            } else {
                                selected - 1
                            };
                        }
                    }
                    Key::Special(ScanCode::RIGHT) | Key::Special(ScanCode::DOWN) => {
                        if !entries.is_empty() {
                            selected = (selected + 1) % entries.len();
                        }
                    }
                    Key::Special(ScanCode::FUNCTION_1) => {
                        if let Err(msg) = chainload::reboot_to_firmware_setup() {
                            error_msg = Some(msg);
                        }
                    }
                    Key::Special(ScanCode::ESCAPE) => {
                        return Status::SUCCESS;
                    }
                    Key::Printable(ch) => {
                        let c = char::from(ch);
                        if c == '\r' || c == '\n' {
                            if let Some(entry) = entries.get(selected) {
                                if let Err(msg) = chainload::boot_entry(entry) {
                                    error_msg = Some(msg);
                                }
                            }
                        } else if ('1'..='9').contains(&c) {
                            let idx = (c as usize) - ('1' as usize);
                            if let Some(entry) = entries.get(idx) {
                                selected = idx;
                                if let Err(msg) = chainload::boot_entry(entry) {
                                    error_msg = Some(msg);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 2. Poll optional SimplePointer mouse
        if let Some(ptr_proto) = pointer.as_mut() {
            if let Ok(Some(state)) = ptr_proto.read_state() {
                let dx = (state.relative_movement[0] / 4096) as isize;
                let dy = (state.relative_movement[1] / 4096) as isize;
                if dx != 0 || dy != 0 {
                    mouse_x = (mouse_x + dx).clamp(0, (screen_w as isize) - 1);
                    mouse_y = (mouse_y + dy).clamp(0, (screen_h as isize) - 1);
                    mouse_pos = Some((mouse_x, mouse_y));
                    countdown_ms = None;
                    dirty = true;

                    if let Some(hovered) = ui.hit_test_card(mouse_x, mouse_y) {
                        selected = hovered;
                    }
                }

                let left_btn = state.button[0];
                if left_btn && !prev_left_btn {
                    countdown_ms = None;
                    error_msg = None;
                    dirty = true;
                    if let Some(clicked) = ui.hit_test_card(mouse_x, mouse_y) {
                        selected = clicked;
                        if let Some(entry) = entries.get(clicked) {
                            if let Err(msg) = chainload::boot_entry(entry) {
                                error_msg = Some(msg);
                            }
                        }
                    }
                }
                prev_left_btn = left_btn;
            }
        }

        // 3. Advance countdown timer
        if let Some(rem) = countdown_ms.as_mut() {
            if *rem <= TICK_MS {
                countdown_ms = None;
                if let Some(entry) = entries.get(selected) {
                    if let Err(msg) = chainload::boot_entry(entry) {
                        error_msg = Some(msg);
                        dirty = true;
                    }
                }
            } else {
                *rem -= TICK_MS;
                let cur_sec = (*rem + 999) / 1000;
                // Redraw smoothly every ~100ms (4 ticks) or on second change for smooth bar
                if last_displayed_sec != Some(cur_sec) || (*rem % 100) < TICK_MS {
                    last_displayed_sec = Some(cur_sec);
                    dirty = true;
                }
            }
        }

        boot::stall(TICK_US);
    }
}
