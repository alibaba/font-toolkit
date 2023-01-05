use crate::metrics::CharMetrics;
use crate::*;
use ab_glyph_rasterizer::{Point as AbPoint, Rasterizer};
use pathfinder_content::outline::{Contour, ContourIterFlags, Outline};
use pathfinder_content::segment::{Segment, SegmentFlags, SegmentKind};
use pathfinder_content::stroke::{LineCap, LineJoin, OutlineStrokeToFill, StrokeStyle};
use pathfinder_geometry::line_segment::LineSegment2F;
use pathfinder_geometry::vector::Vector2F;
use ttf_parser::{OutlineBuilder, Rect};
use usvg::PathData;
pub use usvg::PathSegment;

impl Font {
    /// Output the outline instructions of a glyph
    pub fn outline(&self, c: char) -> Option<(Glyph, Outline)> {
        #[cfg(not(wasm))]
        self.load().ok()?;
        let mut builder = PathBuilder::new();
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        let CharMetrics {
            glyph_id,
            bbox,
            advanced_x,
            units,
            ..
        } = self.measure_char(c)?;
        let outline = f.outline_glyph(glyph_id, &mut builder).unwrap_or(bbox);
        builder.finish();
        let glyph = Glyph {
            units: units as u16,
            path: builder.path,
            bbox: outline,
            advanced_x,
        };
        Some((glyph, builder.outline))
    }

    /// Rasterize the outline of a glyph for a certain font_size, and a possible
    /// stroke. This method is costy
    pub fn bitmap(&self, c: char, font_size: f32, stroke_width: f32) -> Option<GlyphBitmap> {
        if !self.has_glyph(c) {
            return None;
        }
        #[cfg(not(wasm))]
        self.load().ok()?;

        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        let a = f.ascender();
        let d = f.descender();
        let units = f.units_per_em() as f32;
        let factor = font_size / units;
        let (glyph, outline) = self.outline(c)?;
        let advanced_x = glyph.advanced_x as f32 * factor;
        let mut width =
            (glyph.bbox.x_max as f32 * factor).ceil() - (glyph.bbox.x_min as f32 * factor).floor();
        if width == 0.0 {
            width = advanced_x;
        }
        if width == 0.0 {
            width = font_size;
        }
        let mut height =
            (glyph.bbox.y_max as f32 * factor).ceil() - (glyph.bbox.y_min as f32 * factor).floor();

        let mut stroke_x_min = (glyph.bbox.x_min as f32 * factor).floor();
        let mut stroke_y_max = (glyph.bbox.y_max as f32 * factor).ceil();

        // try to render stroke
        let stroke_bitmap = if stroke_width > 0.0 {
            #[cfg(feature = "optimize_stroke_broken")]
            let outline = remove_obtuse_angle(&outline);
            let mut filler = OutlineStrokeToFill::new(
                &outline,
                StrokeStyle {
                    line_width: stroke_width / factor,
                    line_cap: LineCap::default(),
                    line_join: LineJoin::Miter(4.0),
                },
            );
            filler.offset();
            let outline = filler.into_outline();
            let bounds = outline.bounds();
            let width = (bounds.max_x() * factor).ceil() - (bounds.min_x() * factor).floor();
            let height = (bounds.max_y() * factor).ceil() - (bounds.min_y() * factor).floor();
            stroke_x_min = (bounds.origin_x() * factor).floor();
            stroke_y_max = ((bounds.size().y() + bounds.origin_y()) * factor).ceil();
            let mut ras = FontkitRas {
                ras: Rasterizer::new(width as usize, height as usize),
                factor,
                x_min: stroke_x_min,
                y_max: stroke_y_max,
                prev: None,
                start: None,
            };
            ras.load_outline(outline);
            let mut bitmap = vec![0_u8; width as usize * height as usize];
            ras.ras.for_each_pixel_2d(|x, y, alpha| {
                if x < width as u32 && y < height as u32 {
                    bitmap[((height as u32 - y - 1) * width as u32 + x) as usize] =
                        (alpha * 255.0) as u8;
                }
            });
            Some((bitmap, width as u32))
        } else {
            None
        };
        width = width.ceil();
        height = height.ceil();

        let mut ras = FontkitRas {
            ras: Rasterizer::new(width as usize, height as usize),
            factor,
            x_min: (glyph.bbox.x_min as f32 * factor).floor(),
            y_max: (glyph.bbox.y_max as f32 * factor).ceil(),
            prev: None,
            start: None,
        };
        ras.load_outline(outline);
        let mut bitmap = vec![0_u8; width as usize * height as usize];
        ras.ras.for_each_pixel_2d(|x, y, alpha| {
            if x < width as u32 && y < height as u32 {
                bitmap[((height as u32 - y - 1) * width as u32 + x) as usize] =
                    (alpha * 255.0) as u8;
            }
        });

        Some(GlyphBitmap {
            width: width as u16,
            bbox: glyph.bbox,
            factor,
            ascender: a as f32 * factor,
            descender: d as f32 * factor,
            advanced_x,
            bitmap,
            stroke_bitmap,
            stroke_x_correction: (glyph.bbox.x_min as f32 * factor).floor() - stroke_x_min,
            stroke_y_correction: stroke_y_max - (glyph.bbox.y_max as f32 * factor).ceil(),
        })
    }
}

