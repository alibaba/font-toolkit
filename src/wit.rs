use std::path::PathBuf;
use std::sync::atomic::Ordering;

use crate::bindings::exports::alibaba::fontkit::fontkit_interface as fi;
use crate::font::FontKey;
use crate::metrics::TextMetrics;
use crate::{Font, FontKit, GlyphBitmap, StaticFace, VariationData};

use crate::bindings::exports::alibaba::fontkit::fontkit_interface::GuestTextMetrics;

impl fi::GuestFont for StaticFace {
    fn has_glyph(&self, c: char) -> bool {
        self.has_glyph(c)
    }

    fn buffer(&self) -> Vec<u8> {
        self.borrow_buffer().clone()
    }

    fn path(&self) -> String {
        self.borrow_path().to_str().unwrap_or_default().to_string()
    }

    fn key(&self) -> fi::FontKey {
        self.key().into()
    }

    fn measure(&self, text: String) -> Result<fi::TextMetrics, String> {
        Ok(fi::TextMetrics::new(
            self.measure(&text).map_err(|e| e.to_string())?,
        ))
    }

    fn ascender(&self) -> i16 {
        self.ascender()
    }

    fn descender(&self) -> i16 {
        self.descender()
    }

    fn units_per_em(&self) -> u16 {
        self.units_per_em()
    }

    fn bitmap(&self, c: char, font_size: f32, stroke_width: f32) -> Option<fi::GlyphBitmap> {
        Some(fi::GlyphBitmap::new(self.bitmap(
            c,
            font_size,
            stroke_width,
        )?))
    }

    fn underline_metrics(&self) -> Option<fi::LineMetrics> {
        let m = self.underline_metrics()?;
        Some(fi::LineMetrics {
            position: m.position,
            thickness: m.thickness,
        })
    }

    fn glyph_path_string(&self, c: char) -> Option<String> {
        use pathfinder_geometry::transform2d::Transform2F;
        use pathfinder_geometry::vector::Vector2F;

        let (_, mut outline) = self.outline(c)?;
        let height = self.ascender() as f32 - self.descender() as f32;
        outline.transform(&Transform2F::from_scale(Vector2F::new(1.0, -1.0)));
        outline.transform(&Transform2F::from_translation(Vector2F::new(0.0, height)));
        Some(format!("{:?}", outline))
    }
}

impl fi::GuestGlyphBitmap for GlyphBitmap {
    fn width(&self) -> u32 {
        self.width()
    }

    fn height(&self) -> u32 {
        self.height()
    }

    fn bitmap(&self) -> Vec<u8> {
        self.bitmap().clone()
    }

    fn ascender(&self) -> f32 {
        self.ascender()
    }

    fn descender(&self) -> f32 {
        self.descender()
    }

    fn advanced_x(&self) -> f32 {
        self.advanced_x()
    }

    fn x_min(&self) -> f32 {
        self.x_min()
    }

    fn y_max(&self) -> f32 {
        self.y_max()
    }

    fn stroke_x(&self) -> f32 {
        self.stroke_x()
    }

    fn stroke_y(&self) -> f32 {
        self.stroke_y()
    }

    fn stroke_bitmap(&self) -> Option<(Vec<u8>, u32)> {
        let (bitmap, w) = self.stroke_bitmap()?;
        Some((bitmap.clone(), w))
    }
}

impl fi::GuestFontKit for FontKit {
    fn new() -> Self {
        FontKit::new()
    }

    fn add_font_from_buffer(&self, buffer: Vec<u8>) {
        #[cfg(feature = "parse")]
        let _ = self.add_font_from_buffer(buffer);
    }

    fn query(&self, key: fi::FontKey) -> Option<fi::Font> {
        self.query(&FontKey::from(key)).map(fi::Font::new)
    }

    fn exact_match(&self, key: fi::FontKey) -> Option<fi::Font> {
        self.exact_match(&FontKey::from(key)).map(fi::Font::new)
    }

    fn len(&self) -> u32 {
        self.len() as u32
    }

    fn remove(&self, key: fi::FontKey) {
        self.remove(FontKey::from(key))
    }

    fn add_search_path(&self, path: String) {
        #[cfg(feature = "parse")]
        self.search_fonts_from_path(path).unwrap()
    }

    fn fonts_info(&self) -> Vec<fi::FontInfo> {
        self.fonts.iter().flat_map(|i| font_info(&*i)).collect()
    }

    fn measure(&self, key: fi::FontKey, text: String) -> Option<fi::TextMetrics> {
        Some(fi::TextMetrics::new(self.measure(&key.into(), &text)?))
    }

