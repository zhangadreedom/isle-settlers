// Online end-to-end test: two isolated browser contexts (host and guest)
// create and join a room through the UI, chat, trade with each other, survive
// a page reload (reconnect with the saved seat token) and play to the end
// against a bot. Fails on any page error.
//
//   cargo run --release -p isle-server -- --bot-delay 0.3 &   # serves web/dist
//   node web/e2e/online.cjs [screenshot-dir]
//
// Env: E2E_URL (default http://localhost:8080/), E2E_AI=困难 (bot level),
// E2E_PLAYERS=2..6 (room size, default 3; 5-6 seats use the extended island).
//
// At the end the guest opens the stored replay from the game-over dialog and
// a third browser opens the shared replay link.
// Requires `playwright`.
const { chromium } = require('playwright');
const path = require('path');
const fs = require('fs');

const OUT = process.argv[2] || path.join(__dirname, 'out');
fs.mkdirSync(OUT, { recursive: true });
const BASE = process.env.E2E_URL || 'http://localhost:8080/';
const QUERY = '?debug&renderer=webgl';
const RES = ['Brick', 'Lumber', 'Wool', 'Grain', 'Ore'];
const PLAYERS = Number(process.env.E2E_PLAYERS || 3);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function player(browser, name, errors) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(`${name}: ${e.message}`));
  page.on('console', (m) => m.type() === 'error' && errors.push(`${name} console: ${m.text()}`));
  return { name, page, context, stats: { actions: 0, offers: 0, deals: 0, responses: 0, reloads: 0 } };
}

const state = (page) =>
  page.evaluate(() => {
    const g = window.__isle?.game();
    return g ? { view: g.view(), legal: g.legal() } : null;
  });

/** Clicks a board spot that is not covered by HUD panels. */
async function clickSpot(page, kind, ids) {
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
      await page.mouse.click(p.x, p.y);
      return true;
    }
  }
  throw new Error(`no visible ${kind}`);
}

async function rankSettlements(page, ids) {
  return page.evaluate((ids) => {
    const { game } = window.__isle;
    const topology = game().topology;
    const nums = game().board.numbers;
    const pips = (n) => (n === 0 ? 0 : 6 - Math.abs(7 - n));
    const score = (v) => {
      let s = 0;
      for (let t = 0; t < nums.length; t++) for (let k = 0; k < 6; k++) if (topology.tileVertices[t * 6 + k] === v) s += pips(nums[t]);
      return s;
    };
    return ids.sort((a, b) => score(b) - score(a));
  }, ids);
}

