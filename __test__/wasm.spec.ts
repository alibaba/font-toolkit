import { promises as fs } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

import test from 'ava';

import { default as Toolkit } from '../pkg/node/index.js';

const { FontKit } = Toolkit;

const __dirname = dirname(fileURLToPath(import.meta.url));

// init wasm
let fontData: Uint8Array | null = null;

test.before(async () => {
  fontData = await fs.readFile(join(__dirname, '../examples/OpenSans-Italic.ttf'));
});

test('em box', (t) => {
  const fontkit = new FontKit();
  const [key] = fontkit.add_font_from_buffer(fontData!);
  const font = fontkit.query(key);

  t.not(font, undefined);
  t.is(font!.units_per_em, 2048);
});

test('glyph path to_string()', (t) => {
  const fontkit = new FontKit();
  const [key] = fontkit.add_font_from_buffer(fontData!);
  const font = fontkit.query(key);

  t.is(
    font!.glyph_path('A')!.to_string(),
    'M 813 2324 L 317 2324 L 72 2789 L -117 2789 L 682 1327 L 856 1327 L 1040 2789 L 870 2789 L 813 2324 z M 795 2168 L 760 1869 Q 736 1690 731 1519 Q 694 1607 650.5 1694 Q 607 1781 401 2168 L 795 2168 z',
  );
});
