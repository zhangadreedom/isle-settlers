// End-to-end play-through: plays a whole game as the human through the real
// UI (clicks on the 3D board at projected positions, HUD buttons, modals),
// then checks the mobile layout. Fails on any page error.
//
//   pnpm --filter @isle/web build && pnpm --filter @isle/web preview &
//   node web/e2e/play-through.cjs [screenshot-dir]
//
// Env: E2E_URL (default http://localhost:4173/), BUILD_ONLY=1 (never trade
// with players or buy cards; exercises building), TRACE=1.
// Requires the `playwright` package with Chromium available.
const { chromium, devices } = require('playwright');
const path = require('path');
const fs = require('fs');
const OUT = process.argv[2] || path.join(__dirname, 'out');
fs.mkdirSync(OUT, { recursive: true });
const URL = (process.env.E2E_URL || 'http://localhost:4173/') + '?debug&renderer=webgl';
const BUILD_ONLY = !!process.env.BUILD_ONLY;
const P = BUILD_ONLY ? 'b-' : '';

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => m.type() === 'error' && errors.push(`console: ${m.text()}`));

  await page.goto(URL);
  await page.waitForSelector('text=开始游戏');
  await page.waitForFunction(() => !document.querySelector('.lobby button.primary')?.disabled);
  await page.screenshot({ path: `${OUT}/p1-${P}lobby.png` });
  await page.click('.seg >> text=先手');
  await page.click('text=快速');
  await page.click('text=开始游戏');

  const state = () =>
    page.evaluate(() => {
      const g = window.__isle.game();
      if (!g) return null;
      return { view: g.view(), legal: g.legal() };
    });

  // Click a board spot whose screen position is not covered by HUD panels.
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

  const stats = { settlements: 0, roads: 0, cities: 0, bankTrades: 0, offers: 0, deals: 0, discards: 0, robbers: 0, dev: 0, turns: 0 };
  const shots = { setup: false, offer: false, robber: false, mid: false };
  let triedOfferTurn = -1;
  const deadline = Date.now() + 420_000;

  while (Date.now() < deadline) {
    const s = await state();
    const v = s.view;
    if (v.winner !== null) break;
    const acts = s.legal.actions;
    const types = new Set(acts.map((a) => a.type));
    if (!acts.length) {
      await page.waitForTimeout(120);
      continue;
    }

    if (v.phase.type === 'SetupSettlement') {
      if (!shots.setup) {
        await page.waitForTimeout(300);
        await page.screenshot({ path: `${OUT}/p1-${P}setup.png` });
        shots.setup = true;
      }
      // Prefer high-production spots (sum of pips of adjacent tiles).
      const ranked = await page.evaluate((ids) => {
        const { game, topology } = window.__isle;
        const nums = game().board.numbers;
        const pips = (n) => (n === 0 ? 0 : 6 - Math.abs(7 - n));
        const score = (vtx) => {
          let s = 0;
          for (let t = 0; t < nums.length; t++) for (let k = 0; k < 6; k++) if (topology.tileVertices[t * 6 + k] === vtx) s += pips(nums[t]);
          return s;
        };
        return ids.sort((a, b) => score(b) - score(a));
      }, acts.filter((a) => a.type === 'BuildSettlement').map((a) => a.value));
      await clickSpot('vertex', ranked);
    } else if (v.phase.type === 'SetupRoad' || v.phase.type === 'RoadBuilding') {
      await clickSpot('edge', acts.filter((a) => a.type === 'BuildRoad').map((a) => a.value));
      stats.roads++;
    } else if (types.has('Discard')) {
      const need = v.pendingDiscard[v.me.seat];
      const hand = [...v.me.hand];
      await page.waitForSelector('.modal .stepper');
      const steppers = await page.$$('.modal .stepper');
      let left = need;
      for (let r = 0; r < 5 && left > 0; r++) {
        const n = Math.min(hand[r], left);
        for (let k = 0; k < n; k++) await (await steppers[r].$('button[aria-label="增加"]')).click();
        left -= n;
      }
      await page.click('text=确认弃牌');
      stats.discards++;
    } else if (v.phase.type === 'MoveRobber') {
      const tiles = [...new Set(acts.map((a) => a.value.tile))];
      await clickSpot('tile', tiles);
      if (!shots.robber) {
        await page.waitForTimeout(200);
        await page.screenshot({ path: `${OUT}/p1-${P}robber.png` });
        shots.robber = true;
      }
      await page.waitForTimeout(150);
      const choice = await page.$('.modal .choice');
      if (choice) await choice.click();
      stats.robbers++;
    } else if (types.has('RollDice')) {
      const knight = await page.$('.devcard:not([disabled])');
      if (knight && (await knight.textContent()).includes('骑士') && Math.random() < 0.5) {
        await knight.click();
        stats.dev++;
      } else await page.click('.actions >> text=掷骰子');
    } else if (v.phase.type === 'Main' && v.current === v.me.seat) {
      // A pending offer of ours: confirm with an acceptor or withdraw.
      if (v.trade) {
        const pending = v.trade.responses.some((r, i) => i !== v.me.seat && i < v.players.length && r === 'Pending');
        if (pending) {
          await page.waitForTimeout(150);
          continue;
        }
        if (!shots.offer) {
          await page.screenshot({ path: `${OUT}/p1-${P}trade.png` });
          shots.offer = true;
        }
        const deal = await page.$('.response button.primary:not([disabled])');
        if (deal) {
          await deal.click();
          stats.deals++;
        } else await page.click('text=撤回报价');
        await page.keyboard.press('Escape');
        continue;
      }
      if (types.has('BuildCity')) {
        await page.click('.build >> text=城市');
        await clickSpot('vertex', acts.filter((a) => a.type === 'BuildCity').map((a) => a.value));
        stats.cities++;
      } else if (types.has('BuildSettlement')) {
        await page.click('.build >> text=村庄');
        await clickSpot('vertex', acts.filter((a) => a.type === 'BuildSettlement').map((a) => a.value));
        stats.settlements++;
      } else if (types.has('BuildRoad') && (BUILD_ONLY ? !acts.some((a) => a.type === 'BuildSettlement') : v.players[v.me.seat].roadsLeft > 7)) {
        await page.click('.build >> text=道路');
        await clickSpot('edge', acts.filter((a) => a.type === 'BuildRoad').map((a) => a.value));
        stats.roads++;
      } else if (types.has('BuyDevCard') && !BUILD_ONLY) {
        await page.click('.build >> text=发展卡');
      } else if (!BUILD_ONLY && (await page.$('.devcard:not([disabled])')) && Math.random() < 0.7) {
        const btn = await page.$('.devcard:not([disabled])');
        const label = await btn.textContent();
        await btn.click();
        await page.waitForTimeout(120);
        if (label.includes('丰收')) {
          await (await page.$$('.modal .stepper button[aria-label="增加"]:not([disabled])'))[0].click();
          await (await page.$$('.modal .stepper button[aria-label="增加"]:not([disabled])'))[0].click();
          await page.click('.modal >> text=拿取');
        } else if (label.includes('垄断')) {
          await page.click('.modal .choice >> nth=3');
        }
        stats.dev++;
      } else if (types.has('MaritimeTrade') && (BUILD_ONLY || Math.random() < 0.6)) {
        // Build-only mode trades the most plentiful card for the scarcest one.
        const RES0 = ['Brick', 'Lumber', 'Wool', 'Grain', 'Ore'];
        const hand = v.me.hand;
        const trades = acts.filter((a) => a.type === 'MaritimeTrade');
        const t = BUILD_ONLY
          ? trades.sort((a, b) => hand[RES0.indexOf(b.value.give)] - hand[RES0.indexOf(a.value.give)] || hand[RES0.indexOf(a.value.get)] - hand[RES0.indexOf(b.value.get)])[0]
          : trades[0];
        await page.click('.actions >> text=交易');
        const RES = ['Brick', 'Lumber', 'Wool', 'Grain', 'Ore'];
        await page.click(`.bank-grid .choice >> nth=${RES.indexOf(t.value.give)}`);
        await page.click(`.modal .choices.row .choice >> nth=${RES.indexOf(t.value.get)}`);
        await page.keyboard.press('Escape');
        stats.bankTrades++;
      } else if (!BUILD_ONLY && triedOfferTurn !== v.turn && v.me.hand.some((c) => c > 0)) {
        triedOfferTurn = v.turn;
        const give = v.me.hand.findIndex((c) => c > 0);
        const get = (give + 1 + Math.floor(Math.random() * 4)) % 5;
        await page.click('.actions >> text=交易');
        await page.click('.tabs >> text=与玩家');
        const cols = await page.$$('.trade-cols > div');
        await (await cols[0].$$('button[aria-label="增加"]'))[give].click();
        await (await cols[1].$$('button[aria-label="增加"]'))[get].click();
        await page.click('text=发起报价');
        stats.offers++;
      } else {
        if (!shots.mid && v.turn > 20) {
          await page.screenshot({ path: `${OUT}/p1-${P}midgame.png` });
          shots.mid = true;
        }
        if (process.env.TRACE) console.log('end turn', v.turn, 'hand', JSON.stringify(v.me.hand), 'legal', [...types].join(','), 'phase', v.phase.type);
        await page.click('.actions >> text=结束回合');
        stats.turns++;
      }
    } else {
      await page.waitForTimeout(120);
    }
    await page.waitForTimeout(60);
  }

  const final = await state();
  await page.waitForTimeout(800);
  await page.screenshot({ path: `${OUT}/p1-${P}gameover.png` });
  console.log('winner:', final.view.winner, 'me:', final.view.me.seat, 'turn:', final.view.turn, 'points:', JSON.stringify(final.view.finalPoints));
  console.log('stats:', JSON.stringify(stats));
  console.log('log tail:', (await page.$$eval('.log li', (els) => els.slice(-4).map((e) => e.textContent))).join(' | '));

  const mobile = await browser.newPage({ ...devices['iPhone 13'] });
  mobile.on('pageerror', (e) => errors.push(`mobile pageerror: ${e.message}`));
  await mobile.goto(URL);
  await mobile.waitForFunction(() => !document.querySelector('.lobby button.primary')?.disabled);
  await mobile.screenshot({ path: `${OUT}/p1-${P}mobile-lobby.png` });
  await mobile.click('.seg >> text=先手');
  await mobile.click('text=开始游戏');
  await mobile.waitForFunction(() => window.__isle.game()?.legal().actions.length > 0, null, { timeout: 30000 });
  await mobile.waitForTimeout(500);
  await mobile.screenshot({ path: `${OUT}/p1-${P}mobile-setup.png` });
  console.log('mobile overflow:', await mobile.evaluate(() => document.documentElement.scrollWidth > innerWidth));
  console.log('errors:', errors.length ? errors.slice(0, 10) : 'none');
  await browser.close();
  if (errors.length || final.view.winner === null) process.exit(1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
