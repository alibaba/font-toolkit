import { homedir } from 'os';
import { dirname } from 'path';
import { fileURLToPath } from 'url';

import walk from 'walkdir';

import { fontkitInterface as fi } from '../pkg/fontkit.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

const fontkit = new fi.FontKit();
// fontkit.addSearchPath(__dirname + '/OpenSans-Italic.ttf');
// fontkit.addSearchPath(homedir() + '/Library/Fonts');
walk.sync('./', (path) => {
  if (path.endsWith('.ttf') || path.endsWith('.otf') || path.endsWith('.ttc')) {
    // console.log(path);
    fontkit.addSearchPath(path);
  }
});

// console.log(fontkit.writeData());
// const font = fontkit.query({ family: 'Open Sans' });

// // eslint-disable-next-line no-console
// console.log(font.hasGlyph('c'));
// // eslint-disable-next-line no-console
// console.log(fontkit.fontsInfo());
