use arc_swap::ArcSwap;
use std::collections::HashSet;
#[cfg(feature = "parse")]
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
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

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: talc::TalckWasm = unsafe { talc::TalckWasm::new_global() };

struct Config {
    pub lru_limit: u32,
    pub cache_path: Option<String>,
}

pub struct FontKit {
    fonts: dashmap::DashMap<font::FontKey, Font>,
    fallback_font_key: Option<Box<dyn Fn(font::FontKey) -> font::FontKey + Send + Sync>>,
    emoji_font_key: Option<font::FontKey>,
    pub(crate) config: ArcSwap<Config>,
    hit_counter: Arc<AtomicU32>,
}

impl FontKit {
    /// Create a font registry
    pub fn new() -> Self {
        FontKit {
            fonts: dashmap::DashMap::new(),
            fallback_font_key: None,
            emoji_font_key: None,
            config: ArcSwap::new(Arc::new(Config {
                lru_limit: 0,
                cache_path: None,
            })),
            hit_counter: Arc::default(),
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

    pub fn buffer_size(&self) -> usize {
        self.fonts
            .iter()
            .map(|font| font.buffer_size())
            .sum::<usize>()
    }

    pub fn check_lru(&self) {
        let limit = self.config.load().lru_limit as usize * 1024;
        if limit == 0 {
            return;
        }
        let mut current_size = self.buffer_size();
        let mut loaded_fonts = self.fonts.iter().filter(|f| f.buffer_size() > 0).count();
        while current_size > limit && loaded_fonts > 1 {
            let font = self
                .fonts
                .iter()
                .filter(|f| f.buffer_size() > 0)
                .min_by(|a, b| {
                    b.hit_index
                        .load(Ordering::SeqCst)
                        .cmp(&a.hit_index.load(Ordering::SeqCst))
                });

            let hit_index = font
                .as_ref()
                .map(|f| f.hit_index.load(Ordering::SeqCst))
                .unwrap_or(0);
            if let Some(f) = font {
                f.unload();
            }
            if current_size == self.buffer_size() {
                break;
            }
            current_size = self.buffer_size();
            self.hit_counter.fetch_sub(hit_index, Ordering::SeqCst);
            for f in self.fonts.iter() {
                f.hit_index.fetch_sub(hit_index, Ordering::SeqCst);
            }
            loaded_fonts = self.fonts.iter().filter(|f| f.buffer_size() > 0).count();
        }
    }

    /// Add fonts from a buffer. This will load the fonts and store the buffer
    /// in FontKit. Type information is inferred from the magic number using
    /// `infer` crate.
    #[cfg(feature = "parse")]
    pub fn add_font_from_buffer(&self, buffer: Vec<u8>) -> Result<(), Error> {
        use std::fs::File;
        use std::io::Write;
        use std::path::PathBuf;
        use std::str::FromStr;

        let mut font = Font::from_buffer(buffer.clone(), self.hit_counter.clone())?;
        let key = font.first_key();
        if let Some(v) = self.fonts.get(&key) {
            if let Some(path) = v.path().cloned() {
                font.set_path(path);
            }
        }
        let cache_path = self.config.load().cache_path.clone();
        if let Some(mut path) = cache_path.and_then(|p| PathBuf::from_str(&p).ok()) {
            if font.path().is_none() {
                path.push(format!(
                    "{}_{}_{}_{}.ttf",
                    key.family.replace(['.', ' '], "_"),
                    key.italic.unwrap_or_default(),
                    key.stretch.unwrap_or(5),
                    key.weight.unwrap_or(400)
                ));
                let mut f = File::create(&path)?;
                f.write_all(&buffer)?;
                font.set_path(path);
            }
        }
        self.fonts.insert(key, font);
        self.check_lru();
        Ok(())
    }

    /// Recursively scan a local path for fonts, this method will not store the
    /// font buffer to reduce memory consumption
    #[cfg(feature = "parse")]
    pub fn search_fonts_from_path(&self, path: impl AsRef<Path>) -> Result<(), Error> {
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
            None => return Ok(()),
        };
        match ext {
            "ttf" | "otf" | "ttc" | "woff2" | "woff" => {
                let mut file = std::fs::File::open(&path).unwrap();
                buffer.clear();
                file.read_to_end(&mut buffer).unwrap();
                let mut font = match Font::from_buffer(buffer, self.hit_counter.clone()) {
                    Ok(f) => f,
                    Err(e) => {
                        log::warn!("Failed loading font {:?}: {:?}", path, e);
                        return Ok(());
                    }
                };
                font.set_path(path.to_path_buf());
                font.unload();
                self.fonts.insert(font.first_key(), font);
                self.check_lru();
            }
            _ => {}
        }
        Ok(())
    }

    pub fn exact_match(&self, key: &font::FontKey) -> Option<StaticFace> {
        let face = self.query(key)?;
        let mut patched_key = key.clone();
        if patched_key.weight.is_none() {
            patched_key.weight = Some(400);
        }
        if patched_key.stretch.is_none() {
            patched_key.stretch = Some(5);
        }
        if patched_key.italic.is_none() {
            patched_key.italic = Some(false);
        }
        if face.key() == patched_key {
            return Some(face);
        } else {
            return None;
        }
    }

    pub fn query(&self, key: &font::FontKey) -> Option<StaticFace> {
        let result = self.fonts.get(&self.query_font(key)?)?.face(key).ok();
        self.check_lru();
        result
    }

    pub(crate) fn query_font(&self, key: &font::FontKey) -> Option<font::FontKey> {
        let mut search_results = self
            .fonts
            .iter()
            .map(|item| item.key().clone())
            .collect::<HashSet<_>>();
        let filters = Filter::from_key(key);
        for filter in filters {
            let mut s = search_results.clone();
            let is_family = if let Filter::Family(_) = filter {
                true
            } else {
                false
            };
            s.retain(|key| {
                let font = self.fonts.get(key).unwrap();
                font.fulfils(&filter)
            });
            match s.len() {
                1 => return s.iter().next().cloned(),
                0 if is_family => return None,
                0 => {}
                _ => search_results = s,
            }
        }
        None
    }

    pub fn keys(&self) -> Vec<FontKey> {
        self.fonts
            .iter()
            .flat_map(|i| {
                i.value()
                    .variants()
                    .iter()
                    .map(|i| i.key.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

enum Filter<'a> {
    Family(&'a str),
    Italic(bool),
    Weight(u16),
    Stretch(u16),
    Variations(&'a Vec<(String, f32)>),
}

impl<'a> Filter<'a> {
    pub fn from_key(key: &'a FontKey) -> Vec<Filter<'a>> {
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

        filters.push(Filter::Variations(&key.variations));

        // Fallback weight logic
        filters.push(Filter::Weight(0));
        filters
    }
}
