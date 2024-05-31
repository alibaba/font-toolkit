use thiserror::Error;

use crate::PositionedChar;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Unrecognized buffer")]
    UnrecognizedBuffer,
    #[error("MIME {0} not supported as a font")]
    UnsupportedMIME(&'static str),
    #[error("Font doesn't have a proper name")]
    EmptyName,
    #[error(transparent)]
    Parser(#[from] ttf_parser::FaceParsingError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Glyph {c} not found in font")]
    GlyphNotFound { c: char },
    #[cfg(feature = "woff2-patched")]
    #[error(transparent)]
    Woff2(#[from] woff2_patched::decode::DecodeError),
    #[error("Metrics mismatch: values {value:?} metrics {metrics:?}")]
    MetricsMismatch {
        value: Vec<char>,
        metrics: Vec<PositionedChar>,
    },
    #[cfg(feature = "png")]
    #[error("Color space not support when decoding rastered image, {0:?}")]
    PngNotSupported(png::ColorType),
    #[cfg(feature = "png")]
    #[error(transparent)]
    PngDocde(#[from] png::DecodingError),
}
