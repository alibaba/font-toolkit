use crate::*;
use pathfinder_content::outline::Outline;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::Vector2F;

/// Interface for querying data in a Font
#[wasm_bindgen(js_name = "Font")]
pub struct FontWasm {
    pub(super) ptr: *const Font,
}

#[wasm_bindgen(js_class = "Font")]
impl FontWasm {
    fn font(&self) -> &Font {
        unsafe { &*self.ptr }
    }

    /// Check if font contains a certain char
    pub fn has_glyph(&self, c: char) -> bool {
        self.font().has_glyph(c)
    }

    /// Output the glyph's path in SVG path `d` style
    #[cfg(feature = "ras")]
    pub fn glyph_path(&self, c: char) -> Option<GlyphPath> {
        let font = self.font();
        let (_, mut outline) = font.outline(c)?;
        let height = font.ascender() as f32 - font.descender() as f32;
        outline.transform(&Transform2F::from_scale(Vector2F::new(1.0, -1.0)));
        outline.transform(&Transform2F::from_translation(Vector2F::new(0.0, height)));
        Some(GlyphPath { outline })
    }

    #[wasm_bindgen(getter)]
    pub fn key(&self) -> FontKey {
        self.font().key()
    }

    #[wasm_bindgen(getter)]
    pub fn ascender(&self) -> i16 {
        self.font().ascender()
    }

    #[wasm_bindgen(getter)]
    pub fn descender(&self) -> i16 {
        self.font().descender()
    }

    #[wasm_bindgen(getter)]
    pub fn units_per_em(&self) -> u16 {
        self.font().units_per_em()
    }
}

#[wasm_bindgen]
pub struct GlyphPath {
    outline: Outline,
}

#[wasm_bindgen]
impl GlyphPath {
    /// Scale the path
    pub fn scale(&mut self, scale: f32) {
        self.outline.transform(&Transform2F::from_scale(scale));
    }

    /// Translate the path
    pub fn translate(&mut self, x: f32, y: f32) {
        self.outline
            .transform(&Transform2F::from_translation(Vector2F::new(x, y)));
    }

    /// Output the path following SVG <path> `d` style
    pub fn to_string(&mut self) -> String {
        format!("{:?}", self.outline)
    }
}
