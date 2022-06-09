Toolkit used to load, match, measure and render texts.

**NOTE: This project is a work in progress. Text measuring and positioning is a complex topic. A more mature library is a sensible choice.**

Compile the module:

```bash
npm run build
npm run build:wasi
node examples/node.mjs
```

# Font querying

This module uses a special font matching logic. A font's identity contains the font family (name),
weight, italic, and stretch (font width). When querying, any subset of the memtioned information is allowed
except that the font family is required.

The query is then splitted into 4 filters, in the order of family -> weight -> italic -> stretch. Each filter
could strim the result set, and:

- If after any filter the result contains only one font, it is immediately returned.
- If after all filters, 0 or more than 1 font are left, the query fails

# General API (WASM API)

### `new FontKit()`

Create a new font registry

### `fontkit.add_font_from_buffer(buffer: Uint8Array) -> FontKey`

Add a font from a buffer, return the standard key of this font

### `fontkit.query(key: FontKey) -> Font | undefined`

Query a font

### `font.has_glyph(c: char) -> boolean`

Check if the font contains glyph for a given char

### `font.ascender() -> number`

The ascender metric of the font

### `font.descender() -> number`

The descender metric of the font

### `font.units_per_em() -> number`

The units of metrics of this font

### `font.glyph_path(c: char) -> GlyphPath | undefined`

Get the glyph path of a char, if any

### `glyphPath.scale(scaleFactor: number)`

The glyph of font is very large (by the unit of `units_per_em`), the method could scale the path for convenience

### `glyphPath.translate(x: number, y: number)`

Translate the path

### `glyphPath.to_string() -> string`

Export the glyph path as an SVG path string


# Node.js Support

Currently this module supports Node.js via `WASI` [API](https://nodejs.org/docs/latest/api/wasi.html). This requires
the `--experimental-wasi-unstable-preview1` flag and only limited APIs are provided.

## Example

```js
import { dirname } from 'path';
import { fileURLToPath } from 'url';

// Import the Node.js specific entry module
import { FontKitIndex } from '../node.js';

// Get current path, or you could use any path containing font files
const __dirname = dirname(fileURLToPath(import.meta.url));

// Create an instance of `FontKitIndex`, which is generally a font registry.
// By doing this, an object is created in Rust. So be sure to call `.free()`
// later
const fontkit = new FontKitIndex();

// Await here is needed as it initiate the WASM module
await fontkit.initiate();

// Add a search path, fonts are recursively indexed. This method could
// be called multiple times
fontkit.addSearchPath(__dirname);

// Query a font, additional params including weight, stretch, italic are supported
const font = fontkit.font('Open Sans');

// Get the actual path of the font
console.log(font.path());

// Free the memory of the registry
fontkit.free();
```

Being a minimal API, after getting the actual path of the font, you could load the file into
a `Uint8Array` buffer, and use the normal `WASM` APIs to load / use the font. So basically the
Node.js API here is only for indexing and querying fonts.

## API

### `new FontKitIndex()`

Create a new font registry **only for indexing**. Holding the info (not the actual buffers) of the fonts it found.

### `fontkitIndex.addSearchPath(path: string)`

Search recursively in a path for new fonts. Supports ttf/otf/woff/woff2 fonts. The fonts are
loaded to grab their info, and then immediately released.

### `fontkitIndex.query(family: string, weight?=400, italic?=false, strech?=5) -> Font | undefined`

Query a font. Check _querying_ for details

### `fontkitIndex.free()`

Free the registry. Further calls will panic the program

### `font.path() -> string`

Get the path of the font
