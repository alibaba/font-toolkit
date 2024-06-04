use dashmap::mapref::one::Ref;
use std::collections::HashSet;
#[cfg(feature = "parse")]
use std::path::Path;
pub use ttf_parser::LineMetrics;

#[cfg(all(target_arch = "wasm32", feature = "wit"))]
mod bindings;
mod conv;
mod error;
mod font;
#[cfg(feature = "metrics")]
mod metrics;
#[cfg(feature = "ras")]
mod ras;
#[cfg(all(target_arch = "wasm32", feature = "wit"))]
mod wit;

pub use error::Error;
pub use font::*;
#[cfg(feature = "metrics")]
pub use metrics::*;
#[cfg(feature = "ras")]
pub use ras::*;
pub use tiny_skia_path::{self, PathSegment};

#[cfg(all(target_arch = "wasm32", feature = "wit"))]
pub use bindings::exports::alibaba::fontkit::fontkit_interface::TextMetrics;

pub struct FontKit {
    fonts: dashmap::DashMap<font::FontKey, Font>,
    fallback_font_key: Option<Box<dyn Fn(font::FontKey) -> font::FontKey + Send + Sync>>,
    emoji_font_key: Option<font::FontKey>,
}

impl FontKit {
    /// Create a font registry
    pub fn new() -> Self {
        FontKit {
            fonts: dashmap::DashMap::new(),
            fallback_font_key: None,
            emoji_font_key: None,
        }
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    /// Setup a font as fallback. When measure fails, FontKit will use this
    /// fallback to measure, if possible
    pub fn set_fallback(
        &mut self,
        font_key: Option<impl Fn(font::FontKey) -> font::FontKey + Send + Sync + 'static>,
    ) {
        self.fallback_font_key = font_key.map(|f| Box::new(f) as _);
    }

    pub fn set_emoji(&mut self, font_key: font::FontKey) {
        self.emoji_font_key = Some(font_key)
    }

    #[cfg(feature = "metrics")]
    pub fn measure(&self, font_key: &font::FontKey, text: &str) -> Option<metrics::TextMetrics> {
        match self
            .query(&font_key)
            .and_then(|font| font.measure(text).ok())
        {
            Some(metrics) => {
                let has_missing = metrics.has_missing();
                let fallback_fontkey = self.fallback_font_key.as_ref().and_then(|key| {
                    let key = (key)(font_key.clone());
                    let font = self.query(&key)?;
                    Some(font.key().clone())
                });
                if has_missing {
                    if let Some(font) = fallback_fontkey.as_ref().and_then(|key| self.query(key)) {
                        if let Ok(new_metrics) = font.measure(text) {
                            metrics.replace(new_metrics, true);
                        }
                    }
                }
                // if after fallback font, still has missing, then detect emoji font
                let has_missing = metrics.has_missing();
                if has_missing {
                    if let Some(font) = self.emoji_font_key.as_ref().and_then(|key| self.query(key))
                    {
                        if let Ok(new_metrics) = font.measure(text) {
                            metrics.replace(new_metrics, true);
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

    pub fn remove(&self, key: font::FontKey) {
        self.fonts.remove(&key);
    }

    /// Add fonts from a buffer. This will load the fonts and store the buffer
    /// in FontKit. Type information is inferred from the magic number using
    /// `infer` crate.
    ///
    /// If the given buffer is a font collection (ttc), multiple keys will be
    /// returned.
    #[cfg(feature = "parse")]
    pub fn add_font_from_buffer(&self, buffer: Vec<u8>) -> Result<Vec<font::FontKey>, Error> {
        Ok(Font::from_buffer(&buffer)?
            .into_iter()
            .map(|font| {
                let key = font.key().clone();
                self.fonts.insert(key.clone(), font);
                key
            })
            .collect::<Vec<_>>())
    }

    pub fn font_keys(&self) -> impl Iterator<Item = font::FontKey> + '_ {
        return self.fonts.iter().map(|i| {
            log::debug!("{:?}", i.names);
            i.key().clone()
        });
    }

    /// Recursively scan a local path for fonts, this method will not store the
    /// font buffer to reduce memory consumption
    #[cfg(feature = "parse")]
    pub fn search_fonts_from_path(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        if let Some(fonts) = load_font_from_path(&path) {
            for font in fonts {
                self.fonts.insert(font.key(), font);
            }
        }
        // }
        Ok(())
    }

    pub fn exact_match(&self, key: &font::FontKey) -> Option<Ref<'_, FontKey, Font>> {
        return self.fonts.get(key);
    }

    pub fn query(&self, key: &font::FontKey) -> Option<Ref<'_, FontKey, Font>> {
        if let Some(result) = self.exact_match(key) {
            return Some(result as _);
        }
        let mut filters = vec![Filter::Family(&key.family)];
        if let Some(italic) = key.italic {
            filters.push(Filter::Italic(italic));
        }
        if let Some(weight) = key.weight {
            filters.push(Filter::Weight(weight));
        }
        if let Some(stretch) = key.stretch {
            filters.push(Filter::Stretch(stretch));
        }

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
                        let font = self.fonts.get(key).unwrap();
                        font.has_name(f)
                    })
                }
                Filter::Italic(i) => s.retain(|key| key.italic == Some(i)),
                Filter::Weight(w) => s.retain(|key| w == 0 || key.weight == Some(w)),
                Filter::Stretch(st) => s.retain(|key| key.stretch == Some(st)),
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

#[cfg(feature = "parse")]
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
    Weight(u16),
    Stretch(u16),
}
