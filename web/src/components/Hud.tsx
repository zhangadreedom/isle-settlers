import { createMemo, createSignal, For, Show } from 'solid-js';
import type { Game } from '../game/controller';
import { COSTS, DEV_CARDS, DEV_HELP, DEV_NAME, RESOURCES, RESOURCE_CSS, RESOURCE_SHORT } from '../game/text';
import type { DevCard } from '../generated/proto';
import { Die, ResIcon, SeatDot, SetView } from './bits';

export function PlayerList(props: { game: Game }) {
  const g = props.game;
  return (
    <aside class="panel players-panel" data-inset>
      <For each={g.view().players}>
        {(p) => {
          const isMe = () => p.seat === g.me;
          const points = () => (isMe() ? (g.view().me?.victoryPoints ?? p.publicVictoryPoints) : p.publicVictoryPoints);
          return (
            <div
              class="seat"
              id={`seat-${p.seat}`}
              classList={{ current: g.view().current === p.seat, me: isMe(), winner: g.view().winner === p.seat }}
            >
              <div class="seat-head">
                <SeatDot seat={p.seat} />
                <span class="seat-name">{g.name(p.seat)}</span>
                <Show when={g.thinking() === p.seat}>
                  <span class="thinking" aria-label="思考中">
                    <i />
                    <i />
                    <i />
                  </span>
                </Show>
                <span class="vp" title="分数">
                  {points()}
                  <small> 分</small>
                </span>
              </div>
              <div class="seat-stats muted">
                <span title="手牌">牌 {p.resourceCount}</span>
                <span title="发展卡">卡 {p.devCardCount}</span>
                <span title="已出骑士">骑 {p.playedDevCards[0]}</span>
                <span title="最长连续道路">路 {p.longestRoad}</span>
              </div>
              <Show when={g.view().longestRoadOwner === p.seat || g.view().largestArmyOwner === p.seat}>
                <div class="badges">
                  <Show when={g.view().longestRoadOwner === p.seat}>
                    <span class="badge">最长道路 +2</span>
                  </Show>
                  <Show when={g.view().largestArmyOwner === p.seat}>
                    <span class="badge">最大骑士 +2</span>
                  </Show>
                </div>
              </Show>
            </div>
          );
        }}
      </For>
      <div class="bank muted">
        <span>银行</span>
        <For each={RESOURCES}>
          {(_, i) => (
            <span class="bank-item">
              <ResIcon r={i()} />
              {g.view().bank[i()]}
            </span>
          )}
        </For>
        <span class="bank-item">发展卡 {g.view().devDeckLen}</span>
      </div>
    </aside>
  );
}

export function DiceView(props: { game: Game }) {
  return (
    <Show when={props.game.dice()} keyed>
      {(d) => (
        <div class="dice" classList={{ seven: d.values[0] + d.values[1] === 7 }}>
          <Die value={d.values[0]} />
          <Die value={d.values[1]} />
          <span class="dice-sum">{d.values[0] + d.values[1]}</span>
        </div>
      )}
    </Show>
  );
}

export function GameLog(props: { game: Game }) {
  const [open, setOpen] = createSignal(true);
  let list!: HTMLOListElement;
  const entries = createMemo(() => {
    const l = props.game.log();
    queueMicrotask(() => list && (list.scrollTop = list.scrollHeight));
    return l;
  });
  return (
    <section class="panel log" classList={{ collapsed: !open() }} data-inset>
      <header>
        <h3>对局记录</h3>
        <button class="icon" onClick={() => setOpen(!open())} aria-label={open() ? '收起' : '展开'}>
          {open() ? '−' : '+'}
        </button>
      </header>
      <ol ref={list}>
        <For each={entries()}>
          {(e) => (
            <li>
              {e.seat !== null ? <SeatDot seat={e.seat} /> : <span class="dot blank" />}
              {e.text}
            </li>
          )}
        </For>
      </ol>
    </section>
  );
}

/** What the human should do now. */
function prompt(g: Game): string {
  const v = g.view();
  if (v.winner !== null) return `${g.name(v.winner)}获胜`;
  if (g.me === null) return `观战中 · ${g.name(v.current)}的回合`;
  const myTurn = v.current === g.me;
  switch (v.phase.type) {
    case 'SetupSettlement':
      return myTurn ? `放置你的第 ${v.players[g.me].settlementsLeft === 5 ? 1 : 2} 个村庄（点击发光的交叉点）` : `${g.name(v.current)} 正在选择开局位置…`;
    case 'SetupRoad':
      return myTurn ? '放置一条与村庄相连的道路' : `${g.name(v.current)} 正在放置道路…`;
    case 'PreRoll':
      return myTurn ? '掷骰子（也可以先打一张发展卡）' : `${g.name(v.current)} 的回合`;
    case 'Discard':
      return v.pendingDiscard[g.me] > 0 ? `掷出 7：请弃掉 ${v.pendingDiscard[g.me]} 张牌` : '等待其他玩家弃牌…';
    case 'MoveRobber':
      return myTurn ? '把强盗移到另一个地块（点击发光的地块）' : `${g.name(v.current)} 正在移动强盗…`;
    case 'RoadBuilding':
      return myTurn ? `免费放置道路（还剩 ${v.phase.value.remaining} 条）` : `${g.name(v.current)} 正在修路…`;
    case 'Main':
      if (!myTurn) return `${g.name(v.current)} 的回合`;
      if (v.trade) {
        const responses = v.trade.responses.slice(0, v.players.length).filter((_, i) => i !== g.me);
        if (responses.includes('Pending')) return '等待其他玩家回应你的交易…';
        return responses.includes('Accepted') ? '选择一位接受的玩家成交，或撤回报价' : '没有人接受，请撤回报价';
      }
      if (g.mode()) return `选择位置建造${g.mode() === 'road' ? '道路' : g.mode() === 'settlement' ? '村庄' : '城市'}（再点一次按钮取消）`;
      return '交易、建造、打发展卡，或结束回合';
    case 'GameOver':
      return '游戏结束';
  }
}

