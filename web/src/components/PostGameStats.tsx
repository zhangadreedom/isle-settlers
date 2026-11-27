import { createMemo, createSignal, For, Show } from 'solid-js';
import { PLAYER_CSS } from '../board/palette';
import { analyzeReplayRecord } from '../game/stats';
import type { ReplayRecord } from '../generated/proto';
import { SeatDot } from './bits';

const DICE_SUMS = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] as const;

function diceBarColor(sum: number): string {
  if (sum === 6 || sum === 8) return '#d94841';
  if (sum === 7) return '#c98a2c';
  return '#2f9aa8';
}

export function PostGameStats(props: { record: ReplayRecord; name: (seat: number) => string }) {
  const stats = createMemo(() => {
    try {
      return analyzeReplayRecord(props.record);
    } catch {
      return null;
    }
  });
  const [tab, setTab] = createSignal<'dice' | 'vp' | 'econ'>('dice');
  const isCitadels = () => props.record.config.scenario === 'Citadels';

  return (
    <Show when={stats()}>
      {(s) => {
        const diceGeometry = createMemo(() => {
          const st = s();
          const padL = 14;
          const padR = 14;
          const baseY = 124;
          const chartH = 98;
          const slotW = (360 - padL - padR) / DICE_SUMS.length;
          const barW = 19;
          const maxVal = Math.max(
            1,
            ...DICE_SUMS.map((n) => Math.max(st.diceCounts[n], st.expectedCounts[n])),
          );
          const items = DICE_SUMS.map((n, idx) => {
            const cx = padL + (idx + 0.5) * slotW;
            const count = st.diceCounts[n];
            const exp = st.expectedCounts[n];
            const barH = (count / maxVal) * chartH;
            const expY = baseY - (exp / maxVal) * chartH;
            return {
              n,
              cx,
              bx: cx - barW / 2,
              barW,
              count,
              exp,
              barH,
              barY: baseY - barH,
              expY,
            };
          });
          const expectedPolyline = items.map((d) => `${d.cx.toFixed(1)},${d.expY.toFixed(1)}`).join(' ');
          return { padL, padR, baseY, items, expectedPolyline };
        });

        const vpGeometry = createMemo(() => {
          const st = s();
          const padL = 30;
          const padR = 14;
          const topY = 14;
          const baseY = 132;
          const plotW = 360 - padL - padR;
          const plotH = baseY - topY;
          const targetVp = props.record.config.victoryPoints;
          const maxVp = Math.max(
            targetVp,
            4,
            ...st.vpTimeline.flatMap((pt) => pt.points),
          );
          const lastTurn = st.vpTimeline[st.vpTimeline.length - 1]?.turn ?? st.totalTurns;
          const maxTurn = Math.max(1, lastTurn);
          const xOf = (turn: number) => padL + (turn / maxTurn) * plotW;
          const yOf = (vp: number) => baseY - (vp / maxVp) * plotH;
          const midVp = Math.round(maxVp / 2);

          const series = Array.from({ length: props.record.config.numPlayers }, (_, seat) => {
            const pts =
              st.vpTimeline.length === 1
                ? [
                    { x: padL, y: yOf(st.vpTimeline[0].points[seat] ?? 0) },
                    { x: padL + plotW, y: yOf(st.vpTimeline[0].points[seat] ?? 0) },
                  ]
                : st.vpTimeline.map((row) => ({
                    x: xOf(row.turn),
                    y: yOf(row.points[seat] ?? 0),
                  }));
            const lastPt = pts[pts.length - 1] ?? { x: padL, y: baseY };
            const finalScore =
              st.vpTimeline[st.vpTimeline.length - 1]?.points[seat] ?? 0;
            return {
              seat,
              color: PLAYER_CSS[seat] ?? '#eef4f6',
              isWinner: props.record.winner === seat,
              polyline: pts.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' '),
              lastX: lastPt.x,
              lastY: lastPt.y,
              finalScore,
            };
          });

          return {
            padL,
            padR,
            baseY,
            plotW,
            targetVp,
            yTarget: yOf(targetVp),
            midVp,
            yMid: yOf(midVp),
            maxTurn,
            series,
          };
        });

        return (
          <section
            class="post-game-stats rules-box"
            data-testid="post-game-stats"
            style={{ gap: '8px', margin: '4px 0' }}
          >
            <div class="tabs seg small" role="tablist" aria-label="数据复盘页签">
              <button
                type="button"
                role="tab"
                aria-selected={tab() === 'dice'}
                classList={{ on: tab() === 'dice' }}
                onClick={() => setTab('dice')}
              >
                骰子分布
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab() === 'vp'}
                classList={{ on: tab() === 'vp' }}
                onClick={() => setTab('vp')}
              >
                胜分走势
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab() === 'econ'}
                classList={{ on: tab() === 'econ' }}
                onClick={() => setTab('econ')}
              >
                经济明细
              </button>
            </div>

            <Show when={tab() === 'dice'}>
              <div>
                <svg
                  viewBox="0 0 360 150"
                  role="img"
                  aria-label="骰子分布柱状图"
                  style={{ width: '100%', height: 'auto', display: 'block' }}
                >
                  <line
                    x1={diceGeometry().padL}
                    y1={diceGeometry().baseY}
                    x2={360 - diceGeometry().padR}
                    y2={diceGeometry().baseY}
                    stroke="rgba(255,255,255,0.2)"
                    stroke-width="1"
                  />
                  <line
                    x1="278"
                    y1="10"
                    x2="294"
                    y2="10"
                    stroke="rgba(238,244,246,0.75)"
                    stroke-width="1.3"
                    stroke-dasharray="3 2"
                  />
                  <circle cx="286" cy="10" r="2" fill="#eef4f6" />
                  <text x="298" y="13" font-size="8.5" fill="#9fb7c2">
                    理论期望
                  </text>

                  <For each={diceGeometry().items}>
                    {(d) => (
                      <g>
                        <rect
                          x={d.bx}
                          y={d.count > 0 ? Math.min(d.barY, diceGeometry().baseY - 2) : diceGeometry().baseY}
                          width={d.barW}
                          height={d.count > 0 ? Math.max(2, d.barH) : 0}
                          rx="3"
                          fill={diceBarColor(d.n)}
                          opacity="0.9"
                        >
                          <title>{`${d.n} 点：实际 ${d.count} 次 / 理论期望 ${d.exp.toFixed(1)} 次`}</title>
                        </rect>
                        <text
                          x={d.cx}
                          y={Math.min(d.barY - 4, diceGeometry().baseY - 4)}
                          text-anchor="middle"
                          font-size="9.5"
                          fill="#eef4f6"
                        >
                          {d.count}
                        </text>
                        <text
                          x={d.cx}
                          y="140"
                          text-anchor="middle"
                          font-size="10"
                          font-weight={d.n === 6 || d.n === 8 ? '700' : '400'}
                          fill={d.n === 6 || d.n === 8 ? '#ff9b96' : '#9fb7c2'}
                        >
                          {d.n}
                        </text>
                      </g>
                    )}
                  </For>

                  <polyline
                    fill="none"
                    stroke="rgba(238,244,246,0.78)"
                    stroke-width="1.4"
                    stroke-dasharray="3 2.5"
                    points={diceGeometry().expectedPolyline}
                  />
                  <For each={diceGeometry().items}>
                    {(d) => (
                      <circle cx={d.cx} cy={d.expY} r="2.1" fill="#eef4f6" opacity="0.88">
                        <title>{`${d.n} 点理论期望：${d.exp.toFixed(2)} 次`}</title>
                      </circle>
                    )}
                  </For>
                </svg>
                <p class="muted small" style={{ 'margin-top': '4px', 'text-align': 'center' }}>
                  {`共掷骰 ${s().totalRolls} 次 · 7点出现 ${s().sevenCount} 次（理论 ${s().expectedCounts[7].toFixed(1)}）· 最热数字 ${s().hottestRoll ?? '—'}`}
                </p>
              </div>
            </Show>

            <Show when={tab() === 'vp'}>
              <div>
                <svg
                  viewBox="0 0 360 155"
                  role="img"
                  aria-label="胜分走势折线图"
                  style={{ width: '100%', height: 'auto', display: 'block' }}
                >
                  <line
                    x1={vpGeometry().padL}
                    y1={vpGeometry().baseY}
                    x2={360 - vpGeometry().padR}
                    y2={vpGeometry().baseY}
                    stroke="rgba(255,255,255,0.2)"
                    stroke-width="1"
                  />
                  <line
                    x1={vpGeometry().padL}
                    y1={vpGeometry().yMid}
                    x2={360 - vpGeometry().padR}
                    y2={vpGeometry().yMid}
                    stroke="rgba(255,255,255,0.07)"
                    stroke-width="1"
                  />
                  <line
                    x1={vpGeometry().padL}
                    y1={vpGeometry().yTarget}
                    x2={360 - vpGeometry().padR}
                    y2={vpGeometry().yTarget}
                    stroke="#f0b429"
                    stroke-width="1.2"
                    stroke-dasharray="4 3"
                    opacity="0.85"
                  />
                  <text
                    x={vpGeometry().padL - 5}
                    y={vpGeometry().yTarget + 3}
                    text-anchor="end"
                    font-size="9"
                    fill="#f0b429"
                  >
                    {vpGeometry().targetVp}分
                  </text>
                  <text
                    x={vpGeometry().padL - 5}
                    y={vpGeometry().yMid + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#9fb7c2"
                  >
                    {vpGeometry().midVp}
                  </text>
                  <text
                    x={vpGeometry().padL - 5}
                    y={vpGeometry().baseY + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#9fb7c2"
                  >
                    0
                  </text>

                  <text x={vpGeometry().padL} y="148" text-anchor="start" font-size="9" fill="#9fb7c2">
                    T0
                  </text>
                  <text
                    x={vpGeometry().padL + vpGeometry().plotW / 2}
                    y="148"
                    text-anchor="middle"
                    font-size="9"
                    fill="#9fb7c2"
                  >
                    {`T${Math.round(vpGeometry().maxTurn / 2)}`}
                  </text>
                  <text x={360 - vpGeometry().padR} y="148" text-anchor="end" font-size="9" fill="#9fb7c2">
                    {`T${vpGeometry().maxTurn}`}
                  </text>

                  <For each={vpGeometry().series}>
                    {(line) => (
                      <g>
                        <polyline
                          fill="none"
                          stroke={line.color}
                          stroke-width={line.isWinner ? '2.4' : '1.7'}
                          stroke-linejoin="round"
                          stroke-linecap="round"
                          points={line.polyline}
                        />
                        <circle
                          cx={line.lastX}
                          cy={line.lastY}
                          r={line.isWinner ? '3.2' : '2.3'}
                          fill={line.color}
                        >
                          <title>{`${props.name(line.seat)}：${line.finalScore} 分`}</title>
                        </circle>
                      </g>
                    )}
                  </For>
                </svg>

                <div
                  class="row small"
                  style={{ gap: '6px 12px', 'justify-content': 'center', 'margin-top': '4px' }}
                >
                  <For each={vpGeometry().series}>
                    {(line) => (
                      <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px' }}>
                        <SeatDot seat={line.seat} />
                        <span>{props.name(line.seat)}</span>
                        <span class="muted">{line.finalScore}分</span>
                      </span>
                    )}
                  </For>
                </div>
              </div>
            </Show>

            <Show when={tab() === 'econ'}>
              <div style={{ 'overflow-x': 'auto' }}>
                <table
                  class="stats-table"
                  style={{
                    width: '100%',
                    'border-collapse': 'collapse',
                    'font-size': '12.5px',
                    'font-variant-numeric': 'tabular-nums',
                  }}
                >
                  <thead>
                    <tr class="muted">
                      <th style={{ 'text-align': 'left', 'font-weight': '500', padding: '4px 6px' }}>玩家</th>
                      <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>产出</th>
                      <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>交易</th>
                      <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>掠夺/被抢</th>
                      <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>爆牌弃置</th>
                    </tr>
                  </thead>
                  <tbody>
                    <For each={s().economy}>
                      {(econ) => {
                        const baseProduced = () => econ.produced + econ.gold;
                        const totalTrades = () => econ.maritimeTrades + econ.playerTrades;
                        return (
                          <tr style={{ 'border-top': '1px solid rgba(255,255,255,0.06)' }}>
                            <td style={{ padding: '5px 6px', display: 'flex', 'align-items': 'center', gap: '6px' }}>
                              <SeatDot seat={econ.seat} />
                              <span>{props.name(econ.seat)}</span>
                              <Show when={econ.barbarianPoints > 0}>
                                <span class="badge" title="击退黑帆独得卫城分">
                                  +{econ.barbarianPoints}卫
                                </span>
                              </Show>
                            </td>
                            <td
                              style={{ padding: '5px 6px', 'text-align': 'right' }}
                              title={
                                econ.gold > 0
                                  ? `常规产出 ${econ.produced} + 金矿/水渠自选 ${econ.gold}`
                                  : `常规产出 ${econ.produced}`
                              }
                            >
                              {isCitadels() ? `${baseProduced()}+${econ.commodities}货` : baseProduced()}
                            </td>
                            <td
                              style={{ padding: '5px 6px', 'text-align': 'right' }}
                              title={`海贸 ${econ.maritimeTrades} 次 / 玩家交易 ${econ.playerTrades} 次`}
                            >
                              {totalTrades()}
                            </td>
                            <td style={{ padding: '5px 6px', 'text-align': 'right' }}>
                              +{econ.stolenGain} / -{econ.stolenLoss}
                            </td>
                            <td style={{ padding: '5px 6px', 'text-align': 'right' }}>{econ.discarded}</td>
                          </tr>
                        );
                      }}
                    </For>
                  </tbody>
                </table>
              </div>
            </Show>
          </section>
        );
      }}
    </Show>
  );
}
