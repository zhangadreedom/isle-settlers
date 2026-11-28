import { createEffect, createMemo, createSignal, For, Show, type JSX } from 'solid-js';
import { CITY_FLAG, EMPTY } from '../engine';
import type { Game } from '../game/controller';

/** Curated 3-minute beginner tutorial match preset (`humanSeat = 0`, `friendlyRobber = true`, `eventDeck = true`). */
export const TUTORIAL_PRESET = {
  players: 3,
  seed: 20261128,
  scenario: 'Classic' as const,
  victoryPoints: 8,
};

interface StepSpec {
  step: 1 | 2 | 3 | 4 | 5 | 6;
  title: string;
  body: string;
  tip: string;
}

const STEPS: readonly StepSpec[] = [
  {
    step: 1,
    title: '第 1/6 步 · 建立首座村庄',
    body: '点击棋盘上发光的交叉点建立你的第一座村庄。优先挑选同时毗邻砖（丘陵）、木（森林）与麦/矿的 6、8、5、9 高产交叉点！',
    tip: '红字 6 与 8 出现概率最高（各 5/36），三块地形交汇处每轮都能稳拿资源。',
  },
  {
    step: 2,
    title: '第 2/6 步 · 铺设起始道路',
    body: '点击刚才村庄旁发光的边，铺设你的第一条起始道路。后续新村庄必须沿着己方道路向外延伸（且与任意建筑至少间隔 2 条边）。',
    tip: '朝内陆高产点或沿海 3:1 / 2:1 港口方向铺路，能为扩张抢占先机。',
  },
  {
    step: 3,
    title: '第 3/6 步 · 二轮落村拿起始牌',
    body: '开局采用「1→2→3→3→2→1」蛇形顺位！轮到你放完第二座村庄与相连道路后，将立即获得第二座村庄相邻地形各 1 张起始资源手牌！',
    tip: '第二座村庄尽量补齐首村缺少的资源（例如首村主产砖木，二村补齐麦矿羊）。',
  },
  {
    step: 4,
    title: '第 4/6 步 · 掷骰与产出',
    body: '轮到你的准备阶段（PreRoll）时，点击右下角【掷骰子】（或直接按空格键）。全桌所有数字等于骰子之和的地块都会自动向相邻村庄飞牌产出！',
    tip: '任何人的回合掷出对应点数，只要你有相邻村庄（1 张）或城市（2 张）都能拿牌！',
  },
  {
    step: 5,
    title: '第 5/6 步 · 贸易与修路扩张',
    body: '主阶段请看底部手牌栏！手牌凑不齐时点击【交易】（可用 4:1 银行、沿海 3:1/2:1 港口换所缺资源，或向 AI 报价），再点击【道路】或【村庄】扩张版图。',
    tip: '修路消耗 1砖+1木，建村消耗 1砖+1木+1羊+1麦（每座村庄价值 1 分）。',
  },
  {
    step: 6,
    title: '第 6/6 步 · 升级城市冲刺 8 分获胜',
    body: '村庄计 1 分，消耗 2麦+3矿点击【城市】升级计 2 分且每次双倍产出！连成 ≥5 条路夺【最长道路 +2 分】，打出 ≥3 张骑士夺【最大骑士 +2 分】，先到 8 分即获胜！',
    tip: '本教学局已开启友好强盗与 36 张均衡点数牌组，祝你旗开得胜！',
  },
];

