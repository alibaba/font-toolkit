import { dirname } from 'path';
import { fileURLToPath } from 'url';

import { FontKitIndex } from '../node.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));

const fontkit = new FontKitIndex();
await fontkit.initiate();
fontkit.addSearchPath(__dirname);
const font = fontkit.font('Open Sans');

fontkit.free();