/** Performs at most one UI action for `p`. Returns false once the game is over. */
async function step(p, other, shared) {
  const { page } = p;
  const s = await state(page);
  if (!s) return true;
  const v = s.view;
  if (v.winner !== null) return false;
  const acts = s.legal.actions;
  const types = new Set(acts.map((a) => a.type));
  if (!acts.length) return true;
  const me = v.me.seat;
  const of = (type) => acts.filter((a) => a.type === type);

  if (v.phase.type === 'SetupSettlement') {
    await clickSpot(page, 'vertex', await rankSettlements(page, of('BuildSettlement').map((a) => a.value)));
  } else if (v.phase.type === 'SetupRoad' || v.phase.type === 'RoadBuilding') {
    await clickSpot(page, 'edge', of('BuildRoad').map((a) => a.value));
  } else if (types.has('Discard')) {
    await page.waitForSelector('.modal .stepper');
    const steppers = await page.$$('.modal .stepper');
    let left = v.pendingDiscard[me];
    for (let r = 0; r < 5 && left > 0; r++) {
      for (let k = 0; k < Math.min(v.me.hand[r], left); k++) await (await steppers[r].$('button[aria-label="增加"]')).click();
      left -= Math.min(v.me.hand[r], left);
    }
    await page.click('text=确认弃牌');
  } else if (types.has('RespondTrade')) {
    // Answer once; the offer can be withdrawn at any moment.
    if (v.trade.responses[me] !== 'Pending') return true;
    try {
      await page.waitForSelector('.incoming button', { timeout: 3000 });
      if (!shared.offerShot) {
        shared.offerShot = true;
        await page.screenshot({ path: `${OUT}/p2-incoming-offer.png` });
      }
      const accept = await page.$('.incoming button.primary:not([disabled])');
      if (accept) await accept.click({ timeout: 3000 });
      else await page.click('.incoming button:has-text("拒绝")', { timeout: 3000 });
      p.stats.responses++;
    } catch {
      return true;
    }
  } else if (v.phase.type === 'MoveRobber') {
    await clickSpot(page, 'tile', [...new Set(acts.map((a) => a.value.tile))]);
    await sleep(150);
    const choice = await page.$('.modal .choice');
    if (choice) await choice.click();
  } else if (types.has('RollDice')) {
    await page.click('.actions >> text=掷骰子');
  } else if (v.phase.type === 'SpecialBuild' && v.toMove === me) {
    p.stats.special = (p.stats.special ?? 0) + 1;
    if (types.has('BuildSettlement')) {
      await page.click('.build >> text=村庄');
      await clickSpot(page, 'vertex', of('BuildSettlement').map((a) => a.value));
    } else if (types.has('BuildCity')) {
      await page.click('.build >> text=城市');
      await clickSpot(page, 'vertex', of('BuildCity').map((a) => a.value));
    } else {
      await page.click('.actions >> text=跳过');
    }
  } else if (v.phase.type === 'Main' && v.current === me) {
    if (v.trade) {
      const waiting = v.trade.responses.some((r, i) => i !== me && i < v.players.length && r === 'Pending');
      if (waiting) return true;
      const deal = await page.$('.response button.primary');
      if (deal) {
        await deal.click();
        p.stats.deals++;
      } else await page.click('button:has-text("撤回报价")');
      await page.keyboard.press('Escape');
    } else if (types.has('BuildCity')) {
      await page.click('.build >> text=城市');
      await clickSpot(page, 'vertex', of('BuildCity').map((a) => a.value));
    } else if (types.has('BuildSettlement')) {
      await page.click('.build >> text=村庄');
      await clickSpot(page, 'vertex', of('BuildSettlement').map((a) => a.value));
    } else if (types.has('BuildRoad')) {
      await page.click('.build >> text=道路');
      await clickSpot(page, 'edge', of('BuildRoad').map((a) => a.value));
    } else if (p.name === 'host' && p.stats.offers < 3 && shared.offeredTurn !== v.turn && (await tryOffer(p, other, v))) {
      shared.offeredTurn = v.turn;
    } else if (types.has('MaritimeTrade')) {
      const hand = v.me.hand;
      const t = of('MaritimeTrade').sort(
        (a, b) => hand[RES.indexOf(b.value.give)] - hand[RES.indexOf(a.value.give)] || hand[RES.indexOf(a.value.get)] - hand[RES.indexOf(b.value.get)],
      )[0];
      await page.click('.actions >> text=交易');
      await page.click(`.bank-grid .choice >> nth=${RES.indexOf(t.value.give)}`);
      await page.click(`.modal .choices.row .choice >> nth=${RES.indexOf(t.value.get)}`);
      await page.keyboard.press('Escape');
    } else {
      await page.click('.actions >> text=结束回合');
    }
  } else {
    return true;
  }
  p.stats.actions++;
  // Online actions are confirmed asynchronously: wait for the server's update
  // (or give up quietly if the action was rejected) before deciding again.
  await page
    .waitForFunction((before) => window.__isle?.game()?.legal().version !== before, s.legal.version, { timeout: 4000 })
    .catch(() => {});
  return true;
}

/** Host offers 1 card it has for 1 card the guest has. */
async function tryOffer(host, guest, v) {
  const g = await state(guest.page);
  if (!g) return false;
  const give = v.me.hand.findIndex((c) => c > 0);
  const get = g.view.me.hand.findIndex((c, i) => c > 0 && i !== give);
  if (give < 0 || get < 0) return false;
  const { page } = host;
  await page.click('.actions >> text=交易');
  await page.click('.tabs >> text=与玩家');
  const cols = await page.$$('.trade-cols > div');
  await (await cols[0].$$('button[aria-label="增加"]'))[give].click();
  await (await cols[1].$$('button[aria-label="增加"]'))[get].click();
  await page.click('text=发起报价');
  host.stats.offers++;
  return true;
}

async function playLoop(p, other, shared, deadline) {
  while (Date.now() < deadline) {
    // The guest reloads once mid-game: it must rejoin its seat by itself.
    if (p.name === 'guest' && !shared.reloaded) {
      const s = await state(p.page);
      if (s && s.view.turn >= 8) {
        shared.reloaded = true;
        const seat = s.view.me.seat;
        await p.page.reload();
        await p.page.waitForFunction(() => window.__isle?.game()?.view(), null, { timeout: 30000 });
        const back = await state(p.page);
        if (back.view.me.seat !== seat) throw new Error('rejoined a different seat');
        p.stats.reloads++;
        await p.page.screenshot({ path: `${OUT}/p2-guest-after-reload.png` });
      }
    }
    let more;
    try {
      more = await step(p, other, shared);
    } catch (e) {
      await p.page.screenshot({ path: `${OUT}/p2-error-${p.name}.png` });
      const s = await state(p.page);
      console.error(`${p.name} failed; phase`, JSON.stringify(s?.view.phase), 'current', s?.view.current, 'me', s?.view.me?.seat,
        'trade', JSON.stringify(s?.view.trade), 'legal', s?.legal.actions.map((a) => a.type).join(','));
      throw e;
    }
    if (!more) return;
    await sleep(80);
  }
  throw new Error(`${p.name}: game did not finish in time`);
}

