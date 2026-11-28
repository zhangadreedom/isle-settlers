import { batch, createEffect, createSignal, onCleanup, onMount, Show } from 'solid-js';
import { toggleMute } from './audio/sound';
import { isColorblindMode, onColorblindChange, toggleColorblindMode } from './board/a11y';
import type { BoardRenderer, Insets, PickKind } from './board/BoardRenderer';
import { DailyChallengeModal, SetupAdvisorOverlay } from './components/DailyChallenge';
import { DiceView, Flights, GameLog, HandBar, PlayerList, SoundToggle, ThemeToggle, TileTooltip } from './components/Hud';
import { Lobby, randomSeed, type CreateRoom } from './components/Lobby';
import { MapWorkshopModal } from './components/MapWorkshop';
import {
  CityUpgradeModal,
  CoinMarketModal,
  DiscardModal,
  ExplorersModal,
  FishMarketModal,
  GameOverModal,
  GoldModal,
  IncomingTrade,
  MonopolyModal,
  RouteChoiceModal,
  TradeModal,
  VictimModal,
  YearOfPlentyModal,
} from './components/Modals';
import { QuickChatTrigger, SeatSpeechBubbles } from './components/QuickChatBar';
import { JoinPrompt, RoomLobby } from './components/Room';
import { RulesModal } from './components/RulesDrawer';
import { LangToggle, ShortcutsModal } from './components/ShortcutsModal';
import { TUTORIAL_PRESET, TutorialCoach } from './components/Tutorial';
import { EngineError, loadEngine, Match, previewCustomMap, Replay, topology } from './engine';
import { boardData, createGame, type Game } from './game/controller';
import { recordDailyResult, type DailyChallengeSpec } from './game/daily';
import { createLocalSource, createOnlineSource, createReplaySource, replayUrl, type LocalOptions } from './game/source';
import { EMBEDDED } from './embed';
import { errorText } from './game/text';
import { saveLocalReplay } from './game/replay-history';
import { t } from './i18n';
import type { DevCard, HouseRules, ReplayRecord, Scenario } from './generated/proto';
import { account, initAccounts } from './net/account';
import { createConnection, savedName, savedToken, type Connection } from './net/connection';
import { autoSubmitDailyReplay } from './net/daily-api';
import { initPwa } from './pwa';

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
  const [previewScenario, setPreviewScenario] = createSignal<Scenario>('Classic');
  const [customMap, setCustomMap] = createSignal<string | null>(null);
  const [rulesOpen, setRulesOpen] = createSignal(false);
  const [workshopOpen, setWorkshopOpen] = createSignal(false);
  const [dailyOpen, setDailyOpen] = createSignal(false);
  const [shortcutsOpen, setShortcutsOpen] = createSignal(false);
  const [tutorialActive, setTutorialActive] = createSignal(false);
  const [colorblind, setColorblind] = createSignal(isColorblindMode());
  const [tradeOpen, setTradeOpen] = createSignal(false);
  const [citadelOpen, setCitadelOpen] = createSignal(false);
  const [fishOpen, setFishOpen] = createSignal(false);
  const [coinOpen, setCoinOpen] = createSignal(false);
  const [explorersOpen, setExplorersOpen] = createSignal(false);
  const [devModal, setDevModal] = createSignal<'YearOfPlenty' | 'Monopoly' | null>(null);
  // The log (with chat) starts open on wide screens and as a toggle on phones.
  const [logOpen, setLogOpen] = createSignal(window.innerWidth > 720);
  let lastLocal: LocalOptions | null = null;
  let activeDailyKey: string | null = null;
  onCleanup(onColorblindChange(setColorblind));

  onMount(async () => {
    if (!EMBEDDED) {
      initPwa();
      void initAccounts();
    }
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

  async function startLocal(o: LocalOptions, dailyDateKey: string | null = null, isTutorial = false) {
    const r = renderer();
    if (!r) return;
    lastLocal = o;
    activeDailyKey = dailyDateKey;
    setTutorialActive(isTutorial);
    game()?.dispose();
    setGame(null);
    setTradeOpen(false);
    setCitadelOpen(false);
    setFishOpen(false);
    setCoinOpen(false);
    setExplorersOpen(false);
    const source = await createLocalSource(o, (rec) => {
      saveLocalReplay(rec);
      if (activeDailyKey && o.humanSeat !== null) {
        const won = rec.winner === o.humanSeat;
        const g = game();
        const turns = g?.view().turn ?? 0;
        const pts = g?.view().finalPoints?.[o.humanSeat] ?? g?.view().me?.victoryPoints ?? 0;
        recordDailyResult(activeDailyKey, won, turns, pts);
        void autoSubmitDailyReplay(activeDailyKey, o.humanSeat, rec);
      }
    });
    setGame(createGame(source, r));
  }

  /** Opens the replay viewer on a record (JSON text), optionally seeking straight to `initialStep`. */
  function openReplay(json: string, link: string | null = null, initialStep?: number): boolean {
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
    queueMicrotask(() => {
      const src = createReplaySource(replay, link);
      const next = createGame(src, r);
      if (initialStep !== undefined) src.replay?.seek(initialStep);
      setGame(next);
    });
    return true;
  }

  function watchReplay(record: ReplayRecord, link: string | null, initialStep?: number) {
    if (!openReplay(JSON.stringify(record), link, initialStep)) return;
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
    // One update: with the game gone but the connection still set, the
    // snapshot effect below would rebuild a game from the old snapshot.
    batch(() => {
      setConn(null);
      setGame(null);
      setJoinRoom(null);
      setTradeOpen(false);
    });
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

  // The server sends a snapshot when the game starts, on rejoin, or on an immediate rematch.
  let mountedSnapshot: unknown = null;
  createEffect(() => {
    const c = conn();
    const room = c?.room();
    const snapshot = c?.snapshot();
    const r = renderer();
    const cur = game();
    if (!c || c.status() === 'closed') return;
    // Host returned the room to the pre-game lobby after GameOver.
    if (room && !room.started && cur?.online) {
      cur.dispose();
      mountedSnapshot = null;
      setGame(null);
      return;
    }
    if (!snapshot || !r) return;
    if (!cur) {
      mountedSnapshot = snapshot;
      setGame(createGame(createOnlineSource(c, snapshot), r));
    } else if (cur.online && cur.view().winner !== null && snapshot !== mountedSnapshot && snapshot.view.winner === null) {
      // Immediate same-room rematch: swap in a fresh game with the new board.
      cur.dispose();
      mountedSnapshot = snapshot;
      setGame(createGame(createOnlineSource(c, snapshot), r));
    } else {
      mountedSnapshot = snapshot;
    }
  });

  // In the menus, show a map in the background (including any DIY custom map).
  createEffect(() => {
    const r = renderer();
    const seed = previewSeed();
    const players = previewPlayers();
    const scenario = previewScenario();
    const code = conn()?.room()?.rules.customMap ?? customMap();
    if (!r || game()) return;
    if (code) {
      try {
        const info = previewCustomMap(conn()?.room()?.seats.length ?? players, conn()?.room()?.scenario ?? scenario, code);
        r.setBoard(boardData(info), topology(info.layout));
        const desert = info.terrain.indexOf('Desert');
        r.setPieces({ buildings: [], roads: [], robber: desert >= 0 ? desert : 0 }, false);
        return;
      } catch {
        // Fall back to procedural preview if custom map is for another layout.
      }
    }
    const m = new Match(players, seed, null, 'greedy', scenario);
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
    if (
      !(
        (v.phase.type === 'Main' && v.current === g.me) ||
        ((v.phase.type === 'SpecialBuild' || v.phase.type === 'PairedTurn') && v.toMove === g.me)
      )
    ) {
      setTradeOpen(false);
      setCitadelOpen(false);
      setFishOpen(false);
      setCoinOpen(false);
      setExplorersOpen(false);
      setDevModal(null);
    }
  });

  const onKey = (e: KeyboardEvent) => {
    if ((e.target as HTMLElement).closest('input, select, textarea')) return;
    if (e.key === '?') {
      e.preventDefault();
      setShortcutsOpen(!shortcutsOpen());
      return;
    }
    if (e.key === 'm' || e.key === 'M') {
      toggleMute();
      return;
    }
    if (e.key === 'r' || e.key === 'R') {
      setRulesOpen(!rulesOpen());
      return;
    }
    const g = game();
    if (!g) return;
    if (e.key === 'Escape') {
      g.setMode(null);
      setTradeOpen(false);
      setCitadelOpen(false);
      setFishOpen(false);
      setCoinOpen(false);
      setExplorersOpen(false);
      setRulesOpen(false);
      setShortcutsOpen(false);
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
    } else if (!g.replay) {
      if ((e.key === 'e' || e.key === 'E') && g.has('EndTurn')) g.act((a) => a.type === 'EndTurn');
      else if (e.key === '1' && g.has('BuildRoad')) g.setMode('road');
      else if (e.key === '2' && g.has('BuildSettlement')) g.setMode('settlement');
      else if (e.key === '3' && g.has('BuildCity')) g.setMode('city');
      else if (e.key === '4') {
        if (g.has('BuildShip')) g.setMode('ship');
        else if (g.has('BuildBridge')) g.setMode('bridge');
      } else if ((e.key === 't' || e.key === 'T') && g.view().phase.type === 'Main' && g.view().current === g.me) {
        setTradeOpen(!tradeOpen());
      }
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

  const activeScenario = (): Scenario => game()?.config.scenario ?? conn()?.room()?.scenario ?? previewScenario();
  const activeRules = (): Partial<HouseRules> | null => {
    const g = game();
    if (g) {
      return {
        victoryPoints: g.config.victoryPoints,
        friendlyRobber: g.config.friendlyRobber,
        pairedTurns: g.config.pairedTurns,
        eventDeck: g.config.eventDeck,
        turnTimerSecs: conn()?.room()?.rules.turnTimerSecs ?? 0,
        customMap: g.config.customMap ?? null,
      };
    }
    return conn()?.room()?.rules ?? { customMap: customMap() };
  };

  const menu = () => (
    <Show
      when={conn()}
      fallback={
        <Show
          when={joinRoom()}
          fallback={
            <Lobby
              ready={!!renderer()}
              renderer={renderer()}
              seed={previewSeed()}
              players={previewPlayers()}
              onPlayers={setPreviewPlayers}
              scenario={previewScenario()}
              onScenario={setPreviewScenario}
              customMap={customMap()}
              onCustomMap={setCustomMap}
              onOpenReplay={(json) => openReplay(json)}
              onOpenRules={() => setRulesOpen(true)}
              onOpenWorkshop={() => setWorkshopOpen(true)}
              onOpenDaily={() => setDailyOpen(true)}
              onOpenShortcuts={() => setShortcutsOpen(true)}
              onStartTutorial={() => {
                void startLocal(
                  {
                    players: TUTORIAL_PRESET.players,
                    humanSeat: 0,
                    bot: 'greedy',
                    seed: TUTORIAL_PRESET.seed,
                    fast: false,
                    scenario: TUTORIAL_PRESET.scenario,
                    rules: {
                      victoryPoints: TUTORIAL_PRESET.victoryPoints,
                      friendlyRobber: true,
                      pairedTurns: false,
                      eventDeck: true,
                      turnTimerSecs: 0,
                      customMap: null,
                    },
                  },
                  null,
                  true,
                );
              }}
              onSeed={setPreviewSeed}
              onStartLocal={(o) => startLocal(o)}
              onCreateRoom={createRoom}
              onJoinRoom={(room, directName) => {
                if (directName || savedToken(room)) {
                  connect({ type: 'Join', room, name: directName || savedName() || '玩家' });
                } else {
                  setJoinRoom(room);
                }
              }}
            />
          }
        >
          {(room) => <JoinPrompt room={room()} onJoin={(name) => connect({ type: 'Join', room: room(), name })} onCancel={() => leave()} />}
        </Show>
      }
    >
      {(c) => (
        <RoomLobby
          conn={c()}
          renderer={renderer()}
          onLeave={() => leave()}
          onOpenRules={() => setRulesOpen(true)}
          onOpenWorkshop={() => setWorkshopOpen(true)}
        />
      )}
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
                ← {t('大厅', 'Lobby')}
              </button>
              <Show when={conn()}>
                {(c) => <span class="room-tag muted">{t('房间', 'Room')} {c().roomId()}</span>}
              </Show>
              <Show when={conn()?.room()?.ranked}>
                <span class="room-tag mode-tag ranked">{t('排位赛', 'Ranked')}</span>
              </Show>
              <Show when={g().turnSecondsLeft()}>
                {(secs) => (
                  <span class="turn-timer" classList={{ urgent: secs() <= 10 }} title="每步倒计时，超时将由 AI 代打一步">
                    ⏱ {secs()}s
                  </span>
                )}
              </Show>
              <Show when={g().replay}>
                <span class="room-tag muted">{t('回放', 'Replay')}</span>
              </Show>
              <Show when={!g().replay ? g().quickChat : undefined}>
                {(send) => <QuickChatTrigger onSend={(emote) => send()(emote)} class="ghost" />}
              </Show>
              <button class="ghost" onClick={() => setRulesOpen(true)}>
                {t('规则', 'Rules')}
              </button>
              <button class="ghost" onClick={() => setShortcutsOpen(true)} title="快捷键与色盲无障碍设置 (?)">
                ?
              </button>
              <LangToggle class="ghost" />
              <ThemeToggle renderer={renderer()} class="ghost" />
              <SoundToggle class="ghost" />
              <button
                class="ghost log-toggle"
                onClick={() => {
                  const next = !logOpen();
                  setLogOpen(next);
                  if (next) g().clearUnreadChat();
                }}
                aria-expanded={logOpen()}
              >
                {logOpen() ? '收起记录' : g().online ? '记录 · 聊天' : '记录'}
                <Show when={!logOpen() && g().unreadChat() > 0}>
                  <span class="unread-badge">{g().unreadChat()}</span>
                </Show>
              </button>
            </header>
            <DiceView game={g()} />
            <PlayerList game={g()} />
            <GameLog
              game={g()}
              open={logOpen()}
              onToggle={() => {
                const next = !logOpen();
                setLogOpen(next);
                if (next) g().clearUnreadChat();
              }}
            />
            <HandBar
              game={g()}
              onTrade={() => setTradeOpen(true)}
              onCitadel={() => setCitadelOpen(true)}
              onFishMarket={() => setFishOpen(true)}
              onCoinMarket={() => setCoinOpen(true)}
              onExplorers={() => setExplorersOpen(true)}
              onDevCard={(c) => playDevCard(g(), c)}
            />
            <Flights game={g()} />
            <SeatSpeechBubbles bubbles={g().bubbles()} />
            <TileTooltip game={g()} />
            <Show when={tutorialActive() && !g().online && !g().replay}>
              <TutorialCoach game={g()} onExitTutorial={() => setTutorialActive(false)} />
            </Show>
            <Show when={!tutorialActive() && renderer()}>
              {(r) => <SetupAdvisorOverlay game={g()} project={(kind, id) => r().project(kind, id)} />}
            </Show>

            <Show when={!g().replay && g().me !== null && g().view().phase.type === 'Discard' && g().view().pendingDiscard[g().me!] > 0}>
              <DiscardModal game={g()} />
            </Show>
            <Show when={!g().replay && g().me !== null && g().view().phase.type === 'Gold' && (g().view().pendingGold[g().me!] ?? 0) > 0}>
              <GoldModal game={g()} />
            </Show>
            <RouteChoiceModal game={g()} />
            <VictimModal game={g()} />
            <Show when={!g().replay}>
              <IncomingTrade game={g()} />
            </Show>
            <Show when={tradeOpen()}>
              <TradeModal game={g()} onClose={() => setTradeOpen(false)} />
            </Show>
            <Show when={citadelOpen()}>
              <CityUpgradeModal game={g()} onClose={() => setCitadelOpen(false)} />
            </Show>
            <Show when={fishOpen()}>
              <FishMarketModal game={g()} onClose={() => setFishOpen(false)} />
            </Show>
            <Show when={coinOpen()}>
              <CoinMarketModal game={g()} onClose={() => setCoinOpen(false)} />
            </Show>
            <Show when={explorersOpen()}>
              <ExplorersModal game={g()} onClose={() => setExplorersOpen(false)} />
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
                onWatchReplay={(r, link) => watchReplay(r, link)}
                onSeekReplay={(r, step, link) => watchReplay(r, link, step)}
                room={conn()?.room()}
                conn={conn()}
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

      <Show when={rulesOpen()}>
        <RulesModal
          scenario={activeScenario()}
          rules={activeRules()}
          customMap={activeRules()?.customMap ?? null}
          onClose={() => setRulesOpen(false)}
        />
      </Show>

      <Show when={shortcutsOpen()}>
        <ShortcutsModal
          colorblind={colorblind()}
          onToggleColorblind={() => {
            const next = toggleColorblindMode();
            setColorblind(next);
            renderer()?.setColorblind(next);
          }}
          onClose={() => setShortcutsOpen(false)}
        />
      </Show>

      <Show when={dailyOpen()}>
        <DailyChallengeModal
          embedded={EMBEDDED}
          onClose={() => setDailyOpen(false)}
          onStartDaily={(spec: DailyChallengeSpec) => {
            setDailyOpen(false);
            if (conn()) leave(false);
            setPreviewScenario(spec.scenario);
            setPreviewPlayers(spec.players);
            setPreviewSeed(spec.seed);
            setCustomMap(null);
            void startLocal(
              {
                players: spec.players,
                humanSeat: 0,
                bot: spec.bot,
                seed: spec.seed,
                fast: false,
                scenario: spec.scenario,
                rules: {
                  victoryPoints: spec.victoryPoints,
                  friendlyRobber: spec.friendlyRobber,
                  pairedTurns: false,
                  eventDeck: true,
                  turnTimerSecs: 0,
                  customMap: null,
                },
              },
              spec.dateKey,
            );
          }}
        />
      </Show>

      <Show when={workshopOpen()}>
        <MapWorkshopModal
          initialScenario={conn()?.room()?.scenario ?? previewScenario()}
          initialPlayers={conn()?.room()?.seats.length ?? previewPlayers()}
          initialCode={conn()?.room()?.rules.customMap ?? customMap()}
          embedded={EMBEDDED}
          onClose={() => setWorkshopOpen(false)}
          onPlayLocal={(sc, pl, code) => {
            setPreviewScenario(sc);
            setPreviewPlayers(pl);
            setCustomMap(code);
            setWorkshopOpen(false);
            if (conn()) leave(false);
            void startLocal({
              players: pl,
              humanSeat: 0,
              bot: 'greedy',
              seed: previewSeed(),
              fast: false,
              scenario: sc,
              rules: {
                victoryPoints: null,
                friendlyRobber: false,
                pairedTurns: false,
                eventDeck: false,
                turnTimerSecs: 0,
                customMap: code,
              },
            });
          }}
          onUseInRoom={
            EMBEDDED
              ? undefined
              : (sc, pl, code) => {
                  setWorkshopOpen(false);
                  const c = conn();
                  const rm = c?.room();
                  if (c && rm && !rm.started && rm.scenario === sc && rm.seats.length === pl) {
                    c.send({ type: 'SetRules', value: { rules: { ...rm.rules, customMap: code } } });
                  } else {
                    if (c) leave(false);
                    setPreviewScenario(sc);
                    setPreviewPlayers(pl);
                    setCustomMap(code);
                    createRoom({
                      players: pl,
                      bots: 'Normal',
                      name: account()?.username ?? (savedName() || '玩家'),
                      ranked: false,
                      scenario: sc,
                      rules: {
                        victoryPoints: null,
                        friendlyRobber: false,
                        pairedTurns: false,
                        eventDeck: false,
                        turnTimerSecs: 0,
                        customMap: code,
                      },
                      isPublic: true,
                    });
                  }
                }
          }
        />
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
      <footer class="renderer-info muted">{backend() ? `v${__APP_VERSION__} · ${backend()} · 按需渲染` : `v${__APP_VERSION__}`}</footer>
      <Show when={status()}>
        <div class="loading">{status()}</div>
      </Show>
    </div>
  );
}
