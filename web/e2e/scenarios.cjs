// End-to-end v0.3 scenario & UX regression suite:
//   1. Rules Quick-Reference modal (`规则`) + scenario tab switching
//   2. Sound Mute / Unmute toggle (`音效` / `静音`)
//   3. DIY Map Workshop (`地图工坊`) + `一键均衡 6/8` + `IM1:` short code + SVG preview/export UI
//   4. Automated AI spectator match (`Citadels`, custom `8 分`, `快速`) -> GameOverModal -> PostGameStats SVG charts
//
// Usage:
//   pnpm --filter @isle/web build && pnpm --filter @isle/web preview &
//   node web/e2e/scenarios.cjs [screenshot-dir]
//
// Env:
//   E2E_URL (default http://127.0.0.1:4173)
//   E2E_SCENARIO (default Citadels; supports Citadels or FogIslands)
//   E2E_PLAYERS (default 3)

const path = require('path');
const fs = require('fs');

let playwright;
try {
  playwright = require('playwright');
} catch {
  console.error('Playwright is not installed. Run `pnpm add -D playwright` before running web/e2e/scenarios.cjs.');
  process.exit(1);
}

const { chromium } = playwright;
const OUT = process.argv[2] || path.join(__dirname, 'out');
fs.mkdirSync(OUT, { recursive: true });

const BASE_URL = (process.env.E2E_URL || 'http://127.0.0.1:4173').replace(/\/+$/, '');
const TARGET_URL = `${BASE_URL}/?debug&renderer=webgl`;
const SCENARIO = process.env.E2E_SCENARIO || 'Citadels';
const PLAYERS = Number(process.env.E2E_PLAYERS || 3);

