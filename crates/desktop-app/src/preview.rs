use eframe::egui::{
    self, Color32, FontId, Mesh, Pos2, Rect, Rounding, Sense, Shape, Stroke, Vec2,
};
use modernboot_shared::{BootConfig, BootEntry, OsKind, ThemeColors};

fn to_color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Renders an interactive live preview of the ModernDualBoot UEFI boot screen
/// using the exact [`ThemeColors`] and settings from `config`.
pub fn render_boot_preview(
    ui: &mut egui::Ui,
    config: &BootConfig,
    selected_idx: &mut usize,
) {
    let available_width = ui.available_width().max(320.0);
    let preview_width = available_width;
    let preview_height = (preview_width * 9.0 / 16.0).clamp(260.0, 440.0);

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(preview_width, preview_height),
        Sense::click(),
    );

    if !ui.is_rect_visible(rect) {
        return;
    }

    let colors = config.theme.effective_colors();
    let painter = ui.painter_at(rect);

    // 1. Vertical gradient background (bg_top -> bg_bottom)
    draw_vertical_gradient(&painter, rect, to_color32(colors.bg_top), to_color32(colors.bg_bottom));

    // Subtle outer frame border
    painter.rect_stroke(
        rect,
        Rounding::same(8.0),
        Stroke::new(1.0_f32, to_color32(colors.card_border)),
    );

    let scale = (rect.width() / 800.0).clamp(0.55, 1.25);
    let pad = 28.0 * scale;

    // 2. Header: "ModernBoot" + subtitle + optional countdown timer pill
    let header_pos = Pos2::new(rect.left() + pad, rect.top() + pad);
    painter.text(
        header_pos,
        egui::Align2::LEFT_TOP,
        "ModernBoot",
        FontId::proportional(24.0 * scale),
        to_color32(colors.text_primary),
    );
    painter.text(
        Pos2::new(header_pos.x, header_pos.y + 28.0 * scale),
        egui::Align2::LEFT_TOP,
        "Select an operating system to start",
        FontId::proportional(12.5 * scale),
        to_color32(colors.text_secondary),
    );

    if config.theme.show_clock_or_timer {
        let timer_text = if config.timeout_seconds == 0 {
            "Auto-boot disabled".to_string()
        } else {
            format!("Booting in {}s", config.timeout_seconds)
        };
        let pill_w = 135.0 * scale;
        let pill_h = 28.0 * scale;
        let pill_rect = Rect::from_min_size(
            Pos2::new(rect.right() - pad - pill_w, rect.top() + pad),
            Vec2::new(pill_w, pill_h),
        );
        painter.rect_filled(
            pill_rect,
            Rounding::same(pill_h / 2.0),
            to_color32(colors.card_bg),
        );
        painter.rect_stroke(
            pill_rect,
            Rounding::same(pill_h / 2.0),
            Stroke::new(1.0_f32, to_color32(colors.card_border)),
        );
        painter.text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            timer_text,
            FontId::proportional(11.5 * scale),
            to_color32(colors.accent),
        );
    }

    // 3. Visible OS entry cards
    let visible_entries: Vec<&BootEntry> = config.entries.iter().filter(|e| !e.hidden).collect();

    let cards_area = Rect::from_min_max(
        Pos2::new(rect.left() + pad, rect.top() + 86.0 * scale),
        Pos2::new(rect.right() - pad, rect.bottom() - 52.0 * scale),
    );

    if visible_entries.is_empty() {
        painter.text(
            cards_area.center(),
            egui::Align2::CENTER_CENTER,
            "No visible boot entries.\nAdd or unhide an OS entry, or enable Auto-Scan.",
            FontId::proportional(14.0 * scale),
            to_color32(colors.text_secondary),
        );
    } else {
        if *selected_idx >= visible_entries.len() {
            *selected_idx = 0;
        }

        let count = visible_entries.len().min(5);
        let gap = 14.0 * scale;
        let max_card_w = 160.0 * scale;
        let avail_w = cards_area.width();
        let card_w = ((avail_w - gap * (count.saturating_sub(1) as f32)) / count as f32)
            .min(max_card_w);
        let card_h = (cards_area.height() * 0.78).clamp(110.0 * scale, 185.0 * scale);
        let total_w = card_w * count as f32 + gap * (count.saturating_sub(1) as f32);
        let start_x = cards_area.center().x - total_w / 2.0;
        let start_y = cards_area.center().y - card_h / 2.0;

        let radius = ((config.theme.corner_radius as f32) * scale).clamp(0.0, 28.0);
        let corner_radius = Rounding::same(radius);

        let click_pos = if response.clicked() {
            response.interact_pointer_pos()
        } else {
            None
        };

        for (i, entry) in visible_entries.iter().take(count).enumerate() {
            let card_rect = Rect::from_min_size(
                Pos2::new(start_x + i as f32 * (card_w + gap), start_y),
                Vec2::new(card_w, card_h),
            );

            if let Some(pos) = click_pos {
                if card_rect.contains(pos) {
                    *selected_idx = i;
                }
            }

            let is_selected = *selected_idx == i;
            let bg = if is_selected {
                to_color32(colors.card_selected_bg)
            } else {
                to_color32(colors.card_bg)
            };
            let border = if is_selected {
                Stroke::new(2.0_f32 * scale.max(0.8), to_color32(colors.accent))
            } else {
                Stroke::new(1.0_f32, to_color32(colors.card_border))
            };

            painter.rect_filled(card_rect, corner_radius, bg);
            painter.rect_stroke(card_rect, corner_radius, border);

            // Default star/dot indicator
            let is_default = config.default_entry_id.as_deref() == Some(entry.id.as_str());
            if is_default {
                painter.circle_filled(
                    Pos2::new(card_rect.right() - 12.0 * scale, card_rect.top() + 12.0 * scale),
                    3.5 * scale,
                    to_color32(colors.accent),
                );
            }

            // OS Vector Icon
            let icon_center = Pos2::new(card_rect.center().x, card_rect.top() + card_h * 0.36);
            draw_os_icon(
                &painter,
                icon_center,
                22.0 * scale,
                entry.os_kind,
                &colors,
                is_selected,
            );

            // Badge pill
            let badge_w = 38.0 * scale;
            let badge_h = 16.0 * scale;
            let badge_rect = Rect::from_center_size(
                Pos2::new(card_rect.center().x, card_rect.top() + card_h * 0.65),
                Vec2::new(badge_w, badge_h),
            );
            painter.rect_filled(
                badge_rect,
                Rounding::same(4.0),
                to_color32(colors.badge_bg),
            );
            painter.text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                entry.os_kind.default_badge(),
                FontId::monospace(9.5 * scale),
                to_color32(colors.text_secondary),
            );

            // Entry label (truncated cleanly if needed)
            let label = truncate_label(&entry.label, 16);
            painter.text(
                Pos2::new(card_rect.center().x, card_rect.top() + card_h * 0.83),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(12.5 * scale),
                to_color32(colors.text_primary),
            );
        }
    }

    // 4. Footer hints
    let footer_y = rect.bottom() - 22.0 * scale;
    painter.text(
        Pos2::new(rect.left() + pad, footer_y),
        egui::Align2::LEFT_CENTER,
        "[←/→] Select   [Enter] Boot   (Click cards to preview)",
        FontId::proportional(11.0 * scale),
        to_color32(colors.text_secondary),
    );

    if config.show_firmware_setup {
        painter.text(
            Pos2::new(rect.right() - pad, footer_y),
            egui::Align2::RIGHT_CENTER,
            "[F2] UEFI Firmware",
            FontId::proportional(11.0 * scale),
            to_color32(colors.text_secondary),
        );
    }
}

