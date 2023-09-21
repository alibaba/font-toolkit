use fontkit::{Error, FontKey};
use std::fs;
use std::io::Read;

#[test]
pub fn test_font_loading() -> Result<(), Error> {
    let mut buf = vec![];
    let mut f = fs::File::open("examples/OpenSans-Italic.ttf")?;
    f.read_to_end(&mut buf)?;
    let fontkit = fontkit::FontKit::new();
    let _ = fontkit.add_font_from_buffer(buf)?;
    Ok(())
}

#[test]
pub fn test_variable_font_loading() -> Result<(), Error> {
    let mut buf = vec![];
    let mut f = fs::File::open("examples/AlimamaFangYuanTiVF.ttf")?;
    f.read_to_end(&mut buf)?;
    let fontkit = fontkit::FontKit::new();
    let _ = fontkit.add_font_from_buffer(buf)?;
    let mut key = FontKey::default();
    key.family = "AlimamaFangYuanTiVF-Medium-Round".into();
    let bitmap_1 = fontkit
        .query(&key)
        .and_then(|font| font.bitmap('G', 10.0, 0.0))
        .map(|g| g.bitmap.iter().filter(|p| **p > 0).count());
    key.family = "AlimamaFangYuanTiVF-Thin-Round".into();
    let bitmap_2 = fontkit
        .query(&key)
        .and_then(|font| font.bitmap('G', 10.0, 0.0))
        .map(|g| g.bitmap.iter().filter(|p| **p > 0).count());
    assert!(bitmap_1 > bitmap_2);
    Ok(())
}
