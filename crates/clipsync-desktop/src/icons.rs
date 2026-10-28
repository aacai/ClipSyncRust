//! Material icon geometry, rasterised once into a texture atlas.
//!
//! The path data is the same 24x24 viewport data `UiIcons.kt` hands to Compose's
//! `ImageVector`, so both clients draw identical outlines. egui can only fill
//! convex polygons, so the shapes are filled on the CPU with a non-zero winding
//! scanline fill and uploaded as one atlas.

use std::sync::OnceLock;

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, CursorIcon, Pos2, Rect, Response, Sense, Shape,
    TextureId, TextureOptions, Ui, Vec2,
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Icon {
    Search,
    Send,
    Plus,
    Select,
    Undo,
    Trash,
    Copy,
    Pin,
    More,
    Key,
    Help,
    Settings,
    Logout,
    Close,
    Check,
    Warning,
    Devices,
    Clipboard,
    Refresh,
    Export,
    Edit,
    Note,
    Duplicate,
    Info,
    ArrowUp,
    ArrowDown,
    Top,
    Bottom,
    Position,
    Shield,
    Translate,
    Attach,
    Download,
    OpenInNew,
    ArrowBack,
    ChevronRight,
}

/// Atlas slots are assigned by declaration order.
const ALL: [Icon; 36] = [
    Icon::Search,
    Icon::Send,
    Icon::Plus,
    Icon::Select,
    Icon::Undo,
    Icon::Trash,
    Icon::Copy,
    Icon::Pin,
    Icon::More,
    Icon::Key,
    Icon::Help,
    Icon::Settings,
    Icon::Logout,
    Icon::Close,
    Icon::Check,
    Icon::Warning,
    Icon::Devices,
    Icon::Clipboard,
    Icon::Refresh,
    Icon::Export,
    Icon::Edit,
    Icon::Note,
    Icon::Duplicate,
    Icon::Info,
    Icon::ArrowUp,
    Icon::ArrowDown,
    Icon::Top,
    Icon::Bottom,
    Icon::Position,
    Icon::Shield,
    Icon::Translate,
    Icon::Attach,
    Icon::Download,
    Icon::OpenInNew,
    Icon::ArrowBack,
    Icon::ChevronRight,
];

const VIEWPORT: f32 = 24.0;
const CELL: usize = 64;
const COLS: usize = 8;
const SUB_ROWS: usize = 4;

struct Atlas {
    texture: TextureId,
    uv: [Rect; ALL.len()],
}

static ATLAS: OnceLock<Atlas> = OnceLock::new();

pub fn install(ctx: &egui::Context) {
    if ATLAS.get().is_some() {
        return;
    }

    let rows = ALL.len().div_ceil(COLS);
    let width = COLS * CELL;
    let height = rows * CELL;
    let mut pixels = vec![Color32::TRANSPARENT; width * height];
    let mut uv = [Rect::NOTHING; ALL.len()];

    for (slot, icon) in ALL.iter().enumerate() {
        let col = (slot % COLS) * CELL;
        let row = slot / COLS * CELL;
        rasterize(&contours(*icon), |x, y, coverage| {
            pixels[(row + y) * width + col + x] = Color32::from_gray(coverage);
        });
        uv[slot] = Rect::from_min_max(
            Pos2::new(col as f32 / width as f32, row as f32 / height as f32),
            Pos2::new(
                (col + CELL) as f32 / width as f32,
                (row + CELL) as f32 / height as f32,
            ),
        );
    }

    let texture = ctx.tex_manager().write().alloc(
        "clipsync-icons".to_owned(),
        ColorImage::new([width, height], pixels).into(),
        TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        },
    );

    let _ = ATLAS.set(Atlas { texture, uv });
}

pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let Some(atlas) = ATLAS.get() else {
        return;
    };
    let slot = ALL
        .iter()
        .position(|candidate| *candidate == icon)
        .unwrap_or(0);
    painter.add(Shape::image(atlas.texture, rect, atlas.uv[slot], color));
}

/// 应用 logo，和安装包里的 `.icns` / `.ico` 是同一张图（`packaging/app.png`）。
const APP_ICON_PNG: &[u8] = include_bytes!("../packaging/app.png");
/// 托盘槽位只有 16~24px，拿 256 的原图过去会被系统缩成一团糊。
const TRAY_ICON_PNG: &[u8] = include_bytes!("../packaging/hicolor/32x32/apps/clipsync-rust.png");

fn decode(png: &[u8]) -> egui::IconData {
    let decoded = image::load_from_memory(png)
        .expect("内嵌的 app 图标解不开")
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    egui::IconData {
        rgba: decoded.into_raw(),
        width,
        height,
    }
}

