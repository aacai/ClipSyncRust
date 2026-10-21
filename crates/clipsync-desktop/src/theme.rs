use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use eframe::egui::{
    self, epaint::Shadow, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Margin, Pos2, Rect, Stroke, TextStyle, Visuals,
};

use crate::settings::ThemeMode;

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_high: Color32,
    pub card: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub green: Color32,
    pub amber: Color32,
    pub red: Color32,
    pub on_red: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,
    pub inverse_surface: Color32,
    pub inverse_text: Color32,
    pub glass_fill: Color32,
    pub glass_border: Color32,
    pub hairline: Color32,
    pub hover: Color32,
    pub inset: Color32,
    pub inset_border: Color32,
    pub widget_hover: Color32,
    pub switch_off: Color32,
    pub row_idle: Color32,
    pub row_cursor: Color32,
    pub row_border: Color32,
    pub aura: [Color32; 3],
}

pub const TOP_INSET: f32 = if cfg!(target_os = "macos") {
    40.0
} else {
    18.0
};
pub const MAX_CONTENT: f32 = 940.0;
pub const CONTENT_MARGIN: f32 = 6.0;
pub const ROW_INSET: f32 = 14.0;

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            dark,
            bg: Color32::from_rgb(0x0e, 0x10, 0x14),
            surface: Color32::from_rgb(0x1a, 0x12, 0x18),
            surface_high: Color32::from_rgb(0x1e, 0x24, 0x2d),
            card: Color32::from_rgb(0x26, 0x1b, 0x21),
            border: Color32::from_rgb(0x3a, 0x2a, 0x33),
            text: Color32::from_rgb(0xf0, 0xe4, 0xe9),
            muted: Color32::from_rgb(0xbf, 0xa5, 0xb0),
            faint: Color32::from_rgb(0x8d, 0x77, 0x82),
            accent: Color32::from_rgb(0xf4, 0xa8, 0xc0),
            accent_text: Color32::from_rgb(0x3d, 0x0f, 0x20),
            green: Color32::from_rgb(0x4a, 0xde, 0x80),
            amber: Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: Color32::from_rgb(0xf8, 0x71, 0x71),
            on_red: Color32::from_rgb(0x45, 0x0a, 0x0a),
            error_container: Color32::from_rgb(0x4c, 0x1d, 0x1d),
            on_error_container: Color32::from_rgb(0xff, 0xc9, 0xc9),
            inverse_surface: Color32::from_rgb(0xf0, 0xe4, 0xe9),
            inverse_text: Color32::from_rgb(0x1a, 0x12, 0x18),
            glass_fill: Color32::from_white_alpha(13),
            glass_border: Color32::from_white_alpha(23),
            hairline: Color32::from_white_alpha(10),
            hover: Color32::from_white_alpha(10),
            inset: Color32::from_white_alpha(5),
            inset_border: Color32::from_white_alpha(14),
            widget_hover: Color32::from_rgb(0x2a, 0x20, 0x27),
            switch_off: Color32::from_white_alpha(18),
            row_idle: Color32::from_rgba_unmultiplied(0x26, 0x1b, 0x21, 128),
            row_cursor: Color32::from_white_alpha(9),
            row_border: Color32::from_rgba_unmultiplied(0x3a, 0x2a, 0x33, 110),
            aura: [
                Color32::from_rgb(0x25, 0x63, 0xeb),
                Color32::from_rgb(0x7c, 0x3a, 0xed),
                Color32::from_rgb(0x0e, 0xa5, 0xe9),
            ],
        }
    } else {
        Palette {
            dark,
            bg: Color32::from_rgb(0xfb, 0xf3, 0xf5),
            surface: Color32::from_rgb(0xff, 0xfb, 0xfb),
            surface_high: Color32::from_rgb(0xf9, 0xed, 0xf0),
            card: Color32::from_rgb(0xff, 0xfb, 0xfb),
            border: Color32::from_rgb(0xe7, 0xd5, 0xdd),
            text: Color32::from_rgb(0x24, 0x1a, 0x1e),
            muted: Color32::from_rgb(0x6b, 0x55, 0x60),
            faint: Color32::from_rgb(0xa0, 0x87, 0x94),
            accent: Color32::from_rgb(0xc0, 0x42, 0x6b),
            accent_text: Color32::WHITE,
            green: Color32::from_rgb(0x15, 0x80, 0x3d),
            amber: Color32::from_rgb(0xb4, 0x53, 0x09),
            red: Color32::from_rgb(0xc2, 0x45, 0x3f),
            on_red: Color32::WHITE,
            error_container: Color32::from_rgb(0xfc, 0xe9, 0xe7),
            on_error_container: Color32::from_rgb(0x7a, 0x1b, 0x18),
            inverse_surface: Color32::from_rgb(0x2a, 0x1f, 0x24),
            inverse_text: Color32::from_rgb(0xfb, 0xee, 0xf1),
            glass_fill: Color32::from_white_alpha(205),
            glass_border: Color32::from_white_alpha(235),
            hairline: Color32::from_white_alpha(150),
            hover: Color32::from_rgba_unmultiplied(0x24, 0x1a, 0x1e, 12),
            inset: Color32::from_rgba_unmultiplied(0x24, 0x1a, 0x1e, 7),
            inset_border: Color32::from_rgba_unmultiplied(0x24, 0x1a, 0x1e, 20),
            widget_hover: Color32::from_rgb(0xf3, 0xe2, 0xe8),
            switch_off: Color32::from_rgba_unmultiplied(0x24, 0x1a, 0x1e, 46),
            row_idle: Color32::from_rgba_unmultiplied(0xff, 0xfb, 0xfb, 224),
            row_cursor: Color32::from_rgba_unmultiplied(0xc0, 0x42, 0x6b, 20),
            row_border: Color32::from_rgba_unmultiplied(0xe7, 0xd5, 0xdd, 180),
            aura: [
                Color32::from_rgb(0xbf, 0xd3, 0xff),
                Color32::from_rgb(0xdd, 0xcb, 0xf7),
                Color32::from_rgb(0xc3, 0xe9, 0xfa),
            ],
        }
    }
}

