use arc_swap::ArcSwap;
use ouroboros::self_referencing;
#[cfg(not(wasm))]
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
#[cfg(not(wasm))]
use std::path::Path;
use std::path::PathBuf;
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

#[cfg_attr(wasm, wasm_bindgen)]
pub struct Width(ParserWidth);

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

impl From<u16> for Width {
    fn from(stretch: u16) -> Self {
        Width(match stretch {
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
        })
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
    pub fn new(family: &str, weight: u32, italic: bool, stretch: Width) -> Self {
        FontKey {
            family: family.to_string(),
            weight,
            italic,
            stretch: stretch.0.to_number() as u32,
        }
    }

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
    #[allow(unused)]
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
        let ty = infer::get(buffer).ok_or(Error::UnrecognizedBuffer)?;
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
            a => return Err(Error::UnsupportedMIME(a.0)),
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

    #[cfg(not(wasm))]
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

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }
}

#[self_referencing]
pub struct StaticFace {
    buffer: Vec<u8>,
    #[borrows(buffer)]
    #[covariant]
    pub(crate) face: Face<'this>,
}

#[cfg_attr(wasm, wasm_bindgen)]
pub struct FontKit {
    fonts: Vec<Font>,
}

#[cfg_attr(wasm, wasm_bindgen)]
impl FontKit {
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

    pub fn len(&self) -> usize {
        self.fonts.len()
    }
}

impl FontKit {
    /// Create a font registry
    #[allow(clippy::new_without_default)]
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
            let ext = ext.as_deref();
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
                1 => return s.values().next().copied(),
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

#[cfg(not(wasm))]
std::thread_local! {
    static ALLOCS: RefCell<HashMap<u64, usize>> = RefCell::new(HashMap::new());
}

#[cfg(not(wasm))]
static mut FONTKIT: Option<FontKit> = None;

#[cfg(not(wasm))]
#[no_mangle]
pub unsafe extern "C" fn build_font_kit(custom_path: *const u8, len: usize) {
    if FONTKIT.is_some() {
        return;
    }
    let custom_path = std::slice::from_raw_parts(custom_path, len);
    let custom_path = std::str::from_utf8(custom_path).unwrap();
    let mut fontkit = FontKit::new();
    if custom_path != "" {
        fontkit.search_fonts_from_path(&custom_path).unwrap();
    }
    FONTKIT = Some(fontkit);
}

#[cfg(not(wasm))]
#[no_mangle]
pub unsafe extern "C" fn font_for_face(
    font_face: *const u8,
    len: usize,
    weight: u32,
    italic: bool,
    stretch: u16,
) -> *const u8 {
    let font_face = std::slice::from_raw_parts(font_face, len);
    let font_face = std::str::from_utf8(font_face).unwrap();
    let fontkit = FONTKIT.as_ref().unwrap();
    let key = FontKey::new(font_face, weight, italic, stretch.into());
    let font = fontkit.query(&key);
    match font {
        Some(font) => font as *const _ as *const u8,
        None => {
            eprintln!("{:?} not found in {} fonts", key, fontkit.len());
            std::ptr::null()
        }
    }
}

#[cfg(not(wasm))]
#[no_mangle]
pub unsafe extern "C" fn path_for_font(font: *const u8) -> *const u8 {
    let font = &*(font as *const Font);
    let path = match font.path().and_then(|p| p.to_str()).map(|p| p.to_string()) {
        Some(p) => p,
        None => return 0 as _,
    };
    let buffer = path.as_bytes().to_vec();
    let (ptr, len) = into_raw(buffer);
    ALLOCS.with(|map| map.borrow_mut().insert(ptr as u64, len));
    ptr
}

#[cfg(not(wasm))]
#[no_mangle]
pub fn alloc() -> *mut u8 {
    let v = vec![0_u8; 1024];
    let (ptr, _) = into_raw(v);
    ptr
}

#[cfg(not(wasm))]
fn into_raw<T>(mut v: Vec<T>) -> (*mut T, usize) {
    let ptr = v.as_mut_ptr();
    let len = v.len();
    v.shrink_to_fit();
    std::mem::forget(v);
    (ptr, len)
}

#[cfg(not(wasm))]
#[no_mangle]
pub unsafe fn mfree(ptr: *mut u8) {
    let _ = Vec::from_raw_parts(ptr, 1024, 1024);
}

#[cfg(not(wasm))]
#[no_mangle]
pub unsafe fn free_str(ptr: *mut u8) {
    if let Some(len) = ALLOCS.with(|map| map.borrow_mut().remove(&(ptr as u64))) {
        let _ = Vec::from_raw_parts(ptr, len, len);
    }
}

#[cfg(not(wasm))]
#[no_mangle]
pub fn str_length(ptr: *const u8) -> u32 {
    ALLOCS.with(|map| {
        map.borrow()
            .get(&(ptr as u64))
            .map(|l| *l as u32)
            .unwrap_or_default()
    })
}