struct PathBuilder {
    path: PathData,
    outline: Outline,
    contour: Contour,
}

impl PathBuilder {
    pub fn new() -> Self {
        PathBuilder {
            path: PathData::default(),
            outline: Outline::new(),
            contour: Contour::new(),
        }
    }

    pub fn finish(&mut self) {
        if !self.contour.is_empty() {
            self.outline
                .push_contour(std::mem::replace(&mut self.contour, Contour::new()));
        }
    }
}

impl OutlineBuilder for PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.push_move_to(x as f64, y as f64);
        let mut c = Contour::new();
        c.push_endpoint(Vector2F::new(x, y));
        self.contour = c;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.contour.push_endpoint(Vector2F::new(x, y));
        self.path.push_line_to(x as f64, y as f64);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.contour
            .push_quadratic(Vector2F::new(x1, y1), Vector2F::new(x, y));
        self.path
            .push_quad_to(x1 as f64, y1 as f64, x as f64, y as f64);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.contour.push_cubic(
            Vector2F::new(x1, y1),
            Vector2F::new(x2, y2),
            Vector2F::new(x, y),
        );
        self.path.push_curve_to(
            x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64, y as f64,
        );
    }

    fn close(&mut self) {
        self.contour.close();
        let c = std::mem::replace(&mut self.contour, Contour::new());
        self.outline.push_contour(c);
        self.path.push_close_path();
    }
}

struct FontkitRas {
    ras: Rasterizer,
    factor: f32,
    x_min: f32,
    y_max: f32,
    prev: Option<AbPoint>,
    start: Option<AbPoint>,
}

impl FontkitRas {
    fn load_outline(&mut self, outline: Outline) {
        for contour in outline.into_contours() {
            let mut started = false;
            for segment in contour.iter(ContourIterFlags::IGNORE_CLOSE_SEGMENT) {
                if !started {
                    let start = segment.baseline.from();
                    self.move_to(start.x(), start.y());
                    started = true;
                }
                let to = segment.baseline.to();
                if segment.is_line() {
                    self.line_to(to.x(), to.y());
                } else if segment.is_quadratic() {
                    let ctrl = segment.ctrl.from();
                    self.quad_to(ctrl.x(), ctrl.y(), to.x(), to.y());
                } else if segment.is_cubic() {
                    let ctrl1 = segment.ctrl.from();
                    let ctrl2 = segment.ctrl.to();
                    self.curve_to(ctrl1.x(), ctrl1.y(), ctrl2.x(), ctrl2.y(), to.x(), to.y());
                }
            }
            if contour.is_closed() {
                self.close();
            }
        }
    }
}

impl OutlineBuilder for FontkitRas {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = AbPoint {
            x: x * self.factor - self.x_min,
            y: self.y_max - y * self.factor,
        };
        self.prev = Some(p);
        self.start = Some(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = AbPoint {
            x: x * self.factor - self.x_min,
            y: self.y_max - y * self.factor,
        };
        if let Some(prev) = self.prev.take() {
            self.ras.draw_line(prev, to);
        }
        self.prev = Some(to);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let to = AbPoint {
            x: x * self.factor - self.x_min,
            y: self.y_max - y * self.factor,
        };
        let c = AbPoint {
            x: x1 * self.factor - self.x_min,
            y: self.y_max - y1 * self.factor,
        };
        if let Some(prev) = self.prev.take() {
            self.ras.draw_quad(prev, c, to);
        }
        self.prev = Some(to);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let to = AbPoint {
            x: x * self.factor - self.x_min,
            y: self.y_max - y * self.factor,
        };

        let c1 = AbPoint {
            x: x1 * self.factor - self.x_min,
            y: self.y_max - y1 * self.factor,
        };
        let c2 = AbPoint {
            x: x2 * self.factor - self.x_min,
            y: self.y_max - y2 * self.factor,
        };
        if let Some(prev) = self.prev.take() {
            self.ras.draw_cubic(prev, c1, c2, to);
        }
        self.prev = Some(to);
    }

