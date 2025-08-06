use crate::{Error, StaticFace};
pub use compose::*;
use std::borrow::Cow;
use std::sync::{Arc, RwLock};
use ttf_parser::{GlyphId, Rect};
use unicode_bidi::{BidiInfo, Level};
use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, ScriptExtension};

mod arabic;
mod compose;

impl StaticFace {
    /// Measure a string slice. If certain character is missing, the related
    /// [`CharMetrics`] 's `missing` field will be `true`
    pub fn measure(&self, text: &str) -> Result<TextMetrics, Error> {
        let mut positions = vec![];
        let mut prev = 0 as char;
        let mut value = Cow::Borrowed(text);
        let scripts = ScriptExtension::for_str(&*value);
        if scripts.contains_script(Script::Arabic) {
            value = Cow::Owned(arabic::fix_arabic_ligatures_char(&*value));
        }
        let bidi = BidiInfo::new(&value, None);
        let (value, levels) = if bidi.has_rtl() {
            let prev = value.clone();
            let (strs, data): (Vec<String>, Vec<_>) = bidi
                .paragraphs
                .iter()
                .map(|para| {
                    let (levels, runs) = bidi.visual_runs(para, para.range.clone());
                    let line = para.range.clone();

                    if runs.iter().all(|run| levels[run.start].is_ltr()) {
                        let result: String = prev[line.clone()].into();
                        let info = prev[line]
                            .chars()
                            .map(|c| (c, Level::ltr()))
                            .collect::<Vec<_>>();
                        return (result, info);
                    }

                    let mut result = String::with_capacity(line.len());
                    let mut info = vec![];
                    for run in runs {
                        if levels[run.start].is_rtl() {
                            result.extend(prev[run.clone()].chars().rev());
                            info.extend(prev[run].chars().rev().map(|c| (c, Level::rtl())));
                        } else {
                            result.push_str(&prev[run.clone()]);
                            info.extend(prev[run].chars().rev().map(|c| (c, Level::ltr())));
                        }
                    }
                    (result, info)
                })
                .unzip();
            let value = strs.join("");
            let levels = data
                .into_iter()
                .flatten()
                .map(|(_, levels)| levels)
                .collect::<Vec<_>>();
            (Cow::Owned(value), levels)
        } else {
            let levels = value.nfc().map(|_| Level::ltr()).collect();
            (value, levels)
        };
        let (height, line_gap) = self.with_face(|f| (f.height(), f.line_gap()));
        for (char_code, level) in value.nfc().zip(levels.into_iter()) {
            if char_code == '\n' {
                continue;
            }
            let m = self.measure_char(char_code).unwrap_or_else(|| CharMetrics {
                bbox: Rect {
                    x_min: 0,
                    y_min: 0,
                    x_max: 1,
                    y_max: 1,
                },
                missing: true,
                c: char_code,
                glyph_id: GlyphId(0),
                advanced_x: 0,
                lsb: 0,
                units: 0.0,
                height,
            });
            let kerning = self.kerning(prev, char_code).unwrap_or(0);
            prev = char_code;
            let metrics = PositionedChar {
                kerning: kerning as i32,
                metrics: m,
                level,
            };
            positions.push(metrics);
        }

        Ok(TextMetrics {
            positions: Arc::new(RwLock::new(positions)),
            line_gap,
            content_height: height,
            ascender: self.ascender(),
            units: self.units_per_em(),
        })
    }

    /// Measure the metrics of a single unicode charactor
    pub(crate) fn measure_char(&self, c: char) -> Option<CharMetrics> {
        self.with_face(|f| {
            let height = f.height();
            let units = f.units_per_em() as f32;
            let glyph_id = f.glyph_index(c)?;
            let bbox = f.glyph_bounding_box(glyph_id).or_else(|| {
                Some(Rect {
                    x_min: 0,
                    y_min: 0,
                    x_max: f.glyph_hor_advance(glyph_id)? as i16,
                    y_max: units as i16,
                })
            })?;
            let lsb = f.glyph_hor_side_bearing(glyph_id).unwrap_or(0);
            // ttf-parser added phantom points support, so the original fix is no
            // longer needed. Check history for details.
            let advanced_x = f.glyph_hor_advance(glyph_id)?;
            Some(CharMetrics {
                c,
                glyph_id,
                advanced_x,
                bbox,
                lsb,
                units,
                height,
                missing: false,
            })
        })
    }

