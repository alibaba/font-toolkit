import { fontkitInterface } from './pkg/fontkit.js';

fontkitInterface.FontKit.prototype.has = function (key) {
  const font = this.exactMatch(key);
  const hasFont = !!font;
  if (font) font[Symbol.dispose]();
  return hasFont;
};

export const Font = fontkitInterface.Font;
export const FontKit = fontkitInterface.FontKit;
export const TextMetrics = fontkitInterface.TextMetrics;
export const numberWidthToStr = fontkitInterface.numberWidthToStr;
export const strWidthToNumber = fontkitInterface.strWidthToNumber;
export const GlyphBitmap = fontkitInterface.GlyphBitmap;