    fn close(&mut self) {
        if let (Some(a), Some(b)) = (self.start.take(), self.prev.take()) {
            self.ras.draw_line(b, a);
        }
    }
}

/// The outline of a glyph, with some metrics data
pub struct Glyph {
    pub units: u16,
    pub path: PathData,
    pub bbox: Rect,
    pub advanced_x: u16,
}

/// Rasterized data of a [Glyph](Glyph)
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    width: u16,
    bbox: ttf_parser::Rect,
    factor: f32,
    pub ascender: f32,
    pub descender: f32,
    pub advanced_x: f32,
    pub bitmap: Vec<u8>,
    pub stroke_bitmap: Option<(Vec<u8>, u32)>,
    pub stroke_x_correction: f32,
    pub stroke_y_correction: f32,
}

impl GlyphBitmap {
    pub fn width(&self) -> u32 {
        self.width as u32
    }

    pub fn height(&self) -> u32 {
        self.bitmap.len() as u32 / self.width as u32
    }

    pub fn x_min(&self) -> f32 {
        self.bbox.x_min as f32 * self.factor
    }

    pub fn y_min(&self) -> f32 {
        self.bbox.y_min as f32 * self.factor
    }

    pub fn x_max(&self) -> f32 {
        self.bbox.x_max as f32 * self.factor
    }

    pub fn y_max(&self) -> f32 {
        self.bbox.y_max as f32 * self.factor
    }
}

#[cfg(feature = "optimize_stroke_broken")]
fn calc_distance(p1: Vector2F, p2: Vector2F) -> f32 {
    ((p1.x() - p2.x()).powi(2) + (p1.y() - p2.y()).powi(2)).sqrt()
}

#[cfg(feature = "optimize_stroke_broken")]
fn remove_obtuse_angle(outline: &Outline) -> Outline {
    let mut segments: Vec<Segment> = vec![];
    let mut head_index: usize = 0;
    for contour in outline.contours() {
        for (index, segment) in contour
            .iter(ContourIterFlags::IGNORE_CLOSE_SEGMENT)
            .enumerate()
        {
            if index == 0 {
                head_index = segments.len();
                segments.push(Segment {
                    baseline: segment.baseline,
                    ctrl: segment.ctrl,
                    kind: SegmentKind::None,
                    flags: SegmentFlags::FIRST_IN_SUBPATH,
                });
            }
            let from = segment.baseline.from();
            let to = segment.baseline.to();
            if segment.is_quadratic() {
                let ctrl = segment.ctrl.from();
                let d = segment.baseline.square_length().sqrt();
                let d1 = calc_distance(ctrl, from);
                let d2 = calc_distance(ctrl, to);
                if d1 <= 10.0 || d2 <= 10.0 {
                    let mut cos = (d1 * d1 + d * d - d2 * d2) / 2.0 * d1 * d;
                    if cos > 0.0 {
                        cos = (d2 * d2 + d * d - d1 * d1) / 2.0 * d2 * d;
                    }
                    if cos <= 0.0 {
                        segments.push(Segment::line(LineSegment2F::new(from, to)));
                        continue;
                    }
                }
            }
            if segment.is_cubic() {
                // TODO
            }
            segments.push(segment)
        }
        let mut last_seg = segments.last().unwrap().clone();
        let first_seg_pos = segments[head_index].baseline.from();
        if last_seg.kind == SegmentKind::Line && first_seg_pos == last_seg.baseline.to() {
            segments.pop();
        }
        last_seg.flags = SegmentFlags::CLOSES_SUBPATH;
        segments.push(last_seg);
    }
    Outline::from_segments(segments.into_iter())
}