(async () => {
  const browser = await chromium.launch();
  const errors = [];
  const host = await player(browser, 'host', errors);
  const guest = await player(browser, 'guest', errors);

  // Host creates a 3-seat room.
  await host.page.goto(BASE + QUERY);
  await host.page.waitForFunction(() => !document.querySelector('.lobby button.primary')?.disabled);
  await host.page.click('.tabs >> text=联机对战');
  await host.page.fill('input[aria-label="昵称"]', 'Alice');
  await host.page.click(`.seg >> text=${PLAYERS} 人`);
  // E2E_AI=简单|普通|困难 sets the level of the bot filling the free seat.
  if (process.env.E2E_AI) await host.page.click(`.seg >> text=${process.env.E2E_AI}`);
  await host.page.click('button:has-text("创建房间")');
  await host.page.waitForSelector('.invite input');
  const link = await host.page.inputValue('.invite input');
  const roomId = link.match(/#\/r\/(\w+)/)[1];
  console.log('room:', roomId);

  // Guest follows the invite link.
  await guest.page.goto(BASE + QUERY + `#/r/${roomId}`);
  await guest.page.waitForSelector('text=加入房间');
  await guest.page.screenshot({ path: `${OUT}/p2-guest-join.png` });
  await guest.page.fill('input[aria-label="昵称"]', 'Bob');
  await guest.page.click('button[type="submit"] >> text=加入');
  await guest.page.waitForSelector('.seats');
  await host.page.waitForFunction(() => document.querySelector('.seats')?.textContent.includes('Bob'));
  await host.page.screenshot({ path: `${OUT}/p2-room-host.png` });
  await guest.page.screenshot({ path: `${OUT}/p2-room-guest.png` });

  await host.page.click('button:has-text("开始游戏")');
  for (const p of [host, guest]) await p.page.waitForFunction(() => window.__isle?.game()?.view(), null, { timeout: 30000 });

  // Chat.
  await host.page.fill('.chat-form input', '大家好，有木头可以换吗？');
  await host.page.click('.chat-form >> text=发送');
  await guest.page.waitForFunction(() => [...document.querySelectorAll('.log li.chat')].some((li) => li.textContent.includes('木头')));

  const shared = {};
  const deadline = Date.now() + (PLAYERS > 4 ? 15 : 8) * 60_000;
  let midShot = false;
  const shooter = (async () => {
    while (!midShot && Date.now() < deadline) {
      const s = await state(host.page);
      if (s && s.view.turn >= 12) {
        midShot = true;
        await host.page.screenshot({ path: `${OUT}/p2-midgame-host.png` });
        await guest.page.screenshot({ path: `${OUT}/p2-midgame-guest.png` });
      }
      await sleep(500);
    }
  })();
  await Promise.all([playLoop(host, guest, shared, deadline), playLoop(guest, host, shared, deadline)]);
  midShot = true;
  await shooter;

  await sleep(800);
  await host.page.screenshot({ path: `${OUT}/p2-gameover-host.png` });
  await guest.page.screenshot({ path: `${OUT}/p2-gameover-guest.png` });
  const [hs, gs] = [await state(host.page), await state(guest.page)];
  console.log('winner:', hs.view.winner, 'same for guest:', gs.view.winner === hs.view.winner, 'turns:', hs.view.turn);
  console.log('final points:', JSON.stringify(hs.view.finalPoints));
  console.log('host:', JSON.stringify(host.stats), 'guest:', JSON.stringify(guest.stats));
  if (PLAYERS > 4 && !host.stats.special && !guest.stats.special) errors.push('never saw a special building phase');

  // The stored replay, from the dialog and through a shared link.
  await guest.page.click('.modal >> text=观看回放');
  await guest.page.waitForSelector('.replay-bar');
  const hash = await guest.page.evaluate(() => location.hash);
  if (!/^#\/replay\/\w+$/.test(hash)) errors.push(`unexpected replay link ${hash}`);
  const viewer = await player(browser, 'viewer', errors);
  await viewer.page.goto(BASE + QUERY + hash);
  await viewer.page.waitForSelector('.replay-bar', { timeout: 30000 });
  const total = await viewer.page.evaluate(() => window.__isle.game().replay.length);
  await viewer.page.evaluate((n) => window.__isle.game().replay.seek(n), total);
  await sleep(300);
  const end = await state(viewer.page);
  if (end.view.winner !== hs.view.winner || JSON.stringify(end.view.buildings) !== JSON.stringify(hs.view.buildings)) {
    errors.push('shared replay does not end like the game');
  }
  await viewer.page.evaluate((n) => window.__isle.game().replay.seek(Math.floor(n / 2)), total);
  await sleep(300);
  await viewer.page.screenshot({ path: `${OUT}/p2-replay-link.png` });
  console.log('replay:', hash, total, 'actions');
  console.log('errors:', errors.length ? errors.slice(0, 10) : 'none');
  await browser.close();
  if (errors.length || hs.view.winner === null || gs.view.winner !== hs.view.winner || !shared.reloaded) process.exit(1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
