// Embedded single-player build test: serves `web/dist-embed` under a sub-path
// (so every asset must be relative), blocks localStorage like a sandboxed
// iframe, and plays a whole game against the hard AI through the real UI.
// Fails on page errors, downloads, requests outside the sub-path, any online
// or download control being visible, or a non-WebGL2 renderer.
//
//   pnpm build:embed
//   node web/e2e/embed.cjs [screenshot-dir]
//
// Env: E2E_AI=简单|普通|困难 (default 困难), E2E_PLAYERS=2..6 (default 2),
// E2E_DIST (default web/dist-embed).
// Requires the `playwright` package with Chromium available.
const { chromium } = require('playwright');
const http = require('http');
const path = require('path');
const fs = require('fs');

const OUT = process.argv[2] || path.join(__dirname, 'out');
fs.mkdirSync(OUT, { recursive: true });
const DIST = path.resolve(process.env.E2E_DIST || path.join(__dirname, '..', 'dist-embed'));
const PREFIX = '/sub/';
const AI = process.env.E2E_AI || '困难';
const PLAYERS = Number(process.env.E2E_PLAYERS || 2);
const RES = ['Brick', 'Lumber', 'Wool', 'Grain', 'Ore'];
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
};

if (!fs.existsSync(path.join(DIST, 'index.html'))) {
  console.error(`${DIST}/index.html not found; run \`pnpm build:embed\` first`);
  process.exit(1);
}

/** Static server for DIST at PREFIX; everything else is a recorded 404. */
function serve() {
  const served = [];
  const outside = [];
  const missing = [];
  const server = http.createServer((req, res) => {
    const { pathname } = new URL(req.url, 'http://localhost');
    if (!pathname.startsWith(PREFIX)) {
      if (pathname !== '/favicon.ico') outside.push(pathname);
      res.writeHead(404).end();
      return;
    }
    const rel = decodeURIComponent(pathname.slice(PREFIX.length)) || 'index.html';
    const file = path.normalize(path.join(DIST, rel));
    if (!file.startsWith(DIST + path.sep)) {
      res.writeHead(403).end();
      return;
    }
    fs.readFile(file, (err, data) => {
      if (err) {
        missing.push(pathname);
        res.writeHead(404).end();
        return;
      }
      served.push(pathname);
      res.writeHead(200, { 'Content-Type': MIME[path.extname(file).toLowerCase()] || 'application/octet-stream' });
      res.end(data);
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, served, outside, missing, port: server.address().port }));
  });
}

