use arc_swap::ArcSwap;
#[cfg(node)]
use napi_derive::napi;
use ouroboros::self_referencing;
use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use ttf_parser::{Face, Width as ParserWidth};
#[cfg(not(wasm))]
use walkdir::WalkDir;
#[cfg(wasm)]
use wasm_bindgen::prelude::*;

mod conv;
mod error;
#[cfg(feature = "metrics")]
mod metrics;
#[cfg(feature = "ras")]
mod ras;
#[cfg(wasm)]
mod wasm;

pub use error::Error;

struct Width(ParserWidth);

impl FromStr for Width {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Width(match s {
            "ultra-condensed" => ParserWidth::UltraCondensed,
            "condensed" => ParserWidth::Condensed,
            "normal" => ParserWidth::Normal,
            "expanded" => ParserWidth::Expanded,
            "ultra-expanded" => ParserWidth::UltraExpanded,
            "extra-condensed" => ParserWidth::ExtraCondensed,
            "semi-condensed" => ParserWidth::SemiCondensed,
            "semi-expanded" => ParserWidth::SemiExpanded,
            "extra-expanded" => ParserWidth::ExtraExpanded,
            _ => ParserWidth::Normal,
        }))
    }
}

impl ToString for Width {
    fn to_string(&self) -> String {
        match self.0 {
            ParserWidth::UltraCondensed => "ultra-condensed",
            ParserWidth::Condensed => "condensed",
            ParserWidth::Normal => "normal",
            ParserWidth::Expanded => "expanded",
            ParserWidth::UltraExpanded => "ultra-expanded",
            ParserWidth::ExtraCondensed => "extra-condensed",
            ParserWidth::SemiCondensed => "semi-condensed",
            ParserWidth::SemiExpanded => "semi-expanded",
            ParserWidth::ExtraExpanded => "extra-expanded",
        }
        .to_string()
    }
}

#[cfg_attr(node, napi)]
#[cfg_attr(wasm, wasm_bindgen)]
#[cfg_attr(features = "serde", serde::Serialize)]
#[cfg_attr(features = "serde", serde::Deserialize)]
#[derive(Clone, Hash, PartialEq, PartialOrd, Eq, Debug, Default)]
pub struct FontKey {
    /// Font weight, same as CSS [font-weight](https://developer.mozilla.org/en-US/docs/Web/CSS/font-weight#common_weight_name_mapping)
    pub weight: u32,
    /// Italic or not, boolean
    pub italic: bool,
    stretch: u32,
    family: String,
}

impl fmt::Display for FontKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FontKey({}, {}, {}, {})",
            self.family, self.weight, self.italic, self.stretch
        )
    }
}

#[cfg_attr(wasm, wasm_bindgen)]
impl FontKey {
    #[inline]
    fn stretch_enum(&self) -> ParserWidth {
        match self.stretch {
            1 => ParserWidth::UltraCondensed,
            2 => ParserWidth::ExtraCondensed,
            3 => ParserWidth::Condensed,
            4 => ParserWidth::SemiCondensed,
            5 => ParserWidth::Normal,
            6 => ParserWidth::SemiExpanded,
            7 => ParserWidth::Expanded,
            8 => ParserWidth::ExtraExpanded,
            9 => ParserWidth::UltraExpanded,
            _ => ParserWidth::Normal,
        }
    }

    #[cfg_attr(wasm, wasm_bindgen(getter = stretch))]
    /// Font stretch, same as css [font-stretch](https://developer.mozilla.org/en-US/docs/Web/CSS/font-stretch)
    pub fn stretch_str(&self) -> String {
        Width(self.stretch_enum()).to_string()
    }

    #[cfg_attr(wasm, wasm_bindgen(getter))]
    /// Font family string
    pub fn family(&self) -> String {
        self.family.clone()
    }
}

#[derive(Clone)]
struct Name {
    pub name: String,
    pub language_id: u16,
}

pub struct Font {
    key: FontKey,
    names: Vec<Name>,
    face: ArcSwap<Option<StaticFace>>,
    path: Option<PathBuf>,
}

