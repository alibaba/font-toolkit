const { Buffer } = require('buffer')
const { readFileSync, writeFileSync } = require('fs')
const { join } = require('path')

const { FontKit } = require('../wasm-node')

const FONT_PATH = join(__dirname, 'OpenSans-Italic.ttf')

const data = readFileSync(FONT_PATH)
const fontkit = new FontKit()
const key = fontkit.add_font_from_buffer(data)
const font = fontkit.query(key)
const svgPath = font.glyph_path('A').to_string()
console.info('em-box = ', font.units_per_em)

const svg = `<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <path d="${svgPath}" fill="blue" />
</svg>
`
writeFileSync(join(__dirname, './out.svg'), Buffer.from(svg))
console.info(svg)
