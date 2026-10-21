use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    Move,
    Shield,
    Globe,
    Attach,
    Download,
    OpenInNew,
}

pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 24.0;
    let pen = Pen {
        painter,
        origin: rect.min,
        scale,
        stroke: Stroke::new((rect.width() * 0.078).max(1.25), color),
    };
    use Icon::*;
    match icon {
        Search => {
            pen.circle(10.5, 10.5, 5.6);
            pen.line((14.6, 14.6), (19.5, 19.5));
        }
        Send => {
            pen.polyline(
                &[(3.0, 11.0), (21.0, 3.5), (13.0, 21.0), (10.2, 13.2)],
                true,
            );
            pen.line((10.2, 13.2), (21.0, 3.5));
        }
        Plus => {
            pen.line((12.0, 5.0), (12.0, 19.0));
            pen.line((5.0, 12.0), (19.0, 12.0));
        }
        Select => {
            pen.round_rect(4.0, 4.0, 20.0, 20.0, 3.0);
        }
        Undo => {
            pen.arc(12.0, 13.0, 7.8, 190.0, 350.0);
            pen.polyline(&[(4.4, 9.4), (4.2, 13.6), (8.2, 13.2)], false);
        }
        Trash => {
            pen.line((4.0, 7.0), (20.0, 7.0));
            pen.polyline(&[(9.0, 4.0), (15.0, 4.0), (15.0, 7.0)], false);
            pen.polyline(&[(9.0, 4.0), (9.0, 7.0)], false);
            pen.polyline(&[(6.0, 7.0), (7.2, 20.0), (16.8, 20.0), (18.0, 7.0)], false);
            pen.line((10.2, 11.0), (10.2, 16.6));
            pen.line((13.8, 11.0), (13.8, 16.6));
        }
        Copy => {
            pen.round_rect(8.0, 3.0, 20.0, 15.0, 2.0);
            pen.round_rect(4.0, 8.0, 16.0, 21.0, 2.0);
        }
        Pin => {
            pen.line((8.5, 3.0), (15.5, 3.0));
            pen.polyline(
                &[(10.0, 4.5), (14.0, 4.5), (13.0, 13.0), (11.0, 13.0)],
                true,
            );
            pen.line((12.0, 13.0), (12.0, 21.0));
        }
        More => {
            pen.disc(12.0, 5.2, 1.45);
            pen.disc(12.0, 12.0, 1.45);
            pen.disc(12.0, 18.8, 1.45);
        }
        Key => {
            pen.circle(8.0, 12.0, 4.2);
            pen.line((12.2, 12.0), (20.5, 12.0));
            pen.line((16.4, 12.0), (16.4, 16.2));
            pen.line((19.2, 12.0), (19.2, 15.4));
        }
        Help => {
            pen.circle(12.0, 12.0, 8.6);
            pen.text(12.0, 12.0, "?", 0.66);
        }
        Settings => {
            pen.circle(12.0, 12.0, 3.3);
            for step in 0..8 {
                let angle = step as f32 * std::f32::consts::TAU / 8.0;
                let (sin, cos) = angle.sin_cos();
                pen.line(
                    (12.0 + cos * 5.6, 12.0 + sin * 5.6),
                    (12.0 + cos * 8.4, 12.0 + sin * 8.4),
                );
            }
        }
        Logout => {
            pen.polyline(&[(10.0, 4.0), (4.5, 4.0), (4.5, 20.0), (10.0, 20.0)], false);
            pen.line((11.0, 12.0), (20.5, 12.0));
            pen.polyline(&[(17.0, 8.4), (20.6, 12.0), (17.0, 15.6)], false);
        }
        Close => {
            pen.line((6.0, 6.0), (18.0, 18.0));
            pen.line((18.0, 6.0), (6.0, 18.0));
        }
        Check => {
            pen.polyline(&[(5.0, 12.3), (10.0, 17.2), (19.2, 7.0)], false);
        }
        Warning => {
            pen.polyline(&[(12.0, 3.6), (21.2, 20.0), (2.8, 20.0)], true);
            pen.line((12.0, 9.4), (12.0, 14.2));
            pen.disc(12.0, 17.0, 1.05);
        }
        Devices => {
            pen.round_rect(2.8, 5.0, 15.2, 14.0, 1.6);
            pen.line((8.8, 14.0), (8.8, 18.0));
            pen.line((5.8, 18.2), (11.8, 18.2));
            pen.round_rect(16.0, 8.4, 21.2, 20.0, 1.6);
        }
        Clipboard => {
            pen.round_rect(5.0, 5.0, 19.0, 20.5, 2.0);
            pen.round_rect(9.0, 2.8, 15.0, 6.2, 1.0);
            pen.line((8.6, 11.0), (15.4, 11.0));
            pen.line((8.6, 15.0), (13.2, 15.0));
        }
        Refresh => {
            pen.arc(12.0, 12.0, 7.4, 20.0, 300.0);
            pen.polyline(&[(17.8, 4.4), (18.4, 9.0), (13.9, 8.2)], false);
        }
        Export => {
            pen.polyline(
                &[(5.0, 11.5), (5.0, 20.0), (19.0, 20.0), (19.0, 11.5)],
                false,
            );
            pen.line((12.0, 16.5), (12.0, 4.0));
            pen.polyline(&[(8.0, 8.0), (12.0, 4.0), (16.0, 8.0)], false);
        }
        Edit => {
            pen.polyline(
                &[
                    (4.5, 19.5),
                    (6.4, 13.4),
                    (16.0, 3.8),
                    (20.2, 8.0),
                    (10.6, 17.6),
                ],
                true,
            );
            pen.line((14.0, 5.8), (18.2, 10.0));
        }
        Note => {
            pen.polyline(
                &[
                    (5.0, 4.0),
                    (19.0, 4.0),
                    (19.0, 14.0),
                    (14.0, 19.5),
                    (5.0, 19.5),
                ],
                true,
            );
            pen.polyline(&[(14.0, 19.4), (14.0, 14.0), (19.0, 14.0)], false);
            pen.line((8.4, 9.0), (15.0, 9.0));
            pen.line((8.4, 13.0), (12.4, 13.0));
        }
        Duplicate => {
            pen.round_rect(7.0, 3.0, 17.0, 13.0, 2.0);
            pen.round_rect(3.0, 7.0, 13.0, 17.0, 2.0);
            pen.line((8.0, 12.0), (8.0, 16.0));
            pen.line((6.0, 14.0), (10.0, 14.0));
        }
        Info => {
            pen.circle(12.0, 12.0, 8.6);
            pen.disc(12.0, 7.8, 1.15);
            pen.line((12.0, 11.0), (12.0, 16.8));
        }
        ArrowUp => {
            pen.polyline(&[(6.0, 14.4), (12.0, 8.2), (18.0, 14.4)], false);
        }
        ArrowDown => {
            pen.polyline(&[(6.0, 9.6), (12.0, 15.8), (18.0, 9.6)], false);
        }
        Top => {
            pen.line((5.0, 4.0), (19.0, 4.0));
            pen.polyline(&[(6.0, 14.4), (12.0, 8.2), (18.0, 14.4)], false);
            pen.line((12.0, 9.0), (12.0, 20.0));
        }
        Bottom => {
            pen.line((5.0, 20.0), (19.0, 20.0));
            pen.polyline(&[(6.0, 9.6), (12.0, 15.8), (18.0, 9.6)], false);
            pen.line((12.0, 4.0), (12.0, 15.0));
        }
        Move => {
            pen.line((12.0, 3.5), (12.0, 20.5));
            pen.line((3.5, 12.0), (20.5, 12.0));
            pen.polyline(&[(9.4, 6.0), (12.0, 3.4), (14.6, 6.0)], false);
            pen.polyline(&[(9.4, 18.0), (12.0, 20.6), (14.6, 18.0)], false);
            pen.polyline(&[(6.0, 9.4), (3.4, 12.0), (6.0, 14.6)], false);
            pen.polyline(&[(18.0, 9.4), (20.6, 12.0), (18.0, 14.6)], false);
        }
        Shield => {
            pen.polyline(
                &[
                    (12.0, 3.0),
                    (20.0, 6.0),
                    (20.0, 12.0),
                    (12.0, 21.0),
                    (4.0, 12.0),
                    (4.0, 6.0),
                ],
                true,
            );
            pen.polyline(&[(8.2, 12.0), (11.0, 14.8), (16.0, 8.8)], false);
        }
        Globe => {
            pen.circle(12.0, 12.0, 8.6);
            pen.line((3.4, 12.0), (20.6, 12.0));
            pen.polyline(
                &[
                    (12.0, 3.4),
                    (8.6, 6.4),
                    (8.0, 11.0),
                    (9.6, 16.0),
                    (12.0, 20.6),
                    (14.2, 16.0),
                    (16.0, 11.0),
                    (15.2, 6.6),
                    (12.0, 3.4),
                ],
                false,
            );
        }
        Attach => {
            pen.line((16.5, 5.4), (16.5, 17.0));
            pen.arc(12.0, 17.0, 4.5, 0.0, 180.0);
            pen.line((7.5, 17.0), (7.5, 5.0));
            pen.arc(12.0, 5.0, 4.5, 180.0, 360.0);
            pen.line((9.2, 6.2), (9.2, 15.4));
            pen.arc(10.95, 15.4, 1.75, 0.0, 180.0);
            pen.line((12.7, 15.4), (12.7, 6.2));
            pen.line((9.2, 6.2), (12.7, 6.2));
        }
        Download => {
            pen.line((12.0, 3.4), (12.0, 15.0));
            pen.polyline(&[(7.6, 10.7), (12.0, 15.1), (16.4, 10.7)], false);
            pen.polyline(
                &[(4.2, 16.6), (4.2, 20.2), (19.8, 20.2), (19.8, 16.6)],
                false,
            );
        }
        OpenInNew => {
            pen.polyline(
                &[
                    (12.4, 4.0),
                    (5.0, 4.0),
                    (5.0, 19.2),
                    (20.0, 19.2),
                    (20.0, 11.6),
                ],
                false,
            );
            pen.line((11.2, 12.8), (20.0, 4.0));
            pen.polyline(&[(14.7, 4.0), (20.0, 4.0), (20.0, 9.3)], false);
        }
    }
}

