//! Procedural geometric/vector OS icons rendered into [`Canvas`].

use crate::canvas::Canvas;
use modernboot_shared::OsKind;

/// Draws a crisp procedural icon for `os_kind` centered at `(cx, cy)` with total
/// bounding size `size`.
pub fn draw_os_icon(
    canvas: &mut Canvas,
    os_kind: OsKind,
    icon_override: Option<&str>,
    cx: isize,
    cy: isize,
    size: isize,
    accent: [u8; 3],
) {
    if icon_override == Some("firmware") {
        draw_gear_icon(canvas, cx, cy, size, accent);
        return;
    }

    match os_kind {
        OsKind::Windows => draw_windows_icon(canvas, cx, cy, size),
        OsKind::Linux => draw_tux_icon(canvas, cx, cy, size),
        OsKind::Ubuntu => draw_ubuntu_icon(canvas, cx, cy, size),
        OsKind::Fedora => draw_fedora_icon(canvas, cx, cy, size),
        OsKind::Arch => draw_arch_icon(canvas, cx, cy, size),
        OsKind::Debian => draw_debian_icon(canvas, cx, cy, size),
        OsKind::SteamOS => draw_steamos_icon(canvas, cx, cy, size),
        OsKind::MacOS => draw_macos_icon(canvas, cx, cy, size),
        OsKind::UefiShell => draw_gear_icon(canvas, cx, cy, size, accent),
        OsKind::Unknown => draw_generic_os_icon(canvas, cx, cy, size, accent),
    }
}

/// Windows 4-pane logo.
fn draw_windows_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let blue = [0, 164, 239];
    let half = (size * 38) / 100;
    let gap = (size / 14).max(2);
    let pane = (half - gap).max(4) as usize;
    c.fill_rect(cx - gap - pane as isize, cy - gap - pane as isize, pane, pane, blue);
    c.fill_rect(cx + gap, cy - gap - pane as isize, pane, pane, blue);
    c.fill_rect(cx - gap - pane as isize, cy + gap, pane, pane, blue);
    c.fill_rect(cx + gap, cy + gap, pane, pane, blue);
}

/// Stylized Tux penguin badge for generic Linux / UKI.
fn draw_tux_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let r = (size * 42) / 100;
    // Outer badge ring
    c.fill_circle(cx, cy, r, [30, 41, 59]);
    // Penguin body
    c.fill_circle(cx, cy + r / 6, (r * 62) / 100, [15, 17, 21]);
    // Head
    c.fill_circle(cx, cy - r / 3, (r * 40) / 100, [15, 17, 21]);
    // White belly
    c.fill_circle(cx, cy + r / 5, (r * 42) / 100, [241, 245, 249]);
    // Eyes
    let eye_r = (r / 10).max(2);
    c.fill_circle(cx - r / 6, cy - (r * 38) / 100, eye_r, [255, 255, 255]);
    c.fill_circle(cx + r / 6, cy - (r * 38) / 100, eye_r, [255, 255, 255]);
    // Orange beak & feet
    let beak_w = (r / 3).max(4) as usize;
    let beak_h = (r / 6).max(3) as usize;
    c.fill_rect(cx - (beak_w as isize) / 2, cy - r / 5, beak_w, beak_h, [245, 158, 11]);
    c.fill_rect(cx - r / 2, cy + (r * 65) / 100, beak_w, beak_h, [245, 158, 11]);
    c.fill_rect(cx + r / 6, cy + (r * 65) / 100, beak_w, beak_h, [245, 158, 11]);
}

/// Ubuntu orange circle-of-friends emblem.
fn draw_ubuntu_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let orange = [233, 84, 32];
    let white = [255, 255, 255];
    let r = (size * 42) / 100;
    c.fill_circle(cx, cy, r, orange);
    c.fill_ring(cx, cy, (r * 62) / 100, (r * 42) / 100, white);
    let dot_r = (r / 6).max(3);
    let orbit = (r * 58) / 100;
    // 3 nodes at ~0°, 120°, 240°
    c.fill_circle(cx + orbit, cy, dot_r, white);
    c.fill_circle(cx - orbit / 2, cy - (orbit * 86) / 100, dot_r, white);
    c.fill_circle(cx - orbit / 2, cy + (orbit * 86) / 100, dot_r, white);
}

/// Fedora blue rounded badge with stylized 'f'.
fn draw_fedora_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let blue = [60, 110, 180];
    let white = [255, 255, 255];
    let s = ((size * 78) / 100) as usize;
    c.fill_rounded_rect(cx - (s as isize) / 2, cy - (s as isize) / 2, s, s, s / 3, blue);
    let r = (size * 22) / 100;
    c.fill_ring(cx + r / 3, cy - r / 3, r, (r * 60) / 100, white);
    c.fill_ring(cx - r / 3, cy + r / 3, r, (r * 60) / 100, white);
    let bar = (r / 2).max(3) as usize;
    c.fill_rect(cx - bar as isize / 2, cy - r, bar, (r * 2) as usize, white);
}

