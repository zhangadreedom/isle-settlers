import { createEffect, createSignal, onCleanup, onMount, Show } from 'solid-js';
import type { BoardRenderer, Insets, PickKind } from './board/BoardRenderer';
import { DiceView, Flights, GameLog, HandBar, PlayerList } from './components/Hud';
import { Lobby, randomSeed } from './components/Lobby';
import { DiscardModal, GameOverModal, MonopolyModal, TradeModal, VictimModal, YearOfPlentyModal } from './components/Modals';
import { loadEngine, Match, type Topology } from './engine';
import { boardData, createGame, type Game, type GameOptions } from './game/controller';
import type { DevCard } from './generated/proto';

declare global {
  interface Window {
    /** Test hook, present with `?debug` in the URL. */
    __isle?: { game: () => Game | null; project: (kind: PickKind, id: number) => { x: number; y: number }; topology: Topology };
  }
}

/**
 * Screen space taken by HUD panels marked `data-inset`. Each panel is
 * attributed to the side where it hides the least of the screen.
 */
function measureInsets(): Insets {
  const w = innerWidth;
  const h = innerHeight;
  const insets: Insets = { top: 0, right: 0, bottom: 0, left: 0 };
  for (const el of document.querySelectorAll<HTMLElement>('[data-inset]')) {
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height) continue;
    const options: [keyof Insets, number][] = [
      ['top', r.bottom],
      ['bottom', h - r.top],
      ['left', r.right],
      ['right', w - r.left],
    ];
    const area = ([side, extent]: [keyof Insets, number]) => extent * (side === 'top' || side === 'bottom' ? w : h);
    const [side, extent] = options.reduce((a, b) => (area(b) < area(a) ? b : a));
    insets[side] = Math.max(insets[side], extent + 8);
  }
  return insets;
}

