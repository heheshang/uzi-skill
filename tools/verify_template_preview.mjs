/**
 * Render a report page in headless Chrome and assert the layout is complete:
 * every {{PLACEHOLDER}} substituted, every injected slot populated, no broken
 * images. Run against the template (checks the preview layer) or a real report
 * (checks the generated output).
 *
 * Usage:
 *   node tools/verify_template_preview.mjs [--file page.html] [--shot out.png] [--keep]
 *
 * Exit code 0 = clean, 1 = problems found.
 */
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// puppeteer-core lives in the managed workspace; ESM ignores NODE_PATH so
// resolve it explicitly through a require rooted at that package.json.
const WS = '/Users/shang/.workbuddy-ai/binaries/node/workspace';
const require = createRequire(WS + '/package.json');
const puppeteer = require('puppeteer-core');

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');

const argv = process.argv.slice(2);
const fileIdx = argv.indexOf('--file');
const TPL = fileIdx >= 0
  ? resolve(argv[fileIdx + 1])
  : resolve(ROOT, 'assets/report-template.html');
const shotIdx = argv.indexOf('--shot');
const SHOT = shotIdx >= 0 ? argv[shotIdx + 1] : resolve(ROOT, 'reports/template-preview.png');
const keep = argv.includes('--keep');
const isTemplate = TPL.endsWith('report-template.html');

if (!existsSync(TPL)) {
  console.error('missing page:', TPL);
  process.exit(2);
}

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: 'new',
  args: ['--no-sandbox', '--allow-file-access-from-files', '--window-size=1440,1000'],
});