fn draw_vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

fn draw_os_icon(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    kind: OsKind,
    colors: &ThemeColors,
    selected: bool,
) {
    let primary = if selected {
        to_color32(colors.accent)
    } else {
        to_color32(colors.text_primary)
    };
    let secondary = to_color32(colors.text_secondary);

    match kind {
        OsKind::Windows => {
            let gap = radius * 0.12;
            let pane = radius * 0.42;
            let tl = Rect::from_min_size(
                Pos2::new(center.x - pane - gap * 0.5, center.y - pane - gap * 0.5),
                Vec2::splat(pane),
            );
            let tr = Rect::from_min_size(
                Pos2::new(center.x + gap * 0.5, center.y - pane - gap * 0.5),
                Vec2::splat(pane),
            );
            let bl = Rect::from_min_size(
                Pos2::new(center.x - pane - gap * 0.5, center.y + gap * 0.5),
                Vec2::splat(pane),
            );
            let br = Rect::from_min_size(
                Pos2::new(center.x + gap * 0.5, center.y + gap * 0.5),
                Vec2::splat(pane),
            );
            for r in [tl, tr, bl, br] {
                painter.rect_filled(r, Rounding::same(2.0), primary);
            }
        }
        OsKind::Arch => {
            let pts = vec![
                Pos2::new(center.x, center.y - radius * 0.9),
                Pos2::new(center.x + radius * 0.85, center.y + radius * 0.8),
                Pos2::new(center.x, center.y + radius * 0.35),
                Pos2::new(center.x - radius * 0.85, center.y + radius * 0.8),
            ];
            painter.add(Shape::convex_polygon(pts, primary, Stroke::NONE));
        }
        OsKind::Ubuntu => {
            painter.circle_stroke(center, radius * 0.68, Stroke::new(radius * 0.18, primary));
            for angle in [0.0_f32, 2.094, 4.188] {
                let p = Pos2::new(
                    center.x + angle.cos() * radius * 0.72,
                    center.y + angle.sin() * radius * 0.72,
                );
                painter.circle_filled(p, radius * 0.18, to_color32(colors.card_bg));
                painter.circle_filled(p, radius * 0.12, primary);
            }
        }
        OsKind::Fedora => {
            painter.circle_filled(center, radius * 0.82, primary);
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "f",
                FontId::proportional(radius * 1.3),
                to_color32(colors.card_bg),
            );
        }
        OsKind::Debian => {
            painter.circle_stroke(center, radius * 0.72, Stroke::new(radius * 0.16, primary));
            painter.circle_stroke(
                Pos2::new(center.x + radius * 0.1, center.y),
                radius * 0.38,
                Stroke::new(radius * 0.14, primary),
            );
        }
        OsKind::SteamOS => {
            painter.circle_stroke(center, radius * 0.78, Stroke::new(radius * 0.16, primary));
            painter.circle_filled(
                Pos2::new(center.x + radius * 0.25, center.y - radius * 0.18),
                radius * 0.24,
                primary,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - radius * 0.5, center.y + radius * 0.25),
                    Pos2::new(center.x + radius * 0.25, center.y - radius * 0.18),
                ],
                Stroke::new(radius * 0.18, primary),
            );
        }
        OsKind::Linux => {
            painter.circle_filled(center, radius * 0.76, primary);
            painter.circle_filled(
                Pos2::new(center.x, center.y + radius * 0.18),
                radius * 0.42,
                to_color32(colors.card_bg),
            );
        }
        OsKind::MacOS => {
            painter.circle_filled(center, radius * 0.72, primary);
            painter.circle_filled(
                Pos2::new(center.x + radius * 0.65, center.y - radius * 0.1),
                radius * 0.32,
                to_color32(colors.card_bg),
            );
        }
        OsKind::UefiShell => {
            let box_rect = Rect::from_center_size(center, Vec2::splat(radius * 1.55));
            painter.rect_stroke(
                box_rect,
                Rounding::same(4.0),
                Stroke::new(radius * 0.14, primary),
            );
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                ">_",
                FontId::monospace(radius * 0.95),
                primary,
            );
        }
        OsKind::Unknown => {
            painter.circle_stroke(center, radius * 0.72, Stroke::new(radius * 0.14, secondary));
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "?",
                FontId::proportional(radius * 1.1),
                secondary,
            );
        }
    }
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
