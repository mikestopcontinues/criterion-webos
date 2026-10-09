use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

/// Interface symbols use geometry rather than the current text font's coverage.
#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Search,
    Home,
    Sparkle,
    FilmReel,
    Account,
    Space,
    Backspace,
    Microphone,
    Close,
    ArrowUp,
    ArrowDown,
    Check,
    Reset,
    ChevronRight,
    More,
}

impl Icon {
    pub(crate) fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        let scale = rect.width().min(rect.height()) / 24.0;
        let origin = rect.center() - Vec2::splat(12.0 * scale);
        let pos = |x: f32, y: f32| origin + Vec2::new(x, y) * scale;
        let stroke = Stroke::new(2.0 * scale, color);
        let line = |a: [f32; 2], b: [f32; 2]| {
            p.line_segment([pos(a[0], a[1]), pos(b[0], b[1])], stroke);
        };
        let path = |points: &[[f32; 2]]| {
            p.add(Shape::line(
                points.iter().map(|v| pos(v[0], v[1])).collect(),
                stroke,
            ));
        };
        let circle = |x, y, radius| {
            p.circle_stroke(pos(x, y), radius * scale, stroke);
        };
        let dot = |x, y, radius| {
            p.circle_filled(pos(x, y), radius * scale, color);
        };
        match self {
            Self::Search => {
                circle(10.0, 10.0, 7.0);
                line([15.0, 15.0], [21.0, 21.0]);
            }
            Self::Home => {
                path(&[[3.0, 9.0], [12.0, 3.0], [21.0, 9.0]]);
                path(&[[5.0, 11.0], [5.0, 21.0], [19.0, 21.0], [19.0, 11.0]]);
            }
            Self::Sparkle => path(&[
                [12.0, 2.0],
                [15.0, 9.0],
                [22.0, 12.0],
                [15.0, 15.0],
                [12.0, 22.0],
                [9.0, 15.0],
                [2.0, 12.0],
                [9.0, 9.0],
                [12.0, 2.0],
            ]),
            Self::FilmReel => {
                circle(11.0, 11.0, 8.5);
                line([11.0, 19.5], [21.0, 19.5]);
                for [x, y] in [
                    [11.0, 6.0],
                    [6.5, 9.5],
                    [8.0, 15.0],
                    [14.0, 15.0],
                    [15.5, 9.5],
                ] {
                    dot(x, y, 1.2);
                }
            }
            Self::Account => {
                circle(12.0, 6.0, 4.0);
                path(&[
                    [3.5, 21.0],
                    [5.0, 18.0],
                    [8.0, 16.0],
                    [12.0, 15.0],
                    [16.0, 16.0],
                    [19.0, 18.0],
                    [20.5, 21.0],
                ]);
            }
            Self::Space => path(&[[2.0, 9.0], [2.0, 14.0], [22.0, 14.0], [22.0, 9.0]]),
            Self::Backspace => {
                path(&[[8.0, 6.0], [2.0, 12.0], [8.0, 18.0]]);
                line([2.0, 12.0], [22.0, 12.0]);
            }
            Self::Microphone => {
                p.rect_filled(
                    Rect::from_min_max(pos(9.0, 2.0), pos(15.0, 14.0)),
                    egui::CornerRadius::same((3.0 * scale).round() as u8),
                    color,
                );
                path(&[
                    [5.0, 10.0],
                    [5.0, 13.0],
                    [6.0, 16.0],
                    [8.0, 18.0],
                    [12.0, 19.0],
                    [16.0, 18.0],
                    [18.0, 16.0],
                    [19.0, 13.0],
                    [19.0, 10.0],
                ]);
                line([12.0, 19.0], [12.0, 22.0]);
                line([8.0, 22.0], [16.0, 22.0]);
            }
            Self::Close => {
                line([5.0, 5.0], [19.0, 19.0]);
                line([5.0, 19.0], [19.0, 5.0]);
            }
            Self::ArrowUp => {
                line([12.0, 21.0], [12.0, 3.0]);
                path(&[[6.0, 9.0], [12.0, 3.0], [18.0, 9.0]]);
            }
            Self::ArrowDown => {
                line([12.0, 3.0], [12.0, 21.0]);
                path(&[[6.0, 15.0], [12.0, 21.0], [18.0, 15.0]]);
            }
            Self::Check => path(&[[3.0, 12.0], [9.0, 18.0], [21.0, 6.0]]),
            Self::Reset => {
                let points = (0..=16)
                    .map(|index| {
                        let angle = -std::f32::consts::PI * 0.75
                            + index as f32 * std::f32::consts::PI * 1.75 / 16.0;
                        pos(12.0 + 8.0 * angle.cos(), 12.0 + 8.0 * angle.sin())
                    })
                    .collect();
                p.add(Shape::line(points, stroke));
                path(&[[10.0, 6.0], [6.0, 6.0], [6.0, 10.0]]);
            }
            Self::ChevronRight => path(&[[8.0, 4.0], [16.0, 12.0], [8.0, 20.0]]),
            Self::More => {
                for x in [5.0, 12.0, 19.0] {
                    dot(x, 12.0, 1.5);
                }
            }
        }
    }
}

pub(crate) fn centered(center: Pos2, size: f32) -> Rect {
    Rect::from_center_size(center, Vec2::splat(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_bounded_geometry_without_text_shapes() {
        for icon in [
            Icon::Search,
            Icon::Home,
            Icon::Sparkle,
            Icon::FilmReel,
            Icon::Account,
            Icon::Space,
            Icon::Backspace,
            Icon::Microphone,
            Icon::Close,
            Icon::ArrowUp,
            Icon::ArrowDown,
            Icon::Check,
            Icon::Reset,
            Icon::ChevronRight,
            Icon::More,
        ] {
            let ctx = egui::Context::default();
            let rect = Rect::from_min_size(Pos2::new(20.0, 30.0), Vec2::new(40.0, 32.0));
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                icon.paint(ui.painter(), rect, Color32::WHITE);
            });
            output.textures_delta.clear();
            assert!((1..=8).contains(&output.shapes.len()));
            for shape in output.shapes {
                assert!(matches!(
                    shape.shape,
                    Shape::LineSegment { .. } | Shape::Path(_) | Shape::Circle(_) | Shape::Rect(_)
                ));
                assert!(rect.contains_rect(shape.shape.visual_bounding_rect()));
                assert_eq!(shape.clip_rect, rect);
            }
        }
    }
}
