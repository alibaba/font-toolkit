// node --experimental-wasi-unstable-preview1 examples/node.mjs

import fs from 'fs';
import { dirname } from 'path';
import { fileURLToPath } from 'url';

import { FontKitIndex } from '../node.js';
import { default as Toolkit } from '../pkg/node/index.js';

const { FontKit } = Toolkit;

const __dirname = dirname(fileURLToPath(import.meta.url));

const fontIndex = new FontKitIndex();
await fontIndex.initiate();
fontIndex.addSearchPath(__dirname);
const fontFilePath = fontIndex.font('Noto Serif CJK SC').path();
console.info('已加载字体：', fontFilePath, '\n');

const fontkit = new FontKit();
const buffer = fs.readFileSync(fontFilePath);
const [key] = fontkit.add_font_from_buffer(buffer);
const font = fontkit.query(key);

console.info('查询已加载字体中的 glyph path');
console.info('不', font.glyph_path('不')?.to_string());
console.info('鼾', font.glyph_path('鼾')?.to_string());

fontIndex.free();
