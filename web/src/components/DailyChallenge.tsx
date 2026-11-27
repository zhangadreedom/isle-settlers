import { createEffect, createMemo, createSignal, For, onCleanup, Show, type JSX } from 'solid-js';
import type { Game } from '../game/controller';
import {
  computeSetupAdvice,
  isAdvisorEnabled,
  onAdvisorChange,
  toggleAdvisor,
  type SetupAdvice,
} from '../game/advisor';
import {
  formatDailyShareText,
  getDailyChallenge,
  getDailyRecord,
  scenarioDisplayName,
  type DailyChallengeSpec,
} from '../game/daily';
import { Modal } from './bits';

function copyTextSafe(text: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    return navigator.clipboard
      .writeText(text)
      .then(() => true)
      .catch(() => fallbackExecCopy(text));
  }
  return Promise.resolve(fallbackExecCopy(text));
}

function fallbackExecCopy(text: string): boolean {
  try {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    ta.style.pointerEvents = 'none';
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand('copy');
    ta.remove();
    return ok;
  } catch {
    return false;
  }
}

export function DailyChallengeModal(props: {
  embedded?: boolean;
  onClose: () => void;
  onStartDaily: (spec: DailyChallengeSpec) => void;
}) {
  const spec = createMemo(() => getDailyChallenge());
  const record = createMemo(() => getDailyRecord(spec().dateKey));
  const [copied, setCopied] = createSignal(false);
  let copyTimer: number | undefined;

  onCleanup(() => {
    window.clearTimeout(copyTimer);
  });

  const handleCopyShare = () => {
    const rec = record();
    if (!rec) return;
    const card = formatDailyShareText(spec(), rec);
    void copyTextSafe(card).then((ok) => {
      if (!ok) return;
      setCopied(true);
      window.clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => setCopied(false), 1800);
    });
  };

  return (
    <Modal title="每日同图挑战赛" onClose={props.onClose}>
      <div
        style={{
          display: 'flex',
          'flex-direction': 'column',
          gap: '10px',
          padding: '12px 14px',
          'border-radius': '10px',
          background: 'linear-gradient(145deg, rgba(240, 180, 41, 0.14), rgba(255, 255, 255, 0.04))',
          border: '1px solid rgba(240, 180, 41, 0.32)',
        }}
      >
        <div class="row" style={{ 'justify-content': 'space-between', 'align-items': 'center' }}>
          <span class="mode-tag ranked" style={{ 'font-weight': 600 }}>
            📅 {spec().dateKey}
          </span>
          <span class="seed" style={{ 'font-size': '13px', color: 'var(--accent)' }}>
            种子 #{spec().seed}
          </span>
        </div>

        <div>
          <div style={{ 'font-size': '18px', 'font-weight': 700, 'letter-spacing': '0.02em' }}>
            {spec().title}
          </div>
          <p class="muted small" style={{ 'margin-top': '4px' }}>
            {spec().subtitle}
          </p>
        </div>

        <div class="row" style={{ gap: '6px', 'margin-top': '2px' }}>
          <span class="badge">剧本 · {scenarioDisplayName(spec().scenario)}</span>
          <span class="badge">{spec().players} 人同图</span>
          <span class="badge">
            AI · {spec().bot === 'mcts' ? '困难 MCTS' : '普通贪心'}
          </span>
          <Show when={spec().friendlyRobber}>
            <span class="badge">友好强盗</span>
          </Show>
          <span class="badge">{spec().victoryPoints} 分获胜</span>
        </div>
      </div>

      <Show
        when={record()}
        fallback={
          <div class="profile-card" style={{ gap: '4px' }}>
            <div style={{ 'font-weight': 600 }}>今日战绩 · 尚未出征</div>
            <p class="muted small">
              所有玩家在今日共享完全相同的岛屿地形、数字圆片与起始港口。
              {props.embedded ? '本局记录在当前会话内即时可分享。' : '通关后将自动记录最佳回合数！'}
            </p>
          </div>
        }
      >
        {(rec) => (
          <div
            class="profile-card"
            style={{
              border: rec().won
                ? '1px solid rgba(70, 167, 88, 0.5)'
                : '1px solid rgba(255, 255, 255, 0.12)',
              background: rec().won ? 'rgba(70, 167, 88, 0.12)' : 'rgba(255, 255, 255, 0.05)',
            }}
          >
            <div class="row" style={{ 'justify-content': 'space-between' }}>
              <span style={{ 'font-weight': 700, color: rec().won ? '#6ee787' : 'var(--text)' }}>
                {rec().won ? '🏆 今日已获胜！' : '⚔️ 今日最佳记录'}
              </span>
              <span class="muted small">已挑战 {rec().attempts} 次</span>
            </div>
            <div class="row" style={{ gap: '14px', 'font-size': '13px' }}>
              <span>
                最佳用时：<b>{rec().bestTurns !== null ? `${rec().bestTurns} 回合` : '待通关'}</b>
              </span>
              <span>
                最高得分：<b>{rec().points} 分</b>
              </span>
              <span class="muted small">最近一局 {rec().turns} 回合</span>
            </div>
            <pre
              style={{
                margin: '4px 0 0',
                padding: '8px 10px',
                'border-radius': '8px',
                background: 'rgba(0, 0, 0, 0.28)',
                'font-family': 'inherit',
                'font-size': '12px',
                'white-space': 'pre-wrap',
                color: 'var(--muted)',
              }}
            >
              {formatDailyShareText(spec(), rec())}
            </pre>
          </div>
        )}
      </Show>

      <footer>
        <button
          disabled={!record()}
          title={record() ? '复制无剧透战绩分享文本' : '完成一场今日挑战后可复制分享卡'}
          onClick={handleCopyShare}
        >
          {copied() ? '已复制！' : '复制战绩分享卡'}
        </button>
        <button class="primary" onClick={() => props.onStartDaily(spec())}>
          开始今日挑战
        </button>
      </footer>
    </Modal>
  );
}