static DARK: AtomicBool = AtomicBool::new(true);

pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

pub fn p() -> Palette {
    palette(DARK.load(Ordering::Relaxed))
}

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn glass_fill() -> Color32 {
    p().glass_fill
}

pub fn glass_frame() -> egui::Frame {
    let palette = p();
    egui::Frame::new()
        .fill(palette.glass_fill)
        .stroke(Stroke::new(1.0, palette.glass_border))
        .corner_radius(CornerRadius::same(20))
        .shadow(Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if palette.dark { 70 } else { 36 }),
        })
        .inner_margin(Margin::symmetric(14, 10))
}

pub fn dialog_frame() -> egui::Frame {
    let palette = p();
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(24))
        .shadow(Shadow {
            offset: [0, 14],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(if palette.dark { 120 } else { 60 }),
        })
        .inner_margin(Margin::symmetric(20, 18))
}

pub fn resolve_dark(ctx: &egui::Context, mode: ThemeMode) -> bool {
    match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => ctx.system_theme() != Some(egui::Theme::Light),
    }
}

pub fn apply(ctx: &egui::Context, mode: ThemeMode) -> bool {
    let dark = resolve_dark(ctx, mode);
    set_dark(dark);
    let (preference, native) = match mode {
        ThemeMode::System => (
            egui::ThemePreference::System,
            egui::SystemTheme::SystemDefault,
        ),
        ThemeMode::Light => (egui::ThemePreference::Light, egui::SystemTheme::Light),
        ThemeMode::Dark => (egui::ThemePreference::Dark, egui::SystemTheme::Dark),
    };
    ctx.set_theme(preference);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(native));
    dark
}

pub fn configure(ctx: &egui::Context) {
    install_fonts(ctx);
    for (theme, dark) in [(egui::Theme::Dark, true), (egui::Theme::Light, false)] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = visuals(dark);
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
        ctx.set_style_of(theme, style);
    }
}

