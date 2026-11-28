import { createMemo, createSignal, For, onCleanup, Show } from 'solid-js';
import { isMuted, onMuteChange, toggleMute } from '../audio/sound';
import type { BoardRenderer, BoardTheme } from '../board/BoardRenderer';
import { t } from '../i18n';
import { Verified } from './Account';
import type { BuildMode, Game } from '../game/controller';
import { EMBEDDED } from '../embed';
import type { ReplayControls } from '../game/source';
import {
  BOT_LEVEL_NAME,
  CITY_TRACKS,
  COMMODITIES,
  COMMODITY_CSS,
  COMMODITY_SHORT,
  COSTS,
  DEV_CARDS,
  DEV_HELP,
  DEV_NAME,
  RESOURCES,
  RESOURCE_CSS,
  RESOURCE_NAME,
  RESOURCE_SHORT,
  TERRAIN_NAME,
  TRACK_NAME,
} from '../game/text';
import type { DevCard } from '../generated/proto';
import { CommIcon, Die, ResIcon, SeatDot, SetView } from './bits';

/** Islands in a bonus mask. */
const islandCount = (mask: number) => [...mask.toString(2)].filter((b) => b === '1').length;

export function PlayerList(props: { game: Game }) {
  const g = props.game;
  const activeStrength = (seat: number) =>
    (g.view().citadels?.knights ?? []).filter((k) => k.owner === seat && k.active).reduce((a, k) => a + k.level, 0);
  const totalDefense = () =>
    (g.view().citadels?.knights ?? []).filter((k) => k.active).reduce((a, k) => a + k.level, 0);
  const totalVulnCities = () => {
    const c = g.view().citadels;
    if (!c) return 0;
    const metros = new Set(c.metropolises.filter((v): v is number => v !== null));
    return g.view().buildings.filter((b, v) => b !== 255 && (b & 0x80) !== 0 && !metros.has(v)).length;
  };
  const metrosOf = (seat: number) =>
    CITY_TRACKS.filter((_, i) => g.view().citadels?.metropolisOwners[i] === seat);

  return (
    <aside class="panel players-panel" classList={{ many: g.view().players.length > 4 }} data-inset>
      <For each={g.view().players}>
        {(p) => {
          const isMe = () => p.seat === g.me;
          const points = () => (isMe() ? (g.view().me?.victoryPoints ?? p.publicVictoryPoints) : p.publicVictoryPoints);
          return (
            <div
              class="seat"
              id={`seat-${p.seat}`}
              classList={{
                current: g.view().current === p.seat,
                building:
                  (g.view().phase.type === 'SpecialBuild' || g.view().phase.type === 'PairedTurn') &&
                  g.view().toMove === p.seat,
                me: isMe(),
                winner: g.view().winner === p.seat,
              }}
            >
              <div class="seat-head">
                <SeatDot seat={p.seat} />
                <span class="seat-name">{g.name(p.seat)}</span>
                <Show when={g.online && g.seat(p.seat).registered}>
                  <Verified />
                </Show>
                <Show when={g.replay && g.seat(p.seat).bot}>
                  <span class="presence bot" title={`${BOT_LEVEL_NAME[g.replay!.players[p.seat]?.bot ?? 'Normal']} AI`}>
                    AI
                  </span>
                </Show>
                <Show when={g.online && p.seat !== g.me}>
                  <Show when={g.seat(p.seat).bot}>
                    <span class="presence bot">AI</span>
                  </Show>
                  <Show when={!g.seat(p.seat).bot && g.seat(p.seat).autopilot}>
                    <span class="presence auto" title="掉线期间由 AI 代打">托管</span>
                  </Show>
                  <Show when={!g.seat(p.seat).bot && !g.seat(p.seat).connected && !g.seat(p.seat).autopilot}>
                    <span class="presence off" title="连接已断开">离线</span>
                  </Show>
                </Show>
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
                <span title="基础资源手牌">牌 {p.resourceCount}</span>
                <Show when={g.citadels}>
                  <span title="商品手牌（纸/布/币）">货 {p.commodityCount}</span>
                </Show>
                <span title="发展卡">卡 {p.devCardCount}</span>
                <Show when={!g.citadels}>
                  <span title="已出骑士">骑 {p.playedDevCards[0]}</span>
                </Show>
                <Show when={g.citadels}>
                  <span title="已激活铁卫战力">卫 {activeStrength(p.seat)}</span>
                  <span title="商贸/军政/学术等级">科 {p.cityProgress.join('/')}</span>
                </Show>
                <Show when={g.fishermen}>
                  <span title="海鱼筹码（上限 7 条，可在鱼市兑换奇策）">鱼 {p.fishCount}</span>
                </Show>
                <Show when={g.caravans}>
                  <span title="黄金河金币（满 2 枚可兑资源，≥4 枚争富甲一方 +2 分）">币 {p.coins}</span>
                  <span title={`跨河石拱桥（还能造 ${p.bridgesLeft} 座）`}>桥 {3 - p.bridgesLeft}</span>
                </Show>
                <span title={g.seafaring ? '最长连续商路（道路和船）' : '最长连续道路'}>路 {p.longestRoad}</span>
                <Show when={g.seafaring}>
                  <span title={`海上的船（还能造 ${p.shipsLeft} 艘）`}>船 {15 - p.shipsLeft}</span>
                </Show>
              </div>
              <Show
                when={
                  g.view().longestRoadOwner === p.seat ||
                  g.view().largestArmyOwner === p.seat ||
                  islandCount(p.islands) > 0 ||
                  p.defenderPoints > 0 ||
                  metrosOf(p.seat).length > 0 ||
                  g.view().oldBootOwner === p.seat ||
                  g.view().wealthiestOwner === p.seat ||
                  (g.caravans && g.view().wealthiestOwner !== null && p.coins === 0)
                }
              >
                <div class="badges">
                  <Show when={g.view().longestRoadOwner === p.seat}>
                    <span class="badge">{g.seafaring ? '最长商路' : '最长道路'} +2</span>
                  </Show>
                  <Show when={g.view().largestArmyOwner === p.seat}>
                    <span class="badge">最大骑士 +2</span>
                  </Show>
                  <Show when={islandCount(p.islands) > 0}>
                    <span class="badge" title="首次在新岛屿建村庄的奖励">新岛 +{2 * islandCount(p.islands)}</span>
                  </Show>
                  <For each={metrosOf(p.seat)}>
                    {(t) => (
                      <span class="badge" title={`${TRACK_NAME[t]}大都会（免疫黑帆舰队）`}>
                        {TRACK_NAME[t]}都 +2
                      </span>
                    )}
                  </For>
                  <Show when={p.defenderPoints > 0}>
                    <span class="badge" title="击退黑帆舰队独得首功">卫城 +{p.defenderPoints}</span>
                  </Show>
                  <Show when={g.view().oldBootOwner === p.seat}>
                    <span class="badge boot-badge" title="破旧皮靴：获胜目标 +1 分，可在鱼市转送给分不低于你的玩家">
                      皮靴（需+1分）
                    </span>
                  </Show>
                  <Show when={g.view().wealthiestOwner === p.seat}>
                    <span class="badge" title="全场金币最多（≥4 币），奖励 +2 分">
                      富甲一方 +2
                    </span>
                  </Show>
                  <Show when={g.caravans && g.view().wealthiestOwner !== null && p.coins === 0}>
                    <span class="badge boot-badge" title="已有首富产生，身无分文（0 币）扣 1 分">
                      赤贫 −1
                    </span>
                  </Show>
                </div>
              </Show>
            </div>
          );
        }}
      </For>
      <Show when={g.view().citadels}>
        {(cit) => (
          <div
            class="barbarian-bar"
            classList={{ danger: totalDefense() < totalVulnCities() }}
            title="每次掷骰 50% 前进 1 格；满 7 格攻城，比较全岛激活铁卫战力与非大都会城市总数"
          >
            <span>黑帆 {cit().barbarianPos}/{cit().barbarianSteps}</span>
            <span class="barbarian-steps">
              <For each={[...Array(cit().barbarianSteps).keys()]}>
                {(i) => <i classList={{ on: i < cit().barbarianPos, danger: cit().barbarianPos >= cit().barbarianSteps - 1 && i < cit().barbarianPos }} />}
              </For>
            </span>
            <span class="muted">
              铁卫 {totalDefense()} / 城池 {totalVulnCities()}
            </span>
          </div>
        )}
      </Show>
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
        <Show when={g.view().citadels}>
          {(cit) => (
            <For each={COMMODITIES}>
              {(_, i) => (
                <span class="bank-item">
                  <CommIcon c={i()} />
                  {cit().commodityBank[i()]}
                </span>
              )}
            </For>
          )}
        </Show>
        <span class="bank-item">发展卡 {g.view().devDeckLen}</span>
        <Show when={g.view().diceDeckLeft !== null}>
          <span class="bank-item" title="均衡点数牌组剩余张数（剩 5 张时重洗）">
            点数牌 {g.view().diceDeckLeft}
          </span>
        </Show>
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

/** Game log with chat. On phones it is an overlay that leaves the board layout alone. */
export function GameLog(props: { game: Game; open: boolean; onToggle: () => void }) {
  const open = () => props.open;
  const setOpen = (_: boolean) => props.onToggle();
  let list!: HTMLOListElement;
  const entries = createMemo(() => {
    const l = props.game.log();
    queueMicrotask(() => list && (list.scrollTop = list.scrollHeight));
    return l;
  });
  return (
    <section class="panel log" classList={{ collapsed: !open() }} data-inset={window.innerWidth > 720 ? '' : undefined}>
      <header>
        <h3>对局记录</h3>
        <button class="icon" onClick={() => setOpen(!open())} aria-label={open() ? '收起' : '展开'}>
          {open() ? '−' : '+'}
        </button>
      </header>
      <ol ref={list}>
        <For each={entries()}>
          {(e) => (
            <li classList={{ chat: !!e.chat }}>
              {e.seat !== null ? <SeatDot seat={e.seat} /> : <span class="dot blank" />}
              <span>
                {e.chat ? <b>{e.chat.name}：</b> : null}
                {e.text}
              </span>
            </li>
          )}
        </For>
      </ol>
      <Show when={props.game.chat}>
        {(send) => {
          const [text, setText] = createSignal('');
          return (
            <form
              class="chat-form"
              onSubmit={(e) => {
                e.preventDefault();
                if (text().trim()) send()(text());
                setText('');
              }}
            >
              <input
                value={text()}
                maxLength={200}
                placeholder="聊天，比如谈谈交易条件…"
                onInput={(e) => setText(e.currentTarget.value)}
                aria-label="聊天消息"
              />
              <button type="submit">发送</button>
            </form>
          );
        }}
      </Show>
    </section>
  );
}

const MODE_PROMPT: Record<Exclude<BuildMode, null>, string> = {
  road: '选择位置建造道路（再点一次按钮取消）',
  settlement: '选择位置建造村庄（再点一次按钮取消）',
  city: '选择位置建造城市（再点一次按钮取消）',
  ship: '选择临海的边造船（再点一次按钮取消）',
  move: '先点一艘可以移动的船，再点它要去的位置（再点一次按钮取消）',
  knight: '选择道路相连的空交叉点招募初阶铁卫（再点一次取消）',
  activate: '点选一名未激活的铁卫消耗 1 麦激活（再点一次取消）',
  promote: '点选一名铁卫消耗 1 羊 1 矿晋升阶位（再点一次取消）',
  knightMove: '先点一名已激活铁卫，再点调遣目标或更低阶对手铁卫（再点一次取消）',
  chase: '先点强盗旁已激活的铁卫，再点强盗要去的地块（再点一次取消）',
  fishRoad: '消耗 5 条海鱼：点选发光位置免费铺筑道路（再点一次取消）',
  bridge: '消耗 2 砖 + 1 木：点选发光黄金河段架设石拱桥（立得 +2 金币）',
};

function modePrompt(g: Game): string {
  const m = g.mode();
  if (m === 'move' && g.moveFrom() !== null) return '点这艘船要去的位置（再点一次按钮取消）';
  if (m === 'knightMove' && g.knightFrom() !== null) return '点选铁卫要移往的目标交叉点（再点一次取消）';
  if (m === 'chase' && g.knightFrom() !== null) return '点选要把强盗/海盗赶往的目标格（再点一次取消）';
  return m ? MODE_PROMPT[m] : '';
}

/** What the human should do now. */
function prompt(g: Game): string {
  const v = g.view();
  if (v.winner !== null) return `${g.name(v.winner)}获胜`;
  if (g.replay) {
    const sub =
      v.phase.type === 'SpecialBuild'
        ? `，${g.name(v.phase.value.builder)} 特殊建造中`
        : v.phase.type === 'PairedTurn'
          ? `，${g.name(v.phase.value.partner)} 搭档副回合`
          : '';
    return `回放 · ${g.name(v.current)}的回合${sub}`;
  }
  if (v.phase.type === 'SpecialBuild') {
    const builder = v.phase.value.builder;
    if (builder === g.me) {
      if (g.mode()) return modePrompt(g);
      return '特殊建造：可以建造或买发展卡（不能交易），或者跳过';
    }
    return `${g.name(builder)} 特殊建造中…`;
  }
  if (v.phase.type === 'PairedTurn') {
    const partner = v.phase.value.partner;
    if (partner === g.me) {
      if (g.mode()) return modePrompt(g);
      return '搭档副回合：可以建造、与银行/港口贸易、打发展卡，或结束副回合';
    }
    return `${g.name(partner)} 正在进行搭档副回合…`;
  }
  if (g.me === null) return `观战中 · ${g.name(v.current)}的回合`;
  const myTurn = v.toMove === g.me || v.current === g.me;
  switch (v.phase.type) {
    case 'SetupSettlement': {
      const second = g.citadels && v.players[g.me].settlementsLeft === 4;
      if (v.current !== g.me) return `${g.name(v.current)} 正在选择开局位置…`;
      return second
        ? '放置你的起始城市（点击发光的交叉点，将获得周围起始资源）'
        : `放置你的第 ${v.players[g.me].settlementsLeft === 5 ? 1 : 2} 个村庄（点击发光的交叉点）`;
    }
    case 'SetupRoad':
      if (g.seafaring) return myTurn ? '放置一条与村庄相连的道路或船（临海的边可以造船）' : `${g.name(v.current)} 正在放置道路或船…`;
      return myTurn ? '放置一条与村庄相连的道路' : `${g.name(v.current)} 正在放置道路…`;
    case 'Gold': {
      const owed = v.pendingGold[g.me] ?? 0;
      return owed > 0 ? `金矿产出：任选 ${owed} 张资源` : '等待其他玩家选择金矿资源…';
    }
    case 'PreRoll':
      return myTurn ? '掷骰子（也可以先打一张发展卡）' : `${g.name(v.current)} 的回合`;
    case 'Discard':
      return v.pendingDiscard[g.me] > 0 ? `掷出 7：请弃掉 ${v.pendingDiscard[g.me]} 张牌` : '等待其他玩家弃牌…';
    case 'MoveRobber':
      if (g.seafaring) return myTurn ? '移动强盗（点陆地）或海盗（点海面）' : `${g.name(v.current)} 正在移动强盗或海盗…`;
      return myTurn ? '把强盗移到另一个地块（点击发光的地块）' : `${g.name(v.current)} 正在移动强盗…`;
    case 'RoadBuilding':
      if (g.seafaring) return myTurn ? `免费放置道路或船（还剩 ${v.phase.value.remaining}）` : `${g.name(v.current)} 正在修路造船…`;
      return myTurn ? `免费放置道路（还剩 ${v.phase.value.remaining} 条）` : `${g.name(v.current)} 正在修路…`;
    case 'PlaceCamel':
      return myTurn
        ? '引导骆驼商队：点击绿洲或商道末端发光边放置 1 头骆驼（穿村+1分、重合道路路长翻倍）'
        : `${g.name(v.toMove ?? v.current)} 正在引导骆驼商队…`;
    case 'Main':
      if (!myTurn) return `${g.name(v.current)} 的回合`;
      if (v.trade) {
        const responses = v.trade.responses.slice(0, v.players.length).filter((_, i) => i !== g.me);
        if (responses.includes('Pending')) return '等待其他玩家回应你的交易…';
        return responses.includes('Accepted') ? '选择一位接受的玩家成交，或撤回报价' : '没有人接受，请撤回报价';
      }
      if (g.mode()) return modePrompt(g);
      return '交易、建造、打发展卡，或结束回合';
    case 'GameOver':
      return '游戏结束';
  }
  return '';
}

export function HandBar(props: {
  game: Game;
  onTrade: () => void;
  onCitadel?: () => void;
  onFishMarket?: () => void;
  onCoinMarket?: () => void;
  onDevCard: (card: DevCard) => void;
}) {
  const g = props.game;
  const me = () => g.view().me;
  const playable = (card: DevCard) => {
    const type = { Knight: 'PlayKnight', RoadBuilding: 'PlayRoadBuilding', YearOfPlenty: 'PlayYearOfPlenty', Monopoly: 'PlayMonopoly' }[
      card as Exclude<DevCard, 'VictoryPoint'>
    ];
    return type !== undefined && g.has(type as never);
  };
  const isMain = () => !g.replay && g.view().phase.type === 'Main' && g.view().current === g.me;
  const isPaired = () => !g.replay && g.view().phase.type === 'PairedTurn' && g.view().toMove === g.me;
  const isSpecialBuild = () => !g.replay && g.view().phase.type === 'SpecialBuild' && g.view().toMove === g.me;
  const canActMain = () => isMain() || isPaired();

  return (
    <div class="handbar-wrap" data-inset>
      <Show when={g.replay}>{(r) => <ReplayBar game={g} replay={r()} />}</Show>
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
              <Show when={g.citadels}>
                <For each={COMMODITIES}>
                  {(c, i) => (
                    <div
                      class="card comm-card"
                      classList={{ empty: m().commodities[i()] === 0 }}
                      style={{ '--c': COMMODITY_CSS[i()] }}
                      title="商品卡（用于升级城邦科技与商馆交易）"
                    >
                      <span class="card-name">{COMMODITY_SHORT[c]}</span>
                      <span class="card-count">{m().commodities[i()]}</span>
                    </div>
                  )}
                </For>
              </Show>
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
              <Show when={canActMain() || isSpecialBuild()}>
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
                <Show when={g.seafaring}>
                  <button class="build" classList={{ on: g.mode() === 'ship' }} disabled={!g.has('BuildShip')} onClick={() => g.setMode('ship')} title="船：沿海岸和海面延伸，登上新岛屿">
                    船
                    <SetView set={COSTS[4].cost} />
                  </button>
                  <Show when={canActMain()}>
                    <button
                      class="build"
                      classList={{ on: g.mode() === 'move' }}
                      disabled={!g.has('MoveShip')}
                      onClick={() => g.setMode('move')}
                      title="每回合可以把一艘航线末端的船移到别处（本回合造的船除外）"
                    >
                      移船
                    </button>
                  </Show>
                </Show>
                <Show when={g.citadels && canActMain()}>
                  <button
                    class="build"
                    classList={{ on: g.mode() === 'knight' }}
                    disabled={!g.has('RecruitKnight')}
                    onClick={() => g.setMode('knight')}
                    title="招募初阶铁卫（1 羊 + 1 矿）"
                  >
                    铁卫
                    <SetView set={[0, 0, 1, 0, 1]} />
                  </button>
                  <button
                    class="build"
                    classList={{ on: g.mode() === 'activate' }}
                    disabled={!g.has('ActivateKnight')}
                    onClick={() => g.setMode('activate')}
                    title="激活铁卫（1 麦），抵御黑帆舰队"
                  >
                    激活
                    <SetView set={[0, 0, 0, 1, 0]} />
                  </button>
                  <button
                    class="build"
                    classList={{ on: g.mode() === 'promote' }}
                    disabled={!g.has('PromoteKnight')}
                    onClick={() => g.setMode('promote')}
                    title="晋升铁卫阶位（1 羊 + 1 矿）"
                  >
                    晋升
                  </button>
                  <Show when={g.has('MoveKnight')}>
                    <button
                      class="build"
                      classList={{ on: g.mode() === 'knightMove' }}
                      onClick={() => g.setMode('knightMove')}
                      title="调遣已激活铁卫沿道路移动或驱逐对手低阶铁卫"
                    >
                      调遣
                    </button>
                  </Show>
                  <Show when={g.has('ChaseRobber')}>
                    <button
                      class="build"
                      classList={{ on: g.mode() === 'chase' }}
                      onClick={() => g.setMode('chase')}
                      title="使用相邻激活铁卫驱赶强盗"
                    >
                      驱匪
                    </button>
                  </Show>
                  <button
                    class="build"
                    classList={{ primary: g.has('UpgradeCity') }}
                    onClick={() => props.onCitadel?.()}
                    title="查看并升级商贸/军政/学术城邦科技与大都会"
                  >
                    城邦科技
                  </button>
                </Show>
                <Show when={g.fishermen && canActMain()}>
                  <button
                    class="build"
                    classList={{
                      on: g.mode() === 'fishRoad',
                      primary:
                        (g.me !== null && (g.view().players[g.me]?.fishCount ?? 0) >= 2) ||
                        g.has('PassOldBoot'),
                    }}
                    onClick={() => props.onFishMarket?.()}
                    title="消耗海鱼兑换奇策，或转送破旧皮靴"
                  >
                    鱼市 ({g.me !== null ? g.view().players[g.me]?.fishCount ?? 0 : 0})
                  </button>
                </Show>
                <Show when={g.caravans}>
                  <button
                    class="build"
                    classList={{ on: g.mode() === 'bridge' }}
                    disabled={!g.has('BuildBridge')}
                    onClick={() => g.setMode('bridge')}
                    title="跨黄金河架设石拱桥（2砖 + 1木，奖励 +2 金币）"
                  >
                    石桥
                    <SetView set={[2, 1, 0, 0, 0]} />
                  </button>
                  <Show when={canActMain()}>
                    <button
                      class="build"
                      classList={{ primary: g.has('SpendCoinsResource') }}
                      disabled={!g.has('SpendCoinsResource')}
                      onClick={() => props.onCoinMarket?.()}
                      title="每 2 枚金币可从银行购买任意 1 张资源"
                    >
                      金币兑物 ({g.me !== null ? g.view().players[g.me]?.coins ?? 0 : 0})
                    </button>
                  </Show>
                </Show>
                <button class="build" disabled={!g.has('BuyDevCard')} onClick={() => g.act((a) => a.type === 'BuyDevCard')}>
                  发展卡
                  <SetView set={COSTS[3].cost} />
                </button>
                <Show
                  when={canActMain()}
                  fallback={
                    <button class="primary" disabled={!g.has('EndTurn')} onClick={() => g.act((a) => a.type === 'EndTurn')}>
                      跳过
                    </button>
                  }
                >
                  <button onClick={props.onTrade}>交易</button>
                  <button class="primary" disabled={!g.has('EndTurn')} onClick={() => g.act((a) => a.type === 'EndTurn')}>
                    {isPaired() ? '结束副回合' : '结束回合'}
                  </button>
                </Show>
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

const SPEEDS = [0.5, 1, 2, 4];

/** Replay transport: play/pause, scrubbing, speed and perspective. */
export function ReplayBar(props: { game: Game; replay: ReplayControls }) {
  const r = props.replay;
  const g = props.game;
  const [copied, setCopied] = createSignal(false);
  const link = () => g.replayLink?.() ?? null;
  return (
    <div class="panel replay-bar" role="group" aria-label="回放控制">
      <div class="replay-row">
        <button class="icon" onClick={() => r.step(-1)} disabled={r.position() === 0} aria-label="后退一步" title="后退一步">
          ‹
        </button>
        <button class="primary replay-play" onClick={() => r.toggle()} aria-label={r.playing() ? '暂停' : '播放'}>
          {r.playing() ? '暂停' : r.position() >= r.length ? '重播' : '播放'}
        </button>
        <button class="icon" onClick={() => r.step(1)} disabled={r.position() >= r.length} aria-label="前进一步" title="前进一步">
          ›
        </button>
        <input
          class="replay-seek grow"
          type="range"
          min={0}
          max={r.length}
          value={r.position()}
          onInput={(e) => r.seek(Number(e.currentTarget.value))}
          aria-label="进度"
        />
        <span class="replay-pos muted">
          {r.position()}/{r.length}
        </span>
      </div>
      <div class="replay-row">
        <div class="seg small">
          <For each={SPEEDS}>
            {(x) => (
              <button classList={{ on: r.speed() === x }} onClick={() => r.setSpeed(x)}>
                {x}×
              </button>
            )}
          </For>
        </div>
        <select
          value={r.viewer() ?? -1}
          onChange={(e) => r.setViewer(Number(e.currentTarget.value) < 0 ? null : Number(e.currentTarget.value))}
          aria-label="视角"
        >
          <option value={-1}>公开视角</option>
          <For each={r.players}>{(_, i) => <option value={i()}>{g.name(i())} 的手牌</option>}</For>
        </select>
        <Show when={link()}>
          {(url) => (
            <button
              onClick={() => {
                void navigator.clipboard?.writeText(url()).then(() => {
                  setCopied(true);
                  window.setTimeout(() => setCopied(false), 1500);
                });
              }}
            >
              {copied() ? '已复制' : '复制链接'}
            </button>
          )}
        </Show>
        <Show when={!EMBEDDED}>
          <button onClick={() => downloadReplay(r.record)}>下载</button>
        </Show>
      </div>
    </div>
  );
}

/** Saves a replay record as a JSON file. */
export function downloadReplay(record: import('../generated/proto').ReplayRecord) {
  const blob = new Blob([JSON.stringify(record)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  const stamp = new Date().toISOString().slice(0, 16).replace(/[-:T]/g, '');
  a.href = url;
  a.download = `isle-replay-${stamp}.json`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

const THEME_ORDER: BoardTheme[] = ['day', 'sunset', 'night'];
const THEME_LABEL: Record<BoardTheme, string> = { day: '晴日', sunset: '黄昏', night: '星夜' };

function initialTheme(): BoardTheme {
  try {
    const v = localStorage.getItem('isle.theme');
    if (v === 'day' || v === 'sunset' || v === 'night') return v;
  } catch {
    // ignore blocked storage
  }
  return 'day';
}

/** Cycles 3D board lighting atmosphere between Day, Sunset, and Tabletop Night. */
export function ThemeToggle(props: { renderer?: BoardRenderer; class?: string }) {
  const [theme, setThemeState] = createSignal<BoardTheme>(initialTheme());
  const cycle = () => {
    const next = THEME_ORDER[(THEME_ORDER.indexOf(theme()) + 1) % THEME_ORDER.length];
    setThemeState(next);
    try {
      localStorage.setItem('isle.theme', next);
    } catch {
      // ignore blocked storage
    }
    props.renderer?.setTheme(next);
  };
  return (
    <button
      type="button"
      class={props.class ?? 'ghost'}
      title="切换 3D 棋盘光影氛围（晴日 / 黄昏暖阳 / 星夜桌灯）"
      onClick={cycle}
    >
      {THEME_LABEL[theme()]}
    </button>
  );
}

/** Mute / unmute button synced with the procedural Web Audio synthesizer. */
export function SoundToggle(props: { class?: string }) {
  const [muted, setMutedState] = createSignal(isMuted());
  onCleanup(onMuteChange(setMutedState));
  return (
    <button
      type="button"
      class={props.class ?? 'ghost'}
      aria-pressed={!muted()}
      aria-label={muted() ? '已静音，点击开启音效' : '音效开启，点击静音'}
      title={muted() ? '点击开启桌游音效' : '点击静音'}
      onClick={() => setMutedState(toggleMute())}
    >
      {muted() ? t('静音', 'Muted') : t('音效', 'SFX')}
    </button>
  );
}

const TERRAIN_RESOURCE: Partial<Record<import('../generated/proto').Terrain, import('../generated/proto').Resource>> = {
  Hills: 'Brick',
  Forest: 'Lumber',
  Pasture: 'Wool',
  Fields: 'Grain',
  Mountains: 'Ore',
};

/** Floating 3D hex inspection tooltip showing terrain, roll pips and probability. */
export function TileTooltip(props: { game: Game }) {
  const hover = () => props.game.tileHover();
  return (
    <Show when={hover()}>
      {(h) => {
        const res = () => TERRAIN_RESOURCE[h().terrain];
        const pct = () => ((h().pips / 36) * 100).toFixed(1);
        const hot = () => h().number === 6 || h().number === 8;
        return (
          <div
            class="tile-tooltip"
            style={{
              left: `${Math.min(innerWidth - 190, Math.max(12, h().clientX + 14))}px`,
              top: `${Math.max(12, h().clientY - 46)}px`,
            }}
          >
            <div class="tile-tooltip-head">
              <b>{TERRAIN_NAME[h().terrain]}</b>
              <Show when={res()}>{(r) => <span class="muted">产{RESOURCE_NAME[r()]}</span>}</Show>
              <Show when={h().terrain === 'Gold'}>
                <span class="muted">产任意资源</span>
              </Show>
              <Show when={h().number > 0}>
                <span class="tile-num" classList={{ hot: hot() }}>
                  {h().number} 点
                </span>
              </Show>
            </div>
            <Show when={h().pips > 0}>
              <div class="tile-tooltip-sub muted">
                <span>{'•'.repeat(h().pips)}</span>
                <span>
                  {h().pips}/36 · {pct()}%
                </span>
              </div>
            </Show>
            <Show when={h().hasRobber}>
              <div class="tile-tooltip-warn">强盗封锁中（暂停产出）</div>
            </Show>
            <Show when={h().hasPirate}>
              <div class="tile-tooltip-warn">海盗封锁海域（禁航）</div>
            </Show>
          </div>
        );
      }}
    </Show>
  );
}

