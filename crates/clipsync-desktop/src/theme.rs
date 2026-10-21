use std::sync::Arc;

use eframe::egui::{
    self, epaint::Shadow, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Margin, Pos2, Rect, Stroke, TextStyle, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x20);
pub const SURFACE: Color32 = Color32::from_rgb(0x1a, 0x12, 0x18);
pub const SURFACE_HIGH: Color32 = Color32::from_rgb(0x1e, 0x24, 0x2d);
pub const CARD: Color32 = Color32::from_rgb(0x26, 0x1b, 0x21);
pub const BORDER: Color32 = Color32::from_rgb(0x3a, 0x2a, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xf0, 0xe4, 0xe9);
pub const MUTED: Color32 = Color32::from_rgb(0xbf, 0xa5, 0xb0);
pub const FAINT: Color32 = Color32::from_rgb(0x8d, 0x77, 0x82);
pub const ACCENT: Color32 = Color32::from_rgb(0xf4, 0xa8, 0xc0);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x3d, 0x0f, 0x20);
pub const GREEN: Color32 = Color32::from_rgb(0x4a, 0xde, 0x80);
pub const AMBER: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const RED: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
pub const ON_RED: Color32 = Color32::from_rgb(0x45, 0x0a, 0x0a);
pub const ERROR_CONTAINER: Color32 = Color32::from_rgb(0x4c, 0x1d, 0x1d);
pub const ON_ERROR_CONTAINER: Color32 = Color32::from_rgb(0xff, 0xc9, 0xc9);
pub const INVERSE_SURFACE: Color32 = Color32::from_rgb(0xf0, 0xe4, 0xe9);
pub const INVERSE_TEXT: Color32 = Color32::from_rgb(0x1a, 0x12, 0x18);
pub const TOP_INSET: f32 = if cfg!(target_os = "macos") {
    40.0
} else {
    18.0
};
pub const MAX_CONTENT: f32 = 940.0;
pub const CONTENT_MARGIN: f32 = 6.0;
pub const ROW_INSET: f32 = 14.0;

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

const AURORA_BLUE: Color32 = Color32::from_rgb(0x3b, 0x77, 0xf0);
const AURORA_PURPLE: Color32 = Color32::from_rgb(0x7c, 0x3a, 0xed);
const AURORA_CYAN: Color32 = Color32::from_rgb(0x0e, 0xa5, 0xe9);

pub fn glass_fill() -> Color32 {
    Color32::from_white_alpha(13)
}

pub fn glass_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(glass_fill())
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(23)))
        .corner_radius(CornerRadius::same(20))
        .shadow(Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(70),
        })
        .inner_margin(Margin::symmetric(14, 10))
}

pub fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(24))
        .shadow(Shadow {
            offset: [0, 14],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(120),
        })
        .inner_margin(Margin::symmetric(20, 18))
}

pub fn configure(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
    ctx.request_repaint();

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let mut visuals = Visuals::dark();
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.extreme_bg_color = SURFACE_HIGH;
    visuals.faint_bg_color = Color32::from_white_alpha(8);
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.30);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.text_cursor.stroke = Stroke::new(1.5, ACCENT);
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    visuals.popup_shadow = visuals.window_shadow;

    visuals.widgets.noninteractive.bg_fill = SURFACE_HIGH;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE_HIGH;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.bg_fill = SURFACE_HIGH;
    visuals.widgets.inactive.weak_bg_fill = Color32::from_white_alpha(7);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(14));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x20, 0x27);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_white_alpha(10);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.55));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.20);
    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.16);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(10);
    }
    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.menu_margin = Margin::same(5);
    style.spacing.window_margin = Margin::same(16);
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );
    ctx.set_style_of(egui::Theme::Dark, style);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    #[cfg(target_os = "macos")]
    {
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
            fonts
                .font_data
                .insert("sf-pro".to_owned(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .get_mut(&FontFamily::Proportional)
                .expect("proportional family")
                .insert(0, "sf-pro".to_owned());
        }
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Menlo.ttc") {
            fonts.font_data.insert(
                "menlo".to_owned(),
                Arc::new(FontData::from_blob(Arc::new(bytes), 0)),
            );
            fonts
                .families
                .get_mut(&FontFamily::Monospace)
                .expect("monospace family")
                .insert(0, "menlo".to_owned());
        }
        let cjk = std::fs::read("/System/Library/Fonts/Hiragino Sans GB.ttc")
            .map(|bytes| FontData::from_blob(Arc::new(bytes), 0))
            .or_else(|_| {
                std::fs::read("/System/Library/Fonts/Supplemental/Arial Unicode.ttf")
                    .map(FontData::from_owned)
            });
        if let Ok(font) = cjk {
            fonts.font_data.insert("cjk".to_owned(), Arc::new(font));
            fonts
                .families
                .get_mut(&FontFamily::Proportional)
                .expect("proportional family")
                .push("cjk".to_owned());
            fonts
                .families
                .get_mut(&FontFamily::Monospace)
                .expect("monospace family")
                .push("cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn paint_background(ctx: &egui::Context, rect: Rect) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(rect, 0.0, BG);
    let blobs = [
        (Pos2::new(80.0, 100.0), 240.0, AURORA_BLUE),
        (Pos2::new(380.0, 180.0), 215.0, AURORA_PURPLE),
        (Pos2::new(160.0, 660.0), 190.0, AURORA_CYAN),
    ];
    for (center, radius, color) in blobs {
        paint_soft_ball(&painter, center, radius, color);
    }
}

fn paint_soft_ball(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = egui::Mesh::default();
    let core = color.gamma_multiply(0.43);
    let segments = 64;
    mesh.colored_vertex(center, core);
    let plateau = radius * 0.55;
    for step in 0..=segments {
        let angle = step as f32 / segments as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            center + egui::vec2(angle.cos(), angle.sin()) * plateau,
            core,
        );
    }
    for step in 0..=segments {
        let angle = step as f32 / segments as f32 * std::f32::consts::TAU;
        let point = center + egui::vec2(angle.cos(), angle.sin()) * radius;
        mesh.colored_vertex(point, Color32::TRANSPARENT);
    }
    for step in 0..segments {
        let inner = step as u32 + 1;
        let middle = inner + segments as u32 + 1;
        mesh.add_triangle(0, inner, inner + 1);
        mesh.add_triangle(inner, middle, middle + 1);
        mesh.add_triangle(inner, middle + 1, inner + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}