    fn read_data(&self, data: String) {
        let result: Result<Vec<serde_json::Map<String, serde_json::Value>>, _> =
            serde_json::from_str(&data);
        if let Ok(data) = result {
            for item in data {
                let path = item
                    .get("path")
                    .and_then(|v| serde_json::from_value::<Option<PathBuf>>(v.clone()).ok());
                let variants = item
                    .get("variants")
                    .and_then(|v| serde_json::from_value::<Vec<VariationData>>(v.clone()).ok());
                if let (Some(path), Some(variants)) = (path, variants) {
                    let font = Font::new(path, variants, self.hit_counter.clone());
                    let key = font.first_key();
                    self.fonts.insert(key, font);
                }
            }
        }
    }

    fn write_data(&self) -> String {
        let mut result = vec![];
        for value in self.fonts.iter() {
            let font = value.value();
            let mut value = serde_json::Map::new();
            value.insert(
                "path".to_string(),
                serde_json::to_value(font.path()).unwrap(),
            );
            value.insert(
                "variants".to_string(),
                serde_json::to_value(font.variants()).unwrap(),
            );
            result.push(value);
        }
        serde_json::to_string(&result).unwrap()
    }

    fn query_font_info(&self, key: fi::FontKey) -> Option<Vec<fi::FontInfo>> {
        let font = self.fonts.get(&self.query_font(&key)?)?;
        Some(font_info(&*font))
    }

    fn set_lru_limit(&self, limit: u32) {
        self.lru_limit.store(limit, Ordering::SeqCst);
    }
}

impl GuestTextMetrics for TextMetrics {
    fn new(value: String) -> Self {
        TextMetrics::new(value)
    }

    fn duplicate(&self) -> fi::TextMetrics {
        fi::TextMetrics::new(Clone::clone(self))
    }

    fn width(&self, font_size: f32, letter_spacing: f32) -> f32 {
        self.width(font_size, letter_spacing)
    }

    fn height(&self, font_size: f32, line_height: Option<f32>) -> f32 {
        self.height(font_size, line_height)
    }

    fn ascender(&self, font_size: f32) -> f32 {
        <TextMetrics as crate::Metrics>::ascender(self, font_size)
    }

    fn line_gap(&self) -> f32 {
        self.line_gap() as f32 / self.units() as f32
    }

    fn slice(&self, start: u32, count: u32) -> fi::TextMetrics {
        fi::TextMetrics::new(self.slice(start, count))
    }

    fn value(&self) -> String {
        TextMetrics::value(&self)
    }

    fn is_rtl(&self) -> bool {
        self.is_rtl()
    }

    fn append(&self, other: fi::TextMetrics) {
        TextMetrics::append(self, other.get::<TextMetrics>().clone())
    }

    fn count(&self) -> u32 {
        self.count() as u32
    }

    fn replace(&self, other: fi::TextMetrics, fallback: bool) {
        TextMetrics::replace(self, other.get::<TextMetrics>().clone(), fallback);
    }

    fn split_by_width(&self, font_size: f32, letter_spacing: f32, width: f32) -> fi::TextMetrics {
        fi::TextMetrics::new(self.split_by_width(font_size, letter_spacing, width))
    }

    fn chars(&self) -> Vec<char> {
        let p = self.positions.read().unwrap();
        p.iter().map(|c| c.metrics.c).collect()
    }

    fn units(&self) -> f32 {
        self.units() as f32
    }
}

struct Component;

impl fi::Guest for Component {
    type Font = StaticFace;
    type FontKit = FontKit;
    type GlyphBitmap = GlyphBitmap;
    type TextMetrics = TextMetrics;

    fn str_width_to_number(width: String) -> u16 {
        crate::str_width_to_number(&width)
    }

    fn number_width_to_str(width: u16) -> String {
        crate::number_width_to_str(width).to_string()
    }
}

crate::bindings::export!(Component with_types_in crate::bindings);

fn font_info(font: &Font) -> Vec<fi::FontInfo> {
    font.variants()
        .iter()
        .map(|v| fi::FontInfo {
            style_names: v
                .style_names
                .iter()
                .map(|n| fi::Name {
                    id: n.id,
                    name: n.name.clone(),
                    language_id: n.language_id,
                })
                .collect(),
            key: fi::FontKey::from(v.key.clone()),
            names: v
                .names
                .iter()
                .map(|n| fi::Name {
                    id: n.id,
                    name: n.name.clone(),
                    language_id: n.language_id,
                })
                .collect(),
            path: font.path().and_then(|p| Some(p.to_str()?.to_string())),
        })
        .collect::<Vec<_>>()
}
