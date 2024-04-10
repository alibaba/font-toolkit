import { homedir } from 'os';
import { dirname } from 'path';
import { fileURLToPath } from 'url';

import { fontkitInterface as fi } from '../pkg/fontkit.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

const fontkit = new fi.FontKit();
fontkit.addSearchPath(__dirname + '/OpenSans-Italic.ttf');
fontkit.addSearchPath(homedir() + '/Library/Fonts');

const font = fontkit.query({ family: 'Open Sans' });

// eslint-disable-next-line no-console
console.log(font.hasGlyph('c'));
// eslint-disable-next-line no-console
console.log(fontkit.fontsInfo());
