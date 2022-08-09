use crate::{Error, Font};
pub use compose::*;
use std::borrow::Cow;
use ttf_parser::{GlyphId, Rect};
use unicode_bidi::{BidiInfo, Level};
use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, ScriptExtension};
#[cfg(wasm)]
use wasm_bindgen::prelude::*;

mod arabic;
mod compose;

impl Font {
    /// Measure a string slice. If certain character is missing, the related
    /// [`CharMetrics`] 's `missing` field will be `true`
    pub fn measure(&self, text: &str) -> Result<TextMetrics, Error> {
        #[cfg(not(wasm))]
        self.load()?;
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let font = f.borrow_face();
        let mut positions = vec![];
        let mut prev = 0 as char;
        let mut value = Cow::Borrowed(text);
        let scripts = ScriptExtension::for_str(&*value);
        if scripts.contains_script(Script::Arabic) {
            value = Cow::Owned(arabic::fix_arabic_ligatures_char(&*value));
        }
        let bidi = BidiInfo::new(&value, None);
        let (value, levels) = if bidi.has_rtl() {
            let value = bidi
                .paragraphs
                .iter()
                .map(|para| {
                    let line = para.range.clone();
                    bidi.reorder_line(para, line)
                })
                .collect::<Vec<_>>()
                .join("");
            let levels = bidi
                .paragraphs
                .iter()
                .flat_map(|para| {
                    let line = para.range.clone();
                    bidi.reordered_levels(para, line).into_iter()
                })
                .collect::<Vec<_>>();
            (Cow::Owned(value), levels)
        } else {
            (value, vec![])
        };
        let height = font.height();
        let line_gap = font.line_gap();
        for char_code in value.nfc() {
            if char_code == '\n' {
                continue;
            }
            // let direction = if let Some(level) = levels.get(index) {
            //     if level.is_ltr() {
            //         TextDirection::LTR
            //     } else {
            //         TextDirection::RTL
            //     }
            // } else {
            //     text.direction
            // };
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
            };
            positions.push(metrics);
        }

        Ok(TextMetrics {
            value: value.to_string(),
            levels,
            positions,
            line_gap,
            content_height: height,
            ascender: font.ascender(),
            units: font.units_per_em(),
        })
    }

    /// Measure the metrics of a single unicode charactor
    pub(crate) fn measure_char(&self, c: char) -> Option<CharMetrics> {
        let f = self.face.load();
        let f = f.as_ref().as_ref()?;
        let f = f.borrow_face();
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
        Some(CharMetrics {
            c,
            glyph_id,
            advanced_x: f.glyph_hor_advance(glyph_id)?,
            bbox,
            lsb: f.glyph_hor_side_bearing(glyph_id).unwrap_or(0),
            units,
            height,
            missing: false,
        })
    }

    /// Check if there's any kerning data between two charactors, units are
    /// handled
    fn kerning(&self, prev: char, c: char) -> Option<i16> {
        let f = self.face.load();
        let f = f.as_ref().as_ref()?;
        let f = f.borrow_face();
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
    }
}

#[cfg_attr(wasm, wasm_bindgen)]
#[derive(Debug, Clone, Default)]
pub struct TextMetrics {
    value: String,
    levels: Vec<Level>,
    pub(crate) positions: Vec<PositionedChar>,
    content_height: i16,
    ascender: i16,
    line_gap: i16,
    units: u16,
}

#[cfg_attr(wasm, wasm_bindgen)]
impl TextMetrics {
    pub fn width(&self, font_size: f32, letter_spacing: f32) -> f32 {
        let factor = font_size / self.units as f32;
        self.positions.iter().fold(0.0, |current, p| {
            current
                + p.kerning as f32 * factor
                + p.metrics.advanced_x as f32 * factor
                + letter_spacing
        })
    }

    pub fn width_trim_start(&self, font_size: f32, letter_spacing: f32) -> f32 {
        self.width(font_size, letter_spacing)
            - if self.positions[0].metrics.c == ' ' {
                self.positions[0].metrics.advanced_x as f32 / self.positions[0].metrics.units
                    * font_size
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

    pub fn content_height(&self) -> i16 {
        self.content_height
    }

    pub fn ascender(&self) -> i16 {
        self.ascender
    }

    pub fn line_gap(&self) -> i16 {
        self.line_gap
    }

    pub fn units(&self) -> u16 {
        self.units
    }

    pub fn new(value: String) -> TextMetrics {
        TextMetrics {
            value,
            ..TextMetrics::default()
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn value(&self) -> &str {
        self.value.as_str()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn value(&self) -> String {
        self.value.clone()
    }

    #[cfg(not(wasm))]
    pub fn levels(&self) -> Vec<Level> {
        self.levels.clone()
    }

    pub(crate) fn trim_start(&mut self) {
        let trim = self.value.trim_start().to_string();
        let mut len = self.value.len() - trim.len();
        if len == 0 {
            return;
        }
        self.value = trim;
        while len > 0 {
            self.levels.remove(0);
            self.positions.remove(0);
            len -= 1;
        }
    }

    pub(crate) fn pop(&mut self) {
        self.value.pop();
        self.levels.pop();
        self.positions.pop();
    }
}

impl TextMetrics {
    pub fn positions(&self) -> &[PositionedChar] {
        &self.positions
    }
}

#[cfg_attr(wasm, wasm_bindgen)]
#[derive(Debug, Clone)]
pub struct PositionedChar {
    /// Various metrics data of current character
    pub metrics: CharMetrics,
    /// Kerning between previous and current character
    pub kerning: i32,
}

/// Metrics for a single unicode charactor in a certain font
#[cfg_attr(wasm, wasm_bindgen)]
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
