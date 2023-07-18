#![feature(doc_auto_cfg)]

use arc_swap::ArcSwap;
use ouroboros::self_referencing;
use serde::{Deserialize, Serialize};
#[cfg(not(wasm))]
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Deref;
#[cfg(not(wasm))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
pub use ttf_parser::LineMetrics;
use ttf_parser::{Face, Width as ParserWidth};
// #[cfg(not(wasm))]
// use walkdir::WalkDir;
#[cfg(wasm)]
use tsify::Tsify;
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
#[cfg(feature = "metrics")]
pub use metrics::*;
#[cfg(feature = "ras")]
pub use ras::*;

#[cfg_attr(wasm, wasm_bindgen)]
pub fn str_width_to_number(width: &str) -> u16 {
    match width {
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
    }
    .to_number()
}

#[cfg_attr(wasm, wasm_bindgen)]
pub fn number_width_to_str(width: u16) -> String {
    match width {
        1 => "ultra-condensed",
        2 => "extra-condensed",
        3 => "condensed",
        4 => "semi-condensed",
        5 => "normal",
        6 => "semi-expanded",
        7 => "expanded",
        8 => "extra-expanded",
        9 => "ultra-expanded",
        _ => "normal",
    }
    .to_string()
}

#[cfg_attr(wasm, derive(Tsify, Serialize, Deserialize))]
#[cfg_attr(wasm, tsify(into_wasm_abi, from_wasm_abi))]
pub struct FontKeyArray(Vec<FontKey>);

// https://github.com/serde-rs/serde/issues/368
struct GenericDefault<const U: u32>;

impl<const U: u32> GenericDefault<U> {
    fn value() -> u32 {
        U
    }
}

#[cfg_attr(wasm, derive(Tsify))]
#[cfg_attr(wasm, tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Hash, PartialEq, PartialOrd, Eq, Debug, Default, Serialize, Deserialize)]
pub struct FontKey {
    /// Font weight, same as CSS [font-weight](https://developer.mozilla.org/en-US/docs/Web/CSS/font-weight#common_weight_name_mapping)
    #[serde(default = "GenericDefault::<400>::value")]
    pub weight: u32,
    /// Italic or not, boolean
    #[serde(default)]
    pub italic: bool,
    /// Font stretch, same as css [font-stretch](https://developer.mozilla.org/en-US/docs/Web/CSS/font-stretch)
    #[serde(default = "GenericDefault::<5>::value")]
    pub stretch: u32,
    /// Font family string
    pub family: String,
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

#[derive(Clone, Debug, Serialize)]
struct Name {
    pub name: String,
    #[allow(unused)]
    pub language_id: u16,
}

#[derive(Clone)]
pub struct Font {
    key: FontKey,
    index: u32,
    names: Vec<Name>,
    style_names: Vec<Name>,
    face: Arc<ArcSwap<Option<StaticFace>>>,
    path: Option<PathBuf>,
}

impl Font {
    pub fn key(&self) -> FontKey {
        self.key.clone()
    }

    fn from_buffer(mut buffer: &[u8]) -> Result<Vec<Self>, Error> {
        let ty = infer::get(buffer).ok_or(Error::UnrecognizedBuffer)?;
        let mut font_count = 1;
        let buffer = match (ty.mime_type(), ty.extension()) {
            #[cfg(feature = "woff2")]
            ("application/font-woff", "woff2") => woff2::convert_woff2_to_ttf(&mut buffer)?,
            #[cfg(feature = "woff")]
            ("application/font-woff", "woff") => {
                use std::io::Cursor;

                let reader = Cursor::new(buffer);
                let mut otf_buf = Cursor::new(Vec::new());
                conv::woff::convert_woff_to_otf(reader, &mut otf_buf)?;
                otf_buf.into_inner()
            }
            ("application/font-sfnt", _) => buffer.to_vec(),
            ("application/font-collection", _) => {
                font_count = ttf_parser::fonts_in_collection(&buffer).unwrap_or(1);
                buffer.to_vec()
            }
            a => return Err(Error::UnsupportedMIME(a.0)),
        };

        (0..font_count)
            .map(|index| Font::from_buffer_with_index(buffer.clone(), index))
            .collect::<Result<_, _>>()
    }

