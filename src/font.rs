use arc_swap::ArcSwap;
#[cfg(feature = "parse")]
use byteorder::{BigEndian, ReadBytesExt};
#[cfg(feature = "parse")]
use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use ouroboros::self_referencing;
use std::fmt;
use std::hash::Hash;
#[cfg(feature = "parse")]
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
pub use ttf_parser::LineMetrics;
use ttf_parser::{Face, Fixed, Tag, VariationAxis, Width as ParserWidth};

use crate::{Error, Filter};

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

#[derive(Clone, PartialEq, PartialOrd, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FontKey {
    /// Font weight, same as CSS [font-weight](https://developer.mozilla.org/en-US/docs/Web/CSS/font-weight#common_weight_name_mapping)
    pub weight: Option<u16>,
    /// Italic or not, boolean
    pub italic: Option<bool>,
    /// Font stretch, same as css [font-stretch](https://developer.mozilla.org/en-US/docs/Web/CSS/font-stretch)
    pub stretch: Option<u16>,
    /// Font family string
    pub family: String,
    pub variations: Vec<(String, f32)>,
}

impl Eq for FontKey {}

impl Hash for FontKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.weight.hash(state);
        self.italic.hash(state);
        self.stretch.hash(state);
        self.family.hash(state);
        self.variations
            .iter()
            .map(|(s, v)| (s, OrderedFloat(*v)))
            .collect::<Vec<_>>()
            .hash(state);
    }
}

impl FontKey {
    pub fn new_with_family(family: String) -> Self {
        FontKey {
            weight: Some(400),
            italic: Some(false),
            stretch: Some(5),
            family,
            variations: vec![],
        }
    }
}

