use fontkit::Error;
use std::fs;
use std::io::Read;

#[test]
pub fn test_font_loading() -> Result<(), Error> {
    let mut buf = vec![];
    let mut f = fs::File::open("examples/OpenSans-Italic.ttf")?;
    f.read_to_end(&mut buf)?;
    let mut fontkit = fontkit::FontKit::new();
    let _ = fontkit.add_font_from_buffer(buf)?;
    Ok(())
}
