// Where game state comes from. The presentation layer (controller.ts) only
// talks to this interface, so local and online games share all UI code.

import { createSignal, type Accessor } from 'solid-js';
import { EngineError, WorkerMatch, type BotKind, type Replay } from '../engine';
import type { MatchState } from './worker';
import type { Action, BoardInfo, ChatLine, Event, LegalActions, PlayerView, ReplayPlayer, ReplayRecord, Snapshot } from '../generated/proto';
import type { Connection } from '../net/connection';

export interface SeatMeta {
  name: string;
  bot: boolean;
  connected: boolean;
  autopilot: boolean;
}

export interface SourceState {
  view: PlayerView;
  legal: LegalActions;
  events: Event[];
  /** Full resynchronisation (start / reconnect): replace the log, no animations. */
  resync: boolean;
  /** Recent events to rebuild the log from, on resync. */
  log?: Event[];
}

export interface SourceHandlers {
  state: (s: SourceState) => void;
  error: (code: string) => void;
  chat: (line: ChatLine) => void;
}

/** Playback of a recorded game. */
export interface ReplayControls {
  readonly length: number;
  readonly players: ReplayPlayer[];
  readonly record: ReplayRecord;
  /** Actions applied so far. */
  position: Accessor<number>;
  playing: Accessor<boolean>;
  /** Playback speed multiplier. */
  speed: Accessor<number>;
  /** Seat whose private view is shown, or null for the public view. */
  viewer: Accessor<number | null>;
  toggle(): void;
  seek(position: number): void;
  /** One action forwards (+1) or backwards (-1); pauses playback. */
  step(delta: 1 | -1): void;
  setSpeed(speed: number): void;
  setViewer(seat: number | null): void;
}

export interface GameSource {
  readonly board: BoardInfo;
  readonly me: number | null;
  readonly online: boolean;
  /** Present when watching a replay. */
  readonly replay?: ReplayControls;
  /** The finished game as a replay, if available. */
  exportReplay?(): Promise<ReplayRecord | null>;
  /** Shareable link to the finished game's replay. */
  replayLink?(): string | null;
  seat(seat: number): SeatMeta;
  /** Seat expected to act next (a bot or another player), -1 for none. */
  thinking(): number;
  start(handlers: SourceHandlers): void;
  /** Plays an action from `legal.actions`, or a trade proposal. */
  play(action: Action, legal: LegalActions): boolean;
  chat?(text: string): void;
  dispose(): void;
}

// ------------------------------------------------------------------ local

export interface LocalOptions {
  players: number;
  /** Human seat, or null to watch bots play. */
  humanSeat: number | null;
  bot: BotKind;
  seed: number;
  fast: boolean;
}

/**
 * A match hosted in a Web Worker, with bots paced like a real table. Worker
 * replies arrive in request order and carry the full state, so applying
 * them as they come keeps everything consistent.
 */