struct Pen<'a> {
    painter: &'a egui::Painter,
    origin: Pos2,
    scale: f32,
    stroke: Stroke,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.scale
    }

    fn line(&self, a: (f32, f32), b: (f32, f32)) {
        self.painter
            .line_segment([self.at(a.0, a.1), self.at(b.0, b.1)], self.stroke);
    }

    fn polyline(&self, points: &[(f32, f32)], closed: bool) {
        let mut points: Vec<Pos2> = points.iter().map(|&(x, y)| self.at(x, y)).collect();
        if closed && points.len() > 1 {
            points.push(points[0]);
        }
        self.painter.add(egui::Shape::line(points, self.stroke));
    }

    fn circle(&self, x: f32, y: f32, radius: f32) {
        self.painter
            .circle_stroke(self.at(x, y), radius * self.scale, self.stroke);
    }

    fn disc(&self, x: f32, y: f32, radius: f32) {
        self.painter
            .circle_filled(self.at(x, y), radius * self.scale, self.stroke.color);
    }

    fn round_rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
        self.painter.rect_stroke(
            Rect::from_min_max(self.at(x0, y0), self.at(x1, y1)),
            CornerRadius::same((radius * self.scale).max(1.0) as u8),
            self.stroke,
            StrokeKind::Inside,
        );
    }

    fn arc(&self, x: f32, y: f32, radius: f32, from: f32, to: f32) {
        let segments = 24;
        let mut points = Vec::with_capacity(segments + 1);
        for step in 0..=segments {
            let angle = (from + (to - from) * step as f32 / segments as f32).to_radians();
            points.push(self.at(x + radius * angle.cos(), y + radius * angle.sin()));
        }
        self.painter.add(egui::Shape::line(points, self.stroke));
    }

    fn text(&self, x: f32, y: f32, text: &str, size: f32) {
        self.painter.text(
            self.at(x, y),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(24.0 * self.scale * size),
            self.stroke.color,
        );
    }
}

pub fn icon_button(ui: &mut Ui, icon: Icon, enabled: bool, tip: &str) -> Response {
    icon_button_tinted(ui, icon, theme::TEXT, enabled, tip)
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
    let hovered = enabled && response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), Color32::from_white_alpha(10));
    }
    let color = if enabled { color } else { theme::FAINT };
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
