import { promises as fs } from 'fs'
import { join } from 'path'

import test from 'ava'

import { FontKit } from '../wasm-node'

// init wasm
let fontData = null

test.before(async () => {
  fontData = await fs.readFile(join(__dirname, '../examples/OpenSans-Italic.ttf'))
})

test('em box', (t) => {
  const fontkit = new FontKit()
  const key = fontkit.add_font_from_buffer(fontData)
  const font = fontkit.query(key)

  t.is(font.units_per_em, 2048)
})

test('glyph path to_string()', (t) => {
  const fontkit = new FontKit()
  const key = fontkit.add_font_from_buffer(fontData)
  const font = fontkit.query(key)

  t.is(
    font.glyph_path('A').to_string(),
    'M 813 -465 L 317 -465 L 72 0 L -117 0 L 682 -1462 L 856 -1462 L 1040 0 L 870 0 L 813 -465 z M 795 -621 L 760 -920 Q 736 -1099 731 -1270 Q 694 -1182 650.5 -1095 Q 607 -1008 401 -621 L 795 -621 z',
  )
})
