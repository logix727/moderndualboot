//! Offscreen 32-bit ARGB/ABGR software framebuffer canvas.

use alloc::vec;
use alloc::vec::Vec;
use uefi::proto::console::gop::PixelFormat;

use crate::font;

/// Software rendering canvas backed by a `Vec<u32>` pixel buffer formatted
/// to match the active GOP [`PixelFormat`].
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
    pub buffer: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, format: PixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            buffer: vec![0u32; width * height],
        }
    }

    #[inline]
    pub fn pack(&self, rgb: [u8; 3]) -> u32 {
        match self.format {
            PixelFormat::Rgb => {
                (u32::from(rgb[0])) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[2]) << 16)
            }
            // Bgr (and BitMask/BltOnly default to standard UEFI BltPixel B,G,R,0 layout)
            _ => (u32::from(rgb[2])) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[0]) << 16),
        }
    }

    #[inline]
    pub fn put_pixel(&mut self, x: isize, y: isize, rgb: [u8; 3]) {
        if x < 0 || y < 0 {
            return;
        }
        let (ux, uy) = (x as usize, y as usize);
        if ux < self.width && uy < self.height {
            self.buffer[uy * self.width + ux] = self.pack(rgb);
        }
    }

    pub fn fill_vertical_gradient(&mut self, top: [u8; 3], bottom: [u8; 3]) {
        let h = self.height.max(1);
        for y in 0..self.height {
            let t = y as u32;
            let denom = (h - 1).max(1) as u32;
            let r = lerp_u8(top[0], bottom[0], t, denom);
            let g = lerp_u8(top[1], bottom[1], t, denom);
            let b = lerp_u8(top[2], bottom[2], t, denom);
            let packed = self.pack([r, g, b]);
            let row_start = y * self.width;
            self.buffer[row_start..row_start + self.width].fill(packed);
        }
    }

    pub fn fill_rect(&mut self, x: isize, y: isize, w: usize, h: usize, rgb: [u8; 3]) {
        let packed = self.pack(rgb);
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w as isize).min(self.width as isize).max(0) as usize;
        let y1 = (y + h as isize).min(self.height as isize).max(0) as usize;
        for row in y0..y1 {
            let start = row * self.width + x0;
            let end = row * self.width + x1;
            if start < end {
                self.buffer[start..end].fill(packed);
            }
        }
    }

    pub fn fill_rounded_rect(
        &mut self,
        x: isize,
        y: isize,
        w: usize,
        h: usize,
        radius: usize,
        rgb: [u8; 3],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let r = radius.min(w / 2).min(h / 2) as isize;
        let r2 = r * r;
        let wi = w as isize;
        let hi = h as isize;

        for dy in 0..hi {
            for dx in 0..wi {
                let in_corner = if dx < r && dy < r {
                    let cx = r - 1 - dx;
                    let cy = r - 1 - dy;
                    cx * cx + cy * cy > r2
                } else if dx >= wi - r && dy < r {
                    let cx = dx - (wi - r);
                    let cy = r - 1 - dy;
                    cx * cx + cy * cy > r2
                } else if dx < r && dy >= hi - r {
                    let cx = r - 1 - dx;
                    let cy = dy - (hi - r);
                    cx * cx + cy * cy > r2
                } else if dx >= wi - r && dy >= hi - r {
                    let cx = dx - (wi - r);
                    let cy = dy - (hi - r);
                    cx * cx + cy * cy > r2
                } else {
                    false
                };

                if !in_corner {
                    self.put_pixel(x + dx, y + dy, rgb);
                }
            }
        }
    }

    pub fn stroke_rounded_rect(
        &mut self,
        x: isize,
        y: isize,
        w: usize,
        h: usize,
        radius: usize,
        thickness: usize,
        rgb: [u8; 3],
    ) {
        if w == 0 || h == 0 || thickness == 0 {
            return;
        }
        let r = radius.min(w / 2).min(h / 2) as isize;
        let r_outer2 = r * r;
        let r_inner = (r - thickness as isize).max(0);
        let r_inner2 = r_inner * r_inner;
        let wi = w as isize;
        let hi = h as isize;
        let t = thickness as isize;

        for dy in 0..hi {
            for dx in 0..wi {
                let is_edge_band = dx < t || dx >= wi - t || dy < t || dy >= hi - t;
                let in_corner_outer = if dx < r && dy < r {
                    let cx = r - 1 - dx;
                    let cy = r - 1 - dy;
                    let d2 = cx * cx + cy * cy;
                    Some(d2 <= r_outer2 && d2 >= r_inner2)
                } else if dx >= wi - r && dy < r {
                    let cx = dx - (wi - r);
                    let cy = r - 1 - dy;
                    let d2 = cx * cx + cy * cy;
                    Some(d2 <= r_outer2 && d2 >= r_inner2)
                } else if dx < r && dy >= hi - r {
                    let cx = r - 1 - dx;
                    let cy = dy - (hi - r);
                    let d2 = cx * cx + cy * cy;
                    Some(d2 <= r_outer2 && d2 >= r_inner2)
                } else if dx >= wi - r && dy >= hi - r {
                    let cx = dx - (wi - r);
                    let cy = dy - (hi - r);
                    let d2 = cx * cx + cy * cy;
                    Some(d2 <= r_outer2 && d2 >= r_inner2)
                } else {
                    None
                };

                let draw = match in_corner_outer {
                    Some(on_arc) => on_arc,
                    None => is_edge_band,
                };
                if draw {
                    self.put_pixel(x + dx, y + dy, rgb);
                }
            }
        }
    }

    pub fn fill_circle(&mut self, cx: isize, cy: isize, radius: isize, rgb: [u8; 3]) {
        if radius <= 0 {
            return;
        }
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.put_pixel(cx + dx, cy + dy, rgb);
                }
            }
        }
    }

    pub fn fill_ring(
        &mut self,
        cx: isize,
        cy: isize,
        outer_r: isize,
        inner_r: isize,
        rgb: [u8; 3],
    ) {
        let o2 = outer_r * outer_r;
        let i2 = inner_r * inner_r;
        for dy in -outer_r..=outer_r {
            for dx in -outer_r..=outer_r {
                let d2 = dx * dx + dy * dy;
                if d2 <= o2 && d2 >= i2 {
                    self.put_pixel(cx + dx, cy + dy, rgb);
                }
            }
        }
    }

    pub fn draw_text(&mut self, x: isize, y: isize, text: &str, scale: usize, rgb: [u8; 3]) {
        let mut cursor_x = x;
        let step = (8 * scale) as isize;
        for ch in text.chars() {
            let glyph = font::glyph_8x8(ch);
            for (row, &bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if (bits & (0x80 >> col)) != 0 {
                        self.fill_rect(
                            cursor_x + (col * scale) as isize,
                            y + (row * scale) as isize,
                            scale,
                            scale,
                            rgb,
                        );
                    }
                }
            }
            cursor_x += step;
        }
    }

    pub fn draw_text_centered(
        &mut self,
        center_x: isize,
        y: isize,
        text: &str,
        scale: usize,
        rgb: [u8; 3],
    ) {
        let w = font::measure_text_width(text, scale) as isize;
        self.draw_text(center_x - w / 2, y, text, scale, rgb);
    }
}

#[inline]
fn lerp_u8(a: u8, b: u8, num: u32, den: u32) -> u8 {
    let a = a as i32;
    let b = b as i32;
    (a + ((b - a) * num as i32) / den as i32).clamp(0, 255) as u8
}