export function HandBar(props: { game: Game; onTrade: () => void; onDevCard: (card: DevCard) => void }) {
  const g = props.game;
  const me = () => g.view().me;
  const playable = (card: DevCard) => {
    const type = { Knight: 'PlayKnight', RoadBuilding: 'PlayRoadBuilding', YearOfPlenty: 'PlayYearOfPlenty', Monopoly: 'PlayMonopoly' }[
      card as Exclude<DevCard, 'VictoryPoint'>
    ];
    return type !== undefined && g.has(type as never);
  };
  const isMain = () => g.view().phase.type === 'Main' && g.view().current === g.me;

  return (
    <div class="handbar-wrap" data-inset>
      <div class="prompt">{prompt(g)}</div>
      <Show when={me()}>
        {(m) => (
          <div class="panel handbar">
            <div class="hand" aria-label="手牌">
              <For each={RESOURCES}>
                {(r, i) => (
                  <div class="card" id={`hand-${i()}`} classList={{ empty: m().hand[i()] === 0 }} style={{ '--c': RESOURCE_CSS[i()] }}>
                    <span class="card-name">{RESOURCE_SHORT[r]}</span>
                    <span class="card-count">{m().hand[i()]}</span>
                  </div>
                )}
              </For>
            </div>

            <div class="devcards" aria-label="发展卡">
              <For each={DEV_CARDS.filter((c) => m().devCards[DEV_CARDS.indexOf(c)] + m().newDevCards[DEV_CARDS.indexOf(c)] > 0)}>
                {(card) => {
                  const i = DEV_CARDS.indexOf(card);
                  const ready = () => m().devCards[i];
                  const fresh = () => m().newDevCards[i];
                  return (
                    <button
                      class="devcard"
                      disabled={card === 'VictoryPoint' || !playable(card)}
                      title={`${DEV_HELP[card]}${fresh() ? `（${fresh()} 张下回合可用）` : ''}`}
                      onClick={() => props.onDevCard(card)}
                    >
                      {DEV_NAME[card]}
                      <b>×{ready() + fresh()}</b>
                    </button>
                  );
                }}
              </For>
            </div>

            <div class="actions">
              <Show when={g.has('RollDice')}>
                <button class="primary" onClick={() => g.act((a) => a.type === 'RollDice')}>
                  掷骰子
                </button>
              </Show>
              <Show when={isMain()}>
                <For each={[['road', '道路', 'BuildRoad', 0], ['settlement', '村庄', 'BuildSettlement', 1], ['city', '城市', 'BuildCity', 2]] as const}>
                  {([mode, label, type, costIndex]) => (
                    <button
                      class="build"
                      classList={{ on: g.mode() === mode }}
                      disabled={!g.has(type)}
                      onClick={() => g.setMode(mode)}
                      title={`${label}：${COSTS[costIndex].points}`}
                    >
                      {label}
                      <SetView set={COSTS[costIndex].cost} />
                    </button>
                  )}
                </For>
                <button class="build" disabled={!g.has('BuyDevCard')} onClick={() => g.act((a) => a.type === 'BuyDevCard')}>
                  发展卡
                  <SetView set={COSTS[3].cost} />
                </button>
                <button onClick={props.onTrade}>交易</button>
                <button class="primary" disabled={!g.has('EndTurn')} onClick={() => g.act((a) => a.type === 'EndTurn')}>
                  结束回合
                </button>
              </Show>
            </div>
          </div>
        )}
      </Show>
    </div>
  );
}

/** Resource chips flying from tiles to the receiving seat. */
export function Flights(props: { game: Game }) {
  return (
    <div class="flights" aria-hidden="true">
      <For each={props.game.flights()}>
        {(f) => {
          let el!: HTMLSpanElement;
          queueMicrotask(() => {
            const target =
              f.seat === props.game.me ? document.getElementById(`hand-${f.resource}`) : document.getElementById(`seat-${f.seat}`);
            if (!el || !target) return;
            const box = target.getBoundingClientRect();
            const dx = box.left + box.width / 2 - f.from.x;
            const dy = box.top + box.height / 2 - f.from.y;
            el.animate(
              [
                { transform: 'translate(-50%, -50%) scale(0.6)', opacity: 0 },
                { transform: 'translate(-50%, -80%) scale(1.15)', opacity: 1, offset: 0.25 },
                { transform: `translate(calc(-50% + ${dx}px), calc(-50% + ${dy}px)) scale(0.8)`, opacity: 0.9 },
              ],
              { duration: 900, easing: 'cubic-bezier(.5,0,.3,1)', fill: 'forwards' },
            );
          });
          return (
            <span ref={el} class="flight" style={{ left: `${f.from.x}px`, top: `${f.from.y}px` }}>
              <ResIcon r={f.resource} size="md" />
            </span>
          );
        }}
      </For>
    </div>
  );
}