/// 任务栏 / 窗口图标要的位图。自己画的剪贴板底板和真正的 app 图标对不上，
/// 装完系统在任务栏里看着就不是同一个应用。
pub fn app_icon() -> egui::IconData {
    decode(APP_ICON_PNG)
}

pub fn tray_icon() -> egui::IconData {
    decode(TRAY_ICON_PNG)
}

pub fn icon_button(ui: &mut Ui, icon: Icon, enabled: bool, tip: &str) -> Response {
    icon_button_tinted(ui, icon, theme::p().text, enabled, tip)
}
pub fn icon_button_tinted(
    ui: &mut Ui,
    icon: Icon,
    color: Color32,
    enabled: bool,
    tip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::splat(33.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), Color32::from_white_alpha(10));
    }
    let color = if enabled { color } else { theme::p().faint };
    paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(19.0)),
        icon,
        color,
    );
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

// ------------------------------------------------------------------------------------
// geometry
// ------------------------------------------------------------------------------------

type Contour = Vec<Pos2>;

fn contours(icon: Icon) -> Vec<Contour> {
    use Icon::*;
    let data = match icon {
        Search => SEARCH,
        Send => SEND,
        Plus => ADD,
        Select => SELECT_ALL,
        Undo => UNDO,
        Trash => DELETE,
        Copy => CONTENT_COPY,
        Pin => PUSH_PIN,
        More => MORE_VERT,
        Key => VPN_KEY,
        Help => HELP_OUTLINE,
        Settings => SETTINGS,
        Logout => LOGOUT,
        Close => CLOSE,
        Check => CHECK_CIRCLE,
        Warning => WARNING,
        Devices => LAPTOP,
        Clipboard => CONTENT_PASTE,
        Refresh => REFRESH,
        Export => FILE_UPLOAD,
        Edit => EDIT,
        Note => EVENT_NOTE,
        Duplicate => FILTER_NONE,
        Info => INFO,
        ArrowUp => ARROW_UPWARD,
        ArrowDown => ARROW_DOWNWARD,
        Top => VERTICAL_ALIGN_TOP,
        Bottom => VERTICAL_ALIGN_BOTTOM,
        Position => FORMAT_LIST_NUMBERED,
        Shield => VERIFIED_USER,
        Translate => TRANSLATE,
        Attach => LINK,
        Download => FILE_DOWNLOAD,
        OpenInNew => OPEN_IN_NEW,
        ArrowBack => ARROW_BACK,
        ChevronRight => CHEVRON_RIGHT,
    };
    parse(data)
}

/// Flatten an SVG path into polygons in the 24x24 icon viewport.
fn parse(data: &str) -> Vec<Contour> {
    let mut scan = Scan::new(data);
    let mut contours: Vec<Contour> = Vec::new();
    let mut current = Contour::new();
    let mut start = Pos2::ZERO;
    let mut cursor = Pos2::ZERO;
    // Second control point of the previous cubic, reflected by smooth cubics.
    let mut last_control: Option<Pos2> = None;
    let mut previous = 0u8;

    loop {
        let before = scan.index;
        let Some(command) = scan.command(previous) else {
            break;
        };
        previous = command;
        let relative = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            b'M' => {
                cursor = scan.point(cursor, relative);
                close(&mut contours, &mut current);
                current.push(cursor);
                start = cursor;
                last_control = None;
                while let Some(point) = scan.optional_point(cursor, relative) {
                    current.push(point);
                    cursor = point;
                }
            }
            b'L' => {
                while let Some(point) = scan.optional_point(cursor, relative) {
                    current.push(point);
                    cursor = point;
                }
                last_control = None;
            }
            b'H' => {
                while let Some(x) = scan.number() {
                    cursor.x = if relative { cursor.x + x } else { x };
                    current.push(cursor);
                }
                last_control = None;
            }
            b'V' => {
                while let Some(y) = scan.number() {
                    cursor.y = if relative { cursor.y + y } else { y };
                    current.push(cursor);
                }
                last_control = None;
            }
            b'C' => {
                while let Some(first) = scan.optional_point(cursor, relative) {
                    let second = scan.point(cursor, relative);
                    let end = scan.point(cursor, relative);
                    cubic(&mut current, cursor, first, second, end);
                    last_control = Some(second);
                    cursor = end;
                }
            }
            b'S' => {
                while let Some(second) = scan.optional_point(cursor, relative) {
                    let first = match last_control {
                        Some(control) => {
                            Pos2::new(2.0 * cursor.x - control.x, 2.0 * cursor.y - control.y)
                        }
                        None => cursor,
                    };
                    let end = scan.point(cursor, relative);
                    cubic(&mut current, cursor, first, second, end);
                    last_control = Some(second);
                    cursor = end;
                }
            }
            b'A' => {
                while let Some(rx) = scan.optional_number() {
                    let ry = scan.number().unwrap_or(0.0);
                    let rotation = scan.number().unwrap_or(0.0);
                    let large = scan.flag();
                    let sweep = scan.flag();
                    let end = scan.point(cursor, relative);
                    arc(&mut current, cursor, end, rx, ry, rotation, large, sweep);
                    last_control = None;
                    cursor = end;
                }
            }
            b'Z' => {
                if current.first() != current.last() {
                    current.push(start);
                }
                close(&mut contours, &mut current);
                cursor = start;
                last_control = None;
            }
            other => unreachable!("unsupported path command {}", other as char),
        }
        if scan.index == before {
            break;
        }
    }

    close(&mut contours, &mut current);
    contours
}

