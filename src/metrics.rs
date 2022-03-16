use crate::{Error, Font};
use std::borrow::Cow;
use ttf_parser::{GlyphId, Rect};
use unicode_bidi::BidiInfo;
use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, ScriptExtension};
#[cfg(wasm)]
use wasm_bindgen::prelude::*;

mod arabic;

impl Font {
    pub fn measure(&self, text: &str, fallback_font: Option<&Font>) -> Result<TextMetrics, Error> {
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
        let (value, _level) = if bidi.has_rtl() {
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
        let mut x_a = 0;
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
            let m = self
                .measure_char(char_code)
                .or_else(|| fallback_font?.measure_char(char_code))
                .ok_or_else(|| Error::GlyphNotFound { c: char_code })?;
            let kerning = self.kerning(prev, char_code).unwrap_or(0);
            x_a += kerning as i32;
            prev = char_code;
            let metrics = PositionedChar { x_a, metrics: m };
            x_a += metrics.metrics.advanced_x as i32;
            positions.push(metrics);
        }

        Ok(TextMetrics {
            positions,
            line_gap,
            content_height: height,
            ascender: font.ascender(),
            units: font.units_per_em(),
        })
    }

    /// Measure the metrics of a single unicode charactor
    pub fn measure_char(&self, c: char) -> Option<CharMetrics> {
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
            height: height,
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
#[derive(Debug, Clone)]
pub struct TextMetrics {
    positions: Vec<PositionedChar>,
    pub content_height: i16,
    pub ascender: i16,
    pub line_gap: i16,
    units: u16,
}

#[derive(Debug, Clone)]
pub struct PositionedChar {
    pub metrics: CharMetrics,
    pub x_a: i32, // font size factor
}

/// Metrics for a single unicode charactor in a certain font
#[derive(Debug, Clone)]
pub struct CharMetrics {
    pub c: char,
    pub glyph_id: GlyphId,
    pub advanced_x: u16,
    pub lsb: i16,
    pub bbox: Rect,
    pub units: f32,
    pub height: i16,
}