fn visuals(dark: bool) -> Visuals {
    let palette = palette(dark);
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.extreme_bg_color = palette.surface_high;
    visuals.faint_bg_color = palette.inset;
    visuals.override_text_color = Some(palette.text);
    visuals.hyperlink_color = palette.accent;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.30);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.text_cursor.stroke = Stroke::new(1.5, palette.accent);
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 50 }),
    };
    visuals.popup_shadow = visuals.window_shadow;

    visuals.widgets.noninteractive.bg_fill = palette.surface_high;
    visuals.widgets.noninteractive.weak_bg_fill = palette.surface_high;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.inactive.bg_fill = palette.surface_high;
    visuals.widgets.inactive.weak_bg_fill = palette.inset;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.inset_border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.hovered.bg_fill = palette.widget_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent.gamma_multiply(0.55));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.active.bg_fill = with_alpha(palette.accent, 48);
    visuals.widgets.active.weak_bg_fill = with_alpha(palette.accent, 38);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.text);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(10);
    }
    visuals
}

/// 苹方 is the one face for every family: egui stacks runs of different faces by
/// box centre rather than by baseline, so a Han glyph next to latin digits lands
/// on a different line.
const PRIMARY_FACE: &[u8] = include_bytes!("../assets/PingFangSC-Regular.otf");

fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(primary_fonts());
}

fn primary_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "primary".to_owned(),
        Arc::new(FontData::from_static(PRIMARY_FACE)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .get_mut(&family)
            .expect("family")
            .insert(0, "primary".to_owned());
    }
    fonts
}

pub fn paint_background(ctx: &egui::Context, rect: Rect) {
    let palette = p();
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(rect, 0.0, palette.bg);
    let blobs = [
        (Pos2::new(80.0, 100.0), 240.0, palette.aura[0]),
        (Pos2::new(380.0, 180.0), 215.0, palette.aura[1]),
        (Pos2::new(160.0, 660.0), 190.0, palette.aura[2]),
    ];
    for (center, radius, color) in blobs {
        paint_soft_ball(&painter, center, radius, color);
    }
}

fn paint_soft_ball(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = egui::Mesh::default();
    let core = color.gamma_multiply(if p().dark { 0.43 } else { 0.8 });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_palettes_keep_text_readable_against_their_surfaces() {
        for dark in [true, false] {
            let colors = palette(dark);
            let luminance = |color: Color32| {
                let channel = |value: u8| {
                    let value = value as f32 / 255.0;
                    if value <= 0.04045 {
                        value / 12.92
                    } else {
                        ((value + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * channel(color.r())
                    + 0.7152 * channel(color.g())
                    + 0.0722 * channel(color.b())
            };
            let ratio = |foreground: Color32| {
                let a = luminance(foreground);
                let b = luminance(colors.surface);
                (a.max(b) + 0.05) / (a.min(b) + 0.05)
            };
            let contrast = ratio(colors.text);
            assert!(contrast > 4.5, "contrast {contrast} (dark={dark})");
            let contrast = ratio(colors.muted);
            assert!(contrast > 3.0, "muted contrast {contrast} (dark={dark})");
        }
    }

    #[test]
    fn the_palette_switches_between_light_and_dark() {
        set_dark(false);
        assert!(!p().dark);
        assert_eq!(p().bg, palette(false).bg);
        set_dark(true);
        assert!(p().dark);
        assert_eq!(p().bg, palette(true).bg);
    }

    #[test]
    fn one_face_serves_both_families() {
        let fonts = primary_fonts();
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            assert_eq!(fonts.families[&family][0], "primary");
        }
        assert!(
            PRIMARY_FACE.starts_with(b"OTTO"),
            "the font asset is truncated"
        );
        assert!(PRIMARY_FACE.len() > 8_000_000, "the font asset is a stub");
    }

    #[test]
    fn every_script_shares_one_face() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for c in ['中', '文', 'A', 'a', '0', '9', '·', '…'] {
                let proportional = ui
                    .ctx()
                    .fonts_mut(|fonts| fonts.glyph_width(&FontId::proportional(14.0), c));
                let monospace = ui
                    .ctx()
                    .fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(14.0), c));
                assert_eq!(proportional, monospace, "{c} resolved to two faces");
                assert!(proportional > 0.0, "{c} has no glyph");
            }
        });
        output.textures_delta.clear();
    }
}
