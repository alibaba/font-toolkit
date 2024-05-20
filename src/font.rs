use arc_swap::ArcSwap;
#[cfg(feature = "parse")]
use byteorder::{BigEndian, ReadBytesExt};
#[cfg(feature = "parse")]
use ordered_float::OrderedFloat;
use ouroboros::self_referencing;
use serde::Serialize;
#[cfg(feature = "parse")]
use std::collections::HashMap;
use std::fmt;
#[cfg(feature = "parse")]
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
pub use ttf_parser::LineMetrics;
use ttf_parser::{Face, Fixed, VariationAxis, Width as ParserWidth};

use crate::Error;

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

#[derive(Clone, Hash, PartialEq, PartialOrd, Eq, Debug, Default)]
pub struct FontKey {
    /// Font weight, same as CSS [font-weight](https://developer.mozilla.org/en-US/docs/Web/CSS/font-weight#common_weight_name_mapping)
    // #[serde(default = "GenericDefault::<400>::value")]
    pub weight: Option<u16>,
    /// Italic or not, boolean
    pub italic: Option<bool>,
    /// Font stretch, same as css [font-stretch](https://developer.mozilla.org/en-US/docs/Web/CSS/font-stretch)
    // #[serde(default = "GenericDefault::<5>::value")]
    pub stretch: Option<u16>,
    /// Font family string
    pub family: String,
}

impl FontKey {
    pub fn new_with_family(family: String) -> Self {
        FontKey {
            weight: Some(400),
            italic: Some(false),
            stretch: Some(5),
            family,
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

#[derive(Clone, Debug, Serialize)]
pub(super) struct Name {
    pub id: u16,
    pub name: String,
    #[allow(unused)]
    pub language_id: u16,
}

#[derive(Clone, Debug)]
pub(super) struct FvarInstance {
    #[allow(unused)]
    pub(super) sub_family: Name,
    pub(super) postscript: Name,
}

#[derive(Clone)]
enum Variant {
    Index(u32),
    Instance {
        coords: Vec<Fixed>,
        names: Vec<FvarInstance>,
        axes: Vec<VariationAxis>,
    },
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

#[derive(Clone)]
pub struct Font {
    key: FontKey,
    /// [Font variation](https://learn.microsoft.com/en-us/typography/opentype/spec/fvar) data
    variant: Variant,
    pub(super) names: Vec<Name>,
    #[allow(unused)]
    pub(super) style_names: Vec<Name>,
    pub(super) face: Arc<ArcSwap<Option<StaticFace>>>,
    pub(super) path: Option<PathBuf>,
}

impl Font {
    pub fn key(&self) -> FontKey {
        self.key.clone()
    }

    #[cfg(feature = "parse")]
    pub(super) fn from_buffer(buffer: &[u8]) -> Result<Vec<Self>, Error> {
        let mut variants = vec![Variant::Index(0)];
        let result = if is_ttf(&buffer) {
            buffer.to_vec()
        } else if is_otf(&buffer) {
            variants = (0..ttf_parser::fonts_in_collection(&buffer).unwrap_or(1))
                .map(|i| Variant::Index(i))
                .collect();
            buffer.to_vec()
        } else {
            buffer.to_vec()
        };
        Ok(variants
            .into_iter()
            .map(|v| Font::from_buffer_with_variant(result.clone(), v))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    #[cfg(feature = "parse")]
    fn from_buffer_with_variant(mut buffer: Vec<u8>, variant: Variant) -> Result<Vec<Self>, Error> {
        #[cfg(feature = "woff2")]
        if is_woff2(&buffer) {
            buffer = woff2::convert_woff2_to_ttf(&mut buffer.as_slice())?;
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

        use ttf_parser::name_id;
        let index = match variant {
            Variant::Index(i) => i,
            _ => 0,
        };
        let mut face = StaticFaceTryBuilder {
            buffer: buffer.clone(),
            face_builder: |buf| Face::parse(buf, index),
        }
        .try_build()?;
        let axes: Vec<VariationAxis> = face
            .borrow_face()
            .tables()
            .fvar
            .map(|v| v.axes.into_iter())
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        if let Variant::Index(_) = variant {
            // get fvar if any
            let mut instances: HashMap<Vec<OrderedFloat<f32>>, Vec<FvarInstance>> = HashMap::new();
            if let (Some(_), Some(name_table)) = (
                face.borrow_face().tables().fvar,
                face.borrow_face().tables().name,
            ) {
                // currently ttf-parser is missing `fvar`'s instance records, we parse them
                // directly from `RawFace`
                let data: &[u8] = face
                    .borrow_face()
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

                let data =
                    &data[(axis_offset as usize + (axis_count as usize * axis_size as usize))..];
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
            if !instances.is_empty() {
                return Ok(instances
                    .into_iter()
                    .map(|(coords, names)| {
                        Font::from_buffer_with_variant(
                            buffer.clone(),
                            Variant::Instance {
                                coords: coords.into_iter().map(|v| Fixed(v.0)).collect(),
                                names,
                                axes: axes.clone(),
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect());
            }
        }
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
        let mut key = FontKey {
            weight: Some(face.borrow_face().weight().to_number()),
            italic: Some(face.borrow_face().is_italic()),
            stretch: Some(face.borrow_face().width().to_number()),
            family: ascii_name.unwrap_or_else(|| names[0].name.clone()),
        };
        if let Variant::Instance {
            coords,
            names,
            axes,
        } = &variant
        {
            let width_axis_index = axes
                .iter()
                .position(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wdth"));
            let weight_axis_index = axes
                .iter()
                .position(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wght"));
            if let Some(value) = width_axis_index.and_then(|i| coords.get(i)) {
                key.stretch = Some(value.0 as u16);
            }
            if let Some(value) = weight_axis_index.and_then(|i| coords.get(i)) {
                key.weight = Some(value.0 as u16);
            }
            key.family = names[0].postscript.name.clone();
            face.with_face_mut(|face| {
                for (coord, axis) in coords.iter().zip(axes.iter()) {
                    face.set_variation(axis.tag, coord.0);
                }
            });
        }
        let font = Font {
            names,
            key,
            variant,
            face: Arc::new(ArcSwap::new(Arc::new(Some(face)))),
            path: None,
            style_names,
        };
        Ok(vec![font])
    }

    pub fn unload(&self) {
        self.face.swap(Arc::new(None));
    }

    pub fn load(&self) -> Result<(), Error> {
        if self.face.load().is_some() {
            return Ok(());
        }
        #[cfg(feature = "parse")]
        if let Some(path) = self.path.as_ref() {
            let mut buffer = Vec::new();
            let mut file = std::fs::File::open(path)?;
            file.read_to_end(&mut buffer).unwrap();
            let mut fonts = Font::from_buffer_with_variant(buffer, self.variant.clone())?;
            fonts.truncate(1);
            if let Some(font) = fonts.pop() {
                self.face.store(font.face.load_full());
            }
        }
        Ok(())
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.load().unwrap();
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
    pub(crate) buffer: Vec<u8>,
    #[borrows(buffer)]
    #[covariant]
    pub(crate) face: Face<'this>,
}
