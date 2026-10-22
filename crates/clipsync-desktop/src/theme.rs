use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

use eframe::egui::{
    self, epaint::Shadow, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id,
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

/// Compose rounds each glass panel with its own shape; the frost has to use the
/// same radius or its hard corners poke outside the panel.
pub const GLASS_RADIUS: u8 = 20;
pub const BAR_RADIUS: u8 = 22;
pub const BANNER_RADIUS: u8 = 14;

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
        .corner_radius(CornerRadius::same(GLASS_RADIUS))
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
        // compose 悬停就出 tooltip。egui 默认还额外要求指针完全静止，本机空闲时 800ms 才重绘一帧，
        // 停手那一刻没有新帧，tooltip 要等到下一轮轮询才冒出来，看着就像这个版本根本没做。
        style.interaction.show_tooltips_only_when_still = false;
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
    for (center, radius, color) in aura_blobs(&palette) {
        paint_soft_ball(&painter, center, radius, color, palette.dark);
    }
}

fn aura_blobs(palette: &Palette) -> [(Pos2, f32, Color32); 3] {
    [
        (Pos2::new(80.0, 100.0), 240.0, palette.aura[0]),
        (Pos2::new(380.0, 180.0), 215.0, palette.aura[1]),
        (Pos2::new(160.0, 660.0), 190.0, palette.aura[2]),
    ]
}

const AURA_PLATEAU: f32 = 0.55;

fn aura_core(color: Color32, dark: bool) -> Color32 {
    color.gamma_multiply(if dark { 0.43 } else { 0.8 })
}

/// How much of a blob is still visible at this distance from its centre.
fn aura_falloff(distance: f32, radius: f32) -> f32 {
    let plateau = radius * AURA_PLATEAU;
    if distance <= plateau {
        1.0
    } else {
        ((radius - distance) / (radius - plateau)).clamp(0.0, 1.0)
    }
}

fn paint_soft_ball(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32, dark: bool) {
    let mut mesh = egui::Mesh::default();
    let core = aura_core(color, dark);
    let segments = 64;
    mesh.colored_vertex(center, core);
    let plateau = radius * AURA_PLATEAU;
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

/// The backdrop exactly as [`paint_background`] renders it, sampled at one point.
fn backdrop_color(at: Pos2, palette: &Palette) -> Color32 {
    let mut acc = palette.bg.to_array().map(|channel| channel as f32);
    for (center, radius, color) in aura_blobs(palette) {
        let core = aura_core(color, palette.dark).to_array();
        let fade = aura_falloff((at - center).length(), radius);
        let alpha = core[3] as f32 / 255.0 * fade;
        for channel in 0..3 {
            acc[channel] = acc[channel] * (1.0 - alpha) + core[channel] as f32 * fade;
        }
        acc[3] = acc[3] * (1.0 - alpha) + alpha * 255.0;
    }
    let to_u8 = |value: f32| (value.clamp(0.0, 255.0) + 0.5) as u8;
    Color32::from_rgba_premultiplied(to_u8(acc[0]), to_u8(acc[1]), to_u8(acc[2]), to_u8(acc[3]))
}

/// One frost texel covers this many logical points.
const FROST_TEXEL: f32 = 4.0;
/// Compose frosts the glass with `blur(20.dp)`.
const FROST_BLUR: f32 = 20.0;
/// Three box passes land on roughly the same spread as one gaussian of that radius.
const FROST_PASSES: usize = 3;
const ARC_SEGMENTS: usize = 8;

struct FrostBake {
    content: Rect,
    texel: f32,
    dark: bool,
    image: Arc<egui::ColorImage>,
}

impl FrostBake {
    fn fits(&self, content: Rect, texel: f32, dark: bool) -> bool {
        self.dark == dark
            && self.texel == texel
            && (self.content.min - content.min).length() < texel
            && (self.content.size() - content.size()).length() < texel
    }
}

static FROST: Mutex<Option<FrostBake>> = Mutex::new(None);

/// Draw the blurred backdrop a glass panel sits on, in the given layer.
///
/// The panel stays translucent, so the frost has to be submitted below it. Floating
/// bars need [`frost_layer`] to keep the scrolling list off their face; a panel that
/// flows with the list frosts the very layer it is drawn in.
pub fn frost(ctx: &egui::Context, rect: Rect, radius: u8, layer: egui::LayerId) {
    let content = ctx.content_rect();
    if !rect.is_finite() || rect.area() <= 0.0 || !content.is_finite() || content.area() <= 0.0 {
        return;
    }
    let dark = p().dark;
    let limit = ctx.input(|input| input.max_texture_side);
    let texel = frost_texel(content, limit);
    let image = frost_image(content, texel, dark);
    let texture = ctx.load_texture_cached(
        "clipsync-frost",
        Id::unique("clip-frost"),
        image,
        egui::TextureOptions::LINEAR,
    );
    ctx.layer_painter(layer).add(egui::Shape::mesh(frost_mesh(
        rect,
        radius as f32,
        content,
        texture.id(),
    )));
}

/// Above the list, below the floating glass bars.
pub fn frost_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, Id::unique("clip-frost"))
}