/// Arch Linux sharp cyan triangle peak.
fn draw_arch_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let cyan = [23, 147, 209];
    let bg = [18, 25, 42];
    let half_h = (size * 42) / 100;
    let half_w = (size * 40) / 100;
    for dy in -half_h..=half_h {
        let progress = (dy + half_h) as f32 / (2 * half_h).max(1) as f32;
        let span = (progress * half_w as f32) as isize;
        for dx in -span..=span {
            // Inner bottom notch
            let in_notch = dy > half_h / 3 && dx.abs() < ((dy - half_h / 3) * 45) / 100;
            if !in_notch {
                c.put_pixel(cx + dx, cy + dy, cyan);
            }
        }
    }
    // Small inner accent dot
    c.fill_circle(cx, cy + half_h / 5, (size / 14).max(2), bg);
}

/// Debian crimson spiral emblem.
fn draw_debian_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let red = [215, 10, 83];
    let r = (size * 40) / 100;
    c.fill_ring(cx, cy, r, (r * 74) / 100, red);
    c.fill_ring(cx + r / 10, cy + r / 10, (r * 58) / 100, (r * 38) / 100, red);
    c.fill_circle(cx + r / 8, cy, (r / 5).max(2), red);
}

/// SteamOS dark-blue / cyan piston emblem.
fn draw_steamos_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let bg = [26, 159, 255];
    let white = [241, 245, 249];
    let r = (size * 42) / 100;
    c.fill_circle(cx, cy, r, bg);
    c.fill_ring(cx + r / 4, cy - r / 5, r / 3, r / 6, white);
    c.fill_circle(cx - r / 3, cy + r / 4, r / 5, white);
    let thick = (r / 5).max(3) as usize;
    c.fill_rect(cx - r / 3, cy, (r / 2) as usize, thick, white);
}

/// macOS silver badge.
fn draw_macos_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize) {
    let silver = [226, 232, 240];
    let r = (size * 34) / 100;
    c.fill_circle(cx - r / 4, cy + r / 8, (r * 70) / 100, silver);
    c.fill_circle(cx + r / 4, cy + r / 8, (r * 70) / 100, silver);
    c.fill_circle(cx + r / 8, cy - (r * 78) / 100, (r / 3).max(3), silver);
}

/// UEFI gear / cogwheel icon.
fn draw_gear_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize, color: [u8; 3]) {
    let r = (size * 34) / 100;
    c.fill_ring(cx, cy, r, (r * 52) / 100, color);
    let tooth = (r / 3).max(4) as usize;
    let offset = r + (tooth as isize) / 3;
    // 4 cardinal teeth + 4 diagonal teeth
    c.fill_rect(cx - (tooth as isize) / 2, cy - offset - (tooth as isize) / 2, tooth, tooth, color);
    c.fill_rect(cx - (tooth as isize) / 2, cy + offset - (tooth as isize) / 2, tooth, tooth, color);
    c.fill_rect(cx - offset - (tooth as isize) / 2, cy - (tooth as isize) / 2, tooth, tooth, color);
    c.fill_rect(cx + offset - (tooth as isize) / 2, cy - (tooth as isize) / 2, tooth, tooth, color);
    let diag = (offset * 70) / 100;
    let dtooth = (tooth * 85 / 100).max(3);
    let dt = dtooth as isize / 2;
    c.fill_rect(cx - diag - dt, cy - diag - dt, dtooth, dtooth, color);
    c.fill_rect(cx + diag - dt, cy - diag - dt, dtooth, dtooth, color);
    c.fill_rect(cx - diag - dt, cy + diag - dt, dtooth, dtooth, color);
    c.fill_rect(cx + diag - dt, cy + diag - dt, dtooth, dtooth, color);
}

/// Generic OS rounded chip icon.
fn draw_generic_os_icon(c: &mut Canvas, cx: isize, cy: isize, size: isize, color: [u8; 3]) {
    let s = ((size * 68) / 100) as usize;
    c.stroke_rounded_rect(cx - (s as isize) / 2, cy - (s as isize) / 2, s, s, s / 4, 3, color);
    let inner = (s / 2).max(8);
    c.fill_rounded_rect(
        cx - (inner as isize) / 2,
        cy - (inner as isize) / 2,
        inner,
        inner,
        inner / 4,
        color,
    );
}