const TIER_STYLE: Record<SetupAdvice['tier'], { bg: string; fg: string; border: string; glow: string }> = {
  S: {
    bg: 'linear-gradient(135deg, #f7c948, #e09f18)',
    fg: '#1b1608',
    border: '#ffe28a',
    glow: '0 3px 12px rgba(240, 180, 41, 0.55)',
  },
  A: {
    bg: 'linear-gradient(135deg, #38bdf8, #1d8ab8)',
    fg: '#061824',
    border: '#93e2ff',
    glow: '0 3px 10px rgba(56, 189, 248, 0.4)',
  },
  B: {
    bg: 'linear-gradient(135deg, #94a3b8, #64748b)',
    fg: '#0f172a',
    border: '#cbd5e1',
    glow: '0 2px 8px rgba(15, 23, 42, 0.45)',
  },
};

export function SetupAdvisorOverlay(props: {
  game: Game;
  project: (kind: 'vertex' | 'edge' | 'tile', id: number) => { x: number; y: number };
}) {
  const [enabled, setEnabled] = createSignal(isAdvisorEnabled());
  const [coords, setCoords] = createSignal<Record<number, { x: number; y: number }>>({});

  onCleanup(onAdvisorChange((on) => setEnabled(on)));

  const active = createMemo(() => {
    const g = props.game;
    const v = g.view();
    return (
      Boolean(v) &&
      !g.online &&
      !g.replay &&
      v.phase.type === 'SetupSettlement' &&
      g.me !== null &&
      v.toMove === g.me
    );
  });

  const legalVertices = createMemo(() => {
    if (!active()) return [];
    return props.game
      .legal()
      .actions.flatMap((a) => (a.type === 'BuildSettlement' ? [a.value] : []));
  });

  const advice = createMemo<SetupAdvice[]>(() => {
    if (!active() || !enabled()) return [];
    const verts = legalVertices();
    if (!verts.length) return [];
    return computeSetupAdvice(
      props.game.board,
      props.game.topology,
      props.game.view(),
      verts,
      props.game.config.scenario,
    );
  });

  // Project top-3 3D vertices into screen space via requestAnimationFrame on camera/window events.
  createEffect(() => {
    const list = advice();
    if (!active() || !enabled() || !list.length) {
      setCoords({});
      return;
    }

    let rafId = 0;
    let settleUntil = 0;

    const updatePositions = () => {
      rafId = 0;
      const next: Record<number, { x: number; y: number }> = {};
      for (const item of list) {
        try {
          const p = props.project('vertex', item.vertex);
          if (Number.isFinite(p.x) && Number.isFinite(p.y)) {
            next[item.vertex] = p;
          }
        } catch {
          // Ignore transient projection calls before renderer layout settles.
        }
      }
      setCoords(next);
      if (performance.now() < settleUntil) {
        rafId = requestAnimationFrame(updatePositions);
      }
    };

    const scheduleSync = (dampingMs = 0) => {
      if (dampingMs > 0) {
        settleUntil = Math.max(settleUntil, performance.now() + dampingMs);
      }
      if (!rafId) {
        rafId = requestAnimationFrame(updatePositions);
      }
    };

    // Initial sync + short window to catch ResizeObserver `setInsets` camera fit.
    scheduleSync(650);

    const onResize = () => scheduleSync(450);
    const onWheel = () => scheduleSync(650);
    const onPointerMove = (ev: PointerEvent) => scheduleSync(ev.buttons !== 0 ? 550 : 0);
    const onTouchMove = () => scheduleSync(650);

    window.addEventListener('resize', onResize, { passive: true });
    window.addEventListener('wheel', onWheel, { passive: true });
    window.addEventListener('pointermove', onPointerMove, { passive: true });
    window.addEventListener('touchmove', onTouchMove, { passive: true });

    onCleanup(() => {
      if (rafId) cancelAnimationFrame(rafId);
      window.removeEventListener('resize', onResize);
      window.removeEventListener('wheel', onWheel);
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('touchmove', onTouchMove);
    });
  });

  const panelWrapStyle = (): JSX.CSSProperties => ({
    position: 'absolute',
    top: '12px',
    left: '50%',
    transform: 'translateX(-50%)',
    display: 'flex',
    'flex-direction': 'column',
    'align-items': 'center',
    gap: '6px',
    'z-index': 18,
    'pointer-events': 'none',
    'max-width': 'calc(100vw - 24px)',
  });

  return (
    <Show when={active()}>
      <>
        {/* Floating non-blocking 3D vertex rank/tier pins */}
        <Show when={enabled()}>
          <div
            aria-hidden="true"
            style={{
              position: 'fixed',
              inset: 0,
              'pointer-events': 'none',
              overflow: 'hidden',
              'z-index': 16,
            }}
          >
            <For each={advice()}>
              {(item) => {
                const pos = () => coords()[item.vertex];
                const theme = TIER_STYLE[item.tier];
                return (
                  <Show when={pos()}>
                    {(p) => (
                      <div
                        style={{
                          position: 'absolute',
                          left: `${p().x}px`,
                          top: `${p().y}px`,
                          transform: 'translate(-50%, calc(-100% - 11px))',
                          'pointer-events': 'none',
                          display: 'flex',
                          'flex-direction': 'column',
                          'align-items': 'center',
                          'user-select': 'none',
                        }}
                      >
                        <div
                          style={{
                            display: 'inline-flex',
                            'align-items': 'center',
                            gap: '4px',
                            padding: '2px 8px',
                            'border-radius': '999px',
                            background: theme.bg,
                            color: theme.fg,
                            border: `1.5px solid ${theme.border}`,
                            'box-shadow': theme.glow,
                            'font-size': '11.5px',
                            'font-weight': 800,
                            'line-height': '16px',
                            'white-space': 'nowrap',
                          }}
                        >
                          <span>#{item.rank}</span>
                          <span>{item.tier}</span>
                        </div>
                        <span
                          style={{
                            width: 0,
                            height: 0,
                            'border-left': '5px solid transparent',
                            'border-right': '5px solid transparent',
                            'border-top': `6px solid ${theme.border}`,
                          }}
                        />
                      </div>
                    )}
                  </Show>
                );
              }}
            </For>
          </div>
        </Show>

        {/* Top-center toggle pill + compact 3-item summary legend card */}
        <div style={panelWrapStyle()}>
          <button
            type="button"
            classList={{ on: enabled() }}
            onClick={() => toggleAdvisor()}
            title="开启或关闭开局落村 AI 选点推荐"
            style={{
              'pointer-events': 'auto',
              'border-radius': '999px',
              padding: '5px 13px',
              'font-size': '12.5px',
              'font-weight': 600,
              background: enabled() ? 'rgba(240, 180, 41, 0.22)' : 'var(--panel-solid)',
              'backdrop-filter': 'blur(8px)',
              'box-shadow': '0 4px 14px rgba(0, 0, 0, 0.3)',
            }}
          >
            🧭 选点参谋{enabled() ? ' · 已开启' : ''}
          </button>

          <Show when={enabled() && advice().length > 0}>
            <div
              class="panel"
              role="region"
              aria-label="选点参谋推荐"
              style={{
                padding: '8px 10px',
                display: 'flex',
                'flex-direction': 'column',
                gap: '5px',
                width: 'min(360px, calc(100vw - 24px))',
                'pointer-events': 'none',
                'box-shadow': '0 8px 24px rgba(0, 0, 0, 0.35)',
              }}
            >
              <For each={advice()}>
                {(item) => {
                  const theme = TIER_STYLE[item.tier];
                  return (
                    <div
                      style={{
                        display: 'flex',
                        'flex-wrap': 'wrap',
                        'align-items': 'center',
                        gap: '6px',
                        'font-size': '12px',
                      }}
                    >
                      <span
                        style={{
                          display: 'inline-flex',
                          'align-items': 'center',
                          gap: '3px',
                          padding: '1px 6px',
                          'border-radius': '999px',
                          background: theme.bg,
                          color: theme.fg,
                          'font-weight': 800,
                          'font-size': '11px',
                          'line-height': '15px',
                        }}
                      >
                        #{item.rank} {item.tier}
                      </span>
                      <span style={{ 'font-weight': 600 }}>{item.summary}</span>
                      <span
                        style={{
                          'margin-left': 'auto',
                          display: 'inline-flex',
                          gap: '4px',
                          'flex-wrap': 'wrap',
                        }}
                      >
                        <For each={item.tags.slice(0, 2)}>
                          {(tag) => (
                            <span
                              class="badge"
                              style={{ 'font-size': '10.5px', padding: '0 6px', 'line-height': '16px' }}
                            >
                              {tag}
                            </span>
                          )}
                        </For>
                      </span>
                    </div>
                  );
                }}
              </For>
            </div>
          </Show>
        </div>
      </>
    </Show>
  );
}
