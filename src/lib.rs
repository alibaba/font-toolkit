use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
pub use ttf_parser::LineMetrics;

#[cfg(target_arch = "wasm32")]
mod bindings;
mod conv;
mod error;
mod font;
#[cfg(feature = "metrics")]
mod metrics;
#[cfg(feature = "ras")]
mod ras;
#[cfg(target_arch = "wasm32")]
mod wit;

pub use error::Error;
pub use font::*;
#[cfg(feature = "metrics")]
pub use metrics::*;
#[cfg(feature = "ras")]
pub use ras::*;
pub use tiny_skia_path::{self, PathSegment};

pub struct FontKit {
    fonts: dashmap::DashMap<FontKey, Font>,
    fallback_font_key: Option<Box<dyn Fn(FontKey) -> FontKey + Send + Sync>>,
}

impl FontKit {
    /// Create a font registry
    pub fn new() -> Self {
        FontKit {
            fonts: dashmap::DashMap::new(),
            fallback_font_key: None,
        }
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    /// Setup a font as fallback. When measure fails, FontKit will use this
    /// fallback to measure, if possible
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

    pub fn remove(&self, key: FontKey) {
        self.fonts.remove(&key);
    }

    /// Add fonts from a buffer. This will load the fonts and store the buffer
    /// in FontKit. Type information is inferred from the magic number using
    /// `infer` crate.
    ///
    /// If the given buffer is a font collection (ttc), multiple keys will be
    /// returned.
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
        return self.fonts.iter().map(|i| {
            log::debug!("{:?}", i.names);
            i.key().clone()
        });
    }

    /// Recursively scan a local path for fonts, this method will not store the
    /// font buffer to reduce memory consumption
    pub fn search_fonts_from_path(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        if let Some(fonts) = load_font_from_path(&path) {
            for font in fonts {
                self.fonts.insert(font.key(), font);
            }
        }
        // }
        Ok(())
    }

    pub fn exact_match(&self, key: &FontKey) -> Option<impl Deref<Target = Font> + '_> {
        return self.fonts.iter().find(|font| *font.key() == *key);
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
            .map(|item| item.key().clone())
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

fn load_font_from_path(path: impl AsRef<std::path::Path>) -> Option<Vec<Font>> {
    use std::io::Read;

    // if path.as_ref().is_dir() {
    //     let mut result = vec![];
    //     if let Ok(data) = fs::read_dir(path) {
    //         for entry in data {
    //             if let Ok(entry) = entry {
    //
    // result.extend(load_font_from_path(&entry.path()).into_iter().flatten());
    //             }
    //         }
    //     }
    //     return Some(result);
    // }

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
