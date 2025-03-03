import {
  FontKit,
  Font,
  FontInfo,
  FontKey,
  AlibabaFontkitFontkitInterface,
  GlyphBitmap,
} from './pkg/interfaces/alibaba-fontkit-fontkit-interface';

export import strWidthToNumber = AlibabaFontkitFontkitInterface.strWidthToNumber;
export import numberWidthToStr = AlibabaFontkitFontkitInterface.numberWidthToStr;

declare module './pkg/interfaces/alibaba-fontkit-fontkit-interface' {
  interface FontKit {
    has: (key: FontKey) => boolean;
  }
}

export { FontKit, Font, FontInfo, FontKey, GlyphBitmap };