export async function createLocalSource(opts: LocalOptions): Promise<GameSource> {
  const worker = new WorkerMatch();
  const first = await worker.create(opts.players, opts.seed, opts.humanSeat, opts.bot);
  const [thinking, setThinking] = createSignal(-1);
  let handlers: SourceHandlers | null = null;
  let current: MatchState = first;
  let timer: number | undefined;
  let humanBusy = false;
  let botBusy = false;
  let disposed = false;

  function emit(state: MatchState, resync = false) {
    if (disposed || !handlers) return;
    current = state;
    handlers.state({ view: state.view, legal: state.legal, events: state.events, resync });
    schedule();
  }

  function delay(): number {
    const v = current.view;
    let ms = 650;
    if (v.phase.type === 'SetupSettlement' || v.phase.type === 'SetupRoad') ms = 450;
    else if (v.phase.type === 'PreRoll') ms = 750;
    else if (v.phase.type === 'Discard') ms = 350;
    // Five bots pass or build after every turn on the extended island.
    else if (v.phase.type === 'SpecialBuild') ms = 260;
    else if (v.trade && v.current === opts.humanSeat) ms = 700;
    return opts.fast ? ms * 0.3 : ms;
  }

  function schedule() {
    if (timer !== undefined || botBusy || disposed) return;
    const seat = current.nextBot;
    setThinking(seat);
    if (seat < 0) return;
    timer = window.setTimeout(async () => {
      timer = undefined;
      if (disposed) return;
      botBusy = true;
      try {
        emit(await worker.stepBot());
      } catch (e) {
        if (!disposed) handlers?.error(e instanceof EngineError ? e.code : String(e));
      } finally {
        botBusy = false;
        schedule();
      }
    }, delay());
  }

  async function run(request: Promise<MatchState>) {
    humanBusy = true;
    try {
      emit(await request);
    } catch (e) {
      if (!disposed) handlers?.error(e instanceof EngineError ? e.code : String(e));
    } finally {
      humanBusy = false;
    }
  }

  return {
    board: first.board!,
    me: opts.humanSeat,
    online: false,
    seat: (s) => ({ name: `AI ${s + 1}`, bot: s !== opts.humanSeat, connected: true, autopilot: false }),
    thinking,
    start(h) {
      handlers = h;
      emit(first, true);
    },
    play(action, legal) {
      // One human action at a time: ignore double clicks until it lands.
      if (humanBusy) return false;
      if (action.type === 'ProposeTrade') {
        void run(worker.proposeTrade(action.value.give, action.value.get));
        return true;
      }
      const index = legal.actions.indexOf(action);
      if (index < 0) {
        handlers?.error('NoSuchAction');
        return false;
      }
      void run(worker.apply(legal.version, index));
      return true;
    },
    async exportReplay() {
      const record = current.replay;
      if (!record) return null;
      return { ...record, players: record.players.map((p) => (p.bot ? p : { ...p, name: '玩家' })) };
    },
    dispose() {
      disposed = true;
      window.clearTimeout(timer);
      worker.terminate();
    },
  };
}

// ------------------------------------------------------------------ online

/** A game hosted by the server, seen through a connection. */
export function createOnlineSource(conn: Connection, first: Snapshot): GameSource {
  let handlers: SourceHandlers | null = null;
  let inFlight = false;
  const [view, setView] = createSignal<PlayerView>(first.view);
  const unsubscribe: (() => void)[] = [];

  const me = () => conn.seat();

  return {
    board: first.board,
    get me() {
      return me();
    },
    online: true,
    seat(s) {
      const info = conn.room()?.seats[s];
      if (info?.type === 'Human') return { name: info.value.name, bot: false, connected: info.value.connected, autopilot: info.value.autopilot };
      return { name: `AI ${s + 1}`, bot: true, connected: true, autopilot: false };
    },
    thinking() {
      const v = view();
      if (v.winner !== null) return -1;
      if (v.phase.type === 'Discard') return v.pendingDiscard.findIndex((n, i) => n > 0 && i !== me());
      return v.toMove !== null && v.toMove !== me() ? v.toMove : -1;
    },
    start(h) {
      handlers = h;
      const resync = (s: Snapshot) => {
        inFlight = false;
        setView(s.view);
        h.state({ view: s.view, legal: s.legal, events: [], resync: true, log: s.log });
      };
      unsubscribe.push(
        conn.onSnapshot(resync),
        conn.onUpdate((u) => {
          inFlight = false;
          setView(u.view);
          h.state({ view: u.view, legal: u.legal, events: u.events, resync: false });
        }),
        conn.onError((code) => {
          inFlight = false;
          h.error(code);
        }),
        conn.onChat((line) => h.chat(line)),
      );
      resync(conn.snapshot() ?? first);
    },
    play(action, legal) {
      // One action at a time: ignore double clicks until the server answers.
      if (inFlight) return false;
      if (!conn.send({ type: 'Act', value: { version: legal.version, action } })) {
        handlers?.error('Offline');
        return false;
      }
      inFlight = true;
      return true;
    },
    chat(text) {
      conn.send({ type: 'Chat', value: { text } });
    },
    async exportReplay() {
      const id = conn.room()?.replay;
      if (!id) return null;
      const res = await fetch(`/api/replays/${id}`);
      return res.ok ? ((await res.json()) as ReplayRecord) : null;
    },
    replayLink() {
      const id = conn.room()?.replay;
      return id ? replayUrl(id) : null;
    },
    dispose() {
      unsubscribe.forEach((f) => f());
    },
  };
}