impl fmt::Display for FontKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FontKey({}, {:?}, {:?}, {:?})",
            self.family, self.weight, self.italic, self.stretch
        )
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct Name {
    pub id: u16,
    pub name: String,
    #[allow(unused)]
    pub language_id: u16,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct FvarInstance {
    #[allow(unused)]
    pub(super) sub_family: Name,
    pub(super) postscript: Name,
}

/// Returns whether a buffer is WOFF font data.
pub fn is_woff(buf: &[u8]) -> bool {
    buf.len() > 4 && buf[0] == 0x77 && buf[1] == 0x4F && buf[2] == 0x46 && buf[3] == 0x46
}

/// Returns whether a buffer is WOFF2 font data.
pub fn is_woff2(buf: &[u8]) -> bool {
    buf.len() > 4 && buf[0] == 0x77 && buf[1] == 0x4F && buf[2] == 0x46 && buf[3] == 0x32
}

/// Returns whether a buffer is TTF font data.
pub fn is_ttf(buf: &[u8]) -> bool {
    buf.len() > 4
        && buf[0] == 0x00
        && buf[1] == 0x01
        && buf[2] == 0x00
        && buf[3] == 0x00
        && buf[4] == 0x00
}

/// Returns whether a buffer is OTF font data.
pub fn is_otf(buf: &[u8]) -> bool {
    buf.len() > 4
        && buf[0] == 0x4F
        && buf[1] == 0x54
        && buf[2] == 0x54
        && buf[3] == 0x4F
        && buf[4] == 0x00
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct VariationData {
    pub key: FontKey,
    pub names: Vec<Name>,
    pub variation_names: Vec<FvarInstance>,
    pub style_names: Vec<Name>,
    pub index: u32,
}

impl VariationData {
    #[cfg(feature = "parse")]
    fn parse_buffer_with_index(buffer: &[u8], index: u32) -> Result<Vec<VariationData>, Error> {
        use ttf_parser::name_id;

        let face = Face::parse(buffer, index)?;
        let axes: Vec<VariationAxis> = face
            .tables()
            .fvar
            .map(|v| v.axes.into_iter())
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        // get fvar if any
        let mut instances: IndexMap<Vec<OrderedFloat<f32>>, Vec<FvarInstance>> = IndexMap::new();
        if let (Some(_), Some(name_table)) = (face.tables().fvar, face.tables().name) {
            // currently ttf-parser is missing `fvar`'s instance records, we parse them
            // directly from `RawFace`
            let data: &[u8] = face
                .raw_face()
                .table(ttf_parser::Tag::from_bytes(b"fvar"))
                .unwrap();
            let mut raw = &*data;
            let _version = raw.read_u32::<BigEndian>()?;
            let axis_offset = raw.read_u16::<BigEndian>()?;
            let _ = raw.read_u16::<BigEndian>()?;
            let axis_count = raw.read_u16::<BigEndian>()?;
            let axis_size = raw.read_u16::<BigEndian>()?;
            let instance_count = raw.read_u16::<BigEndian>()?;
            let instance_size = raw.read_u16::<BigEndian>()?;

            let data = &data[(axis_offset as usize + (axis_count as usize * axis_size as usize))..];
            for i in 0..instance_count {
                let mut raw = &data[(i as usize * instance_size as usize)..];
                let sub_family_name_id = raw.read_u16::<BigEndian>()?;
                let _ = raw.read_u16::<BigEndian>()?;
                let coords = (0..axis_count)
                    .map(|_| {
                        use ttf_parser::FromData;
                        let mut v = [0_u8; 4];
                        raw.read_exact(&mut v)
                            .map(|_| OrderedFloat(Fixed::parse(&v).unwrap().0))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let postscript_name_id = if raw.is_empty() {
                    None
                } else {
                    Some(raw.read_u16::<BigEndian>()?)
                };
                let sub_family = name_table
                    .names
                    .into_iter()
                    .find(|name| name.name_id == sub_family_name_id)
                    .and_then(|name| {
                        Some(Name {
                            id: name.name_id,
                            name: name.to_string().or_else(|| {
                                // try to force unicode encoding
                                Some(std::str::from_utf8(name.name).ok()?.to_string())
                            })?,
                            language_id: name.language_id,
                        })
                    });
                let postscript = name_table
                    .names
                    .into_iter()
                    .find(|name| Some(name.name_id) == postscript_name_id)
                    .and_then(|name| {
                        Some(Name {
                            id: name.name_id,
                            name: name.to_string().or_else(|| {
                                // try to force unicode encoding
                                Some(std::str::from_utf8(name.name).ok()?.to_string())
                            })?,
                            language_id: name.language_id,
                        })
                    });
                if let (Some(sub_family), Some(postscript)) = (sub_family, postscript) {
                    instances.entry(coords).or_default().push(FvarInstance {
                        sub_family,
                        postscript,
                    })
                }
            }
        }
        let instances = instances
            .into_iter()
            .map(|(coords, names)| {
                return (
                    coords.into_iter().map(|v| Fixed(v.0)).collect::<Vec<_>>(),
                    names,
                );
            })
            .collect::<Vec<_>>();
        let mut style_names = vec![];
        let names = face
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
                        id,
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
                        id,
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
        let mut results = vec![];
        let key = FontKey {
            weight: Some(face.weight().to_number()),
            italic: Some(face.is_italic()),
            stretch: Some(face.width().to_number()),
            family: ascii_name.clone().unwrap_or_else(|| names[0].name.clone()),
            variations: vec![],
        };
        for (coords, variation_names) in instances {
            let mut key = key.clone();
            let width_axis_index = axes
                .iter()
                .position(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wdth"));
            let weight_axis_index = axes
                .iter()
                .position(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wght"));
            if let Some(value) = width_axis_index.and_then(|i| coords.get(i)) {
                // mapping wdth to usWidthClass, ref: https://learn.microsoft.com/en-us/typography/opentype/spec/dvaraxistag_wdth
                key.stretch = Some(((value.0 / 100.0) * 5.0).round().min(1.0).max(9.0) as u16);
            }
            if let Some(value) = weight_axis_index.and_then(|i| coords.get(i)) {
                key.weight = Some(value.0 as u16);
            }
            for (coord, axis) in coords.iter().zip(axes.iter()) {
                key.variations
                    .push((String::from_utf8(axis.tag.to_bytes().to_vec())?, coord.0));
            }
            results.push(VariationData {
                key,
                names: names.clone(),
                style_names: style_names.clone(),
                variation_names,
                index,
            });
        }
        if results.is_empty() {
            // this is not a variable font, add normal font data
            results.push(VariationData {
                names,
                key,
                variation_names: vec![],
                style_names,
                index,
            })
        }
        Ok(results)
    }

    fn is_variable(&self) -> bool {
        !self.key.variations.is_empty()
    }

    fn fulfils(&self, query: &Filter) -> bool {
        match *query {
            Filter::Family(name) => {
                if self.key.family == name {
                    return true;
                }
                if self.names.iter().any(|n| n.name == name) {
                    return true;
                }
                if self.is_variable() {
                    use inflections::Inflect;
                    return self.variation_names.iter().any(|n| {
                        n.postscript.name == name
                            || n.postscript
                                .name
                                .replace(&n.sub_family.name, &n.sub_family.name.to_pascal_case())
                                == name
                    });
                }

                false
            }
            Filter::Italic(i) => self.key.italic.unwrap_or_default() == i,
            Filter::Stretch(s) => self.key.stretch.unwrap_or(5) == s,
            Filter::Weight(w) => w == 0 || self.key.weight.unwrap_or(400) == w,
            Filter::Variations(v) => v.iter().all(|(s, v)| {
                self.key
                    .variations
                    .iter()
                    .any(|(ss, sv)| ss == s && v == sv)
            }),
        }
    }
}

pub(crate) struct Font {
    path: Option<PathBuf>,
    buffer: ArcSwap<Vec<u8>>,
    /// [Font variation](https://learn.microsoft.com/en-us/typography/opentype/spec/fvar) and font collection data
    variants: Vec<VariationData>,
    hit_counter: Arc<AtomicU32>,
    pub(crate) hit_index: AtomicU32,
}

impl Font {
    pub fn fulfils(&self, query: &Filter) -> bool {
        self.variants.iter().any(|v| v.fulfils(query))
    }

    pub fn first_key(&self) -> FontKey {
        self.variants[0].key.clone()
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    #[cfg(feature = "parse")]
    pub(super) fn from_buffer(
        mut buffer: Vec<u8>,
        hit_counter: Arc<AtomicU32>,
    ) -> Result<Self, Error> {
        let mut variants = vec![0];
        if is_otf(&buffer) {
            variants = (0..ttf_parser::fonts_in_collection(&buffer).unwrap_or(1)).collect();
        }
        #[cfg(feature = "woff2-patched")]
        if is_woff2(&buffer) {
            buffer = woff2_patched::convert_woff2_to_ttf(&mut buffer.as_slice())?;
        }
        #[cfg(feature = "parse")]
        if is_woff(&buffer) {
            use std::io::Cursor;

            let reader = Cursor::new(buffer);
            let mut otf_buf = Cursor::new(Vec::new());
            crate::conv::woff::convert_woff_to_otf(reader, &mut otf_buf)?;
            buffer = otf_buf.into_inner();
        }
        if buffer.is_empty() {
            return Err(Error::UnsupportedMIME("unknown"));
        }
        let variants = variants
            .into_iter()
            .map(|v| VariationData::parse_buffer_with_index(&buffer, v))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        Ok(Font {
            path: None,
            buffer: ArcSwap::new(Arc::new(buffer)),
            variants,
            hit_index: AtomicU32::default(),
            hit_counter,
        })
    }

    pub fn unload(&self) {
        self.buffer.swap(Arc::default());
    }

    pub fn load(&self) -> Result<(), Error> {
        if !self.buffer.load().is_empty() {
            return Ok(());
        }
        let hit_index = self.hit_counter.fetch_add(1, Ordering::SeqCst);
        self.hit_index.store(hit_index, Ordering::SeqCst);
        #[cfg(feature = "parse")]
        if let Some(path) = self.path.as_ref() {
            let mut buffer = Vec::new();
            let mut file = std::fs::File::open(path)?;
            file.read_to_end(&mut buffer).unwrap();

            #[cfg(feature = "woff2-patched")]
            if is_woff2(&buffer) {
                buffer = woff2_patched::convert_woff2_to_ttf(&mut buffer.as_slice())?;
            }
            #[cfg(feature = "parse")]
            if is_woff(&buffer) {
                use std::io::Cursor;

                let reader = Cursor::new(buffer);
                let mut otf_buf = Cursor::new(Vec::new());
                crate::conv::woff::convert_woff_to_otf(reader, &mut otf_buf)?;
                buffer = otf_buf.into_inner();
            }
            self.buffer.swap(Arc::new(buffer));
        }
        Ok(())
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    pub fn face(&self, key: &FontKey) -> Result<StaticFace, Error> {
        self.load()?;
        let buffer = self.buffer.load().to_vec();
        let filters = Filter::from_key(key);
        let mut queue = self.variants.iter().collect::<Vec<_>>();
        for filter in filters {
            let mut q = queue.clone();
            q.retain(|v| v.fulfils(&filter));
            if q.len() == 1 {
                queue = q;
                break;
            } else if q.is_empty() {
                break;
            } else {
                queue = q;
            }
        }
        let variant = queue[0];
        let mut face = StaticFaceTryBuilder {
            key: variant.key.clone(),
            path: self.path.clone().unwrap_or_default(),
            buffer,
            face_builder: |buf| Face::parse(buf, variant.index),
        }
        .try_build()
        .unwrap();
        face.with_face_mut(|face| {
            for (coord, axis) in &variant.key.variations {
                face.set_variation(Tag::from_bytes_lossy(coord.as_bytes()), *axis);
            }
        });
        Ok(face)
    }

    pub fn variants(&self) -> &[VariationData] {
        &self.variants
    }

    pub(super) fn new(
        path: Option<PathBuf>,
        variants: Vec<VariationData>,
        hit_counter: Arc<AtomicU32>,
    ) -> Self {
        Font {
            path,
            variants,
            buffer: ArcSwap::default(),
            hit_index: AtomicU32::default(),
            hit_counter,
        }
    }

    pub(super) fn buffer_size(&self) -> usize {
        self.buffer.load().len()
    }
}

#[self_referencing]
pub struct StaticFace {
    key: FontKey,
    pub(crate) path: PathBuf,
    pub(crate) buffer: Vec<u8>,
    #[borrows(buffer)]
    #[covariant]
    pub(crate) face: Face<'this>,
}

impl StaticFace {
    pub fn has_glyph(&self, c: char) -> bool {
        let f = self.borrow_face();
        f.glyph_index(c).is_some()
    }

    pub fn ascender(&self) -> i16 {
        let f = self.borrow_face();
        f.ascender()
    }

    pub fn descender(&self) -> i16 {
        let f = self.borrow_face();
        f.descender()
    }

    pub fn units_per_em(&self) -> u16 {
        let f = self.borrow_face();
        f.units_per_em()
    }

    pub fn strikeout_metrics(&self) -> Option<LineMetrics> {
        let f = self.borrow_face();
        f.strikeout_metrics()
    }

    pub fn underline_metrics(&self) -> Option<LineMetrics> {
        let f = self.borrow_face();
        f.underline_metrics()
    }

    pub fn key(&self) -> FontKey {
        self.borrow_key().clone()
    }
}
