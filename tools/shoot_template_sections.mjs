/**
 * Capture high-DPI crops of key regions of the template preview, so the
 * styling can be reviewed without eyeballing a 10,000px full-page shot.
 *
 * Usage: node tools/shoot_template_sections.mjs [outDir]
 */
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const WS = '/Users/shang/.workbuddy-ai/binaries/node/workspace';
const require = createRequire(WS + '/package.json');
const puppeteer = require('puppeteer-core');
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');
const args = process.argv.slice(2);
const fileIdx = args.indexOf('--file');
const TPL = fileIdx >= 0 ? resolve(args[fileIdx + 1]) : resolve(ROOT, 'assets/report-template.html');
const outIdx = args.indexOf('--out');
const OUT = resolve(outIdx >= 0 ? args[outIdx + 1] : resolve(ROOT, 'reports/template-sections'));
if (!existsSync(OUT)) mkdirSync(OUT, { recursive: true });

const SECTIONS = [
  // :has() lets us grab a wrapper by what it contains, without guessing class names
  ['01-hero', ':has(> .score-giant)'],
  ['02-bullbear', '.fighter-arena, .punchline, .bull-bear'],
  ['03-jury', '#jury-seats'],
  ['04-chat', '#chat-messages'],
  ['05-debate', '.debate-rounds'],
  ['06-schools', 'div:has(> [title^="score_mean="])'],
  ['07-dims', '.dim-row'],
  ['08-modeling', '.inst-modeling-wrap'],
  ['09-friendly', '.friendly-trio'],
];

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: 'new',
  args: ['--no-sandbox', '--allow-file-access-from-files'],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 1000, deviceScaleFactor: 2 });
  await page.goto('file://' + TPL, { waitUntil: 'load', timeout: 30000 });
  await new Promise((r) => setTimeout(r, 9000));

  for (const [name, sel] of SECTIONS) {
    const el = await page.$(sel);
    if (!el) { console.log(`MISS ${name} (${sel})`); continue; }
    const box = await el.boundingBox();
    if (!box || box.height < 4) { console.log(`SKIP ${name} (empty)`); continue; }
    // clamp very tall regions so crops stay reviewable
    if (box.height > 2400) box.height = 2400;
    await page.screenshot({
      path: resolve(OUT, `${name}.png`),
      clip: box,
    });
    console.log(`OK   ${name}  ${Math.round(box.width)}x${Math.round(box.height)}`);
  }
} finally {
  await browser.close();
}
