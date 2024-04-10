use crate::bindings::exports::alibaba::fontkit::fontkit_interface as fi;
use crate::*;

use self::bindings::exports::alibaba::fontkit::fontkit_interface::GuestTextMetrics;

impl fi::GuestFont for Font {
    fn has_glyph(&self, c: char) -> bool {
        self.has_glyph(c)
    }

    fn buffer(&self) -> Vec<u8> {
        self.load().unwrap();
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        f.borrow_buffer().clone()
    }

    fn path(&self) -> String {
        self.path()
            .and_then(|p| p.to_str())
            .unwrap_or_default()
            .to_string()
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
}

impl From<fi::FontKey> for FontKey {
    fn from(value: fi::FontKey) -> Self {
        FontKey {
            weight: value.weight.unwrap_or(400) as u32,
            italic: value.italic.unwrap_or(false),
            stretch: value.stretch.unwrap_or(5) as u32,
            family: value.family,
        }
    }
}

impl From<FontKey> for fi::FontKey {
    fn from(value: FontKey) -> Self {
        fi::FontKey {
            weight: Some(value.weight as u16),
            italic: Some(value.italic),
            stretch: Some(value.stretch as u16),
            family: value.family,
        }
    }
}

impl fi::GuestFontKit for FontKit {
    fn new() -> Self {
        FontKit::new()
    }

    fn add_font_from_buffer(&self, buffer: Vec<u8>) -> Vec<fi::FontKey> {
        self.add_font_from_buffer(buffer)
            .into_iter()
            .flatten()
            .map(fi::FontKey::from)
            .collect()
    }

    fn query(&self, key: fi::FontKey) -> Option<fi::Font> {
        self.query(&FontKey::from(key))
            .map(|f| fi::Font::new(f.clone()))
    }

    fn exact_match(&self, key: fi::FontKey) -> Option<fi::Font> {
        self.exact_match(&FontKey::from(key))
            .map(|f| fi::Font::new(f.clone()))
    }

    fn font_keys(&self) -> Vec<fi::FontKey> {
        self.font_keys().map(fi::FontKey::from).collect()
    }

    fn len(&self) -> u32 {
        self.len() as u32
    }

    fn remove(&self, key: fi::FontKey) {
        self.remove(FontKey::from(key))
    }

    fn add_search_path(&self, path: String) {
        self.search_fonts_from_path(path).unwrap()
    }

    fn fonts_info(&self) -> Vec<fi::FontInfo> {
        self.fonts
            .iter()
            .map(|i| fi::FontInfo {
                style_names: i
                    .style_names
                    .iter()
                    .map(|n| fi::Name {
                        id: n.id,
                        name: n.name.clone(),
                        language_id: n.language_id,
                    })
                    .collect(),
                key: fi::FontKey::from(i.key().clone()),
                names: i
                    .names
                    .iter()
                    .map(|n| fi::Name {
                        id: n.id,
                        name: n.name.clone(),
                        language_id: n.language_id,
                    })
                    .collect(),
                path: i.path().and_then(|p| Some(p.to_str()?.to_string())),
            })
            .collect()
    }
}

impl GuestTextMetrics for TextMetrics {
    fn width(&self, font_size: f32, letter_spacing: f32) -> f32 {
        self.width(font_size, letter_spacing)
    }

    fn height(&self, font_size: f32, line_height: Option<f32>) -> f32 {
        self.height(font_size, line_height)
    }

    fn ascender(&self, font_size: f32) -> f32 {
        let factor = font_size / self.units() as f32;
        (self.ascender() as f32 + self.line_gap() as f32 / 2.0) * factor
    }
}

struct Component;

impl fi::Guest for Component {
    type Font = Font;
    type FontKit = FontKit;
    type TextMetrics = TextMetrics;

    fn str_width_to_number(width: String) -> u16 {
        crate::str_width_to_number(&width)
    }

    fn number_width_to_str(width: u16) -> String {
        crate::number_width_to_str(width).to_string()
    }
}

bindings::export!(Component with_types_in bindings);
