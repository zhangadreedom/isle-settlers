// Where game state comes from. The presentation layer (controller.ts) only
// talks to this interface, so local and online games share all UI code.

import { createSignal } from 'solid-js';
import { EngineError, WorkerMatch, type BotKind } from '../engine';
import type { MatchState } from './worker';
import type { Action, BoardInfo, ChatLine, Event, LegalActions, PlayerView, Snapshot } from '../generated/proto';
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

export interface GameSource {
  readonly board: BoardInfo;
  readonly me: number | null;
  readonly online: boolean;
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
    dispose() {
      unsubscribe.forEach((f) => f());
    },
  };
}