export function TutorialCoach(props: { game: Game; onExitTutorial: () => void }) {
  const [minStep, setMinStep] = createSignal(1);
  const [manualStep, setManualStep] = createSignal<number | null>(null);

  const progress = createMemo(() => {
    const v = props.game.view();
    const entries = props.game.log();
    if (!v) {
      return {
        detected: 1,
        myTurn: false,
        phaseType: 'SetupSettlement',
        hasTraded: false,
        vp: 0,
        targetVp: props.game.config.victoryPoints || TUTORIAL_PRESET.victoryPoints,
      };
    }

    const mySeat = props.game.me ?? 0;
    let myBuildings = 0;
    let myCities = 0;
    for (let i = 0; i < v.buildings.length; i++) {
      const b = v.buildings[i];
      if (b !== undefined && b !== EMPTY && (b & ~CITY_FLAG) === mySeat) {
        myBuildings++;
        if ((b & CITY_FLAG) !== 0) myCities++;
      }
    }

    let myRoads = 0;
    for (let i = 0; i < v.roads.length; i++) {
      if (v.roads[i] === mySeat) myRoads++;
    }
    for (let i = 0; i < v.ships.length; i++) {
      if (v.ships[i] === mySeat) myRoads++;
    }

    const pub = v.players[mySeat];
    const devTotal =
      (pub?.devCardCount ?? 0) + (pub ? pub.playedDevCards.reduce((a, b) => a + b, 0) : 0);

    const hasRolled =
      entries.some((e) => e.seat === mySeat && e.text.includes('掷出')) ||
      (v.current === mySeat && v.phase.type !== 'SetupSettlement' && v.phase.type !== 'SetupRoad' && v.phase.type !== 'PreRoll') ||
      v.turn > v.players.length;

    const hasTraded = entries.some(
      (e) =>
        e.seat === mySeat &&
        (e.text.includes('与银行交易') || e.text.includes('完成交易') || e.text.includes('发起交易')),
    );

    const hasExpanded = myRoads > 2 || myBuildings > 2 || myCities > 0 || devTotal > 0;

    let detected = 1;
    if (myBuildings === 0) {
      detected = 1;
    } else if (myBuildings === 1 && myRoads === 0) {
      detected = 2;
    } else if (myBuildings < 2 || myRoads < 2) {
      detected = 3;
    } else if (!hasRolled) {
      detected = 4;
    } else if (!hasExpanded) {
      detected = 5;
    } else {
      detected = 6;
    }

    const vp = v.me?.victoryPoints ?? pub?.publicVictoryPoints ?? 0;
    return {
      detected,
      myTurn: v.toMove === mySeat,
      phaseType: v.phase.type,
      hasTraded,
      vp,
      targetVp: props.game.config.victoryPoints || TUTORIAL_PRESET.victoryPoints,
    };
  });

  // Monotonically advance `minStep` and return to live tracking whenever a new game milestone is reached.
  let prevDetected = 1;
  createEffect(() => {
    const d = progress().detected;
    if (d > prevDetected) {
      prevDetected = d;
      setMinStep((prev) => Math.max(prev, d));
      setManualStep(null);
    }
  });

  const currentStep = createMemo(() => {
    const m = manualStep();
    if (m !== null) return m;
    return Math.min(6, Math.max(minStep(), progress().detected));
  });

  const spec = createMemo(() => STEPS[currentStep() - 1] ?? STEPS[0]);

  const liveStatus = createMemo<string | null>(() => {
    const p = progress();
    const s = currentStep();
    if (s === 3 && !p.myTurn) return '⏳ AI 正在按蛇形顺位落子，稍候即轮到你连放第 2 村与道路…';
    if (s === 3 && p.myTurn && p.phaseType === 'SetupRoad') return '✨ 第 2 座村庄已就位！再点选相邻发光边铺路即可领取起始资源牌。';
    if (s === 4 && !p.myTurn) return '⏳ 等待前序玩家回合结束，轮到你时点击【掷骰子】或按空格键。';
    if (s === 5 && p.hasTraded) return '✓ 已体验交易！现在可点击底部【道路】或【村庄】继续扩张版图。';
    if (s === 6) return `🏆 当前得分：${p.vp} / ${p.targetVp} 分`;
    return null;
  });

  const skipStep = () => {
    const cur = currentStep();
    if (cur >= 6) {
      props.onExitTutorial();
      return;
    }
    const next = cur + 1;
    setManualStep(null);
    setMinStep((prev) => Math.max(prev, next));
  };

  const topOffset = createMemo(() => {
    if (props.game.dice()) return '76px';
    if (progress().phaseType === 'SetupSettlement') return '48px';
    return '14px';
  });

  const cardStyle = (): JSX.CSSProperties => ({
    position: 'absolute',
    top: topOffset(),
    left: '50%',
    transform: 'translateX(-50%)',
    'z-index': 19,
    width: 'min(500px, calc(100vw - 24px))',
    padding: '11px 14px',
    display: 'flex',
    'flex-direction': 'column',
    gap: '8px',
    border: '1px solid rgba(240, 180, 41, 0.45)',
    background: 'rgba(8, 24, 34, 0.92)',
    'box-shadow': '0 10px 28px rgba(0, 0, 0, 0.45)',
    transition: 'top 0.2s ease',
    'pointer-events': 'auto',
  });

  return (
    <Show when={props.game.view()?.winner === null}>
      <section class="panel" style={cardStyle()} role="region" aria-label="新手互动教学引导">
        <div class="row" style={{ 'justify-content': 'space-between', 'align-items': 'center', gap: '8px' }}>
          <div class="row" style={{ gap: '8px', 'align-items': 'center' }}>
            <span
              class="badge"
              style={{
                background: 'rgba(240, 180, 41, 0.24)',
                color: 'var(--accent)',
                'font-weight': 700,
                padding: '2px 8px',
              }}
            >
              🎓 新手引导
            </span>
            <strong style={{ 'font-size': '14px', color: 'var(--text)' }}>{spec().title}</strong>
          </div>

          <div class="row" style={{ gap: '5px', 'align-items': 'center' }} role="tablist" aria-label="教学步骤进度">
            <For each={[1, 2, 3, 4, 5, 6] as const}>
              {(n) => {
                const active = () => currentStep() === n;
                const done = () => n < Math.max(minStep(), progress().detected);
                return (
                  <button
                    type="button"
                    aria-label={`第 ${n} 步`}
                    aria-current={active() ? 'step' : undefined}
                    title={`查看第 ${n}/6 步`}
                    onClick={() => setManualStep(n)}
                    style={{
                      width: active() ? '20px' : '9px',
                      height: '9px',
                      padding: 0,
                      'border-radius': '999px',
                      border: 'none',
                      background: active()
                        ? 'var(--accent)'
                        : done()
                          ? 'rgba(70, 167, 88, 0.85)'
                          : 'rgba(255, 255, 255, 0.22)',
                      transition: 'width 0.18s ease, background 0.18s ease',
                    }}
                  />
                );
              }}
            </For>
          </div>
        </div>

        <p style={{ margin: 0, 'font-size': '13px', 'line-height': 1.48, color: 'var(--text)' }}>{spec().body}</p>

        <div class="muted small" style={{ 'line-height': 1.4 }}>
          💡 {spec().tip}
        </div>

        <Show when={liveStatus()}>
          {(msg) => (
            <div
              class="small"
              style={{
                padding: '4px 9px',
                'border-radius': '7px',
                background: 'rgba(240, 180, 41, 0.12)',
                color: '#f9d56e',
                'font-weight': 600,
              }}
            >
              {msg()}
            </div>
          )}
        </Show>

        <div class="row" style={{ 'justify-content': 'flex-end', gap: '8px', 'margin-top': '2px' }}>
          <Show
            when={currentStep() < 6}
            fallback={
              <>
                <button type="button" class="ghost small" onClick={props.onExitTutorial}>
                  退出教学
                </button>
                <button type="button" class="primary small" onClick={props.onExitTutorial}>
                  已掌握，关闭引导继续畅玩
                </button>
              </>
            }
          >
            <button type="button" class="ghost small" onClick={props.onExitTutorial}>
              退出教学
            </button>
            <button type="button" class="small" onClick={skipStep}>
              跳过本步
            </button>
          </Show>
        </div>
      </section>
    </Show>
  );
}
