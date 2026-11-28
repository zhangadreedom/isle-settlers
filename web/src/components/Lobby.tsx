import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import type { BoardRenderer } from '../board/BoardRenderer';
import { layoutFor } from '../diy/custom-map';
import { EMBEDDED } from '../embed';
import type { BotKind, BotPersonaSuffix } from '../engine';
import type { BotLevel, HouseRules, PublicRoomSummary, Scenario } from '../generated/proto';
import type { LocalOptions } from '../game/source';
import { scenarioBlurb, scenarioTitle, t } from '../i18n';
import { account, accountsEnabled } from '../net/account';
import { fetchPublicRooms, saveName, savedName } from '../net/connection';
import { canInstallPwa, promptInstallPwa } from '../pwa';
import { AccountPanel } from './Account';
import { SoundToggle, ThemeToggle } from './Hud';
import { Rankings } from './Ranking';
import { ReplayHistory } from './ReplayHistory';
import { LangToggle } from './ShortcutsModal';

const randomSeed = () => Math.floor(Math.random() * 2 ** 31);

export interface CreateRoom {
  players: number;
  bots: BotLevel;
  name: string;
  ranked: boolean;
  scenario: Scenario;
  rules: HouseRules;
  isPublic: boolean;
}

const SCENARIO_SHORT: Record<Scenario, string> = {
  Classic: '经典',
  Archipelago: '群岛',
  FogIslands: '迷雾',
  Citadels: '城邦',
  Fishermen: '渔场',
  Caravans: '商队',
};

/** Seats each scenario can be played with. */
const MAX_SEATS: Record<Scenario, number> = {
  Classic: 6,
  Archipelago: 6,
  FogIslands: 6,
  Citadels: 6,
  Fishermen: 6,
  Caravans: 6,
};

const PERSONAS: { suffix: BotPersonaSuffix; zh: string; en: string; tip: string }[] = [
  { suffix: '', zh: '均衡', en: 'Balanced', tip: '全面均衡评估局势' },
  { suffix: '@road', zh: '修路派', en: 'Road', tip: '偏爱铺路造船、跨河修桥与争夺最长商路' },
  { suffix: '@city', zh: '筑城派', en: 'City', tip: '优先控麦矿起城、升级三系科技与大都会' },
  { suffix: '@warlord', zh: '铁卫派', en: 'Warlord', tip: '积极买发展卡、招募激活铁卫与控强盗抢牌' },
  { suffix: '@merchant', zh: '皇商派', en: 'Merchant', tip: '热衷主动发起报价交易、控港口与积攒鱼/金币' },
];

