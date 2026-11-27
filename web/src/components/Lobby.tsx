import { createSignal, For, Show } from 'solid-js';
import { EMBEDDED } from '../embed';
import type { BotKind } from '../engine';
import type { BotLevel, HouseRules, Scenario } from '../generated/proto';
import type { LocalOptions } from '../game/source';
import { account, accountsEnabled } from '../net/account';
import { saveName, savedName } from '../net/connection';
import { AccountPanel } from './Account';
import { Rankings } from './Ranking';
import { ReplayHistory } from './ReplayHistory';

const randomSeed = () => Math.floor(Math.random() * 2 ** 31);

export interface CreateRoom {
  players: number;
  bots: BotLevel;
  name: string;
  ranked: boolean;
  scenario: Scenario;
  rules: HouseRules;
}

/** Seats each scenario can be played with. */
const MAX_SEATS: Record<Scenario, number> = { Classic: 6, Archipelago: 6, FogIslands: 6, Citadels: 6 };

const SCENARIO_DESC: Record<Scenario, string> = {
  Classic: '在六边形岛屿上开拓、交易、建造，先到 10 分者获胜。5–6 人时使用扩展岛屿，支持传统特殊建造或双人搭档回合。',
  Archipelago: '航海 · 群岛：从西边的主岛出发，造船驶过海峡，首次在东边外岛建村庄奖励 2 分；金矿任选资源，海盗封锁航线。先到 12 分获胜，支持 2–6 人。',
  FogIslands: '航海 · 迷雾：从月牙主岛扬帆驶入迷雾，航线触及未知格即翻开地形并获发现奖励，首次登陆迷雾新大陆奖励 2 分。先到 12 分获胜，支持 2–6 人。',
  Citadels: '城邦与铁卫：第二轮直接起城，城市产出造纸/织布/铸币升级三系科技与 4 分大都会；招募并激活实体铁卫抵御黑帆舰队入侵。先到 13 分获胜，支持 2–6 人。',
};

export function Lobby(props: {
  ready: boolean;
  seed: number;
  players: number;
  onPlayers: (players: number) => void;
  scenario: Scenario;
  onScenario: (scenario: Scenario) => void;
  onSeed: (seed: number) => void;
  onOpenReplay: (json: string) => void;
  onStartLocal: (o: LocalOptions) => void;
  onCreateRoom: (o: CreateRoom) => void;
  onJoinRoom: (room: string) => void;
}) {
  const [tab, setTab] = createSignal<'local' | 'online' | 'rank'>('local');
  const players = () => props.players;
  const setPlayers = (n: number) => props.onPlayers(n);
  const scenario = () => props.scenario;
  const setScenario = (s: Scenario) => {
    props.onScenario(s);
    if (players() > MAX_SEATS[s]) setPlayers(MAX_SEATS[s]);
  };
  const [bot, setBot] = createSignal<BotKind>('greedy');
  const [seat, setSeat] = createSignal<'random' | number>('random');
  const [fast, setFast] = createSignal(false);
  const [name, setName] = createSignal(savedName());
  const [code, setCode] = createSignal('');
  const [ranked, setRanked] = createSignal(true);
  const [customVp, setCustomVp] = createSignal<number | null>(null);
  const [friendlyRobber, setFriendlyRobber] = createSignal(false);
  const [pairedTurns, setPairedTurns] = createSignal(false);
  const [turnTimerSecs, setTurnTimerSecs] = createSignal(0);

  const rules = (): HouseRules => ({
    victoryPoints: customVp(),
    friendlyRobber: friendlyRobber(),
    pairedTurns: pairedTurns(),
    turnTimerSecs: turnTimerSecs(),
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
    props.onCreateRoom({ players: players(), bots: level[bot()] ?? 'Normal', name: n, ranked: ranked(), scenario: scenario(), rules: rules() });
  };

  // Accept a bare code or a pasted invite link.
  const roomCode = () => (code().match(/#\/r\/([A-Za-z0-9]+)/)?.[1] ?? code()).trim().toUpperCase();

  return (
    <div class="lobby-wrap">
      <div class="panel lobby">
        <h1>Isle Settlers</h1>
        <p class="muted">{SCENARIO_DESC[scenario()]}</p>

        <Show when={!EMBEDDED}>
          <div class="tabs">
            <button classList={{ on: tab() === 'local' }} onClick={() => setTab('local')}>
              单机 · 对战 AI
            </button>
            <button classList={{ on: tab() === 'online' }} onClick={() => setTab('online')}>
              联机对战
            </button>
            <Show when={accountsEnabled()}>
              <button classList={{ on: tab() === 'rank' }} onClick={() => setTab('rank')}>
                排行榜
              </button>
            </Show>
          </div>
        </Show>

        <Show when={tab() === 'rank'}>
          <Rankings />
        </Show>

        <Show when={tab() !== 'rank'}>
          <div class="field">
            <span>玩法</span>
            <div class="seg">
              <button classList={{ on: scenario() === 'Classic' }} onClick={() => setScenario('Classic')}>
                经典
              </button>
              <button
                classList={{ on: scenario() === 'Archipelago' }}
                onClick={() => setScenario('Archipelago')}
                title="航海扩展：造船出海，登上新岛屿得奖励分，2–4 人"
              >
                航海 · 群岛
              </button>
              <button
                classList={{ on: scenario() === 'FogIslands' }}
                onClick={() => setScenario('FogIslands')}
                title="未知海域探索翻牌，发现新陆地得资源与登岛分，2–6 人"
              >
                航海 · 迷雾
              </button>
              <button
                classList={{ on: scenario() === 'Citadels' }}
                onClick={() => setScenario('Citadels')}
                title="商品产出、三系城邦升级与大都会、实体铁卫抵御黑帆舰队，2–6 人"
              >
                城邦与铁卫
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
            <span>AI 难度</span>
            <div class="seg">
              <button classList={{ on: bot() === 'weighted' }} onClick={() => setBot('weighted')}>
                简单
              </button>
              <button classList={{ on: bot() === 'greedy' }} onClick={() => setBot('greedy')}>
                普通
              </button>
              <button classList={{ on: bot() === 'mcts' }} onClick={() => setBot('mcts')} title="搜索型 AI，会推演后续局面">
                困难
              </button>
            </div>
          </div>

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
              <code class="seed">#{props.seed}</code>
              <button onClick={() => props.onSeed(randomSeed())}>换一张</button>
            </div>
          </div>
          <div class="row lobby-actions">
            <button class="primary big" disabled={!props.ready} onClick={() => startLocal(false)}>
              开始游戏
            </button>
            <button disabled={!props.ready} onClick={() => startLocal(true)}>
              观战 AI 对局
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
          <p class="muted small">
            创建房间后把邀请链接发给朋友；没人坐的位置开局时由 AI 补上。
            <Show when={ranked()}>排位局在至少两名登录玩家参加时计分，AI 和游客不计。</Show>
          </p>
          <div class="row lobby-actions">
            <button class="primary big" disabled={!props.ready} onClick={createRoom}>
              创建房间
            </button>
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
