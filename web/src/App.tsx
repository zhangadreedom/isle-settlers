import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { loadEngine, type Engine, type Game } from './engine';
import { PLAYER_CSS } from './board/palette';
import type { BoardRenderer } from './board/BoardRenderer';

const randomSeed = () => Math.floor(Math.random() * 1_000_000);

export default function App() {
  let stage!: HTMLDivElement;
  let engine: Engine | undefined;
  let game: Game | undefined;
  let renderer: BoardRenderer | undefined;
  let timer: number | undefined;

  const [status, setStatus] = createSignal('加载规则引擎…');
  const [seed, setSeed] = createSignal(randomSeed());
  const [players, setPlayers] = createSignal(4);
  const [points, setPoints] = createSignal<number[]>([]);
  const [hands, setHands] = createSignal<number[]>([]);
  const [turn, setTurn] = createSignal(0);
  const [winner, setWinner] = createSignal(-1);
  const [playing, setPlaying] = createSignal(false);
  const [backend, setBackend] = createSignal('');
  const [drawCalls, setDrawCalls] = createSignal(0);
  const [bench, setBench] = createSignal('');

  onMount(async () => {
    try {
      const loaded = await loadEngine();
      engine = loaded.engine;
      setStatus('加载 3D 场景…');
      // Three.js lives in its own chunk, loaded after the HUD is on screen.
      const { BoardRenderer } = await import('./board/BoardRenderer');
      const forceWebGL = new URLSearchParams(location.search).get('renderer') === 'webgl';
      renderer = await BoardRenderer.create(stage, loaded.topology, forceWebGL);
      setBackend(renderer.backend);
      renderer.onBackendChange = setBackend;
      newGame();
      setStatus('');
    } catch (err) {
      console.error(err);
      setStatus(`初始化失败：${err instanceof Error ? err.message : String(err)}`);
    }
  });

  onCleanup(() => {
    stop();
    renderer?.dispose();
    game?.free();
  });

  function sync() {
    if (!game || !renderer) return;
    renderer.setPieces({ buildings: game.buildings(), roads: game.roads(), robber: game.robber });
    setPoints(Array.from(game.victoryPoints()));
    setHands(Array.from(game.handSizes()));
    setTurn(game.turn);
    setWinner(game.winner);
    requestAnimationFrame(() => setDrawCalls(renderer?.drawCalls ?? 0));
  }

  function newGame() {
    if (!engine || !renderer) return;
    stop();
    game?.free();
    game = new engine.Game(players(), seed());
    renderer.setBoard({ terrain: game.terrain(), numbers: game.numbers(), harbors: game.harbors() });
    sync();
  }

  function start() {
    if (!game) return;
    if (game.winner >= 0) newGame();
    setPlaying(true);
    const tick = () => {
      const running = game!.step(2);
      sync();
      if (running) timer = window.setTimeout(tick, 45);
      else stop();
    };
    tick();
  }

  function stop() {
    window.clearTimeout(timer);
    setPlaying(false);
  }

  function finish() {
    stop();
    game?.step(1_000_000);
    sync();
  }

  async function runBenchmark() {
    if (!engine) return;
    setBench('运行中…');
    await new Promise((r) => setTimeout(r, 30));
    const games = 1000;
    const t0 = performance.now();
    const actions = engine.runBenchmark(games, seed());
    const secs = (performance.now() - t0) / 1000;
    setBench(`WASM：${Math.round(games / secs).toLocaleString()} 局/秒 · ${(actions / secs / 1e6).toFixed(1)}M 动作/秒`);
  }

  return (
    <div class="app">
      <div class="stage" ref={stage} />

      <header class="panel controls">
        <div class="title">
          <h1>Isle Settlers</h1>
          <span class="tag">技术骨架 v0.1</span>
        </div>
        <div class="row">
          <label>
            种子
            <input
              type="number"
              value={seed()}
              onChange={(e) => {
                setSeed(Number(e.currentTarget.value) || 0);
                newGame();
              }}
            />
          </label>
          <label>
            人数
            <select
              value={players()}
              onChange={(e) => {
                setPlayers(Number(e.currentTarget.value));
                newGame();
              }}
            >
              <option value="2">2</option>
              <option value="3">3</option>
              <option value="4">4</option>
            </select>
          </label>
          <button
            onClick={() => {
              setSeed(randomSeed());
              newGame();
            }}
          >
            新地图
          </button>
        </div>
        <div class="row">
          <button class="primary" onClick={() => (playing() ? stop() : start())}>
            {playing() ? '暂停' : 'AI 自动对局'}
          </button>
          <button onClick={finish}>直接到终局</button>
          <button onClick={runBenchmark}>浏览器基准</button>
        </div>
        <Show when={bench()}>
          <p class="muted">{bench()}</p>
        </Show>
      </header>

      <aside class="panel scoreboard">
        <div class="muted">第 {turn()} 回合</div>
        <For each={points()}>
          {(vp, i) => (
            <div class="player" classList={{ winner: winner() === i() }}>
              <span class="dot" style={{ background: PLAYER_CSS[i()] }} />
              <span class="name">AI {i() + 1}</span>
              <span class="cards muted">{hands()[i()]} 张</span>
              <b>{vp} 分</b>
            </div>
          )}
        </For>
        <Show when={winner() >= 0}>
          <div class="result">AI {winner() + 1} 获胜</div>
        </Show>
      </aside>

      <footer class="badge muted">
        {backend() ? `${backend()} · 按需渲染 · ${drawCalls()} 次绘制` : ''}
      </footer>

      <Show when={status()}>
        <div class="loading">{status()}</div>
      </Show>
    </div>
  );
}