(async () => {
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1,
    acceptDownloads: true,
  });
  const page = await context.newPage();

  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  const expectedHealth404 = (m) =>
    m.text().startsWith('Failed to load resource') && /\/api\/health$/.test(m.location().url ?? '');
  page.on('console', (m) => {
    if (m.type() === 'error' && !expectedHealth404(m)) {
      errors.push(`console: ${m.text()}`);
    }
  });

  try {
    await page.goto(TARGET_URL);
    await page.waitForSelector('text=开始游戏', { timeout: 60_000 });
    await page.waitForFunction(() => !document.querySelector('.lobby button.primary')?.disabled, null, {
      timeout: 60_000,
    });

    // ---- 1. Rules Quick-Reference modal (`规则`) ----
    const rulesBtn = page.locator('button:has-text("规则")').first();
    await rulesBtn.waitFor({ state: 'visible', timeout: 10_000 });
    await rulesBtn.click();

    const modal = page.locator('.modal[role="dialog"]');
    await modal.waitFor({ state: 'visible', timeout: 8_000 });
    await modal.locator('h2:has-text("规则速查与建造花费")').waitFor({ state: 'visible', timeout: 5_000 });

    for (const [tabLabel, keyword] of [
      ['航海 · 群岛', '登岛'],
      ['航海 · 迷雾', '迷雾'],
      ['城邦与铁卫', '黑帆'],
      ['经典', '强盗'],
    ]) {
      await modal.locator(`button:has-text("${tabLabel}")`).first().click();
      await page.waitForFunction(
        (kw) => (document.querySelector('.modal')?.textContent ?? '').includes(kw),
        keyword,
        { timeout: 5_000 },
      );
    }
    await page.screenshot({ path: path.join(OUT, 'v03-rules-modal.png') });
    await modal.locator('button[aria-label="关闭"]').click();
    await modal.waitFor({ state: 'hidden', timeout: 5_000 });
    console.log('1/4 rules modal & scenario tabs ok');

    // ---- 2. Sound Mute toggle (`音效` / `静音`) ----
    const soundBtn = page
      .locator('button:has-text("音效"), button:has-text("静音"), button[aria-label*="音效"], button[aria-label*="静音"]')
      .first();
    await soundBtn.waitFor({ state: 'visible', timeout: 8_000 });
    const beforeSound = await soundBtn.evaluate((el) => ({
      text: (el.textContent ?? '').trim(),
      aria: el.getAttribute('aria-label') ?? '',
      pressed: el.getAttribute('aria-pressed') ?? '',
      cls: el.className,
    }));
    await soundBtn.click();
    await page.waitForTimeout(150);
    const afterSound = await soundBtn.evaluate((el) => ({
      text: (el.textContent ?? '').trim(),
      aria: el.getAttribute('aria-label') ?? '',
      pressed: el.getAttribute('aria-pressed') ?? '',
      cls: el.className,
    }));
    if (JSON.stringify(beforeSound) === JSON.stringify(afterSound)) {
      errors.push(`sound button state did not change after click: ${JSON.stringify(beforeSound)}`);
    }
    // Keep muted during fast spectator run or toggle once more cleanly to verify round-trip:
    await soundBtn.click();
    await page.waitForTimeout(100);
    await soundBtn.click(); // leave muted for headless run
    console.log('2/4 sound mute toggle ok');

    // ---- 3. DIY Map Workshop (`地图工坊`) ----
    const workshopBtn = page.locator('button:has-text("地图工坊")').first();
    await workshopBtn.waitFor({ state: 'visible', timeout: 10_000 });
    await workshopBtn.click();
    await modal.waitFor({ state: 'visible', timeout: 8_000 });

    const balanceBtn = modal.locator('button:has-text("一键均衡 6/8")').first();
    await balanceBtn.waitFor({ state: 'visible', timeout: 5_000 });
    await balanceBtn.click();

    const mapCode = await page.waitForFunction(
      () => {
        const root = document.querySelector('.modal');
        if (!root) return null;
        for (const el of root.querySelectorAll('input, textarea, code, pre')) {
          const val = ('value' in el ? el.value : '') || el.textContent || '';
          const m = val.match(/IM1:[A-Za-z0-9+/=_-]+/);
          if (m) return m[0];
        }
        const m = (root.textContent || '').match(/IM1:[A-Za-z0-9+/=_-]+/);
        return m ? m[0] : null;
      },
      null,
      { timeout: 8_000 },
    );
    const codeValue = await mapCode.jsonValue();
    if (!codeValue || !String(codeValue).startsWith('IM1:')) {
      errors.push(`expected IM1: map code in workshop, got ${JSON.stringify(codeValue)}`);
    }

    // Verify SVG board preview and/or SVG export UI inside Map Workshop.
    const hasWorkshopSvgUi = await modal.evaluate((root) => {
      const hasSvg = !!root.querySelector('svg');
      const hasExportControl = [...root.querySelectorAll('button, a')].some((el) =>
        /SVG|导出/.test(el.textContent || el.getAttribute('aria-label') || ''),
      );
      return { hasSvg, hasExportControl };
    });
    if (!hasWorkshopSvgUi.hasSvg && !hasWorkshopSvgUi.hasExportControl) {
      errors.push(`expected SVG preview or SVG export UI in workshop: ${JSON.stringify(hasWorkshopSvgUi)}`);
    }
    await page.screenshot({ path: path.join(OUT, 'v03-map-workshop.png') });

    // Close workshop cleanly so scenario selection in Step 4 is unrestricted.
    const closeWorkshop = modal.locator('button[aria-label="关闭"]').first();
    if (await closeWorkshop.count()) {
      await closeWorkshop.click();
    } else {
      await page.keyboard.press('Escape');
    }
    await modal.waitFor({ state: 'hidden', timeout: 5_000 });
    console.log(`3/4 map workshop ok (${codeValue})`);

    // ---- 4. Automated AI spectator match -> GameOverModal -> PostGameStats ----
    const scenarioLabel = SCENARIO === 'FogIslands' ? '航海 · 迷雾' : '城邦与铁卫';
    await page.locator(`.seg button:has-text("${scenarioLabel}")`).first().click();
    await page.locator(`.seg button:has-text("${PLAYERS} 人")`).first().click();

    // Open DIY house rules (<details class="house-rules">) if collapsed and set victoryPoints = 8.
    const houseDetails = page.locator('details.house-rules');
    if (await houseDetails.count()) {
      const isOpen = await houseDetails.evaluate((el) => el.hasAttribute('open'));
      if (!isOpen) {
        await houseDetails.locator('summary').click();
      }
    }
    await page.locator('button:has-text("8 分")').first().click();
    await page.locator('.seg button:has-text("快速")').first().click();

    const spectateBtn = page
      .locator('button:has-text("观战（全 AI）"), button:has-text("观战 AI 对局"), .lobby-actions button:has-text("观战")')
      .first();
    await spectateBtn.click();

    // Wait for match completion & GameOverModal.
    await page.waitForFunction(
      () => {
        const g = window.__isle?.game?.();
        return g != null && g.view().winner !== null;
      },
      null,
      { timeout: 240_000 },
    );
    await modal.waitFor({ state: 'visible', timeout: 15_000 });

    // Expand `数据复盘` if PostGameStats is behind a toggle button.
    const statsPanel = modal.locator('[data-testid="post-game-stats"], .post-game-stats');
    if (!(await statsPanel.isVisible().catch(() => false))) {
      const statsBtn = modal.locator('button:has-text("数据复盘")').first();
      await statsBtn.waitFor({ state: 'visible', timeout: 8_000 });
      await statsBtn.click();
    }
    await statsPanel.waitFor({ state: 'visible', timeout: 10_000 });

    // Tab 1: 骰子分布
    await statsPanel.locator('button:has-text("骰子分布")').click();
    await statsPanel.locator('svg[viewBox="0 0 360 150"]').waitFor({ state: 'visible', timeout: 5_000 });
    const diceBarCount = await statsPanel.locator('svg[viewBox="0 0 360 150"] rect').count();
    const diceText = await statsPanel.textContent();
    if (diceBarCount < 11 || !diceText?.includes('共掷骰') || !diceText?.includes('7点出现')) {
      errors.push(`dice chart incomplete: bars=${diceBarCount}, text=${diceText}`);
    }
    await page.screenshot({ path: path.join(OUT, 'v03-stats-dice.png') });

    // Tab 2: 胜分走势
    await statsPanel.locator('button:has-text("胜分走势")').click();
    await statsPanel.locator('svg[viewBox="0 0 360 155"]').waitFor({ state: 'visible', timeout: 5_000 });
    const vpLineCount = await statsPanel.locator('svg[viewBox="0 0 360 155"] polyline').count();
    if (vpLineCount < PLAYERS) {
      errors.push(`expected >= ${PLAYERS} VP polylines, got ${vpLineCount}`);
    }
    await page.screenshot({ path: path.join(OUT, 'v03-stats-vp.png') });

    // Tab 3: 经济明细
    await statsPanel.locator('button:has-text("经济明细")').click();
    const econRows = await statsPanel.locator('table tbody tr').count();
    const econText = await statsPanel.textContent();
    if (econRows !== PLAYERS || !econText?.includes('掠夺/被抢') || !econText?.includes('爆牌弃置')) {
      errors.push(`economy table incomplete: rows=${econRows}, expected=${PLAYERS}`);
    }
    await page.screenshot({ path: path.join(OUT, 'v03-stats-econ.png') });

    console.log(`4/4 spectator match & PostGameStats (${diceBarCount} bars, ${vpLineCount} VP lines, ${econRows} econ rows) ok`);
  } catch (err) {
    await page.screenshot({ path: path.join(OUT, 'v03-scenarios-error.png') }).catch(() => {});
    throw err;
  } finally {
    await browser.close();
  }

  if (errors.length > 0) {
    console.error('Regression errors:', errors);
    process.exit(1);
  }
  console.log('All v0.3 scenario regression checks passed with 0 errors.');
})().catch((err) => {
  console.error(err);
  process.exit(1);
});