/// Keeps the bake inside one texture even on an absurdly large window.
fn frost_texel(content: Rect, max_side: usize) -> f32 {
    let limit = max_side.max(1) as f32;
    let scale = (content.width() / limit)
        .max(content.height() / limit)
        .max(1.0);
    FROST_TEXEL * scale
}

fn frost_image(content: Rect, texel: f32, dark: bool) -> Arc<egui::ColorImage> {
    let mut bake = FROST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if bake
        .as_ref()
        .is_none_or(|baked| !baked.fits(content, texel, dark))
    {
        bake.replace(FrostBake {
            content,
            texel,
            dark,
            image: bake_frost(content, texel, dark),
        });
    }
    Arc::clone(&bake.as_ref().expect("baked").image)
}

fn bake_frost(content: Rect, texel: f32, dark: bool) -> Arc<egui::ColorImage> {
    let palette = palette(dark);
    let size = [
        (content.width() / texel).ceil().max(1.0) as usize,
        (content.height() / texel).ceil().max(1.0) as usize,
    ];
    let mut field = Vec::with_capacity(size[0] * size[1] * 4);
    for row in 0..size[1] {
        for column in 0..size[0] {
            let at = Pos2::new(
                content.left() + (column as f32 + 0.5) * texel,
                content.top() + (row as f32 + 0.5) * texel,
            );
            field.extend_from_slice(&backdrop_color(at, &palette).to_array().map(f32::from));
        }
    }
    let radius = (FROST_BLUR / texel).round().max(1.0) as usize;
    let blurred = box_blur(&field, size[0], size[1], radius);
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for pixel in blurred.chunks_exact(4) {
        let to_u8 = |value: f32| (value.clamp(0.0, 255.0) + 0.5) as u8;
        pixels.push(Color32::from_rgba_premultiplied(
            to_u8(pixel[0]),
            to_u8(pixel[1]),
            to_u8(pixel[2]),
            to_u8(pixel[3]),
        ));
    }
    Arc::new(egui::ColorImage::new(size, pixels))
}

/// Separable box blur; three of these passes approximate one gaussian.
fn box_blur(field: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let span = 2 * radius + 1;
    let mut out = field.to_vec();
    let mut next = vec![0.0; out.len()];
    let mut acc = [0.0f32; 4];
    for _ in 0..FROST_PASSES {
        for row in 0..height {
            for column in 0..width {
                for channel in 0..4 {
                    acc[channel] = 0.0;
                }
                for step in 0..span {
                    let source = (column as isize + step as isize - radius as isize)
                        .clamp(0, width.saturating_sub(1) as isize)
                        as usize;
                    let from = (row * width + source) * 4;
                    for channel in 0..4 {
                        acc[channel] += out[from + channel];
                    }
                }
                let target = (row * width + column) * 4;
                for channel in 0..4 {
                    next[target + channel] = acc[channel] / span as f32;
                }
            }
        }
        std::mem::swap(&mut out, &mut next);
        for column in 0..width {
            for row in 0..height {
                for channel in 0..4 {
                    acc[channel] = 0.0;
                }
                for step in 0..span {
                    let source = (row as isize + step as isize - radius as isize)
                        .clamp(0, height.saturating_sub(1) as isize)
                        as usize;
                    let from = (source * width + column) * 4;
                    for channel in 0..4 {
                        acc[channel] += out[from + channel];
                    }
                }
                let target = (row * width + column) * 4;
                for channel in 0..4 {
                    next[target + channel] = acc[channel] / span as f32;
                }
            }
        }
        std::mem::swap(&mut out, &mut next);
    }
    out
}

