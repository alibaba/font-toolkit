const { readFileSync } = require("fs");
const { FontKit } = require("../wasm-node");
const path = require("path");

const FONT_PATH = path.resolve(__dirname, "OpenSans-Italic.ttf")

const data = readFileSync(FONT_PATH);
const fontkit = new FontKit();
const key = fontkit.add_font_from_buffer(data);
const font = fontkit.query(key);
console.log(font.glpyh_path('A'))