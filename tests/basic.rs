use fontkit::{Area, Error, FontKey, FontKit, Line, Span, TextMetrics};
use std::fs;
use std::io::Read;

#[test]
pub fn test_font_loading() -> Result<(), Error> {
    let mut buf = vec![];
    let mut f = fs::File::open("examples/OpenSans-Italic.ttf")?;
    f.read_to_end(&mut buf)?;
    let fontkit = FontKit::new();
    let _ = fontkit.add_font_from_buffer(buf)?;
    Ok(())
}

#[test]
pub fn test_variable_font_loading() -> Result<(), Error> {
    let mut buf = vec![];
    let mut f = fs::File::open("examples/AlimamaFangYuanTiVF.ttf")?;
    f.read_to_end(&mut buf)?;
    let fontkit = FontKit::new();
    let _ = fontkit.add_font_from_buffer(buf)?;
    let mut key = FontKey::default();
    key.family = "AlimamaFangYuanTiVF-Medium-Round".into();
    let bitmap_1 = fontkit
        .query(&key)
        .and_then(|font| font.bitmap('G', 10.0, 0.0))
        .map(|g| g.bitmap().iter().filter(|p| **p > 0).count());
    key.family = "AlimamaFangYuanTiVF-Thin-Round".into();
    let bitmap_2 = fontkit
        .query(&key)
        .and_then(|font| font.bitmap('G', 10.0, 0.0))
        .map(|g| g.bitmap().iter().filter(|p| **p > 0).count());
    assert!(bitmap_1 > bitmap_2);
    Ok(())
}

#[test]
pub fn test_text_wrap() -> Result<(), Error> {
    let fontkit = FontKit::new();
    fontkit.search_fonts_from_path("examples/AlimamaFangYuanTiVF.ttf")?;
    let key = fontkit.font_keys().next().unwrap();
    let mut area = Area::<(), TextMetrics>::new();
    let metrics = fontkit
        .measure(&key, " 傲冬黑色真皮皮衣 穿着舒适显瘦")
        .unwrap();
    let mut span = Span::default();
    span.font_key = key.clone();
    span.size = 66.0;
    span.metrics = metrics;
    area.lines.push(Line {
        spans: vec![span],
        hard_break: true,
    });
    area.unwrap_text();
    area.wrap_text(576.0)?;
    assert_eq!(area.width(), 549.12);
    Ok(())
}
