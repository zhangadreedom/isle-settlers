import { createSignal, For, Show } from 'solid-js';
import { EMBEDDED } from '../embed';
import type { BotKind } from '../engine';
import type { BotLevel } from '../generated/proto';
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
}

export function Lobby(props: {
  ready: boolean;
  seed: number;
  players: number;
  onPlayers: (players: number) => void;
  onSeed: (seed: number) => void;
  onOpenReplay: (json: string) => void;
  onStartLocal: (o: LocalOptions) => void;
  onCreateRoom: (o: CreateRoom) => void;
  onJoinRoom: (room: string) => void;
}) {
  const [tab, setTab] = createSignal<'local' | 'online' | 'rank'>('local');
  const players = () => props.players;
  const setPlayers = (n: number) => props.onPlayers(n);
  const [bot, setBot] = createSignal<BotKind>('greedy');
  const [seat, setSeat] = createSignal<'random' | number>('random');
  const [fast, setFast] = createSignal(false);
  const [name, setName] = createSignal(savedName());
  const [code, setCode] = createSignal('');
  const [ranked, setRanked] = createSignal(true);

  const startLocal = (spectate: boolean) => {
    const n = players();
    const s = seat();
    const humanSeat = spectate ? null : s === 'random' ? Math.floor(Math.random() * n) : Math.min(s, n - 1);
    props.onStartLocal({ players: n, humanSeat, bot: bot(), seed: props.seed, fast: fast() });
  };

  const createRoom = () => {
    const signedIn = account()?.username;
    const n = signedIn ?? (name().trim() || '玩家');
    if (!signedIn) saveName(n);
    const level: Record<string, BotLevel> = { weighted: 'Easy', greedy: 'Normal', mcts: 'Hard' };
    props.onCreateRoom({ players: players(), bots: level[bot()] ?? 'Normal', name: n, ranked: ranked() });
  };

  // Accept a bare code or a pasted invite link.
  const roomCode = () => (code().match(/#\/r\/([A-Za-z0-9]+)/)?.[1] ?? code()).trim().toUpperCase();

  return (
    <div class="lobby-wrap">
      <div class="panel lobby">
        <h1>Isle Settlers</h1>
        <p class="muted">在六边形岛屿上开拓、交易、建造，先到 10 分者获胜。5–6 人时使用扩展岛屿，每回合结束后其他人可以特殊建造。</p>

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
            <span>人数</span>
            <div class="seg">
              <For each={[2, 3, 4, 5, 6]}>
                {(n) => (
                  <button classList={{ on: players() === n }} onClick={() => setPlayers(n)}>
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
