use std::num::NonZeroU32;

use ab_glyph_rasterizer::{Point as AbPoint, Rasterizer};
use fast_image_resize::{FilterType, Image, MulDiv, PixelType, ResizeAlg, Resizer};
use pathfinder_content::outline::{Contour, ContourIterFlags, Outline};
#[cfg(feature = "optimize_stroke_broken")]
use pathfinder_content::segment::{Segment, SegmentFlags, SegmentKind};
use pathfinder_content::stroke::{LineCap, LineJoin, OutlineStrokeToFill, StrokeStyle};
#[cfg(feature = "optimize_stroke_broken")]
use pathfinder_geometry::line_segment::LineSegment2F;
use pathfinder_geometry::vector::Vector2F;
use tiny_skia_path::PathBuilder as PathData;
use ttf_parser::{OutlineBuilder, Rect};

use crate::metrics::CharMetrics;
use crate::*;

impl Font {
    /// Output the outline instructions of a glyph
    pub fn outline(&self, c: char) -> Option<(Glyph, Outline)> {
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
        self.load().ok()?;

        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        let a = f.ascender();
        let d = f.descender();
        let units = f.units_per_em() as f32;
        let factor = font_size / units;
        let glyph_id = f.glyph_index(c)?;
        if let Some(bb) = f.glyph_raster_image(glyph_id, 1) {
            let advanced_x = f.glyph_hor_advance(glyph_id)? as f32 * factor;
            let width = advanced_x;
            let height = width * (bb.height as f32 / bb.width as f32);
            let width_factor = width as f32 / bb.width as f32;
            let height_factor = height as f32 / bb.height as f32;

            let x = bb.x as f32 * width_factor;
            let y = bb.y as f32 * height_factor;

            let bbox = ttf_parser::Rect {
                x_min: (x / factor) as i16,
                y_min: (y / factor) as i16,
                x_max: ((x as f32 + width as f32) / factor) as i16,
                y_max: ((y as f32 + height as f32) / factor) as i16,
            };
            let mut decoder = png::Decoder::new(bb.data);
            decoder.set_transformations(png::Transformations::normalize_to_color8());
            decoder.set_ignore_text_chunk(true);
            let (bitmap, bitmap_width, bitmap_height) = decoder
                .read_info()
                .map_err(|e| e.into())
                .and_then(|mut reader| {
                    let mut buf = vec![0; reader.output_buffer_size()];
                    reader.next_frame(&mut buf)?;
                    let buf: Vec<u8> = match reader.output_color_type().0 {
                        png::ColorType::Rgb => buf
                            .chunks(3)
                            .map(|c| [c[0], c[1], c[2], 255].into_iter())
                            .flatten()
                            .collect(),
                        png::ColorType::Rgba => buf,
                        png::ColorType::GrayscaleAlpha => buf
                            .chunks_mut(2)
                            .map(|c| {
                                c[0] = ((c[0] as u16 * c[1] as u16) >> 8) as u8;
                                [c[0], c[0], c[0], c[1]].into_iter()
                            })
                            .flatten()
                            .collect(),
                        png::ColorType::Grayscale => buf
                            .chunks_mut(1)
                            .map(|c| [c[0], c[0], c[0], 255].into_iter())
                            .flatten()
                            .collect(),
                        _ => return Err(Error::PngNotSupported(reader.info().color_type)),
                    };
                    Ok((buf, reader.info().width, reader.info().height))
                })
                .ok()?;
            // https://docs.rs/fast_image_resize/latest/fast_image_resize/
            let mut image = Image::from_vec_u8(
                NonZeroU32::new(bitmap_width).unwrap(),
                NonZeroU32::new(bitmap_height).unwrap(),
                bitmap,
                PixelType::U8x4,
            )
            .unwrap();
            let alpha_mul_div = MulDiv::default();
            alpha_mul_div
                .multiply_alpha_inplace(&mut image.view_mut())
                .unwrap();
            let mut resizer = Resizer::new(ResizeAlg::Convolution(FilterType::Bilinear));
            let mut dst = Image::new(
                NonZeroU32::new(width as u32).unwrap(),
                NonZeroU32::new(height as u32).unwrap(),
                PixelType::U8x4,
            );
            resizer.resize(&image.view(), &mut dst.view_mut()).unwrap();
            alpha_mul_div
                .divide_alpha_inplace(&mut dst.view_mut())
                .unwrap();
            let bitmap = dst.into_vec();

            Some(GlyphBitmap(GlyphBitmapVariants::Raster(GlyphRasterImage {
                width: width as u16,
                height: height as u16,
                bbox,
                factor,
                ascender: a as f32 * factor,
                descender: d as f32 * factor,
                advanced_x,
                bitmap,
            })))
        } else {
            let (glyph, outline) = self.outline(c)?;
            let advanced_x = glyph.advanced_x as f32 * factor;
            let mut width = (glyph.bbox.x_max as f32 * factor).ceil()
                - (glyph.bbox.x_min as f32 * factor).floor();
            if width == 0.0 {
                width = advanced_x;
            }
            if width == 0.0 {
                width = font_size;
            }
            let mut height = (glyph.bbox.y_max as f32 * factor).ceil()
                - (glyph.bbox.y_min as f32 * factor).floor();

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

            Some(GlyphBitmap(GlyphBitmapVariants::Outline(
                GlyphRasterOutline {
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
                },
            )))
        }
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
        self.path.move_to(x, y);
        let mut c = Contour::new();
        c.push_endpoint(Vector2F::new(x, y));
        self.contour = c;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.contour.push_endpoint(Vector2F::new(x, y));
        self.path.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.contour
            .push_quadratic(Vector2F::new(x1, y1), Vector2F::new(x, y));
        self.path.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.contour.push_cubic(
            Vector2F::new(x1, y1),
            Vector2F::new(x2, y2),
            Vector2F::new(x, y),
        );
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.contour.close();
        let c = std::mem::replace(&mut self.contour, Contour::new());
        self.outline.push_contour(c);
        self.path.close();
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

#[derive(Clone, Debug)]
enum GlyphBitmapVariants {
    Outline(GlyphRasterOutline),
    Raster(GlyphRasterImage),
}

#[derive(Clone, Debug)]
pub struct GlyphBitmap(GlyphBitmapVariants);

/// Rasterized data of a [Glyph](Glyph)
#[derive(Clone, Debug)]
pub struct GlyphRasterOutline {
    width: u16,
    bbox: ttf_parser::Rect,
    factor: f32,
    ascender: f32,
    descender: f32,
    advanced_x: f32,
    bitmap: Vec<u8>,
    stroke_bitmap: Option<(Vec<u8>, u32)>,
    stroke_x_correction: f32,
    stroke_y_correction: f32,
}

#[derive(Clone, Debug)]
pub struct GlyphRasterImage {
    width: u16,
    height: u16,
    bbox: ttf_parser::Rect,
    factor: f32,
    ascender: f32,
    descender: f32,
    advanced_x: f32,
    bitmap: Vec<u8>,
}

impl GlyphBitmap {
    pub fn width(&self) -> u32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.width as u32,
            GlyphBitmapVariants::Raster(g) => g.width as u32,
        }
    }