fn frost_mesh(
    rect: Rect,
    radius: f32,
    content: Rect,
    texture: egui::epaint::TextureId,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(texture);
    let white = Color32::from_rgb(255, 255, 255);
    let push = |mesh: &mut egui::Mesh, pos: Pos2| {
        let uv = (pos - content.min) / content.size();
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: Pos2::new(uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)),
            color: white,
        });
    };
    let center = rect.center();
    push(&mut mesh, center);
    let outline = rounded_outline(rect, radius);
    for point in &outline {
        push(&mut mesh, *point);
    }
    let corners = outline.len() as u32;
    for step in 0..corners {
        mesh.add_triangle(0, 1 + step, 1 + (step + 1) % corners);
    }
    mesh
}

/// A rounded rectangle walked clockwise — convex, so a fan from its centre tiles it.
fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let radius = radius
        .min(rect.width() * 0.5)
        .min(rect.height() * 0.5)
        .max(0.0);
    let corners = [
        (Pos2::new(rect.left() + radius, rect.top() + radius), 180.0),
        (Pos2::new(rect.right() - radius, rect.top() + radius), 270.0),
        (
            Pos2::new(rect.right() - radius, rect.bottom() - radius),
            0.0,
        ),
        (
            Pos2::new(rect.left() + radius, rect.bottom() - radius),
            90.0,
        ),
    ];
    let mut points = Vec::with_capacity(4 * (ARC_SEGMENTS + 1));
    for (center, start) in corners {
        for step in 0..=ARC_SEGMENTS {
            let angle = (start + 90.0 * step as f32 / ARC_SEGMENTS as f32).to_radians();
            points.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
        }
    }
    points
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

    #[test]
    fn the_blur_softens_edges_and_spares_flat_ground() {
        let width = 40;
        let height = 12;
        let mut field = vec![0.0f32; width * height * 4];
        for (index, pixel) in field.chunks_exact_mut(4).enumerate() {
            let level = if index % width < width / 2 {
                0.0
            } else {
                255.0
            };
            pixel[0] = level;
            pixel[1] = level;
            pixel[2] = level;
            pixel[3] = 255.0;
        }
        let blurred = box_blur(&field, width, height, 5);
        let at = |column: usize| blurred[column * 4];
        assert!(
            at(width - 1) > 254.0 && at(0) < 1.0,
            "flat ground must survive the blur"
        );
        assert!(
            at(width / 2) < 250.0 && at(width / 2 - 1) > 5.0,
            "the step must turn into a slope"
        );
        for column in 1..width {
            assert!(
                blurred[column * 4 + 3] > 254.0,
                "alpha must stay opaque at column {column}"
            );
            assert!(
                blurred[(height - 1) * width * 4 + column * 4] - at(column) < 1.0,
                "the blur must not vary along the edge"
            );
            assert!(
                at(column) >= at(column - 1) - 0.001,
                "the slope must be monotonic at {column}"
            );
        }
    }

    #[test]
    fn the_bake_reproduces_the_painted_aura() {
        let colors = palette(true);
        let (center, radius, aura) = aura_blobs(&colors)[0];
        assert_eq!(
            backdrop_color(Pos2::new(9000.0, 9000.0), &colors),
            colors.bg,
            "flat ground must bake as flat ground"
        );
        let core = aura_core(aura, colors.dark).to_array();
        let alpha = core[3] as f32 / 255.0;
        let baked = backdrop_color(center, &colors).to_array();
        for channel in 0..3 {
            let want = colors.bg.to_array()[channel] as f32 * (1.0 - alpha) + core[channel] as f32;
            let delta = (baked[channel] as f32 - want).abs();
            assert!(
                delta < 1.5,
                "channel {channel} baked {baked:?} wants {want}"
            );
        }
        let edge = center - egui::vec2(radius, 0.0);
        assert_eq!(
            backdrop_color(edge, &colors),
            colors.bg,
            "the falloff must end at the rim"
        );
    }

    #[test]
    fn the_fan_tiles_the_panel_exactly() {
        let content = Rect::from_min_size(Pos2::new(20.0, 30.0), egui::vec2(400.0, 300.0));
        let panel = Rect::from_min_size(Pos2::new(60.0, 80.0), egui::vec2(240.0, 60.0));
        let radius = 20.0;
        let mesh = frost_mesh(panel, radius, content, egui::epaint::TextureId::default());
        assert_eq!(mesh.vertices.len(), 1 + 4 * (ARC_SEGMENTS + 1));
        assert_eq!(mesh.indices.len(), 3 * (mesh.vertices.len() - 1));
        let white = Color32::from_rgb(255, 255, 255);
        for vertex in &mesh.vertices {
            assert_eq!(
                vertex.color, white,
                "the texture must pass through untinted"
            );
            assert!(
                vertex.pos.x >= panel.left() - 0.01
                    && vertex.pos.x <= panel.right() + 0.01
                    && vertex.pos.y >= panel.top() - 0.01
                    && vertex.pos.y <= panel.bottom() + 0.01,
                "{:?} escapes the panel",
                vertex.pos
            );
            assert!(vertex.uv.x >= 0.0 && vertex.uv.x <= 1.0);
            assert!(vertex.uv.y >= 0.0 && vertex.uv.y <= 1.0);
        }
        assert_eq!(mesh.vertices[0].pos, panel.center());
        let middle = panel.center() - content.min;
        assert_eq!(
            (mesh.vertices[0].uv.x - middle.x / content.width()).abs(),
            0.0
        );
        let outline: Vec<Pos2> = mesh.vertices[1..].iter().map(|v| v.pos).collect();
        let exact = panel.width() * panel.height() - (4.0 - std::f32::consts::PI) * radius * radius;
        let tiled = polygon_area(&outline);
        assert!(
            (tiled - exact).abs() / exact < 0.01,
            "fan covers {tiled}, rounded rect is {exact}"
        );
    }

    fn polygon_area(points: &[Pos2]) -> f32 {
        let mut sum = 0.0f32;
        for (index, point) in points.iter().enumerate() {
            let next = points[(index + 1) % points.len()];
            sum += point.x * next.y - next.x * point.y;
        }
        sum.abs() * 0.5
    }

    #[test]
    fn frost_sits_between_the_list_and_the_glass_bars() {
        let layer = frost_layer();
        assert!(layer.order > egui::Order::Background);
        assert!(layer.order < egui::Order::Foreground);
        assert_eq!(frost_layer(), layer, "the frost layer must be stable");
    }

    #[test]
    fn frost_uploads_one_texture_the_backend_can_take() {
        let ctx = egui::Context::default();
        let panel = Rect::from_min_size(Pos2::new(10.0, 10.0), egui::vec2(200.0, 60.0));
        let content = ctx.content_rect();
        assert!(content.is_positive(), "the test viewport has no content");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_background(ui.ctx(), content);
            frost(ui.ctx(), panel, GLASS_RADIUS, frost_layer());
        });
        output.textures_delta.clear();
        let manager = ctx.tex_manager();
        let textures = manager.read();
        let bakes: Vec<_> = textures
            .allocated()
            .filter(|(_, meta)| meta.name.contains("frost"))
            .map(|(_, meta)| meta)
            .collect();
        assert_eq!(bakes.len(), 1, "frost must not allocate more than one bake");
        let limit = ctx.input(|input| input.max_texture_side);
        assert!(
            bakes[0].size[0] <= limit && bakes[0].size[1] <= limit,
            "bake {:?} must fit a {limit} side",
            bakes[0].size
        );
    }

    #[test]
    fn the_bake_survives_a_resize_within_one_texel() {
        let small = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0));
        let bake = FrostBake {
            content: small,
            texel: FROST_TEXEL,
            dark: true,
            image: Arc::new(egui::ColorImage::new([1, 1], vec![Color32::BLACK])),
        };
        assert!(bake.fits(small, FROST_TEXEL, true));
        assert!(
            bake.fits(
                Rect::from_min_size(Pos2::ZERO, egui::vec2(403.0, 300.0)),
                FROST_TEXEL,
                true
            ),
            "sub-texel growth must not rebake"
        );
        assert!(!bake.fits(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(420.0, 300.0)),
            FROST_TEXEL,
            true
        ));
        assert!(
            !bake.fits(small, FROST_TEXEL * 2.0, true),
            "a coarser texel must rebake"
        );
        assert!(
            !bake.fits(small, FROST_TEXEL, false),
            "a theme flip must rebake"
        );
    }
}