export default function App() {
  let stage!: HTMLDivElement;
  const params = new URLSearchParams(location.search);
  const [renderer, setRenderer] = createSignal<BoardRenderer>();
  const [topology, setTopology] = createSignal<Topology>();
  const [game, setGame] = createSignal<Game | null>(null);
  const [status, setStatus] = createSignal('加载规则引擎…');
  const [backend, setBackend] = createSignal('');
  const [previewSeed, setPreviewSeed] = createSignal(randomSeed());
  const [tradeOpen, setTradeOpen] = createSignal(false);
  const [devModal, setDevModal] = createSignal<'YearOfPlenty' | 'Monopoly' | null>(null);
  let lastOptions: GameOptions | null = null;

  onMount(async () => {
    try {
      const topo = await loadEngine();
      setTopology(topo);
      setStatus('加载 3D 场景…');
      // Three.js lives in its own chunk, loaded after the lobby is on screen.
      const { BoardRenderer } = await import('./board/BoardRenderer');
      const r = await BoardRenderer.create(stage, topo, params.get('renderer') === 'webgl');
      r.onBackendChange = setBackend;
      setBackend(r.backend);
      setRenderer(r);
      setStatus('');
      if (params.has('debug')) window.__isle = { game, project: (k, id) => r.project(k, id), topology: topo };
    } catch (err) {
      console.error(err);
      setStatus(`初始化失败：${err instanceof Error ? err.message : String(err)}`);
    }
  });

  // In the lobby, show the map that will be played.
  createEffect(() => {
    const r = renderer();
    const seed = previewSeed();
    if (!r || game()) return;
    const m = new Match(4, seed, null, 'greedy');
    r.setBoard(boardData(m.board));
    const v = m.view();
    r.setPieces({ buildings: v.buildings, roads: v.roads, robber: v.robber }, false);
    m.free();
  });

  // Keep the island clear of the HUD while a game is on.
  createEffect(() => {
    const r = renderer();
    if (!r) return;
    if (!game()) {
      r.setInsets({ top: 0, right: 0, bottom: 0, left: 0 });
      return;
    }
    let frame = 0;
    const fit = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => r.setInsets(measureInsets()));
    };
    const observer = new ResizeObserver(fit);
    requestAnimationFrame(() => document.querySelectorAll('[data-inset]').forEach((el) => observer.observe(el)));
    window.addEventListener('resize', fit);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener('resize', fit);
      cancelAnimationFrame(frame);
    });
  });

  // Close the trade window once it is no longer the human's main phase.
  createEffect(() => {
    const g = game();
    if (!g) return;
    const v = g.view();
    if (!(v.phase.type === 'Main' && v.current === g.me)) {
      setTradeOpen(false);
      setDevModal(null);
    }
  });

  const onKey = (e: KeyboardEvent) => {
    const g = game();
    if (!g || (e.target as HTMLElement).closest('input, select, textarea')) return;
    if (e.key === 'Escape') {
      g.setMode(null);
      setTradeOpen(false);
      setDevModal(null);
    } else if (e.key === ' ' && g.has('RollDice')) {
      e.preventDefault();
      g.act((a) => a.type === 'RollDice');
    }
  };
  window.addEventListener('keydown', onKey);

  onCleanup(() => {
    window.removeEventListener('keydown', onKey);
    game()?.dispose();
    renderer()?.dispose();
  });

  function start(o: GameOptions) {
    const r = renderer();
    const t = topology();
    if (!r || !t) return;
    lastOptions = o;
    game()?.dispose();
    setTradeOpen(false);
    setGame(createGame(o, r, t));
  }

  function toLobby() {
    game()?.dispose();
    setGame(null);
    setPreviewSeed(randomSeed());
  }

  function playDevCard(g: Game, card: DevCard) {
    if (card === 'Knight') g.act((a) => a.type === 'PlayKnight');
    else if (card === 'RoadBuilding') g.act((a) => a.type === 'PlayRoadBuilding');
    else if (card === 'YearOfPlenty' || card === 'Monopoly') setDevModal(card);
  }

  return (
    <div class="app">
      <div class="stage" ref={stage} />

      <Show when={game()} fallback={<Lobby ready={!!renderer()} onStart={start} seed={previewSeed()} onSeed={setPreviewSeed} />}>
        {(g) => (
          <>
            <header class="topbar">
              <button class="ghost" onClick={toLobby}>
                ← 大厅
              </button>
            </header>
            <DiceView game={g()} />
            <PlayerList game={g()} />
            <GameLog game={g()} />
            <HandBar game={g()} onTrade={() => setTradeOpen(true)} onDevCard={(c) => playDevCard(g(), c)} />
            <Flights game={g()} />

            <Show when={g().me !== null && g().view().phase.type === 'Discard' && g().view().pendingDiscard[g().me!] > 0}>
              <DiscardModal game={g()} />
            </Show>
            <VictimModal game={g()} />
            <Show when={tradeOpen()}>
              <TradeModal game={g()} onClose={() => setTradeOpen(false)} />
            </Show>
            <Show when={devModal() === 'YearOfPlenty'}>
              <YearOfPlentyModal game={g()} onClose={() => setDevModal(null)} />
            </Show>
            <Show when={devModal() === 'Monopoly'}>
              <MonopolyModal game={g()} onClose={() => setDevModal(null)} />
            </Show>
            <Show when={g().view().winner !== null}>
              <GameOverModal game={g()} onLobby={toLobby} onAgain={() => lastOptions && start({ ...lastOptions, seed: randomSeed() })} />
            </Show>
            <Show when={g().toast()}>
              <div class="toast" role="status">
                {g().toast()}
              </div>
            </Show>
          </>
        )}
      </Show>

      <footer class="renderer-info muted">{backend() ? `${backend()} · 按需渲染` : ''}</footer>
      <Show when={status()}>
        <div class="loading">{status()}</div>
      </Show>
    </div>
  );
}