impl Font {
    fn key(&self) -> &FontKey {
        &self.key
    }

    fn from_buffer(mut buffer: &[u8]) -> Result<Self, Error> {
        use ttf_parser::name_id;
        let ty = infer::get(&buffer).ok_or_else(|| Error::UnrecognizedBuffer)?;
        let buffer = match (ty.mime_type(), ty.extension()) {
            #[cfg(feature = "woff2")]
            ("application/woff", "woff2") => {
                let reader = Cursor::new(&mut buffer);
                let mut otf_buf = Cursor::new(Vec::new());
                conv::woff2::convert_woff2_to_otf(reader, &mut otf_buf)?;
                otf_buf.into_inner()
            }
            ("application/woff", "woff") => {
                let reader = Cursor::new(buffer);
                let mut otf_buf = Cursor::new(Vec::new());
                conv::woff::convert_woff_to_otf(reader, &mut otf_buf)?;
                otf_buf.into_inner()
            }
            ("application/font-sfnt", _) => buffer.to_vec(),
            a @ _ => return Err(Error::UnsupportedMIME(a.0)),
        };
        let face = StaticFaceTryBuilder {
            buffer,
            face_builder: |buf| Face::from_slice(buf, 0),
        }
        .try_build()?;
        let names = face
            .borrow_face()
            .names()
            .into_iter()
            .filter_map(|name| {
                let id = name.name_id;
                if id == name_id::FAMILY
                    || id == name_id::FULL_NAME
                    || id == name_id::POST_SCRIPT_NAME
                    || id == name_id::TYPOGRAPHIC_FAMILY
                {
                    Some(Name {
                        name: name.to_string()?,
                        language_id: name.language_id,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let key = FontKey {
            weight: face.borrow_face().weight().to_number() as u32,
            italic: face.borrow_face().is_italic(),
            stretch: face.borrow_face().width().to_number() as u32,
            family: names[0].name.clone(),
        };
        let font = Font {
            names,
            key,
            face: ArcSwap::new(Arc::new(Some(face))),
            path: None,
        };
        Ok(font)
    }

    pub fn unload(&self) {
        self.face.swap(Arc::new(None));
    }

    #[cfg(not(wasm32))]
    pub fn load(&self) -> Result<(), Error> {
        use std::io::Read;

        if self.face.load().is_some() {
            return Ok(());
        }
        if let Some(path) = self.path.as_ref() {
            let mut buffer = Vec::new();
            let mut file = std::fs::File::open(path)?;
            file.read_to_end(&mut buffer).unwrap();
            let font = Font::from_buffer(&buffer)?;
            self.face.store(font.face.load_full());
        }
        Ok(())
    }

    pub fn has_glyph(&self, c: char) -> bool {
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        f.glyph_index(c).is_some()
    }

    pub fn ascender(&self) -> i16 {
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        f.ascender()
    }

    pub fn descender(&self) -> i16 {
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        f.descender()
    }

    pub fn units_per_em(&self) -> u16 {
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        f.units_per_em()
    }
}

#[self_referencing]
pub struct StaticFace {
    buffer: Vec<u8>,
    #[borrows(buffer)]
    #[covariant]
    pub(crate) face: Face<'this>,
}

#[cfg_attr(node, napi)]
#[cfg_attr(wasm, wasm_bindgen)]
pub struct FontKit {
    fonts: Vec<Font>,
}

#[cfg_attr(node, napi)]
#[cfg_attr(wasm, wasm_bindgen)]
impl FontKit {
    #[cfg(napi)]
    #[nap(constructor)]
    pub fn new_napi() -> FontKit {
        FontKit::new()
    }

    #[cfg(wasm)]
    #[wasm_bindgen(constructor)]
    /// Create a font registry
    pub fn new_wasm() -> Self {
        FontKit { fonts: Vec::new() }
    }

    #[cfg(wasm)]
    #[wasm_bindgen(js_name = "add_font_from_buffer")]
    /// Add a font from a buffer. This will load the font and store the font
    /// buffer in FontKit. Type information is inferred from the magic number
    /// using `infer` crate
    pub fn add_font_from_buffer_wasm(&mut self, buffer: Vec<u8>) -> Result<FontKey, JsValue> {
        Ok(self.add_font_from_buffer(buffer)?)
    }

    #[cfg(wasm)]
    #[wasm_bindgen(js_name = "query")]
    pub fn query_wasm(&self, key: &FontKey) -> Option<wasm::FontWasm> {
        let font = self.query(key)?;
        Some(wasm::FontWasm {
            ptr: font as *const _,
        })
    }
}

impl FontKit {
    /// Create a font registry
    pub fn new() -> Self {
        // #[cfg(wasm)]
        // wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
        // #[cfg(wasm)]
        // console_error_panic_hook::set_once();
        FontKit { fonts: Vec::new() }
    }

    /// Add a font from a buffer. This will load the font and store the font
    /// buffer in FontKit. Type information is inferred from the magic number
    /// using `infer` crate
    pub fn add_font_from_buffer(&mut self, buffer: Vec<u8>) -> Result<FontKey, Error> {
        let font = Font::from_buffer(&buffer)?;
        let key = font.key().clone();
        self.fonts.push(font);
        Ok(key)
    }

    /// Recursively scan a local path for fonts, this method will not store the
    /// font buffer to reduce memory consumption
    #[cfg(not(wasm))]
    pub fn search_fonts_from_path(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        use std::io::Read;

        let mut buffer = Vec::new();
        for entry in WalkDir::new(path) {
            let entry = entry?;
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            let ext = ext.as_ref().map(|s| s.as_str());
            let ext = match ext {
                Some(e) => e,
                None => continue,
            };
            match ext {
                "ttf" | "otf" | "ttc" | "woff2" | "woff" => {
                    let mut file = std::fs::File::open(path).unwrap();
                    buffer.clear();
                    file.read_to_end(&mut buffer).unwrap();
                    let mut font = Font::from_buffer(&buffer)?;
                    font.path = Some(path.to_path_buf());
                    font.unload();
                    self.fonts.push(font);
                }
                _ => continue,
            }
        }
        Ok(())
    }

    #[cfg(all(feature = "fontdb", not(wasm)))]
    pub fn to_fontdb(&self) -> Result<fontdb::Database, Error> {
        let mut db = fontdb::Database::new();
        for font in &self.fonts {
            if let Some(face) = &**font.face.load() {
                db.load_font_data(face.borrow_buffer().clone());
                continue;
            }
            if let Some(path) = font.path.as_ref() {
                db.load_font_file(path)?
            }
        }
        Ok(db)
    }

    pub fn query(&self, key: &FontKey) -> Option<&Font> {
        let mut filters = vec![
            Filter::Family(&key.family),
            Filter::Italic(key.italic),
            Filter::Weight(key.weight),
            Filter::Stretch(key.stretch),
        ];
        // Fallback weight logic
        let mut weight = key.weight;
        if key.weight >= 400 {
            loop {
                weight += 25;
                if weight > 900 {
                    break;
                }
                filters.push(Filter::Weight(weight))
            }
        } else if key.weight > 100 {
            loop {
                weight -= 25;
                if weight < 100 {
                    break;
                }
                filters.push(Filter::Weight(weight))
            }
        }
        let mut search_results = self
            .fonts
            .iter()
            .map(|item| (item.key(), item))
            .collect::<HashMap<_, _>>();
        for filter in filters {
            let mut s = search_results.clone();
            match filter {
                Filter::Family(f) => {
                    s.retain(|key, item| key.family == f || item.names.iter().any(|n| n.name == f))
                }
                Filter::Italic(i) => s.retain(|key, _| key.italic == i),
                Filter::Weight(w) => s.retain(|key, _| key.weight == w),
                Filter::Stretch(st) => s.retain(|key, _| key.stretch == st),
            };
            match s.len() {
                1 => return s.values().next().map(|f| *f),
                0 => {}
                _ => search_results = s,
            }
        }
        None
    }
}

enum Filter<'a> {
    Family(&'a str),
    Italic(bool),
    Weight(u32),
    Stretch(u32),
}