    /// Check if there's any kerning data between two charactors, units are
    /// handled
    fn kerning(&self, prev: char, c: char) -> Option<i16> {
        self.with_face(|f| {
            let pid = f.glyph_index(prev)?;
            let cid = f.glyph_index(c)?;
            let mut kerning = 0;
            for table in f
                .tables()
                .kern
                .into_iter()
                .flat_map(|k| k.subtables.into_iter())
                .filter(|st| st.horizontal && !st.variable)
            {
                if let Some(k) = table.glyphs_kerning(pid, cid) {
                    kerning = k;
                }
            }
            Some(kerning)
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct TextMetrics {
    pub(crate) positions: Arc<RwLock<Vec<PositionedChar>>>,
    content_height: i16,
    ascender: i16,
    line_gap: i16,
    units: u16,
}

impl TextMetrics {
    #[allow(unused)]
    pub fn new(value: String) -> Self {
        let mut m = TextMetrics::default();
        let data = value
            .nfc()
            .map(|c| PositionedChar {
                metrics: CharMetrics {
                    bbox: ttf_parser::Rect {
                        x_min: 0,
                        y_min: 0,
                        x_max: 1,
                        y_max: 1,
                    },
                    glyph_id: GlyphId(0),
                    c,
                    advanced_x: 0,
                    lsb: 0,
                    units: 0.0,
                    height: 0,
                    missing: true,
                },
                kerning: 0,
                level: Level::ltr(),
            })
            .collect::<Vec<_>>();
        m.positions = Arc::new(RwLock::new(data));
        m
    }

    pub(crate) fn append(&self, other: Self) {
        let mut p = self.positions.write().unwrap();
        let mut other = other.positions.write().unwrap();
        p.append(&mut other);
    }

    pub fn count(&self) -> usize {
        let p = self.positions.read().unwrap();
        p.len()
    }

    pub(crate) fn is_rtl(&self) -> bool {
        self.positions
            .read()
            .map(|p| p.iter().any(|p| p.level.is_rtl()))
            .unwrap_or(false)
    }

    pub(crate) fn slice(&self, start: u32, count: u32) -> Self {
        let start = start as usize;
        let count = count as usize;
        let positions = {
            let p = self.positions.read().unwrap();
            if p.is_empty() {
                vec![]
            } else {
                let start = std::cmp::min(start, p.len() - 1);
                let count = std::cmp::min(p.len() - start, count);
                (&p[start..(start + count)]).to_vec()
            }
        };
        TextMetrics {
            positions: Arc::new(RwLock::new(positions)),
            content_height: self.content_height,
            ascender: self.ascender,
            line_gap: self.line_gap,
            units: self.units,
        }
    }

    pub(crate) fn replace(&self, other: Self, fallback: bool) {
        let mut p = self.positions.write().unwrap();
        let mut other_p = other.positions.write().unwrap();
        let other_p = other_p.split_off(0);
        let content_height_factor = self.content_height() as f32 / other.content_height() as f32;
        if fallback {
            for (c, p) in p.iter_mut().zip(other_p.into_iter()) {
                if c.metrics.missing {
                    *c = p;
                    c.metrics.mul_factor(content_height_factor);
                }
            }
        } else {
            *p = other_p;
            for c in p.iter_mut() {
                c.metrics.mul_factor(content_height_factor);
            }
        }
    }

    pub fn has_missing(&self) -> bool {
        self.positions
            .read()
            .map(|p| p.iter().any(|c| c.metrics.missing))
            .unwrap_or_default()
    }

    pub fn width(&self, font_size: f32, letter_spacing: f32) -> f32 {
        self.width_until(
            font_size,
            letter_spacing,
            self.positions.read().map(|p| p.len()).unwrap_or(0),
        )
    }

    pub(crate) fn width_until(&self, font_size: f32, letter_spacing: f32, index: usize) -> f32 {
        if self.units == 0 {
            return 0.0;
        }
        let positions = self.positions.read().unwrap();
        positions.iter().take(index).fold(0.0, |current, p| {
            current + p.kerning as f32 + p.metrics.advanced_x as f32
        }) * font_size
            / self.units as f32
            + letter_spacing * (index as f32)
    }

    pub fn width_trim_start(&self, font_size: f32, letter_spacing: f32) -> f32 {
        let positions = self.positions.read().unwrap();
        if positions.is_empty() {
            return 0.0;
        }
        self.width(font_size, letter_spacing)
            - if positions[0].metrics.c == ' ' {
                positions[0].metrics.advanced_x as f32 / positions[0].metrics.units * font_size
            } else {
                0.0
            }
    }

    pub fn height(&self, font_size: f32, line_height: Option<f32>) -> f32 {
        line_height.map(|h| h * font_size).unwrap_or_else(|| {
            let factor = font_size / self.units as f32;
            (self.content_height as f32 + self.line_gap as f32) * factor
        })
    }

    pub(crate) fn content_height(&self) -> i16 {
        self.content_height
    }

    pub(crate) fn ascender(&self) -> i16 {
        self.ascender
    }

    pub(crate) fn line_gap(&self) -> i16 {
        self.line_gap
    }

    pub fn units(&self) -> u16 {
        self.units
    }

    pub fn value(&self) -> String {
        let rtl = self.is_rtl();
        let positions = self.positions.read().unwrap();
        if rtl {
            let mut result = String::new();
            let mut current_chars = vec![];
            let mut current_level = Level::rtl();
            for p in positions.iter().rev() {
                if current_level == p.level {
                    current_chars.push(p.metrics.c);
                    continue;
                }
                if !current_chars.is_empty() {
                    if current_level.is_rtl() {
                        result.extend(current_chars.clone().into_iter());
                    } else {
                        result.extend(current_chars.clone().into_iter().rev());
                    }
                    current_chars.clear();
                }
                current_chars.push(p.metrics.c);
                current_level = p.level;
            }
            if !current_chars.is_empty() {
                if current_level.is_rtl() {
                    result.extend(current_chars.clone().into_iter());
                } else {
                    result.extend(current_chars.clone().into_iter().rev());
                }
                current_chars.clear();
            }
            result
        } else {
            positions.iter().map(|p| p.metrics.c).collect::<String>()
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionedChar {
    /// Various metrics data of current character
    pub metrics: CharMetrics,
    /// Kerning between previous and current character
    pub kerning: i32,
    pub(crate) level: Level,
}

/// Metrics for a single unicode charactor in a certain font
#[derive(Debug, Clone, Copy)]
pub struct CharMetrics {
    pub(crate) bbox: Rect,
    pub(crate) glyph_id: GlyphId,
    pub c: char,
    pub advanced_x: u16,
    pub lsb: i16,
    pub units: f32,
    pub height: i16,
    pub missing: bool,
}

impl CharMetrics {
    pub(crate) fn mul_factor(&mut self, factor: f32) {
        self.advanced_x = (self.advanced_x as f32 * factor) as u16;
        self.units = self.units * factor;
        self.height = (self.height as f32 * factor) as i16;
        self.lsb = (self.lsb as f32 * factor) as i16;
        self.bbox.x_min = (self.bbox.x_min as f32 * factor) as i16;
        self.bbox.x_max = (self.bbox.x_max as f32 * factor) as i16;
        self.bbox.y_min = (self.bbox.y_min as f32 * factor) as i16;
        self.bbox.y_max = (self.bbox.y_max as f32 * factor) as i16;
    }
}