    fn from_buffer_with_index(buffer: Vec<u8>, index: u32) -> Result<Self, Error> {
        use ttf_parser::name_id;
        let face = StaticFaceTryBuilder {
            buffer: buffer.clone(),
            face_builder: |buf| Face::parse(buf, index),
        }
        .try_build()?;
        let mut style_names = vec![];
        let names = face
            .borrow_face()
            .names()
            .into_iter()
            .filter_map(|name| {
                let id = name.name_id;
                let mut name_str = name.to_string().or_else(|| {
                    // try to force unicode encoding
                    Some(std::str::from_utf8(name.name).ok()?.to_string())
                })?;
                if id == name_id::TYPOGRAPHIC_SUBFAMILY {
                    style_names.push(Name {
                        name: name_str.clone(),
                        language_id: name.language_id,
                    });
                }
                if id == name_id::FAMILY
                    || id == name_id::FULL_NAME
                    || id == name_id::POST_SCRIPT_NAME
                    || id == name_id::TYPOGRAPHIC_FAMILY
                {
                    if id == name_id::POST_SCRIPT_NAME {
                        name_str = name_str.replace(" ", "-");
                    }
                    Some(Name {
                        name: name_str,
                        language_id: name.language_id,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err(Error::EmptyName);
        }
        // Select a good name
        let ascii_name = names
            .iter()
            .map(|item| &item.name)
            .filter(|name| name.is_ascii() && name.len() > 3)
            .min_by(|n1, n2| match n1.len().cmp(&n2.len()) {
                std::cmp::Ordering::Equal => n1
                    .chars()
                    .filter(|c| *c == '-')
                    .count()
                    .cmp(&n2.chars().filter(|c| *c == '-').count()),
                ordering @ _ => ordering,
            })
            .cloned()
            .map(|name| {
                if name.starts_with(".") {
                    (&name[1..]).to_string()
                } else {
                    name
                }
            });
        let key = FontKey {
            weight: face.borrow_face().weight().to_number() as u32,
            italic: face.borrow_face().is_italic(),
            stretch: face.borrow_face().width().to_number() as u32,
            family: ascii_name.unwrap_or_else(|| names[0].name.clone()),
        };
        let font = Font {
            names,
            key,
            index,
            face: Arc::new(ArcSwap::new(Arc::new(Some(face)))),
            path: None,
            style_names,
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
            let font = Font::from_buffer_with_index(buffer, self.index)?;
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

    pub fn strikeout_metrics(&self) -> Option<LineMetrics> {
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        f.strikeout_metrics()
    }

    pub fn underline_metrics(&self) -> Option<LineMetrics> {
        let f = self.face.load();
        let f = f.as_ref().as_ref().unwrap();
        let f = f.borrow_face();
        f.underline_metrics()
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
    #[cfg(not(dashmap))]
    fonts: HashMap<FontKey, Font>,
    #[cfg(dashmap)]
    fonts: dashmap::DashMap<FontKey, Font>,
    fallback_font_key: Option<Box<dyn Fn(FontKey) -> FontKey + Send + Sync>>,
}

#[cfg_attr(wasm, wasm_bindgen)]
impl FontKit {
    /// Create a font registry
    #[cfg_attr(wasm, wasm_bindgen(constructor))]
    pub fn new() -> Self {
        #[cfg(wasm)]
        console_error_panic_hook::set_once();
        FontKit {
            #[cfg(not(dashmap))]
            fonts: HashMap::new(),
            #[cfg(dashmap)]
            fonts: dashmap::DashMap::new(),
            fallback_font_key: None,
        }
    }

    /// Add fonts from a buffer. This will load the fonts and store the buffer
    /// in FontKit. Type information is inferred from the magic number using
    /// `infer` crate.
    ///
    /// If the given buffer is a font collection (ttc), multiple keys will be
    /// returned.
    #[cfg(wasm)]
    #[wasm_bindgen(js_name = "add_font_from_buffer")]
    pub fn add_font_from_buffer_wasm(
        &mut self,
        buffer: Vec<u8>,
    ) -> Result<FontKeyArray, js_sys::Error> {
        Ok(FontKeyArray(self.add_font_from_buffer(buffer)?))
    }

    #[cfg(wasm)]
    #[wasm_bindgen(js_name = "query")]
    pub fn query_wasm(&self, key: FontKey) -> Option<wasm::FontWasm> {
        let font = self.query(&key)?;
        let font = font.deref();
        Some(wasm::FontWasm {
            ptr: font as *const _,
        })
    }

    #[cfg(wasm)]
    #[wasm_bindgen(js_name = "exact_match")]
    pub fn exact_match_wasm(&self, key: FontKey) -> Option<wasm::FontWasm> {
        let font = self.exact_match(&key)?;
        let font = font.deref();
        Some(wasm::FontWasm {
            ptr: font as *const _,
        })
    }

    #[cfg(wasm)]
    #[wasm_bindgen(js_name = "font_keys")]
    pub fn font_keys_wasm(&self) -> FontKeyArray {
        FontKeyArray(self.font_keys().collect())
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    /// Setup a font as fallback. When measure fails, FontKit will use this
    /// fallback to measure, if possible
    #[cfg(not(wasm))]
    pub fn set_fallback(
        &mut self,
        font_key: Option<impl Fn(FontKey) -> FontKey + Send + Sync + 'static>,
    ) {
        self.fallback_font_key = font_key.map(|f| Box::new(f) as _);
    }

    #[cfg(feature = "metrics")]
    pub fn measure(&self, font_key: FontKey, text: &str) -> Option<TextMetrics> {
        match self
            .query(&font_key)
            .and_then(|font| font.measure(text).ok())
        {
            Some(mut metrics) => {
                let has_missing = metrics.positions.iter().any(|c| c.metrics.missing);
                if has_missing {
                    if let Some(font) = self
                        .fallback_font_key
                        .as_ref()
                        .and_then(|key| self.query(&(key)(font_key.clone())))
                    {
                        if let Ok(new_metrics) = font.measure(text) {
                            for (old, new) in metrics
                                .positions
                                .iter_mut()
                                .zip(new_metrics.positions.into_iter())
                            {
                                if old.metrics.missing {
                                    old.metrics = new.metrics;
                                    old.kerning = new.kerning;
                                }
                            }
                        }
                    }
                }
                Some(metrics)
            }
            None => {
                let font = self
                    .fallback_font_key
                    .as_ref()
                    .and_then(|key| self.query(&(key)(font_key.clone())))?;
                font.measure(text).ok()
            }
        }
    }
}

impl FontKit {
    /// Add fonts from a buffer. This will load the fonts and store the buffer
    /// in FontKit. Type information is inferred from the magic number using
    /// `infer` crate.
    ///
    /// If the given buffer is a font collection (ttc), multiple keys will be
    /// returned.
    #[cfg(not(dashmap))]
    pub fn add_font_from_buffer(&mut self, buffer: Vec<u8>) -> Result<Vec<FontKey>, Error> {
        Ok(Font::from_buffer(&buffer)?
            .into_iter()
            .map(|font| {
                let key = font.key().clone();
                self.fonts.insert(key.clone(), font);
                key
            })
            .collect::<Vec<_>>())
    }

    /// Add fonts from a buffer. This will load the fonts and store the buffer
    /// in FontKit. Type information is inferred from the magic number using
    /// `infer` crate.
    ///
    /// If the given buffer is a font collection (ttc), multiple keys will be
    /// returned.
    #[cfg(dashmap)]
    pub fn add_font_from_buffer(&self, buffer: Vec<u8>) -> Result<Vec<FontKey>, Error> {
        Ok(Font::from_buffer(&buffer)?
            .into_iter()
            .map(|font| {
                let key = font.key().clone();
                self.fonts.insert(key.clone(), font);
                key
            })
            .collect::<Vec<_>>())
    }

    pub fn font_keys(&self) -> impl Iterator<Item = FontKey> + '_ {
        #[cfg(dashmap)]
        return self.fonts.iter().map(|i| {
            log::info!("{:?}", i.names);
            i.key().clone()
        });
        #[cfg(not(dashmap))]
        self.fonts.keys()
    }

    /// Recursively scan a local path for fonts, this method will not store the
    /// font buffer to reduce memory consumption
    #[cfg(not(wasm))]
    pub fn search_fonts_from_path(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        #[cfg(not(any(wasm, wasi)))]
        for entry in walkdir::WalkDir::new(path) {
            let entry = entry?;
            log::trace!("new entry {:?}", entry);
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if let Some(fonts) = load_font_from_path(&path) {
                for font in fonts {
                    self.fonts.insert(font.key(), font);
                }
            }
        }
        #[cfg(wasi)]
        if let Some(fonts) = load_font_from_path(path.as_ref()) {
            for font in fonts {
                self.fonts.insert(font.key(), font);
            }
        }
        Ok(())
    }

    pub fn exact_match(&self, key: &FontKey) -> Option<impl Deref<Target = Font> + '_> {
        #[cfg(dashmap)]
        return self.fonts.iter().find(|font| *font.key() == *key);
        #[cfg(not(dashmap))]
        self.fonts.values().find(|font| font.key == *key)
    }

    pub fn remove(&self, key: &FontKey) {
        self.fonts.remove(key);
    }

    pub fn query(&self, key: &FontKey) -> Option<impl Deref<Target = Font> + '_> {
        let mut filters = vec![
            Filter::Family(&key.family),
            Filter::Italic(key.italic),
            Filter::Weight(key.weight),
            Filter::Stretch(key.stretch),
        ];
        // Fallback weight logic
        filters.push(Filter::Weight(0));
        let mut search_results = self
            .fonts
            .iter()
            .map(|item| {
                #[cfg(dashmap)]
                return item.key().clone();
                #[cfg(not(dashmap))]
                item.0.clone()
            })
            .collect::<HashSet<_>>();
        for filter in filters {
            let mut s = search_results.clone();
            let mut is_family = false;
            match filter {
                Filter::Family(f) => {
                    is_family = true;
                    s.retain(|key| {
                        key.family == f
                            || self
                                .fonts
                                .get(key)
                                .unwrap()
                                .names
                                .iter()
                                .any(|n| n.name == f)
                    })
                }
                Filter::Italic(i) => s.retain(|key| key.italic == i),
                Filter::Weight(w) => s.retain(|key| w == 0 || key.weight == w),
                Filter::Stretch(st) => s.retain(|key| key.stretch == st),
            };
            match s.len() {
                1 => return self.fonts.get(s.iter().next()?),
                0 if is_family => return None,
                0 => {}
                _ => search_results = s,
            }
        }
        None
    }
}

#[cfg(not(wasm))]
fn load_font_from_path(path: impl AsRef<std::path::Path>) -> Option<Vec<Font>> {
    use std::io::Read;

    let mut buffer = Vec::new();
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase());
    let ext = ext.as_deref();
    let ext = match ext {
        Some(e) => e,
        None => return None,
    };
    match ext {
        "ttf" | "otf" | "ttc" | "woff2" | "woff" => {
            let mut file = std::fs::File::open(&path).unwrap();
            buffer.clear();
            file.read_to_end(&mut buffer).unwrap();
            let mut fonts = match Font::from_buffer(&buffer) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("Failed loading font {:?}: {:?}", path, e);
                    return None;
                }
            };
            for font in &mut fonts {
                font.path = Some(path.to_path_buf());
                // println!("{:?}", font.names);
                font.unload();
            }
            Some(fonts)
        }
        _ => None,
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
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn build_font_kit() -> *const u8 {
    let fontkit = FontKit::new();
    Box::into_raw(Box::new(fontkit)) as _
}

#[cfg(not(wasm))]
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn add_search_path(fontkit: *mut u8, custom_path: *const u8, len: usize) {
    let fontkit = &mut *(fontkit as *mut FontKit);
    let custom_path = std::slice::from_raw_parts(custom_path, len);
    match std::str::from_utf8(custom_path) {
        Ok("") => {}
        Ok(path) => {
            if let Err(e) = fontkit.search_fonts_from_path(path) {
                eprintln!("{:?}", e);
            }
        }
        Err(e) => {
            eprintln!("{:?}", e)
        }
    }
}

#[cfg(not(wasm))]
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn font_for_face(
    fontkit: *mut FontKit,
    font_face: *const u8,
    len: usize,
    weight: u32,
    italic: bool,
    stretch: u16,
) -> *const u8 {
    let fontkit = &mut *fontkit;
    let font_face = std::slice::from_raw_parts(font_face, len);
    let font_face = std::str::from_utf8_unchecked(font_face);
    let key = FontKey {
        family: font_face.to_string(),
        weight,
        italic,
        stretch: stretch as u32,
    };
    let font = fontkit.query(&key);
    match font {
        Some(font) => font.deref() as *const _ as *const u8,
        None => {
            eprintln!("{:?} not found in {} fonts", key, fontkit.len());
            std::ptr::null()
        }
    }
}

#[cfg(not(wasm))]
#[doc(hidden)]
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
#[doc(hidden)]
#[no_mangle]
pub fn fontkit_alloc() -> *mut u8 {
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
#[doc(hidden)]
#[no_mangle]
pub unsafe fn fontkit_mfree(ptr: *mut u8) {
    let _ = Vec::from_raw_parts(ptr, 1024, 1024);
}

#[cfg(not(wasm))]
#[doc(hidden)]
#[no_mangle]
pub unsafe fn free_fontkit(ptr: *mut FontKit) {
    let _ = Box::from_raw(ptr);
}

#[cfg(not(wasm))]
#[doc(hidden)]
#[no_mangle]
pub unsafe fn free_fontkit_str(ptr: *mut u8) {
    if let Some(len) = ALLOCS.with(|map| map.borrow_mut().remove(&(ptr as u64))) {
        let _ = Vec::from_raw_parts(ptr, len, len);
    }
}

#[cfg(not(wasm))]
#[doc(hidden)]
#[no_mangle]
pub fn fontkit_str_length(ptr: *const u8) -> u32 {
    ALLOCS.with(|map| {
        map.borrow()
            .get(&(ptr as u64))
            .map(|l| *l as u32)
            .unwrap_or_default()
    })
}

#[cfg(not(wasm))]
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn list_all_font(fontkit: *mut FontKit) -> *const u8 {
    let kit = &*fontkit;
    let fonts = kit
        .fonts
        .iter()
        .map(|font| {
            #[cfg(not(dashmap))]
            let font = font.1;
            let key = font.key();
            let path = match font.path().and_then(|p| p.to_str()).map(|p| p.to_string()) {
                Some(p) => p,
                None => "".to_string(),
            };
            serde_json::json!({
                "names": font.names,
                "stretch": number_width_to_str(key.stretch as u16),
                "italic": key.italic,
                "weight": key.weight,
                "family": key.family,
                "styleNames": font.style_names.clone(),
                "path": path,
            })
        })
        .collect::<Vec<_>>();
    let data = serde_json::to_string(&fonts).unwrap();
    let buffer = data.as_bytes().to_vec();
    let (ptr, len) = into_raw(buffer);
    ALLOCS.with(|map| map.borrow_mut().insert(ptr as u64, len));
    ptr
}
