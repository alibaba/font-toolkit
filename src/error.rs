use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Unrecognized buffer")]
    UnrecognizedBuffer,
    #[error("MIME {0} not supported as a font")]
    UnsupportedMIME(&'static str),
    #[error(transparent)]
    Parser(#[from] ttf_parser::FaceParsingError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[cfg(not(all(target_os = "unknown", target_arch = "wasm32")))]
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),
    #[error("Glyph {c} not found in font")]
    GlyphNotFound { c: char },
    #[cfg(feature = "woff2")]
    #[error(transparent)]
    Woff2(#[from] woff2::decode::DecodeError),
}

#[cfg(node)]
impl From<Error> for napi::Error {
    fn from(e: Error) -> Self {
        napi::Error::from_reason(format!("{}", e))
    }
}

#[cfg(wasm)]
impl From<Error> for wasm_bindgen::JsValue {
    fn from(e: Error) -> Self {
        js_sys::TypeError::new(&format!("{}", e)).into()
    }
}
