import { fstatSync, readFileSync } from 'fs';
import { homedir } from 'os';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

import walkdir from 'walkdir';
import { WASI } from 'wasi';

export * from './pkg/node/index.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

const WASM_PATH = resolve(__dirname, 'pkg/wasi/fontkit.wasm');

const wasi = new WASI({
  preopens: { '/': '/' },
  env: { RUST_BACKTRACE: '1', HOME: homedir() },
});

const buf = readFileSync(WASM_PATH);
const wasiModule = await WebAssembly.compile(new Uint8Array(buf));

// Encode string into memory starting at address base.
const encode = (memory, buffer) => {
  for (let i = 0; i < buffer.length; i++) {
    memory[i] = buffer[i];
  }
};

export const FontWeight = Object.freeze({
  Thin: 100,
  ExtraLight: 200,
  Light: 300,
  Normal: 400,
  Medium: 500,
  SemiBold: 600,
  Bold: 700,
  ExtraBold: 800,
  Black: 900,
});

export const FontStretch = Object.freeze({
  UltraCondensed: 1,
  ExtraCondensed: 2,
  Condensed: 3,
  SemiCondensed: 4,
  Normal: 5,
  SemiExpanded: 6,
  Expanded: 7,
  ExtraExpanded: 8,
  UltraExpanded: 9,
});

/**
 * Fontkit is a font registry used to query fonts.
 */
export class FontKitIndex {
  instance = undefined;
  fontkit_ptr = 0;

  /**
   * Initiate the class and setup a FontKit ready to use.
   *
   * **NOTE**: You **MUST** CALL `.free()` when discarding FontKit.
   */
  async initiate() {
    this.instance = await WebAssembly.instantiate(wasiModule, { wasi_snapshot_preview1: wasi.wasiImport });
    wasi.initialize(this.instance);
    this.fontkit_ptr = this.instance.exports.build_font_kit();
  }

  font(fontFamily, weight = 400, isItalic = false, stretch = FontStretch.Normal) {
    const pInput = this.instance.exports.alloc();
    const encoder = new TextEncoder();
    const buffer = encoder.encode(fontFamily);
    const view = new Uint8Array(this.instance.exports.memory.buffer, pInput, buffer.length);
    encode(view, buffer);
    const font = this.instance.exports.font_for_face(
      this.fontkit_ptr,
      pInput,
      fontFamily.length,
      weight,
      isItalic,
      stretch,
    );
    this.instance.exports.mfree(pInput);
    if (font === 0) return undefined;
    else return new Font(this.instance, font);
  }

  addSearchPath(searchPath) {
    const instance = this.instance;
    const ptr = this.fontkit_ptr;
    try {
      walkdir(searchPath, { sync: true }, (path) => {
        if (fstatSync(path).isDirectory()) return;
        const encoder = new TextEncoder();
        const buffer = encoder.encode(path);
        const pInput = instance.exports.alloc();
        const view = new Uint8Array(instance.exports.memory.buffer, pInput, buffer.length);
        encode(view, buffer);
        instance.exports.add_search_path(ptr, pInput, path.length);
        instance.exports.mfree(pInput);
      });
    } catch (e) {
      // Ignore
    }
  }

  free() {
    this.instance.exports.free_fontkit(this.fontkit_ptr);
    this.instance = undefined;
    this.fontkit_ptr = 0;
  }
}

export class Font {
  constructor(instance, ptr) {
    this.ptr = ptr;
    this.instance = instance;
  }

  path() {
    const ptr = this.instance.exports.path_for_font(this.ptr);
    const length = this.instance.exports.str_length(ptr);
    if (length) {
      const buffer = new Uint8Array(this.instance.exports.memory.buffer, ptr, length);
      const path = utf8ArrayToString(buffer);
      this.instance.exports.free_str(ptr);
      return path;
    } else {
      return '';
    }
  }
}

function utf8ArrayToString(aBytes) {
  let sView = '';

  for (let nPart, nLen = aBytes.length, nIdx = 0; nIdx < nLen; nIdx++) {
    nPart = aBytes[nIdx];

    sView += String.fromCharCode(
      nPart > 251 && nPart < 254 && nIdx + 5 < nLen /* six bytes */
        ? /* (nPart - 252 << 30) may be not so safe in ECMAScript! So...: */
          (nPart - 252) * 1073741824 +
            ((aBytes[++nIdx] - 128) << 24) +
            ((aBytes[++nIdx] - 128) << 18) +
            ((aBytes[++nIdx] - 128) << 12) +
            ((aBytes[++nIdx] - 128) << 6) +
            aBytes[++nIdx] -
            128
        : nPart > 247 && nPart < 252 && nIdx + 4 < nLen /* five bytes */
        ? ((nPart - 248) << 24) +
          ((aBytes[++nIdx] - 128) << 18) +
          ((aBytes[++nIdx] - 128) << 12) +
          ((aBytes[++nIdx] - 128) << 6) +
          aBytes[++nIdx] -
          128
        : nPart > 239 && nPart < 248 && nIdx + 3 < nLen /* four bytes */
        ? ((nPart - 240) << 18) + ((aBytes[++nIdx] - 128) << 12) + ((aBytes[++nIdx] - 128) << 6) + aBytes[++nIdx] - 128
        : nPart > 223 && nPart < 240 && nIdx + 2 < nLen /* three bytes */
        ? ((nPart - 224) << 12) + ((aBytes[++nIdx] - 128) << 6) + aBytes[++nIdx] - 128
        : nPart > 191 && nPart < 224 && nIdx + 1 < nLen /* two bytes */
        ? ((nPart - 192) << 6) + aBytes[++nIdx] - 128
        : /* nPart < 127 ? */ /* one byte */
          nPart,
    );
  }

  return sView;
}