/** Link that opens a stored replay. */
export const replayUrl = (id: string) => `${location.origin}${location.pathname}#/replay/${id}`;

// ------------------------------------------------------------------ replay

/** Events shown in the log after a jump: those of the last few actions. */
const LOG_BACK = 40;

/** Watches a recorded game. Every state is recomputed by the engine. */
export function createReplaySource(replay: Replay, link: string | null = null): GameSource {
  const [position, setPosition] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeedSignal] = createSignal(1);
  const [viewer, setViewerSignal] = createSignal<number | null>(null);
  const players = replay.record.players;
  let handlers: SourceHandlers | null = null;
  let timer: number | undefined;
  let lastPhase = 'SetupSettlement';

  const legal = (): LegalActions => ({ version: replay.position, actions: [] });

  function emit(view: PlayerView, events: Event[], resync: boolean, log?: Event[]) {
    lastPhase = view.phase.type;
    setPosition(replay.position);
    handlers?.state({ view, legal: legal(), events, resync, log });
  }

  /** Shows the current position from scratch, with recent events in the log. */
  function resync() {
    const target = replay.position;
    replay.seek(Math.max(0, target - LOG_BACK));
    const log: Event[] = [];
    while (replay.position < target) log.push(...(replay.step(viewer())?.events ?? []));
    emit(replay.view(viewer()), [], true, log);
  }

  function stepOnce(): boolean {
    const step = replay.step(viewer());
    if (!step) return false;
    emit(replay.view(viewer()), step.events, false);
    return true;
  }

  function delay(): number {
    let ms = 650;
    if (lastPhase === 'SetupSettlement' || lastPhase === 'SetupRoad') ms = 450;
    else if (lastPhase === 'PreRoll') ms = 800;
    else if (lastPhase === 'Discard' || lastPhase === 'SpecialBuild') ms = 350;
    return ms / speed();
  }

  function schedule() {
    window.clearTimeout(timer);
    timer = undefined;
    if (!playing()) return;
    timer = window.setTimeout(() => {
      if (!stepOnce()) setPlaying(false);
      else schedule();
    }, delay());
  }

  const controls: ReplayControls = {
    length: replay.length,
    players,
    record: replay.record,
    position,
    playing,
    speed,
    viewer,
    toggle() {
      if (!playing() && replay.position >= replay.length) {
        replay.seek(0);
        resync();
      }
      setPlaying(!playing());
      schedule();
    },
    seek(p) {
      replay.seek(Math.max(0, Math.min(replay.length, Math.round(p))));
      resync();
      schedule();
    },
    step(delta) {
      setPlaying(false);
      schedule();
      if (delta > 0) stepOnce();
      else if (replay.position > 0) {
        replay.seek(replay.position - 1);
        resync();
      }
    },
    setSpeed(x) {
      setSpeedSignal(x);
      schedule();
    },
    setViewer(seat) {
      setViewerSignal(seat);
      resync();
    },
  };

  return {
    board: replay.board,
    get me() {
      return viewer();
    },
    online: false,
    replay: controls,
    seat(s) {
      const p = players[s];
      return { name: p?.name || `AI ${s + 1}`, bot: !!p?.bot, connected: true, autopilot: false };
    },
    thinking: () => -1,
    start(h) {
      handlers = h;
      resync();
    },
    play: () => false,
    async exportReplay() {
      return replay.record;
    },
    replayLink: () => link,
    dispose() {
      window.clearTimeout(timer);
      handlers = null;
      replay.free();
    },
  };
}