    pub fn height(&self) -> u32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.bitmap.len() as u32 / g.width as u32,
            GlyphBitmapVariants::Raster(g) => g.height as u32,
        }
    }

    pub fn x_min(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.bbox.x_min as f32 * g.factor,
            GlyphBitmapVariants::Raster(g) => g.bbox.x_min as f32 * g.factor,
        }
    }

    pub fn y_min(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.bbox.y_min as f32 * g.factor,
            GlyphBitmapVariants::Raster(g) => g.bbox.y_min as f32 * g.factor,
        }
    }

    pub fn x_max(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.bbox.x_max as f32 * g.factor,
            GlyphBitmapVariants::Raster(g) => g.bbox.x_max as f32 * g.factor,
        }
    }

    pub fn y_max(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.bbox.y_max as f32 * g.factor,
            GlyphBitmapVariants::Raster(g) => g.bbox.y_max as f32 * g.factor,
        }
    }

    pub fn advanced_x(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.advanced_x,
            GlyphBitmapVariants::Raster(g) => g.advanced_x,
        }
    }

    pub fn ascender(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.ascender,
            GlyphBitmapVariants::Raster(g) => g.ascender,
        }
    }

    pub fn descender(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.descender,
            GlyphBitmapVariants::Raster(g) => g.descender,
        }
    }

    pub fn stroke_x(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.stroke_x_correction,
            GlyphBitmapVariants::Raster(_) => 0.0,
        }
    }

    pub fn stroke_y(&self) -> f32 {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => g.stroke_y_correction,
            GlyphBitmapVariants::Raster(_) => 0.0,
        }
    }

    pub fn bitmap(&self) -> &Vec<u8> {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => &g.bitmap,
            GlyphBitmapVariants::Raster(g) => &g.bitmap,
        }
    }

    pub fn stroke_bitmap(&self) -> Option<(&Vec<u8>, u32)> {
        match &self.0 {
            GlyphBitmapVariants::Outline(g) => {
                let (bitmap, width) = g.stroke_bitmap.as_ref()?;
                Some((bitmap, *width))
            }
            GlyphBitmapVariants::Raster(_) => None,
        }
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