(async () => {
  const host = await serve();
  const URL = `http://127.0.0.1:${host.port}${PREFIX}?debug`;
  const browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: false });
  // A sandboxed iframe without allow-same-origin: storage access throws.
  await context.addInitScript(() => {
    for (const name of ['localStorage', 'sessionStorage']) {
      Object.defineProperty(window, name, {
        configurable: true,
        get() {
          throw new DOMException(`${name} is disabled`, 'SecurityError');
        },
      });
    }
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => m.type() === 'error' && errors.push(`console: ${m.text()}`));
  page.on('download', () => errors.push('a download was triggered'));

  await page.goto(URL);
  await page.waitForFunction(() => !document.querySelector('.lobby button.primary')?.disabled, null, { timeout: 60000 });

  // ---- embedded lobby ----
  const lobbyText = await page.$eval('.lobby', (el) => el.textContent);
  if (lobbyText.includes('联机对战') || (await page.$('.lobby .tabs'))) errors.push('online play is offered in the embedded build');
  await page.waitForFunction(() => /WebGL2|WebGPU/.test(document.querySelector('.renderer-info')?.textContent ?? ''));
  const backend = await page.$eval('.renderer-info', (el) => el.textContent);
  if (!backend.includes('WebGL2')) errors.push(`expected the WebGL2 renderer, got "${backend}"`);
  await page.screenshot({ path: `${OUT}/embed-lobby.png` });

  await page.click(`.seg >> text=${PLAYERS} 人`);
  await page.click('.seg >> text=先手');
  await page.click('text=快速');
  await page.click(`.seg >> text=${AI}`);
  await page.click('text=开始游戏');

  const state = () =>
    page.evaluate(() => {
      const g = window.__isle.game();
      return g ? { view: g.view(), legal: g.legal() } : null;
    });

  /** Clicks a board spot whose screen position is not covered by HUD panels. */
  async function clickSpot(kind, ids) {
    for (const id of ids) {
      const p = await page.evaluate(
        ([k, i]) => {
          const pt = window.__isle.project(k, i);
          const el = document.elementFromPoint(pt.x, pt.y);
          return el && el.tagName === 'CANVAS' ? pt : null;
        },
        [kind, id],
      );
      if (p) {
        await page.mouse.move(p.x, p.y);
        await page.mouse.click(p.x, p.y);
        return id;
      }
    }
    throw new Error(`no visible ${kind} among ${ids.length}`);
  }

  // ---- a whole game: build what we can, trade with the bank, end the turn ----
  const stats = { settlements: 0, cities: 0, roads: 0, bankTrades: 0, discards: 0, robbers: 0, turns: 0, special: 0 };
  let actedAt = -1;
  const deadline = Date.now() + (PLAYERS > 4 ? 900_000 : 480_000);
  try {
    while (Date.now() < deadline) {
      const s = await state();
      if (!s) {
        // The local match starts asynchronously (it lives in a worker).
        await page.waitForTimeout(100);
        continue;
      }
      const v = s.view;
      if (v.winner !== null) break;
      if (s.legal.version === actedAt) {
        await page.waitForFunction((x) => window.__isle.game()?.legal().version !== x, actedAt, { timeout: 4000 }).catch(() => {});
        actedAt = -1;
        continue;
      }
      const acts = s.legal.actions;
      const types = new Set(acts.map((a) => a.type));
      const of = (type) => acts.filter((a) => a.type === type).map((a) => a.value);
      if (!acts.length) {
        await page.waitForTimeout(120);
        continue;
      }
      const me = v.me.seat;

      if (v.phase.type === 'SetupSettlement') {
        const ranked = await page.evaluate((ids) => {
          const g = window.__isle.game();
          const nums = g.board.numbers;
          const pips = (n) => (n === 0 ? 0 : 6 - Math.abs(7 - n));
          const score = (vtx) => {
            let s = 0;
            for (let t = 0; t < nums.length; t++) for (let k = 0; k < 6; k++) if (g.topology.tileVertices[t * 6 + k] === vtx) s += pips(nums[t]);
            return s;
          };
          return ids.sort((a, b) => score(b) - score(a));
        }, of('BuildSettlement'));
        await clickSpot('vertex', ranked);
      } else if (v.phase.type === 'SetupRoad' || v.phase.type === 'RoadBuilding') {
        await clickSpot('edge', of('BuildRoad'));
        stats.roads++;
      } else if (types.has('Discard')) {
        await page.waitForSelector('.modal .stepper');
        const steppers = await page.$$('.modal .stepper');
        let left = v.pendingDiscard[me];
        for (let r = 0; r < 5 && left > 0; r++) {
          const n = Math.min(v.me.hand[r], left);
          for (let k = 0; k < n; k++) await (await steppers[r].$('button[aria-label="增加"]')).click();
          left -= n;
        }
        await page.click('text=确认弃牌');
        stats.discards++;
      } else if (v.phase.type === 'MoveRobber') {
        await clickSpot('tile', [...new Set(acts.map((a) => a.value.tile))]);
        await page.waitForTimeout(150);
        const choice = await page.$('.modal .choice');
        if (choice) await choice.click();
        stats.robbers++;
      } else if (types.has('RollDice')) {
        await page.click('.actions >> text=掷骰子');
      } else if (v.phase.type === 'SpecialBuild' && v.toMove === me) {
        stats.special++;
        if (types.has('BuildCity')) {
          await page.click('.build >> text=城市');
          await clickSpot('vertex', of('BuildCity'));
        } else if (types.has('BuildSettlement')) {
          await page.click('.build >> text=村庄');
          await clickSpot('vertex', of('BuildSettlement'));
        } else {
          await page.click('.actions >> text=跳过');
        }
      } else if (v.phase.type === 'Main' && v.current === me) {
        if (types.has('BuildCity')) {
          await page.click('.build >> text=城市');
          await clickSpot('vertex', of('BuildCity'));
          stats.cities++;
        } else if (types.has('BuildSettlement')) {
          await page.click('.build >> text=村庄');
          await clickSpot('vertex', of('BuildSettlement'));
          stats.settlements++;
        } else if (types.has('BuildRoad')) {
          await page.click('.build >> text=道路');
          await clickSpot('edge', of('BuildRoad'));
          stats.roads++;
        } else if (types.has('MaritimeTrade')) {
          // Trade the most plentiful card for the scarcest one.
          const hand = v.me.hand;
          const t = of('MaritimeTrade').sort(
            (a, b) => hand[RES.indexOf(b.give)] - hand[RES.indexOf(a.give)] || hand[RES.indexOf(a.get)] - hand[RES.indexOf(b.get)],
          )[0];
          await page.click('.actions >> text=交易');
          await page.click(`.bank-grid .choice >> nth=${RES.indexOf(t.give)}`);
          await page.click(`.modal .choices.row .choice >> nth=${RES.indexOf(t.get)}`);
          await page.keyboard.press('Escape');
          stats.bankTrades++;
        } else {
          await page.click('.actions >> text=结束回合');
          stats.turns++;
        }
      } else {
        await page.waitForTimeout(120);
      }
      actedAt = s.legal.version;
      await page.waitForTimeout(60);
    }
  } catch (e) {
    await page.screenshot({ path: `${OUT}/embed-error.png` }).catch(() => {});
    const s = await state().catch(() => null);
    console.error('failed in phase', JSON.stringify(s?.view.phase));
    throw e;
  }

  const final = await state();
  if (final.view.winner === null) throw new Error('game did not finish in time');
  await page.waitForTimeout(800);
  await page.screenshot({ path: `${OUT}/embed-gameover.png` });
  console.log('winner:', final.view.winner, 'me:', final.view.me.seat, 'turn:', final.view.turn, 'points:', JSON.stringify(final.view.finalPoints));
  console.log('stats:', JSON.stringify(stats));
  if (PLAYERS > 4 && !stats.special) errors.push('never saw a special building phase');

  // ---- replay: watchable, but nothing to download ----
  if (await page.$('.modal button:has-text("下载回放")')) errors.push('replay download offered in the game-over dialog');
  await page.click('.modal >> text=观看回放');
  await page.waitForSelector('.replay-bar');
  if (await page.$('.replay-bar button:has-text("下载")')) errors.push('replay download offered in the replay bar');
  const total = await page.evaluate(() => window.__isle.game().replay.length);
  await page.evaluate((n) => window.__isle.game().replay.seek(n), total);
  await page.waitForTimeout(300);
  const end = await page.evaluate(() => window.__isle.game().view());
  if (end.winner !== final.view.winner || JSON.stringify(end.buildings) !== JSON.stringify(final.view.buildings)) {
    errors.push('replay end state differs from the game');
  }
  await page.evaluate((n) => window.__isle.game().replay.seek(Math.floor(n / 2)), total);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${OUT}/embed-replay.png` });
  console.log('replay:', total, 'actions');

  // ---- back in the lobby: no storage, so no local history, and no crash ----
  await page.click('text=← 大厅');
  await page.click('.lobby details summary');
  const history = await page.$eval('.lobby details', (el) => el.textContent);
  if (!history.includes('还没有已结束的本机对局')) errors.push(`unexpected local history without storage: ${history}`);

  // ---- every request stayed inside the sub-path and found its file ----
  if (host.outside.length) errors.push(`requests outside ${PREFIX}: ${[...new Set(host.outside)].join(', ')}`);
  if (host.missing.length) errors.push(`missing files: ${[...new Set(host.missing)].join(', ')}`);
  const wasm = host.served.filter((p) => p.endsWith('.wasm'));
  const workers = host.served.filter((p) => /worker[^/]*\.js$/.test(p));
  if (!wasm.length) errors.push('no WebAssembly module was loaded');
  if (!workers.length) errors.push('no worker script was loaded');
  console.log('served:', host.served.length, 'files; wasm:', wasm.join(', '), 'worker:', workers.join(', '));

  console.log('errors:', errors.length ? errors.slice(0, 10) : 'none');
  await browser.close();
  host.server.close();
  if (errors.length) process.exit(1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