export function Lobby(props: {
  ready: boolean;
  renderer?: BoardRenderer;
  seed: number;
  players: number;
  onPlayers: (players: number) => void;
  scenario: Scenario;
  onScenario: (scenario: Scenario) => void;
  customMap: string | null;
  onCustomMap: (code: string | null) => void;
  onSeed: (seed: number) => void;
  onOpenReplay: (json: string) => void;
  onOpenRules: () => void;
  onOpenWorkshop: () => void;
  onOpenDaily: () => void;
  onOpenShortcuts: () => void;
  onStartTutorial: () => void;
  onStartLocal: (o: LocalOptions) => void;
  onCreateRoom: (o: CreateRoom) => void;
  onJoinRoom: (room: string) => void;
}) {
  const [tab, setTab] = createSignal<'local' | 'online' | 'rank'>('local');
  const players = () => props.players;
  const setPlayers = (n: number) => {
    const prevLayout = layoutFor(scenario(), players());
    props.onPlayers(n);
    if (props.customMap && layoutFor(scenario(), n) !== prevLayout) props.onCustomMap(null);
  };
  const scenario = () => props.scenario;
  const setScenario = (s: Scenario) => {
    const nextPlayers = Math.min(players(), MAX_SEATS[s]);
    const prevLayout = layoutFor(scenario(), players());
    props.onScenario(s);
    if (players() > MAX_SEATS[s]) props.onPlayers(nextPlayers);
    if (props.customMap && layoutFor(s, nextPlayers) !== prevLayout) props.onCustomMap(null);
  };
  const [botBase, setBotBase] = createSignal<'weighted' | 'greedy' | 'mcts'>('greedy');
  const [persona, setPersona] = createSignal<BotPersonaSuffix>('');
  const bot = (): BotKind => (botBase() === 'weighted' ? 'weighted' : (`${botBase()}${persona()}` as BotKind));
  const [seat, setSeat] = createSignal<'random' | number>('random');
  const [fast, setFast] = createSignal(false);
  const [name, setName] = createSignal(savedName());
  const [code, setCode] = createSignal('');
  const [ranked, setRanked] = createSignal(true);
  const [isPublic, setIsPublic] = createSignal(true);
  const [publicRooms, setPublicRooms] = createSignal<PublicRoomSummary[]>([]);
  const [loadingRooms, setLoadingRooms] = createSignal(false);
  const [customVp, setCustomVp] = createSignal<number | null>(null);
  const [friendlyRobber, setFriendlyRobber] = createSignal(false);
  const [pairedTurns, setPairedTurns] = createSignal(false);
  const [eventDeck, setEventDeck] = createSignal(false);
  const [turnTimerSecs, setTurnTimerSecs] = createSignal(0);

  const refreshRooms = async () => {
    if (EMBEDDED) return;
    setLoadingRooms(true);
    try {
      setPublicRooms(await fetchPublicRooms());
    } finally {
      setLoadingRooms(false);
    }
  };

  createEffect(() => {
    if (EMBEDDED || tab() !== 'online') return;
    void refreshRooms();
    const id = window.setInterval(() => void refreshRooms(), 6000);
    onCleanup(() => window.clearInterval(id));
  });

  const rules = (): HouseRules => ({
    victoryPoints: customVp(),
    friendlyRobber: friendlyRobber(),
    pairedTurns: pairedTurns(),
    eventDeck: eventDeck(),
    turnTimerSecs: turnTimerSecs(),
    customMap: props.customMap,
  });

  const startLocal = (spectate: boolean) => {
    const n = players();
    const s = seat();
    const humanSeat = spectate ? null : s === 'random' ? Math.floor(Math.random() * n) : Math.min(s, n - 1);
    props.onStartLocal({ players: n, humanSeat, bot: bot(), seed: props.seed, fast: fast(), scenario: scenario(), rules: rules() });
  };

  const createRoom = () => {
    const signedIn = account()?.username;
    const n = signedIn ?? (name().trim() || '玩家');
    if (!signedIn) saveName(n);
    const level: Record<string, BotLevel> = { weighted: 'Easy', greedy: 'Normal', mcts: 'Hard' };
    props.onCreateRoom({
      players: players(),
      bots: level[botBase()] ?? 'Normal',
      name: n,
      ranked: ranked(),
      scenario: scenario(),
      rules: rules(),
      isPublic: isPublic(),
    });
  };

  // Accept a bare code or a pasted invite link.
  const roomCode = () => (code().match(/#\/r\/([A-Za-z0-9]+)/)?.[1] ?? code()).trim().toUpperCase();

  return (
    <div class="lobby-wrap">
      <div class="panel lobby">
        <div class="lobby-head">
          <h1>Isle Settlers</h1>
          <div class="row lobby-tools">
            <button class="ghost small" disabled={!props.ready} onClick={props.onStartTutorial} title="3 分钟手把手实战新手教学">
              {t('新手教程', 'Tutorial')}
            </button>
            <button class="ghost small" onClick={props.onOpenDaily} title="全球同种子每日固定剧本挑战与竞速榜">
              {t('每日挑战', 'Daily')}
            </button>
            <button class="ghost small" onClick={props.onOpenWorkshop} title="自定义地形、港口、数字圆片并导出 1:1 激光切割 SVG 与 3D 打印 STL">
              {t('地图工坊', 'DIY Map')}
            </button>
            <button class="ghost small" onClick={props.onOpenRules}>
              {t('规则', 'Rules')}
            </button>
            <button class="ghost small" onClick={props.onOpenShortcuts} title="键盘快捷键与色盲无障碍设置">
              ?
            </button>
            <LangToggle class="ghost small" />
            <ThemeToggle renderer={props.renderer} class="ghost small" />
            <SoundToggle class="ghost small" />
            <Show when={canInstallPwa()}>
              <button class="primary small" onClick={() => void promptInstallPwa()} title="安装为桌面/手机离线应用">
                {t('安装应用', 'Install')}
              </button>
            </Show>
          </div>
        </div>
        <p class="muted">{scenarioBlurb(scenario())}</p>

        <Show when={!EMBEDDED}>
          <div class="tabs">
            <button classList={{ on: tab() === 'local' }} onClick={() => setTab('local')}>
              {t('单机 · 对战 AI', 'Solo vs AI')}
            </button>
            <button classList={{ on: tab() === 'online' }} onClick={() => setTab('online')}>
              {t('联机对战', 'Online')}
            </button>
            <Show when={accountsEnabled()}>
              <button classList={{ on: tab() === 'rank' }} onClick={() => setTab('rank')}>
                {t('排行榜', 'Rankings')}
              </button>
            </Show>
          </div>
        </Show>

        <Show when={tab() === 'rank'}>
          <Rankings />
        </Show>

        <Show when={tab() !== 'rank'}>
          <div class="field">
            <span>{t('玩法', 'Scenario')}</span>
            <div class="seg" style={{ 'flex-wrap': 'wrap' }}>
              <button classList={{ on: scenario() === 'Classic' }} onClick={() => setScenario('Classic')}>
                {scenarioTitle('Classic')}
              </button>
              <button
                classList={{ on: scenario() === 'Archipelago' }}
                onClick={() => setScenario('Archipelago')}
                title="航海扩展：造船出海，登上新岛屿得奖励分，2–6 人"
              >
                {scenarioTitle('Archipelago')}
              </button>
              <button
                classList={{ on: scenario() === 'FogIslands' }}
                onClick={() => setScenario('FogIslands')}
                title="未知海域探索翻牌，发现新陆地得资源与登岛分，2–6 人"
              >
                {scenarioTitle('FogIslands')}
              </button>
              <button
                classList={{ on: scenario() === 'Citadels' }}
                onClick={() => setScenario('Citadels')}
                title="商品产出、三系城邦升级与大都会、实体铁卫抵御黑帆舰队，2–6 人"
              >
                {scenarioTitle('Citadels')}
              </button>
              <button
                classList={{ on: scenario() === 'Fishermen' }}
                onClick={() => setScenario('Fishermen')}
                title="沿海渔场与内陆湖产出海鱼筹码，兑换 5 种奇策并转交破旧皮靴，2–6 人"
              >
                {scenarioTitle('Fishermen')}
              </button>
              <button
                classList={{ on: scenario() === 'Caravans' }}
                onClick={() => setScenario('Caravans')}
                title="沙漠绿洲延伸骆驼商队（穿村+1分、路长翻倍），跨黄金河架设石桥赚取金币，2–6 人"
              >
                {scenarioTitle('Caravans')}
              </button>
            </div>
          </div>
          <div class="field">
            <span>人数</span>
            <div class="seg">
              <For each={[2, 3, 4, 5, 6]}>
                {(n) => (
                  <button
                    classList={{ on: players() === n }}
                    disabled={n > MAX_SEATS[scenario()]}
                    title={n > MAX_SEATS[scenario()] ? '航海地图最多 4 人' : undefined}
                    onClick={() => setPlayers(n)}
                  >
                    {n} 人
                  </button>
                )}
              </For>
            </div>
          </div>
          <div class="field">
            <span>{t('AI 难度', 'AI Level')}</span>
            <div class="seg">
              <button classList={{ on: botBase() === 'weighted' }} onClick={() => setBotBase('weighted')}>
                {t('简单', 'Easy')}
              </button>
              <button classList={{ on: botBase() === 'greedy' }} onClick={() => setBotBase('greedy')}>
                {t('普通', 'Normal')}
              </button>
              <button classList={{ on: botBase() === 'mcts' }} onClick={() => setBotBase('mcts')} title="搜索型 AI，会推演后续局面">
                {t('困难', 'Hard')}
              </button>
            </div>
          </div>
          <Show when={tab() === 'local' && botBase() !== 'weighted'}>
            <div class="field">
              <span>{t('AI 性格', 'Persona')}</span>
              <div class="seg small" style={{ 'flex-wrap': 'wrap' }}>
                <For each={PERSONAS}>
                  {(p) => (
                    <button
                      classList={{ on: persona() === p.suffix }}
                      title={p.tip}
                      onClick={() => setPersona(p.suffix)}
                    >
                      {t(p.zh, p.en)}
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>

          <details class="house-rules">
            <summary>自定义房规与计时</summary>
            <div class="rules-box">
              <div class="field">
                <span>胜利分</span>
                <div class="seg small">
                  <button classList={{ on: customVp() === null }} onClick={() => setCustomVp(null)}>
                    默认
                  </button>
                  <For each={[8, 10, 12, 13, 15]}>
                    {(vp) => (
                      <button classList={{ on: customVp() === vp }} onClick={() => setCustomVp(vp)}>
                        {vp} 分
                      </button>
                    )}
                  </For>
                </div>
              </div>
              <div class="field">
                <span>友好强盗</span>
                <div class="seg small">
                  <button classList={{ on: !friendlyRobber() }} onClick={() => setFriendlyRobber(false)}>
                    标准
                  </button>
                  <button
                    classList={{ on: friendlyRobber() }}
                    onClick={() => setFriendlyRobber(true)}
                    title="只要有其他选择，强盗与海盗不能移入公开分 ≤ 2 分玩家的地块"
                  >
                    开启（保护 ≤2 分）
                  </button>
                </div>
              </div>
              <div class="field">
                <span>点数产生</span>
                <div class="seg small">
                  <button classList={{ on: !eventDeck() }} onClick={() => setEventDeck(false)}>
                    标准双骰
                  </button>
                  <button
                    classList={{ on: eventDeck() }}
                    onClick={() => setEventDeck(true)}
                    title="使用洗匀的 36 张理论概率点数牌发点（剩 5 张重洗），消除极端脸黑"
                  >
                    36张均衡牌组
                  </button>
                </div>
              </div>
              <Show when={players() > 4}>
                <div class="field">
                  <span>5–6人规则</span>
                  <div class="seg small">
                    <button classList={{ on: !pairedTurns() }} onClick={() => setPairedTurns(false)}>
                      传统特殊建造
                    </button>
                    <button
                      classList={{ on: pairedTurns() }}
                      onClick={() => setPairedTurns(true)}
                      title="6 版规则：仅隔 3 座位的搭档进行副回合，可银行贸易与打卡"
                    >
                      双人搭档回合
                    </button>
                  </div>
                </div>
              </Show>
              <Show when={tab() === 'online'}>
                <div class="field">
                  <span>每步限时</span>
                  <div class="seg small">
                    <button classList={{ on: turnTimerSecs() === 0 }} onClick={() => setTurnTimerSecs(0)}>
                      不限时
                    </button>
                    <For each={[30, 60, 90]}>
                      {(secs) => (
                        <button classList={{ on: turnTimerSecs() === secs }} onClick={() => setTurnTimerSecs(secs)}>
                          {secs} 秒
                        </button>
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </div>
          </details>
        </Show>

        <Show when={tab() === 'local'}>
          <div class="field">
            <span>你的座位</span>
            <div class="seg">
              <button classList={{ on: seat() === 'random' }} onClick={() => setSeat('random')}>
                随机
              </button>
              <For each={[...Array(players()).keys()]}>
                {(i) => (
                  <button classList={{ on: seat() === i }} onClick={() => setSeat(i)}>
                    {i === 0 ? '先手' : `${i + 1} 号`}
                  </button>
                )}
              </For>
            </div>
          </div>
          <div class="field">
            <span>节奏</span>
            <div class="seg">
              <button classList={{ on: !fast() }} onClick={() => setFast(false)}>
                正常
              </button>
              <button classList={{ on: fast() }} onClick={() => setFast(true)}>
                快速
              </button>
            </div>
          </div>
          <div class="field">
            <span>地图</span>
            <div class="row">
              <Show
                when={props.customMap}
                fallback={
                  <>
                    <code class="seed">#{props.seed}</code>
                    <button onClick={() => props.onSeed(randomSeed())}>换一张</button>
                    <button onClick={props.onOpenWorkshop}>DIY 工坊</button>
                  </>
                }
              >
                <span class="mode-tag">DIY 自定义地图</span>
                <button onClick={props.onOpenWorkshop}>编辑</button>
                <button onClick={() => props.onCustomMap(null)}>恢复随机</button>
              </Show>
            </div>
          </div>
          <div class="row lobby-actions">
            <button class="primary big" disabled={!props.ready} onClick={() => startLocal(false)}>
              {t('开始游戏', 'Play')}
            </button>
            <button disabled={!props.ready} onClick={() => startLocal(true)}>
              {t('观战 AI 对局', 'Watch AI')}
            </button>
          </div>
          <label class="file-link muted small">
            <input
              type="file"
              accept=".json,application/json"
              disabled={!props.ready}
              onChange={async (e) => {
                const file = e.currentTarget.files?.[0];
                e.currentTarget.value = '';
                if (file) props.onOpenReplay(await file.text());
              }}
            />
            打开回放文件…
          </label>
          <ReplayHistory ready={props.ready} onOpen={props.onOpenReplay} />
        </Show>

        <Show when={tab() === 'online'}>
          <AccountPanel />
          <Show when={!account()}>
            <div class="field">
              <span>昵称</span>
              <input
                class="grow"
                value={name()}
                maxLength={16}
                placeholder="你的名字"
                onInput={(e) => setName(e.currentTarget.value)}
                aria-label="昵称"
              />
            </div>
          </Show>
          <div class="field">
            <span>模式</span>
            <div class="seg">
              <button classList={{ on: ranked() }} onClick={() => setRanked(true)} title="至少两名登录玩家时计入排位分">
                排位
              </button>
              <button classList={{ on: !ranked() }} onClick={() => setRanked(false)}>
                休闲
              </button>
            </div>
          </div>
          <div class="field">
            <span>房间列表</span>
            <div class="seg">
              <button classList={{ on: isPublic() }} onClick={() => setIsPublic(true)} title="显示在下方公开房间目录，其他玩家可直接点击加入">
                公开大厅
              </button>
              <button classList={{ on: !isPublic() }} onClick={() => setIsPublic(false)} title="仅凭房间码或邀请链接进入">
                私密邀请
              </button>
            </div>
          </div>
          <Show when={props.customMap}>
            <div class="row small">
              <span class="mode-tag">已加载 DIY 地图</span>
              <button class="small" onClick={props.onOpenWorkshop}>编辑</button>
              <button class="small" onClick={() => props.onCustomMap(null)}>清除</button>
            </div>
          </Show>
          <p class="muted small">
            创建房间后可分享邀请链接；空位开局时由 AI 补上。
            <Show when={ranked()}>排位局在至少两名登录玩家参加时计分，AI 和游客不计。</Show>
          </p>
          <div class="row lobby-actions">
            <button class="primary big" disabled={!props.ready} onClick={createRoom}>
              创建房间
            </button>
          </div>

          <div class="public-rooms">
            <div class="public-rooms-head">
              <span class="muted small">公开房间（{publicRooms().length}）</span>
              <button class="ghost small" disabled={loadingRooms()} onClick={() => void refreshRooms()}>
                {loadingRooms() ? '刷新中…' : '刷新'}
              </button>
            </div>
            <Show
              when={publicRooms().length > 0}
              fallback={<p class="muted small empty-rooms">暂无公开房间，直接创建一个或输入房间码加入。</p>}
            >
              <ul class="room-list">
                <For each={publicRooms()}>
                  {(r) => (
                    <li class="room-item">
                      <div class="room-item-info">
                        <code class="seed">{r.id}</code>
                        <b>{r.hostName}</b>
                        <span class="mode-tag">{SCENARIO_SHORT[r.scenario]}</span>
                        <Show when={r.ranked}>
                          <span class="mode-tag ranked">排位</span>
                        </Show>
                        <Show when={r.hasCustomMap}>
                          <span class="mode-tag">DIY图</span>
                        </Show>
                        <span class="muted small">
                          {r.humans}/{r.maxPlayers}人{r.started ? ' · 进行中' : ''}
                        </span>
                      </div>
                      <button
                        class={r.started ? '' : 'primary'}
                        disabled={!props.ready}
                        onClick={() => props.onJoinRoom(r.id)}
                      >
                        {r.started ? '观战' : '加入'}
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </div>
          <div class="divider">或</div>
          <form
            class="row"
            onSubmit={(e) => {
              e.preventDefault();
              if (roomCode()) props.onJoinRoom(roomCode());
            }}
          >
            <input
              class="grow"
              value={code()}
              placeholder="房间码或邀请链接"
              onInput={(e) => setCode(e.currentTarget.value)}
              aria-label="房间码"
            />
            <button type="submit" disabled={!props.ready || !roomCode()}>
              加入
            </button>
          </form>
        </Show>
      </div>
    </div>
  );
}

export { randomSeed };
