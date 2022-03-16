const { readFileSync } = require('fs')
const path = require('path')

const { FontKit } = require('../wasm-node')

const FONT_PATH = path.resolve(__dirname, 'OpenSans-Italic.ttf')

const data = readFileSync(FONT_PATH)
const fontkit = new FontKit()
const key = fontkit.add_font_from_buffer(data)
const font = fontkit.query(key)

console.info('em-box = ', font.units_per_em)
console.info(font.glyph_path('A').to_string())
