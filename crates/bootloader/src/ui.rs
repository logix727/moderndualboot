//! Modern GOP double-buffered boot menu renderer.

use alloc::format;
use alloc::vec::Vec;
use modernboot_shared::BootConfig;
use uefi::boot::{self, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput, PixelFormat};

use crate::canvas::Canvas;
use crate::font;
use crate::icons;
use crate::scanner::ResolvedEntry;

/// Bounding rectangle of a rendered OS card for mouse hit-testing.
#[derive(Debug, Clone, Copy)]
pub struct CardRect {
    pub x: isize,
    pub y: isize,
    pub w: usize,
    pub h: usize,
}

impl CardRect {
    pub fn contains(&self, px: isize, py: isize) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.w as isize
            && py < self.y + self.h as isize
    }
}

/// Double-buffered UEFI GOP UI renderer.
pub struct BootUi {
    gop: ScopedProtocol<GraphicsOutput>,
    canvas: Canvas,
    stride: usize,
    pixel_format: PixelFormat,
    card_rects: Vec<CardRect>,
}

impl BootUi {
    /// Initializes UEFI `GraphicsOutput` and allocates the offscreen `Vec<u32>`
    /// double-buffer matching the active mode resolution and pixel format.
    pub fn new() -> Result<Self, &'static str> {
        let handle = boot::get_handle_for_protocol::<GraphicsOutput>()
            .map_err(|_| "GraphicsOutput protocol not found")?;
        let gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle)
            .map_err(|_| "Failed to open GraphicsOutput protocol")?;

        let mode_info = gop.current_mode_info();
        let (width, height) = mode_info.resolution();
        let stride = mode_info.stride();
        let pixel_format = mode_info.pixel_format();

        let canvas = Canvas::new(width, height, pixel_format);

        Ok(Self {
            gop,
            canvas,
            stride,
            pixel_format,
            card_rects: Vec::new(),
        })
    }

    pub fn resolution(&self) -> (usize, usize) {
        (self.canvas.width, self.canvas.height)
    }

    /// Returns the index of the OS card under `(x, y)`, if any.
    pub fn hit_test_card(&self, x: isize, y: isize) -> Option<usize> {
        self.card_rects.iter().position(|r| r.contains(x, y))
    }

    /// Renders the entire boot menu into the offscreen buffer and presents it to GOP.
    pub fn render(
        &mut self,
        config: &BootConfig,
        entries: &[ResolvedEntry],
        selected: usize,
        countdown_ms: Option<u64>,
        total_timeout_ms: u64,
        error_msg: Option<&str>,
        mouse_pos: Option<(isize, isize)>,
    ) {
        let colors = config.theme.effective_colors();
        let w = self.canvas.width;
        let h = self.canvas.height;
        let scale = if w >= 1920 { 2 } else { 1 };
        let title_scale = scale + 1;

        // 1. Vertical gradient background
        self.canvas
            .fill_vertical_gradient(colors.bg_top, colors.bg_bottom);

        // 2. Header
        let header_y = (h / 12).max(24) as isize;
        let center_x = (w / 2) as isize;
        self.canvas
            .draw_text_centered(center_x, header_y, "ModernBoot", title_scale, colors.text_primary);

        let subtitle_y = header_y + (8 * title_scale) as isize + (8 * scale) as isize;
        let subtitle = if let Some(rem_ms) = countdown_ms {
            let secs = (rem_ms + 999) / 1000;
            format!("Booting selected OS in {secs}s  (press any key to cancel)")
        } else {
            String::from("UEFI Boot Manager")
        };
        self.canvas.draw_text_centered(
            center_x,
            subtitle_y,
            &subtitle,
            scale,
            colors.text_secondary,
        );

        // Countdown progress bar
        if config.theme.show_clock_or_timer && total_timeout_ms > 0 {
            if let Some(rem_ms) = countdown_ms {
                let bar_w = (w / 3).clamp(220, 480);
                let bar_h = (4 * scale).max(4);
                let bar_x = center_x - (bar_w as isize) / 2;
                let bar_y = subtitle_y + (8 * scale) as isize + (10 * scale) as isize;
                self.canvas.fill_rounded_rect(
                    bar_x,
                    bar_y,
                    bar_w,
                    bar_h,
                    bar_h / 2,
                    colors.card_border,
                );
                let fill_w = ((bar_w as u64 * rem_ms) / total_timeout_ms) as usize;
                if fill_w > 0 {
                    self.canvas.fill_rounded_rect(
                        bar_x,
                        bar_y,
                        fill_w.min(bar_w),
                        bar_h,
                        bar_h / 2,
                        colors.accent,
                    );
                }
            }
        }

        // 3. OS Entry Cards (horizontal row or wrapped grid)
        self.card_rects.clear();
        let count = entries.len();
        if count == 0 {
            self.canvas.draw_text_centered(
                center_x,
                (h / 2) as isize,
                "No bootable OS entries found.",
                scale,
                colors.text_secondary,
            );
        } else {
            let max_cols = if w >= 1280 { 4 } else { 3 };
            let cols = count.min(max_cols);
            let rows = (count + cols - 1) / cols;

            let gap = (20 * scale).max(16);
            let card_w = ((w * 78 / 100).saturating_sub(gap * (cols - 1)) / cols).clamp(150, 260);
            let card_h = (if rows > 1 { 170 } else { 210 } * scale).min(h * 45 / 100);
            let total_grid_h = rows * card_h + (rows - 1) * gap;
            let start_y = ((h / 2) as isize - (total_grid_h as isize) / 2).max(subtitle_y + 36);
            let corner_r = (config.theme.corner_radius as usize) * scale;

            for (idx, resolved) in entries.iter().enumerate() {
                let row = idx / cols;
                let col = idx % cols;
                let items_in_row = if row == rows - 1 {
                    count - row * cols
                } else {
                    cols
                };
                let row_w = items_in_row * card_w + (items_in_row - 1) * gap;
                let row_start_x = center_x - (row_w as isize) / 2;
                let cx = row_start_x + (col * (card_w + gap)) as isize;
                let cy = start_y + (row * (card_h + gap)) as isize;

                self.card_rects.push(CardRect {
                    x: cx,
                    y: cy,
                    w: card_w,
                    h: card_h,
                });

                let is_selected = idx == selected;
                let bg = if is_selected {
                    colors.card_selected_bg
                } else {
                    colors.card_bg
                };
                self.canvas
                    .fill_rounded_rect(cx, cy, card_w, card_h, corner_r, bg);

                let border_color = if is_selected {
                    colors.accent
                } else {
                    colors.card_border
                };
                let border_thick = if is_selected { 2 * scale } else { 1 };
                self.canvas.stroke_rounded_rect(
                    cx,
                    cy,
                    card_w,
                    card_h,
                    corner_r,
                    border_thick,
                    border_color,
                );

                // Quick-boot number badge (1..9) in top-left of card
                if idx < 9 {
                    let num_str = format!("{}", idx + 1);
                    self.canvas.draw_text(
                        cx + (12 * scale) as isize,
                        cy + (10 * scale) as isize,
                        &num_str,
                        scale,
                        if is_selected {
                            colors.accent
                        } else {
                            colors.text_secondary
                        },
                    );
                }

                // OS Icon
                let icon_size = (card_h * 42 / 100) as isize;
                let icon_cx = cx + (card_w as isize) / 2;
                let icon_cy = cy + (card_h * 38 / 100) as isize;
                icons::draw_os_icon(
                    &mut self.canvas,
                    resolved.entry.os_kind,
                    resolved.entry.icon_override.as_deref(),
                    icon_cx,
                    icon_cy,
                    icon_size,
                    colors.accent,
                );

                // Label (truncated to fit card width)
                let max_chars = (card_w.saturating_sub(16 * scale)) / (8 * scale);
                let label = truncate_label(&resolved.entry.label, max_chars.max(4));
                let label_y = cy + (card_h * 68 / 100) as isize;
                self.canvas.draw_text_centered(
                    icon_cx,
                    label_y,
                    &label,
                    scale,
                    colors.text_primary,
                );

                // OS badge pill
                let badge = if resolved.is_firmware_setup {
                    "BIOS"
                } else {
                    resolved.entry.os_kind.default_badge()
                };
                let badge_w = font::measure_text_width(badge, scale) + 12 * scale;
                let badge_h = 8 * scale + 6 * scale;
                let badge_x = icon_cx - (badge_w as isize) / 2;
                let badge_y = cy + (card_h * 82 / 100) as isize;
                self.canvas.fill_rounded_rect(
                    badge_x,
                    badge_y,
                    badge_w,
                    badge_h,
                    badge_h / 2,
                    colors.badge_bg,
                );
                self.canvas.draw_text_centered(
                    icon_cx,
                    badge_y + (3 * scale) as isize,
                    badge,
                    scale,
                    if is_selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    },
                );
            }
        }

        // 4. Error banner (if chainload failed)
        let footer_y = (h as isize) - (32 * scale as isize);
        if let Some(err) = error_msg {
            let banner_w = (font::measure_text_width(err, scale) + 32 * scale).min(w * 90 / 100);
            let banner_h = 8 * scale + 16 * scale;
            let bx = center_x - (banner_w as isize) / 2;
            let by = footer_y - (banner_h as isize) - (16 * scale as isize);
            self.canvas
                .fill_rounded_rect(bx, by, banner_w, banner_h, 6 * scale, [127, 29, 29]);
            self.canvas.stroke_rounded_rect(
                bx,
                by,
                banner_w,
                banner_h,
                6 * scale,
                1,
                [248, 113, 113],
            );
            self.canvas.draw_text_centered(
                center_x,
                by + (8 * scale) as isize,
                err,
                scale,
                [254, 226, 226],
            );
        }

        // 5. Footer hints
        self.canvas.draw_text_centered(
            center_x,
            footer_y,
            "[<-/->] Select  [Enter] Boot  [F1] Firmware Setup  [Esc] Cancel",
            scale,
            colors.text_secondary,
        );

        // 6. Optional mouse cursor
        if let Some((mx, my)) = mouse_pos {
            draw_cursor(&mut self.canvas, mx, my, colors.accent);
        }

        self.present();
    }

    fn present(&mut self) {
        let w = self.canvas.width;
        let h = self.canvas.height;

        match self.pixel_format {
            PixelFormat::Rgb | PixelFormat::Bgr => {
                let mut fb = self.gop.frame_buffer();
                let stride = self.stride;
                for y in 0..h {
                    let src_row = &self.canvas.buffer[y * w..(y + 1) * w];
                    let dst_byte_offset = y * stride * 4;
                    // Safety: `src_row` is `w` u32s (4 * w bytes), within bounds of the active GOP framebuffer.
                    unsafe {
                        let byte_slice = core::slice::from_raw_parts(
                            src_row.as_ptr().cast::<u8>(),
                            w * 4,
                        );
                        fb.write_value(dst_byte_offset, byte_slice);
                    }
                }
            }
            _ => {
                // Fallback for BitMask / BltOnly modes using BltBuffer
                let blt_buf: Vec<BltPixel> = self
                    .canvas
                    .buffer
                    .iter()
                    .map(|&px| {
                        let b = (px & 0xFF) as u8;
                        let g = ((px >> 8) & 0xFF) as u8;
                        let r = ((px >> 16) & 0xFF) as u8;
                        BltPixel::new(r, g, b)
                    })
                    .collect();
                let _ = self.gop.blt(BltOp::BufferToVideo {
                    buffer: &blt_buf,
                    src: BltRegion::Full,
                    dest: (0, 0),
                    dims: (w, h),
                });
            }
        }
    }
}

use alloc::string::String;

fn truncate_label(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return String::from(s);
    }
    let take = max_chars.saturating_sub(2);
    let mut out: String = s.chars().take(take).collect();
    out.push_str("..");
    out
}

fn draw_cursor(c: &mut Canvas, x: isize, y: isize, accent: [u8; 3]) {
    for dy in 0..14isize {
        let span = if dy < 10 { dy + 1 } else { 14 - dy };
        for dx in 0..span {
            let color = if dx == 0 || dy == 0 || dx == span - 1 {
                [15, 23, 42]
            } else if dx < 3 {
                accent
            } else {
                [248, 250, 252]
            };
            c.put_pixel(x + dx, y + dy, color);
        }
    }
}
