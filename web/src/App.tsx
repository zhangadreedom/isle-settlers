import { createEffect, createSignal, onCleanup, onMount, Show } from 'solid-js';
import type { BoardRenderer, Insets, PickKind } from './board/BoardRenderer';
import { DiceView, Flights, GameLog, HandBar, PlayerList } from './components/Hud';
import { Lobby, randomSeed, type CreateRoom } from './components/Lobby';
import { DiscardModal, GameOverModal, IncomingTrade, MonopolyModal, TradeModal, VictimModal, YearOfPlentyModal } from './components/Modals';
import { JoinPrompt, RoomLobby } from './components/Room';
import { EngineError, loadEngine, Match, Replay, topology } from './engine';
import { boardData, createGame, type Game } from './game/controller';
import { createLocalSource, createOnlineSource, createReplaySource, replayUrl, type LocalOptions } from './game/source';
import { EMBEDDED } from './embed';
import { errorText } from './game/text';
import { saveLocalReplay } from './game/replay-history';
import type { DevCard, ReplayRecord } from './generated/proto';
import { initAccounts } from './net/account';
import { createConnection, savedName, savedToken, type Connection } from './net/connection';

declare global {
  interface Window {
    /** Test hook, present with `?debug` in the URL. */
    __isle?: {
      game: () => Game | null;
      conn: () => Connection | null;
      project: (kind: PickKind, id: number) => { x: number; y: number };
    };
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

/** Room id from an invite link (`#/r/ID`). */
const roomFromHash = () => location.hash.match(/^#\/r\/([A-Za-z0-9]+)/)?.[1]?.toUpperCase() ?? null;
/** Replay id from a replay link (`#/replay/ID`). */
const replayFromHash = () => location.hash.match(/^#\/replay\/([A-Za-z0-9]+)/)?.[1]?.toUpperCase() ?? null;

export default function App() {
  let stage!: HTMLDivElement;
  const params = new URLSearchParams(location.search);
  const [renderer, setRenderer] = createSignal<BoardRenderer>();
  const [game, setGame] = createSignal<Game | null>(null);
  const [conn, setConn] = createSignal<Connection | null>(null);
  const [joinRoom, setJoinRoom] = createSignal<string | null>(null);
  const [status, setStatus] = createSignal('加载规则引擎…');
  const [backend, setBackend] = createSignal('');
  const [previewSeed, setPreviewSeed] = createSignal(randomSeed());
  const [previewPlayers, setPreviewPlayers] = createSignal(4);
  const [tradeOpen, setTradeOpen] = createSignal(false);
  const [devModal, setDevModal] = createSignal<'YearOfPlenty' | 'Monopoly' | null>(null);
  // The log (with chat) starts open on wide screens and as a toggle on phones.
  const [logOpen, setLogOpen] = createSignal(window.innerWidth > 720);
  let lastLocal: LocalOptions | null = null;

  onMount(async () => {
    // Accounts belong to the server; the embedded build has none.
    if (!EMBEDDED) void initAccounts();
    try {
      await loadEngine();
      setStatus('加载 3D 场景…');
      // Three.js lives in its own chunk, loaded after the lobby is on screen.
      const { BoardRenderer } = await import('./board/BoardRenderer');
      const r = await BoardRenderer.create(stage, EMBEDDED || params.get('renderer') === 'webgl');
      r.onBackendChange = setBackend;
      setBackend(r.backend);
      setRenderer(r);
      setStatus('');
      if (params.has('debug')) window.__isle = { game, conn, project: (k, id) => r.project(k, id) };
      openInvite();
    } catch (err) {
      console.error(err);
      setStatus(`初始化失败：${err instanceof Error ? err.message : String(err)}`);
    }
  });

  // ---- navigation ----

  /** Follows an invite or replay link: rejoin silently if we have a seat, else ask for a name. */
  function openInvite() {
    const replayId = replayFromHash();
    if (replayId) {
      void openStoredReplay(replayId);
      return;
    }
    const room = roomFromHash();
    if (!room || room === conn()?.roomId()) return;
    leave(false);
    if (savedToken(room)) connect({ type: 'Join', room, name: savedName() || '玩家' });
    else setJoinRoom(room);
  }
  window.addEventListener('hashchange', openInvite);

  function connect(hello: Parameters<typeof createConnection>[0]) {
    setJoinRoom(null);
    setConn(createConnection(hello));
  }

  function createRoom(o: CreateRoom) {
    connect({ type: 'Create', ...o });
  }

  async function startLocal(o: LocalOptions) {
    const r = renderer();
    if (!r) return;
    lastLocal = o;
    game()?.dispose();
    setGame(null);
    setTradeOpen(false);
    const source = await createLocalSource(o, saveLocalReplay);
    setGame(createGame(source, r));
  }

  /** Opens the replay viewer on a record (JSON text). */
  function openReplay(json: string, link: string | null = null): boolean {
    const r = renderer();
    if (!r) return false;
    let replay: Replay;
    try {
      replay = new Replay(json);
    } catch (e) {
      setStatus('');
      setReplayError(errorText(e instanceof EngineError ? e.code : String(e)));
      return false;
    }
    game()?.dispose();
    conn()?.close();
    setConn(null);
    setJoinRoom(null);
    setTradeOpen(false);
    // Unmount the current game's HUD first: its components hold on to the
    // game object they were created with.
    setGame(null);
    queueMicrotask(() => setGame(createGame(createReplaySource(replay, link), r)));
    return true;
  }

  function watchReplay(record: ReplayRecord, link: string | null) {
    if (!openReplay(JSON.stringify(record), link)) return;
    // The address bar shows the replay link (or nothing for a local game).
    history.replaceState(null, '', link ?? location.pathname + location.search);
  }

  /** A replay stored by the server (`#/replay/ID`). */
  async function openStoredReplay(id: string) {
    setStatus('加载回放…');
    try {
      const res = await fetch(`/api/replays/${id}`);
      if (!res.ok) throw new EngineError('ReplayNotFound');
      openReplay(await res.text(), replayUrl(id));
    } catch (e) {
      setReplayError(errorText(e instanceof EngineError ? e.code : 'ReplayNotFound'));
    } finally {
      setStatus('');
    }
  }

  /** Back to the main menu. */
  function leave(clearHash = true) {
    game()?.dispose();
    conn()?.close();
    setGame(null);
    setConn(null);
    setJoinRoom(null);
    setTradeOpen(false);
    if (clearHash && location.hash) history.replaceState(null, '', location.pathname + location.search);
    setPreviewSeed(randomSeed());
  }

  const [replayError, setReplayError] = createSignal<string | null>(null);

  // Once connected, the address bar holds the invite link.
  createEffect(() => {
    const c = conn();
    if (c?.status() === 'open' && c.roomId()) {
      const hash = `#/r/${c.roomId()}`;
      if (location.hash !== hash) history.replaceState(null, '', location.pathname + location.search + hash);
    }
  });

  // The server sends a snapshot when the game starts (or on rejoin).
  createEffect(() => {
    const c = conn();
    const snapshot = c?.snapshot();
    const r = renderer();
    if (!c || !snapshot || !r || game()) return;
    setGame(createGame(createOnlineSource(c, snapshot), r));
  });

  // In the menus, show a map in the background.
  createEffect(() => {
    const r = renderer();
    const seed = previewSeed();
    const players = previewPlayers();
    if (!r || game()) return;
    const m = new Match(players, seed, null, 'greedy');
    r.setBoard(boardData(m.board), topology(m.board.layout));
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
    if (!((v.phase.type === 'Main' && v.current === g.me) || (v.phase.type === 'SpecialBuild' && v.toMove === g.me))) {
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
    } else if (e.key === ' ' && g.replay) {
      // A focused button handles Space itself.
      if ((e.target as HTMLElement).closest('button')) return;
      e.preventDefault();
      g.replay.toggle();
    } else if ((e.key === 'ArrowRight' || e.key === 'ArrowLeft') && g.replay) {
      g.replay.step(e.key === 'ArrowRight' ? 1 : -1);
    } else if (e.key === ' ' && g.has('RollDice')) {
      e.preventDefault();
      g.act((a) => a.type === 'RollDice');
    }
  };
  window.addEventListener('keydown', onKey);

  onCleanup(() => {
    window.removeEventListener('keydown', onKey);
    window.removeEventListener('hashchange', openInvite);
    game()?.dispose();
    conn()?.close();
    renderer()?.dispose();
  });

  function playDevCard(g: Game, card: DevCard) {
    if (card === 'Knight') g.act((a) => a.type === 'PlayKnight');
    else if (card === 'RoadBuilding') g.act((a) => a.type === 'PlayRoadBuilding');
    else if (card === 'YearOfPlenty' || card === 'Monopoly') setDevModal(card);
  }

  const menu = () => (
    <Show
      when={conn()}
      fallback={
        <Show
          when={joinRoom()}
          fallback={
            <Lobby
              ready={!!renderer()}
              seed={previewSeed()}
              players={previewPlayers()}
              onPlayers={setPreviewPlayers}
              onOpenReplay={(json) => openReplay(json)}
              onSeed={setPreviewSeed}
              onStartLocal={startLocal}
              onCreateRoom={createRoom}
              onJoinRoom={(room) => (savedToken(room) ? connect({ type: 'Join', room, name: savedName() || '玩家' }) : setJoinRoom(room))}
            />
          }
        >
          {(room) => <JoinPrompt room={room()} onJoin={(name) => connect({ type: 'Join', room: room(), name })} onCancel={() => leave()} />}
        </Show>
      }
    >
      {(c) => <RoomLobby conn={c()} onLeave={() => leave()} />}
    </Show>
  );

  return (
    <div class="app">
      <div class="stage" ref={stage} />

      <Show when={game()} fallback={menu()}>
        {(g) => (
          <>
            <header class="topbar">
              <button class="ghost" onClick={() => leave()}>
                ← 大厅
              </button>
              <Show when={conn()}>
                {(c) => <span class="room-tag muted">房间 {c().roomId()}</span>}
              </Show>
              <Show when={g().replay}>
                <span class="room-tag muted">回放</span>
              </Show>
              <button class="ghost log-toggle" onClick={() => setLogOpen(!logOpen())} aria-expanded={logOpen()}>
                {logOpen() ? '收起记录' : g().online ? '记录 · 聊天' : '记录'}
              </button>
            </header>
            <DiceView game={g()} />
            <PlayerList game={g()} />
            <GameLog game={g()} open={logOpen()} onToggle={() => setLogOpen(!logOpen())} />
            <HandBar game={g()} onTrade={() => setTradeOpen(true)} onDevCard={(c) => playDevCard(g(), c)} />
            <Flights game={g()} />

            <Show when={!g().replay && g().me !== null && g().view().phase.type === 'Discard' && g().view().pendingDiscard[g().me!] > 0}>
              <DiscardModal game={g()} />
            </Show>
            <VictimModal game={g()} />
            <Show when={!g().replay}>
              <IncomingTrade game={g()} />
            </Show>
            <Show when={tradeOpen()}>
              <TradeModal game={g()} onClose={() => setTradeOpen(false)} />
            </Show>
            <Show when={devModal() === 'YearOfPlenty'}>
              <YearOfPlentyModal game={g()} onClose={() => setDevModal(null)} />
            </Show>
            <Show when={devModal() === 'Monopoly'}>
              <MonopolyModal game={g()} onClose={() => setDevModal(null)} />
            </Show>
            <Show when={g().view().winner !== null && !g().replay}>
              <GameOverModal
                game={g()}
                onLobby={() => leave()}
                onAgain={g().online ? undefined : () => lastLocal && startLocal({ ...lastLocal, seed: randomSeed() })}
                onWatchReplay={watchReplay}
              />
            </Show>
            <Show when={g().toast()}>
              <div class="toast" role="status">
                {g().toast()}
              </div>
            </Show>
          </>
        )}
      </Show>

      <Show when={replayError()}>
        {(msg) => (
          <div class="banner" role="alert" onClick={() => setReplayError(null)}>
            {msg()}（点击关闭）
          </div>
        )}
      </Show>
      <Show when={conn()?.status() === 'reconnecting'}>
        <div class="banner" role="status">
          连接中断，正在重连…
        </div>
      </Show>
      <footer class="renderer-info muted">{backend() ? `${backend()} · 按需渲染` : ''}</footer>
      <Show when={status()}>
        <div class="loading">{status()}</div>
      </Show>
    </div>
  );
}