try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 1000, deviceScaleFactor: 1 });

  const warns = [];
  page.on('console', (m) => {
    if (m.type() === 'warning' || m.type() === 'error') warns.push(m.text());
  });
  page.on('pageerror', (e) => warns.push('pageerror: ' + e.message));

  await page.goto('file://' + TPL, { waitUntil: 'load', timeout: 30000 });
  // let the preview layer + boot sequence settle
  await new Promise((r) => setTimeout(r, 9000));

  const report = await page.evaluate(() => {
    const LB = String.fromCharCode(123, 123);
    const RX = new RegExp(LB + '[A-Z_0-9]+' + String.fromCharCode(125, 125), 'g');

    // raw placeholders still visible in rendered text
    const raw = [];
    const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null, false);
    while (w.nextNode()) {
      const n = w.currentNode;
      const p = n.parentElement;
      if (p && (p.tagName === 'SCRIPT' || p.tagName === 'STYLE')) continue;
      const m = n.nodeValue.match(RX);
      if (m) raw.push(...m);
    }
    // and in attributes
    document.querySelectorAll('*').forEach((el) => {
      if (el.tagName === 'SCRIPT' || el.tagName === 'STYLE') return;
      for (const a of el.attributes) {
        const m = a.value.match(RX);
        if (m) raw.push(...m);
      }
    });

    const imgs = [...document.querySelectorAll('img')];
    const broken = imgs
      .filter((i) => i.complete && i.naturalWidth === 0)
      .map((i) => i.getAttribute('src'));

    const q = (s) => document.querySelectorAll(s).length;

    // stance histogram over the rendered jury seats
    const stances = { bullish: 0, neutral: 0, bearish: 0 };
    document.querySelectorAll('.seat').forEach((s) => {
      for (const c of s.classList) if (c in stances) stances[c]++;
    });
    // scores shown on the share card, to check top/bottom ordering
    const bestScores = [...document.querySelectorAll('.sc-best')].map((box) =>
      [...box.querySelectorAll('.score-num')].map((n) => +n.textContent));

    // the reasoning block is written with \n-separated bullets; without a
    // white-space rule they collapse into one run-on line
    const reasoning = document.querySelector('.msg-reasoning');
    let reasoningInfo = null;
    if (reasoning) {
      const cs = getComputedStyle(reasoning);
      reasoningInfo = {
        whiteSpace: cs.whiteSpace,
        lines: Math.round(
          reasoning.getBoundingClientRect().height / parseFloat(cs.lineHeight)
        ),
      };
    }

    return {
      rawTokensLeft: raw.length,
      rawTokenSample: [...new Set(raw)].slice(0, 8),
      brokenImgs: broken.length,
      brokenSample: broken.slice(0, 8),
      totalImgs: imgs.length,
      reasoningInfo,
      reasoningText: reasoning ? reasoning.textContent : '',
      seats: q('.seat'),
      seatStances: stances,
      bestScores,
      seatGroups: [...new Set([...document.querySelectorAll('.seat')].map((s) => s.dataset.group))].sort(),
      chatMsgs: q('.chat-msg'),
      dimCards: q('.dim-card'),
      dimRows: q('.dim-row'),
      voteRows: q('.sc-vote-row'),
      bestCells: q('.sc-best-cell'),
      friendlyCards: q('.friendly-card'),
      debateRounds: q('.debate-rounds .round'),
      riskItems: q('.risk-box li'),
      schoolCards: q('[title^="score_mean="]'),
      instBlocks: q('.inst-modeling-wrap > div'),
      bodyH: document.body.scrollHeight,
      bootGone: !document.querySelector('.boot-overlay'),
    };
  });

  await page.screenshot({ path: SHOT, fullPage: true });

  console.log(JSON.stringify(report, null, 2));
  if (warns.length) console.log('console warnings:', JSON.stringify(warns.slice(0, 10), null, 2));
  console.log('screenshot:', SHOT);

  const bad = [];
  if (report.rawTokensLeft !== 0) bad.push(`rawTokensLeft=${report.rawTokensLeft}`);
  if (report.brokenImgs !== 0) bad.push(`brokenImgs=${report.brokenImgs}`);
  if (!report.bootGone) bad.push('boot overlay still present');
  if (report.seats === 0) bad.push('no jury seats rendered');

  // Real reports may contain a single synthesized reasoning sentence. When
  // the source contains explicit line breaks, verify that the CSS preserves
  // them and that the preview's multiline fixture remains readable.
  const ri = report.reasoningInfo;
  if (ri && report.reasoningText.includes('\n') && (ri.whiteSpace !== 'pre-wrap' || ri.lines < 4)) {
    bad.push(`msg-reasoning renders on ${ri.lines} line(s) with white-space:${ri.whiteSpace} (want pre-wrap, >=4 when multiline)`);
  }

  // Slot-count expectations only hold for the template preview, which injects a
  // fixed demo dataset. Real reports vary (a coin has fewer dimension cards).
  if (isTemplate) {
    if (report.seats !== 66) bad.push(`seats=${report.seats} (want 66)`);
    if (report.dimCards !== 22) bad.push(`dimCards=${report.dimCards} (want 22)`);
    if (report.chatMsgs < 10) bad.push(`chatMsgs=${report.chatMsgs}`);
    if (report.debateRounds !== 3) bad.push(`debateRounds=${report.debateRounds} (want 3)`);
    if (report.schoolCards !== 9) bad.push(`schoolCards=${report.schoolCards} (want 9)`);
    if (report.bestCells !== 6) bad.push(`bestCells=${report.bestCells} (want 6)`);
    if (report.riskItems !== 5) bad.push(`riskItems=${report.riskItems} (want 5)`);

    // the demo dataset quotes 31 看多 / 13 中性 / 22 看空 all over the page —
    // make sure the rendered seats actually agree
    const s = report.seatStances;
    if (s.bullish !== 31 || s.neutral !== 13 || s.bearish !== 22) {
      bad.push(`stance histogram ${s.bullish}/${s.neutral}/${s.bearish} (want 31/13/22)`);
    }
    // top-3 bull card must be descending, top-3 bear ascending
    const [bull, bear] = report.bestScores;
    if (bull && bull.some((v, i) => i && v > bull[i - 1])) bad.push(`bull card not descending: ${bull}`);
    if (bear && bear.some((v, i) => i && v < bear[i - 1])) bad.push(`bear card not ascending: ${bear}`);
  }

  if (bad.length) {
    console.error('\nFAIL: ' + bad.join(' · '));
    process.exitCode = 1;
  } else {
    console.log(`\nPASS: ${isTemplate ? 'template' : 'report'} renders cleanly.`);
  }
} finally {
  if (!keep) await browser.close();
}
