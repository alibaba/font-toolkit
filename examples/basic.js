const { Buffer } = require('buffer')
const { readFileSync, writeFileSync } = require('fs')
const { join } = require('path')

const { convertSVGTextToPath, GlobalFonts } = require('@napi-rs/canvas')
const { render } = require('@resvg/resvg-js')

const { FontKit } = require('../wasm-node')

const FONT_PATH = join(__dirname, 'OpenSans-Italic.ttf')
const glyphName = 'B'

const data = readFileSync(FONT_PATH)
const fontkit = new FontKit()
const key = fontkit.add_font_from_buffer(data)
const font = fontkit.query(key)
const svgPath = font.glyph_path(glyphName).to_string()
const width = font.units_per_em // embox
const ascender = font.ascender
const descender = font.descender
const height = ascender - descender

console.info('em-box = ', width)
console.info('ascender = ', ascender)
console.info('descender = ', descender)

const svg = `<svg width="500" viewBox="0 0 ${width} ${height}" xmlns="http://www.w3.org/2000/svg">
  <path d="${svgPath}" fill="blue" />
</svg>
`
writeFileSync(join(__dirname, './out.svg'), Buffer.from(svg))
console.info(svg)

const svgText = `<svg width="500" height="500" xmlns="http://www.w3.org/2000/svg">
  <text fill="green" font-family="Open Sans, Open Sans Italic" font-size="900">${glyphName}</text>
</svg>
`

GlobalFonts.registerFromPath(FONT_PATH)
const result = convertSVGTextToPath(svgText)
console.info('skr-canvas \n', result.toString('utf8'))

writeFileSync(join(__dirname, './skr-canvas.svg'), result)

const pngData = render(result.toString('utf8'))

writeFileSync(join(__dirname, './foo.png'), pngData)
