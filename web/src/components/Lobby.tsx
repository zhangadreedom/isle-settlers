import { createSignal, For } from 'solid-js';
import type { BotKind } from '../engine';
import type { GameOptions } from '../game/controller';

const randomSeed = () => Math.floor(Math.random() * 2 ** 31);

export function Lobby(props: { onStart: (o: GameOptions) => void; seed: number; onSeed: (seed: number) => void; ready: boolean }) {
  const [players, setPlayers] = createSignal(4);
  const [bot, setBot] = createSignal<BotKind>('greedy');
  const [seat, setSeat] = createSignal<'random' | number>('random');
  const [fast, setFast] = createSignal(false);
  const reroll = () => props.onSeed(randomSeed());

  const start = (spectate: boolean) => {
    const n = players();
    const s = seat();
    const humanSeat = spectate ? null : s === 'random' ? Math.floor(Math.random() * n) : Math.min(s, n - 1);
    props.onStart({ players: n, humanSeat, bot: bot(), seed: props.seed, fast: fast() });
  };

  return (
    <div class="lobby-wrap">
      <div class="panel lobby">
        <h1>Isle Settlers</h1>
        <p class="muted">在六边形岛屿上开拓、交易、建造，先到 10 分者获胜。</p>

        <div class="field">
          <span>人数</span>
          <div class="seg">
            <For each={[2, 3, 4]}>
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
          </div>
        </div>
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
            <button onClick={reroll}>换一张</button>
          </div>
        </div>

        <div class="row lobby-actions">
          <button class="primary big" disabled={!props.ready} onClick={() => start(false)}>
            开始游戏
          </button>
          <button disabled={!props.ready} onClick={() => start(true)}>
            观战 AI 对局
          </button>
        </div>
        <p class="muted small">联机对战（邀请链接）将在下个版本推出。</p>
      </div>
    </div>
  );
}

export { randomSeed };