fn close(contours: &mut Vec<Contour>, current: &mut Contour) {
    if current.len() > 2 {
        contours.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn cubic(out: &mut Contour, p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) {
    flatten(out, p0, p1, p2, p3, 0);
}

/// Subdivide until the curve is flatter than the raster grid, then emit the chord.
fn flatten(out: &mut Contour, p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, depth: u8) {
    let chord = p3 - p0;
    let length = chord.length();
    if depth > 12 || length < 0.02 {
        out.push(p3);
        return;
    }
    let normal = Vec2::new(chord.y, -chord.x) / length;
    let bow = ((p1 - p0).dot(normal) + (p2 - p0).dot(normal))
        .abs()
        .max(((p1 - p3).dot(normal) + (p2 - p3).dot(normal)).abs());
    if bow < 0.02 {
        out.push(p3);
        return;
    }
    let a = lerp(p0, p1, 0.5);
    let b = lerp(p1, p2, 0.5);
    let c = lerp(p2, p3, 0.5);
    let d = lerp(a, b, 0.5);
    let e = lerp(b, c, 0.5);
    let mid = lerp(d, e, 0.5);
    flatten(out, p0, a, d, mid, depth + 1);
    flatten(out, mid, e, c, p3, depth + 1);
}

/// Convert an SVG elliptical arc into cubic beziers, then flatten them.
fn arc(
    out: &mut Contour,
    p0: Pos2,
    p1: Pos2,
    rx: f32,
    ry: f32,
    degrees: f32,
    large: bool,
    sweep: bool,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(p1);
        return;
    }
    let angle = degrees.to_radians();
    let (cos_a, sin_a) = (angle.cos(), angle.sin());

    let half = (p0 - p1) / 2.0;
    let x1 = cos_a * half.x + sin_a * half.y;
    let y1 = -sin_a * half.x + cos_a * half.y;

    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let shared = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let numerator = (rx * rx * ry * ry - shared).max(0.0);
    let factor =
        (numerator / shared.max(f32::EPSILON)).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cx = factor * rx * y1 / ry;
    let cy = factor * -(ry * x1 / rx);
    let center = Pos2::new(
        cos_a * cx - sin_a * cy + (p0.x + p1.x) / 2.0,
        sin_a * cx + cos_a * cy + (p0.y + p1.y) / 2.0,
    );

    let start = angle_of((x1 - cx) / rx, (y1 - cy) / ry);
    let mut delta = angle_of((-x1 - cx) / rx, (-y1 - cy) / ry) - start;
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }

    let segments = (delta.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / segments as f32;
    let alpha = (step / 4.0).tan() * 4.0 / 3.0;

    let point = |theta: f32| {
        let (c, s) = (theta.cos(), theta.sin());
        Pos2::new(
            cos_a * rx * c - sin_a * ry * s + center.x,
            sin_a * rx * c + cos_a * ry * s + center.y,
        )
    };
    let tangent = |theta: f32| {
        let (c, s) = (theta.cos(), theta.sin());
        Vec2::new(
            -cos_a * rx * s - sin_a * ry * c,
            -sin_a * rx * s + cos_a * ry * c,
        )
    };

    let mut theta = start;
    for _ in 0..segments {
        let at = point(theta);
        let until = point(theta + step);
        cubic(
            out,
            at,
            at + tangent(theta) * alpha,
            until - tangent(theta + step) * alpha,
            until,
        );
        theta += step;
    }
}

/// Signed angle of a vector, matching SVG's `angleTo` helper.
fn angle_of(x: f32, y: f32) -> f32 {
    let magnitude = (x * x + y * y).sqrt();
    let value = (x / magnitude.max(f32::EPSILON)).clamp(-1.0, 1.0).acos();
    if y < 0.0 {
        -value
    } else {
        value
    }
}

fn lerp(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    a + (b - a) * t
}

struct Scan<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Scan<'a> {
    fn new(data: &'a str) -> Self {
        Self {
            bytes: data.as_bytes(),
            index: 0,
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.bytes.get(self.index).copied(),
            Some(b' ' | b',' | b'\t' | b'\n' | b'\r')
        ) {
            self.index += 1;
        }
    }

    /// Consume a command letter, or repeat the previous one for implicit chains.
    fn command(&mut self, previous: u8) -> Option<u8> {
        self.skip_separators();
        match self.bytes.get(self.index).copied() {
            Some(byte) if byte.is_ascii_alphabetic() => {
                self.index += 1;
                Some(byte)
            }
            Some(_)
                if matches!(
                    previous.to_ascii_uppercase(),
                    b'L' | b'H' | b'V' | b'C' | b'S' | b'A'
                ) =>
            {
                Some(previous)
            }
            _ => None,
        }
    }

    /// Arc flags are single digits that may sit flush against the next number.
    fn flag(&mut self) -> bool {
        self.skip_separators();
        match self.bytes.get(self.index).copied() {
            Some(b'1') => {
                self.index += 1;
                true
            }
            Some(b'0') => {
                self.index += 1;
                false
            }
            _ => self.number().unwrap_or(0.0) != 0.0,
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.index;
        let mut end = self.index;
        if matches!(self.bytes.get(end).copied(), Some(b'+') | Some(b'-')) {
            end += 1;
        }
        let mut digits = 0;
        while matches!(self.bytes.get(end).copied(), Some(byte) if byte.is_ascii_digit()) {
            end += 1;
            digits += 1;
        }
        if self.bytes.get(end).copied() == Some(b'.') {
            end += 1;
            while matches!(self.bytes.get(end).copied(), Some(byte) if byte.is_ascii_digit()) {
                end += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            self.index = start;
            return None;
        }
        if matches!(self.bytes.get(end).copied(), Some(b'e') | Some(b'E')) {
            let mut exponent = end + 1;
            if matches!(self.bytes.get(exponent).copied(), Some(b'+') | Some(b'-')) {
                exponent += 1;
            }
            let mut exponent_digits = 0;
            while matches!(self.bytes.get(exponent).copied(), Some(byte) if byte.is_ascii_digit()) {
                exponent += 1;
                exponent_digits += 1;
            }
            if exponent_digits > 0 {
                end = exponent;
            }
        }
        self.index = end;
        std::str::from_utf8(&self.bytes[start..end])
            .ok()?
            .parse::<f32>()
            .ok()
    }

    fn optional_number(&mut self) -> Option<f32> {
        self.number()
    }

    fn point(&mut self, origin: Pos2, relative: bool) -> Pos2 {
        let x = self.number().unwrap_or(0.0);
        let y = self.number().unwrap_or(0.0);
        offset(origin, x, y, relative)
    }

    fn optional_point(&mut self, origin: Pos2, relative: bool) -> Option<Pos2> {
        let mark = self.index;
        let Some(x) = self.number() else {
            self.index = mark;
            return None;
        };
        let Some(y) = self.number() else {
            self.index = mark;
            return None;
        };
        Some(offset(origin, x, y, relative))
    }
}

fn offset(origin: Pos2, x: f32, y: f32, relative: bool) -> Pos2 {
    if relative {
        Pos2::new(origin.x + x, origin.y + y)
    } else {
        Pos2::new(x, y)
    }
}

// ------------------------------------------------------------------------------------
// fill
// ------------------------------------------------------------------------------------

/// Non-zero winding scanline fill of the 24x24 viewport into a `CELL` grid.
fn rasterize(contours: &[Contour], mut put: impl FnMut(usize, usize, u8)) {
    let scale = CELL as f32 / VIEWPORT;
    let mut coverage = vec![0.0f32; CELL * CELL];
    let mut crossings: Vec<(f32, f32)> = Vec::new();

    for row in 0..CELL {
        for sub in 0..SUB_ROWS {
            let y = row as f32 + (sub as f32 + 0.5) / SUB_ROWS as f32;
            crossings.clear();
            for contour in contours {
                let edges = contour.len();
                if edges < 2 {
                    continue;
                }
                for index in 0..edges {
                    let a = contour[index] * scale;
                    let b = contour[(index + 1) % edges] * scale;
                    let (from, to, direction) = if a.y < b.y { (a, b, 1.0) } else { (b, a, -1.0) };
                    if from.y <= y && y < to.y && to.y > from.y {
                        let t = (y - from.y) / (to.y - from.y);
                        crossings.push((from.x + t * (to.x - from.x), direction));
                    }
                }
            }
            if crossings.is_empty() {
                continue;
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut winding = 0.0f32;
            let mut span = 0.0f32;
            for (x, direction) in std::mem::take(&mut crossings) {
                let before = winding;
                winding += direction;
                if before == 0.0 && winding != 0.0 {
                    span = x;
                } else if before != 0.0 && winding == 0.0 {
                    paint_span(&mut coverage, row, span, x);
                }
            }
        }
    }

    for row in 0..CELL {
        for col in 0..CELL {
            let value = coverage[row * CELL + col] / SUB_ROWS as f32;
            if value > 0.0 {
                put(col, row, (value * 255.0).round().clamp(1.0, 255.0) as u8);
            }
        }
    }
}

fn paint_span(coverage: &mut [f32], row: usize, from: f32, to: f32) {
    if to <= from {
        return;
    }
    let first = from.max(0.0) as usize;
    let last = (to.ceil() as usize).min(CELL);
    for col in first..last {
        let overlap = to.min(col as f32 + 1.0) - from.max(col as f32);
        if overlap > 0.0 {
            coverage[row * CELL + col] += overlap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(icon: Icon) -> Vec<u8> {
        let mut grid = vec![0u8; CELL * CELL];
        rasterize(&contours(icon), |x, y, value| grid[y * CELL + x] = value);
        grid
    }

    #[test]
    fn app_logo_decodes_for_the_tray_and_the_window() {
        let data = app_icon();
        assert_eq!((data.width, data.height), (256, 256));
        assert_eq!(data.rgba.len(), (256 * 256 * 4) as usize);
        let opaque = data.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(
            opaque > data.rgba.len() / 8,
            "logo 几乎是空的：{opaque} 个像素"
        );

        // 托盘切片和窗口那张是同一个 logo 缩出来的，别哪天只剩一个尺寸。
        let tray = tray_icon();
        assert_eq!((tray.width, tray.height), (32, 32));
        assert_eq!(tray.rgba.len(), (32 * 32 * 4) as usize);
    }

    #[test]
    fn absolute_and_relative_commands_agree() {
        let relative = parse("M2,3 h4 v4 h-4 z");
        let absolute = parse("M2,3 L6,3 L6,7 L2,7 Z");
        assert_eq!(relative[0].len(), 5);
        for (a, b) in relative[0].iter().zip(&absolute[0]) {
            assert_eq!((a.x, a.y), (b.x, b.y));
        }
    }

    #[test]
    fn implicit_lineto_after_moveto() {
        // Send is a moveto followed by bare coordinate pairs.
        let contours = parse("M2.01,21L23,12 2.01,3 2,10l15,2 -15,2z");
        assert_eq!(contours.len(), 1);
        assert_eq!(contours[0].len(), 7);
        assert_eq!(contours[0][1], Pos2::new(23.0, 12.0));
        assert_eq!(contours[0][2], Pos2::new(2.01, 3.0));
    }

    #[test]
    fn smooth_cubic_reflects_the_previous_control_point() {
        let straight = parse("M0,0 C0,10 10,10 10,10 S10,10 20,20");
        let mirrored = parse("M0,0 C0,10 10,10 10,10 C10,10 10,10 20,20");
        assert_eq!(straight.len(), 1);
        let last = |contours: &Vec<Contour>| contours[0][contours[0].len() - 1];
        assert_eq!(last(&straight), last(&mirrored));
        assert!(straight[0].len() > 2);
    }

    #[test]
    fn arcs_stay_on_the_circle() {
        let contours = parse("M2,12 A10,10 0 0,1 22,12");
        let radius = 10.0;
        for point in &contours[0] {
            let offset = *point - Pos2::new(12.0, 12.0);
            assert!(
                (offset.length() - radius).abs() < 0.1,
                "{point:?} left the circle"
            );
        }
    }

    #[test]
    fn winding_rule_punches_holes() {
        let ring = parse("M0,0 H24 V24 H0 Z M6,6 V18 H18 V6 Z");
        let grid = {
            let mut grid = vec![0u8; CELL * CELL];
            rasterize(&ring, |x, y, value| grid[y * CELL + x] = value);
            grid
        };
        assert_eq!(grid[0], 255);
        assert_eq!(grid[(CELL / 2) * CELL + CELL / 2], 0);

        let same_winding = parse("M0,0 H24 V24 H0 Z M6,6 H18 V18 V6 Z");
        let filled = {
            let mut grid = vec![0u8; CELL * CELL];
            rasterize(&same_winding, |x, y, value| grid[y * CELL + x] = value);
            grid
        };
        assert_eq!(filled[(CELL / 2) * CELL + CELL / 2], 255);
    }

    #[test]
    fn every_icon_is_inked_and_fits_the_cell() {
        for icon in ALL {
            let grid = alpha(icon);
            let inked: usize = grid.iter().filter(|value| **value > 8).count();
            let area = inked as f32 / (CELL * CELL) as f32;
            assert!(area > 0.01, "{icon:?} renders nothing");
            assert!(area < 0.9, "{icon:?} floods the cell");
        }
    }
}

// Path data copied from ClipSync's `UiIcons.kt`, which Compose feeds to ImageVector.
const ARROW_BACK: &str = "M20,11H7.83l5.59,-5.59L12,4l-8,8 8,8 1.41,-1.41L7.83,13H20v-2z";
const SETTINGS: &str = "M19.14,12.94c0.04,-0.3,0.06,-0.61,0.06,-0.94c0,-0.32,-0.02,-0.64,-0.07,-0.94l2.03,-1.58a0.49,0.49,0,0,0,0.12,-0.61l-1.92,-3.32a0.488,0.488,0,0,0,-0.59,-0.22l-2.39,0.96c-0.5,-0.38,-1.03,-0.7,-1.62,-0.94l-0.36,-2.54a0.484,0.484,0,0,0,-0.48,-0.41h-3.84c-0.24,0,-0.43,0.17,-0.47,0.41l-0.36,2.54c-0.59,0.24,-1.13,0.57,-1.62,0.94l-2.39,-0.96c-0.22,-0.08,-0.47,0,-0.59,0.22L2.74,8.87c-0.12,0.21,-0.08,0.47,0.12,0.61l2.03,1.58c-0.05,0.3,-0.09,0.63,-0.09,0.94s0.02,0.64,0.07,0.94l-2.03,1.58a0.49,0.49,0,0,0,-0.12,0.61l1.92,3.32c0.12,0.22,0.37,0.29,0.59,0.22l2.39,-0.96c0.5,0.38,1.03,0.7,1.62,0.94l0.36,2.54c0.05,0.24,0.24,0.41,0.48,0.41h3.84c0.24,0,0.44,-0.17,0.47,-0.41l0.36,-2.54c0.59,-0.24,1.13,-0.56,1.62,-0.94l2.39,0.96c0.22,0.08,0.47,0,0.59,-0.22l1.92,-3.32c0.12,-0.22,0.07,-0.47,-0.12,-0.61l-2.01,-1.58zM12,15.6c-1.98,0,-3.6,-1.62,-3.6,-3.6s1.62,-3.6,3.6,-3.6s3.6,1.62,3.6,3.6s-1.62,3.6,-3.6,3.6z";
const EDIT: &str = "M3,17.25V21h3.75L17.81,9.94l-3.75,-3.75L3,17.25zM20.71,7.04c0.39,-0.39 0.39,-1.02 0,-1.41l-2.34,-2.34c-0.39,-0.39 -1.02,-0.39 -1.41,0l-1.83,1.83 3.75,3.75 1.83,-1.83z";
const INFO: &str = "M11,7h2v2h-2zM11,11h2v6h-2zM12,2C6.48,2 2,6.48 2,12s4.48,10 10,10 10,-4.48 10,-10S17.52,2 12,2zM10,17v-4h2v4h-2zM14,17h-2v-6h-2v-2h4v8z";
const HELP_OUTLINE: &str = "M11,18h2v-2h-2v2zM12,2C6.48,2 2,6.48 2,12s4.48,10 10,10 10,-4.48 10,-10S17.52,2 12,2zM12,20c-4.41,0 -8,-3.59 -8,-8s3.59,-8 8,-8 8,3.59 8,8 -3.59,8 -8,8zM12,6c-2.21,0 -4,1.79 -4,4h2c0,-1.1 0.9,-2 2,-2 1.1,0 2,0.9 2,2 0,2 -3,1.75 -3,5h2c0,-2.25 3,-2.5 3,-5 0,-2.21 -1.79,-4 -4,-4z";
const EVENT_NOTE: &str = "M17,10L7,10v2h10v-2zM19,3h-1L18,1h-2v2L8,3L8,1L6,1v2L5,3c-1.11,0 -1.99,0.9 -1.99,2L3,19c0,1.1 0.89,2 2,2h14c1.1,0 2,-0.9 2,-2L21,5c0,-1.1 -0.9,-2 -2,-2zM19,19L5,19L5,8h14v11zM14,14L7,14v2h7v-2z";
const ARROW_UPWARD: &str = "M4,12l1.41,1.41L11,7.83V20h2V7.83l5.58,5.59L20,12l-8,-8 -8,8z";
const ARROW_DOWNWARD: &str = "M20,12l-1.41,-1.41L13,16.17V4h-2v12.17l-5.58,-5.59L4,12l8,8 8,-8z";
const VERTICAL_ALIGN_TOP: &str = "M8,11h3v10h2V11h3l-4,-4 -4,4zM4,3v2h16V3H4z";
const VERTICAL_ALIGN_BOTTOM: &str = "M8,13h3V3h2v10h3l-4,4 -4,-4zM4,19v2h16v-2H4z";
const FORMAT_LIST_NUMBERED: &str = "M2,17h2v0.5H3v1h1v0.5H2v1h3v-4H2v1zM3,8h1V4H2v1h1v3zM2,11h1.8L2,13.1v0.9h3v-1H3.2L5,10.9V10H2v1zM20,4h-8v2h8V4zM20,12h-8v2h8v-2zM20,20h-8v2h8v-2z";
const SELECT_ALL: &str = "M19,5v14H5V5H19M19,3H5C3.9,3 3,3.9 3,5v14c0,1.1 0.9,2 2,2h14c1.1,0 2,-0.9 2,-2V5c0,-1.1-0.9,-2-2,-2L19,3z";
const FILTER_NONE: &str = "M3,5L1,5v16c0,1.1 0.9,2 2,2h16v-2L3,21L3,5zM21,1L7,1c-1.1,0 -2,0.9 -2,2v14c0,1.1 0.9,2 2,2h14c1.1,0 2,-0.9 2,-2L23,3c0,-1.1 -0.9,-2 -2,-2zM21,17L7,17L7,3h14v14z";
const TRANSLATE: &str = "M12.87,15.07l-2.54,-2.51 0.03,-0.03c1.74,-1.94 2.98,-4.17 3.71,-6.53L17,6L17,4h-7L10,2L8,2v2L1,4v1.99h11.17C11.5,7.92 10.44,9.75 9,11.35 8.07,10.32 7.3,9.19 6.69,8h-2c0.73,1.63 1.73,3.17 2.98,4.56l-5.09,5.02L4,19l5,-5 3.11,3.11 0.76,-2.04zM18.5,10h-2L12,22h2l1.12,-3h4.75L21,22h2l-4.5,-12zM15.64,17l1.62,-4.33L18.87,17h-3.23z";
const MORE_VERT: &str = "M12,8c1.1,0 2,-0.9 2,-2s-0.9,-2 -2,-2 -2,0.9 -2,2 0.9,2 2,2zM12,10c-1.1,0 -2,0.9 -2,2s0.9,2 2,2 2,-0.9 2,-2 -0.9,-2 -2,-2zM12,16c-1.1,0 -2,0.9 -2,2s0.9,2 2,2 2,-0.9 2,-2 -0.9,-2 -2,-2z";
const FILE_UPLOAD: &str = "M9,16h6v-6h4l-7,-7 -7,7h4v6zM5,18h14v2H5v-2z";
const UNDO: &str = "M12.5,8c-2.65,0 -5.05,0.99 -6.9,2.6L2,9v9h9l-3.62,-3.62c1.39,-1.16 3.16,-1.88 5.12,-1.88 3.95,0 7.29,2.63 8.33,6.26L23.09,14.94C21.88,10.96 17.65,8 12.5,8z";
const ADD: &str = "M19,13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z";
const CONTENT_PASTE: &str = "M19,2h-4.18C14.4,0.84 13.3,0 12,0c-1.3,0 -2.4,0.84 -2.82,2H5c-1.1,0 -2,0.9 -2,2v16c0,1.1 0.9,2 2,2h14c1.1,0 2,-0.9 2,-2V4c0,-1.1 -0.9,-2 -2,-2zM12,2c0.55,0 1,0.45 1,1s-0.45,1 -1,1 -1,-0.45 -1,-1 0.45,-1 1,-1zM19,20H5V4h2v3h10V4h2v16z";
const CONTENT_COPY: &str = "M16,1H4C2.9,1 2,1.9 2,3v14h2V3h12V1zM19,5H8C6.9,5 6,5.9 6,7v14c0,1.1 0.9,2 2,2h11c1.1,0 2,-0.9 2,-2V7C21,5.9 20.1,5 19,5zM19,21H8V7h11V21z";
const PUSH_PIN: &str = "M16,9V4h1c0.55,0 1,-0.45 1,-1s-0.45,-1 -1,-1H7C6.45,2 6,2.45 6,3s0.45,1 1,1h1v5c0,1.66 -1.34,3 -3,3v2h5.97v7l1,1 1,-1v-7H19v-2c-1.66,0 -3,-1.34 -3,-3z";
const DELETE: &str =
    "M6,19c0,1.1 0.9,2 2,2h8c1.1,0 2,-0.9 2,-2V7H6V19zM19,4h-3.5l-1,-1h-5l-1,1H5v2h14V4z";
const FILE_DOWNLOAD: &str = "M19,9h-4V3H9v6H5l7,7L19,9zM5,18v2h14v-2H5z";
const OPEN_IN_NEW: &str = "M19,19H5V5h7V3H5C3.89,3 3,3.9 3,5v14c0,1.1 0.89,2 2,2h14c1.1,0 2,-0.9 2,-2v-7h-2V19zM14,3v2h3.59l-9.83,9.83 1.41,1.41L19,6.41V10h2V3H14z";
const SEND: &str = "M2.01,21L23,12 2.01,3 2,10l15,2 -15,2z";
const LINK: &str = "M3.9,12c0,-1.71 1.39,-3.1 3.1,-3.1h4V7H7C4.24,7 2,9.24 2,12s2.24,5 5,5h4v-1.9H7c-1.71,0 -3.1,-1.39 -3.1,-3.1zM8.5,11h7v2h-7V11zM17,7h-4v1.9h4c1.71,0 3.1,1.39 3.1,3.1s-1.39,3.1 -3.1,3.1h-4V17h4c2.76,0 5,-2.24 5,-5s-2.24,-5 -5,-5z";
const SEARCH: &str = "M15.5,14h-0.79l-0.28,-0.27C15.41,12.59 16,11.11 16,9.5 16,5.91 13.09,3 9.5,3S3,5.91 3,9.5 5.91,16 9.5,16c1.61,0 3.09,-0.59 4.23,-1.57l0.27,0.28v0.79l5,4.99L20.49,19l-4.99,-5zM9.5,14C7.01,14 5,11.99 5,9.5S7.01,5 9.5,5 14,7.01 14,9.5 11.99,14 9.5,14z";
const CLOSE: &str = "M19,6.41L17.59,5 12,10.59 6.41,5 5,6.41 10.59,12 5,17.59 6.41,19 12,13.41 17.59,19 19,17.59 13.41,12z";
const VERIFIED_USER: &str = "M12,1L3,5v6c0,5.55 3.84,10.74 9,12 5.16,-1.26 9,-6.45 9,-12V5L12,1zM10.94,15.54L7.5,12.1l1.41,-1.41 2.03,2.03 4.65,-4.65 1.41,1.41 -6.06,6.06z";
const REFRESH: &str = "M17.65,6.35C16.2,4.9 14.21,4 12,4c-4.42,0 -7.99,3.58 -8,8s3.58,8 8,8c3.73,0 6.84,-2.55 7.73,-6h-2.08c-0.82,2.33 -3.04,4 -5.65,4 -3.31,0 -6,-2.69 -6,-6s2.69,-6 6,-6c1.66,0 3.14,0.69 4.22,1.78L13,11h7V4l-2.35,2.35z";
const CHECK_CIRCLE: &str = "M12,2C6.48,2 2,6.48 2,12s4.48,10 10,10 10,-4.48 10,-10S17.52,2 12,2zM10,17l-5,-5 1.41,-1.41L10,14.17l7.59,-7.59L19,8l-9,9z";
const WARNING: &str = "M1,21h22L12,2 1,21zM13,18h-2v-2h2v2zM13,14h-2v-4h2v4z";
const VPN_KEY: &str = "M12.65,10C11.83,7.67 9.61,6 7,6c-3.31,0 -6,2.69 -6,6s2.69,6 6,6c2.61,0 4.83,-1.67 5.65,-4H17v4h4v-4h2v-2H12.65zM7,14c-1.1,0 -2,-0.9 -2,-2s0.9,-2 2,-2 2,0.9 2,2 -0.9,2 -2,2z";
const LAPTOP: &str = "M20,18c1.1,0 1.99,-0.9 1.99,-2L22,5c0,-1.1 -0.9,-2 -2,-2H4c-1.1,0 -2,0.9 -2,2v11c0,1.1 0.9,2 2,2H0v2h24v-2h-4zM4,5h16v11H4V5z";
const LOGOUT: &str = "M17,7l-1.41,1.41L18.17,11H8v2h10.17l-2.58,2.58L17,17l5,-5zM4,5h8V3H4c-1.1,0 -2,0.9 -2,2v14c0,1.1 0.9,2 2,2h8v-2H4V5z";
const CHEVRON_RIGHT: &str = "M8.59,16.34l4.58,-4.59 -4.58,-4.59L10,5.75l6,6 -6,6z";
