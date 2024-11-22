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
    key.family = "AlimamaFangYuanTiVF-BoldRound".into();
    assert!(fontkit.query(&key).is_some());
    key.family = "AlimamaFangYuanTiVF-Thin".into();
    assert!(fontkit.query(&key).is_none());
    key.weight = Some(200);
    key.italic = Some(false);
    key.stretch = Some(5);
    assert!(fontkit.query(&key).is_some());
    let bitmap_2 = fontkit
        .query(&key)
        .and_then(|font| font.bitmap('G', 10.0, 0.0))
        .map(|g| g.bitmap().iter().filter(|p| **p > 0).count());
    assert!(bitmap_2.is_some());
    assert!(bitmap_1 > bitmap_2);
    Ok(())
}

#[test]
pub fn test_search_font() -> Result<(), Error> {
    let fontkit = FontKit::new();
    fontkit.search_fonts_from_path("examples/AlimamaFangYuanTiVF.ttf")?;
    assert_eq!(fontkit.len(), 18);
    Ok(())
}

#[test]
pub fn test_text_wrap() -> Result<(), Error> {
    let fontkit = FontKit::new();
    fontkit.search_fonts_from_path("examples/AlimamaFangYuanTiVF.ttf")?;
    let mut key = FontKey::default();
    key.family = "AlimamaFangYuanTiVF-Medium-Round".into();
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
    assert_eq!(area.width(), 553.608);
    Ok(())
}

#[test]
pub fn test_complex_text_wrap() -> Result<(), Error> {
    let fontkit = FontKit::new();
    fontkit.search_fonts_from_path("examples/AlimamaFangYuanTiVF.ttf")?;
    let mut key = FontKey::default();
    key.family = "AlimamaFangYuanTiVF-Medium-Round".into();
    let mut area = Area::<(), TextMetrics>::new();
    let metrics = fontkit.measure(&key, "商家").unwrap();
    let mut span = Span::default();
    span.font_key = key.clone();
    span.size = 32.0;
    span.metrics = metrics;
    let metrics = fontkit.measure(&key, "热卖12345678").unwrap();
    let mut span_1 = Span::default();
    span_1.font_key = key.clone();
    span_1.size = 32.0;
    span_1.metrics = metrics;
    area.lines.push(Line {
        spans: vec![span],
        hard_break: true,
    });
    area.lines.push(Line {
        spans: vec![span_1],
        hard_break: true,
    });
    area.unwrap_text();
    area.wrap_text(64.4)?;
    assert_eq!(area.value_string(), "商家\n热卖\n123\n456\n78");
    Ok(())
}

#[test]
pub fn test_lru_cache() -> Result<(), Error> {
    let fontkit = FontKit::new();
    fontkit.set_lru_limit(1);
    fontkit.search_fonts_from_path("examples/AlimamaFangYuanTiVF.ttf")?;
    assert_eq!(fontkit.buffer_size(), 0);
    let key = fontkit.keys().pop().unwrap();
    assert!(fontkit.query(&key).is_some());
    assert_eq!(fontkit.buffer_size(), 7412388);
    fontkit.search_fonts_from_path("examples/OpenSans-Italic.ttf")?;
    let key2 = FontKey::new_with_family("Open Sans".to_string());
    assert!(fontkit.query(&key2).is_some());
    assert_eq!(fontkit.buffer_size(), 212896);
    fontkit.query(&key);
    assert_eq!(fontkit.buffer_size(), 7412388);
    Ok(())
}
