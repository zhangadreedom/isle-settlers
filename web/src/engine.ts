// Typed wrapper around the WebAssembly match host.
import init, * as wasm from './wasm/isle_wasm.js';
import type { Action, BoardInfo, BoardLayout, Event, HouseRules, PlayerView, ReplayRecord, ResourceSet, Scenario } from './generated/proto';
import type { MatchState, WorkerReply, WorkerRequest } from './game/worker';

/** Owner value meaning "nobody" in per-spot arrays. */
export const EMPTY = 255;
/** Flag set on a building owner when the building is a city. */
export const CITY_FLAG = 0x80;

export interface Topology {
  layout: BoardLayout;
  tilePos: Int16Array;
  vertexPos: Int16Array;
  edgeVertices: Uint8Array;
  /** 6 corners per tile, clockwise from the top. */
  tileVertices: Uint8Array;
  harborEdges: Uint8Array;
}

export interface LegalActions {
  version: number;
  actions: Action[];
}

export interface BotStep {
  player: number;
  action: Action;
  events: Event[];
}

/** Error thrown by the engine; `code` is a `RuleError` name or `Stale`. */
export class EngineError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

function call<T>(f: () => string): T {
  try {
    return JSON.parse(f()) as T;
  } catch (e) {
    if (e instanceof Error && !(e instanceof SyntaxError)) throw new EngineError(e.message);
    throw e;
  }
}

export type BotKind = 'greedy' | 'weighted' | 'random' | 'mcts' | `mcts:${number}`;

/** A match hosted entirely in WebAssembly. JS only sees the human's view. */
export class Match {
  private readonly inner: wasm.LocalMatch;
  readonly board: BoardInfo;

  constructor(
    players: number,
    seed: number,
    humanSeat: number | null,
    bot: BotKind,
    scenario: Scenario = 'Classic',
    rules?: Partial<HouseRules>,
  ) {
    this.inner = wasm.LocalMatch.createMatch(
      players,
      seed >>> 0,
      humanSeat ?? -1,
      bot,
      scenario,
      rules?.victoryPoints ?? 0,
      rules?.friendlyRobber ?? false,
      rules?.pairedTurns ?? false,
      rules?.eventDeck ?? false,
      rules?.customMap ?? '',
    );
    this.board = call(() => this.inner.board());
  }

  view(): PlayerView {
    return call(() => this.inner.view());
  }

  legal(): LegalActions {
    return call(() => this.inner.legalActions());
  }

  apply(version: number, index: number): Event[] {
    return call(() => this.inner.apply(version, index));
  }

  proposeTrade(give: ResourceSet, get: ResourceSet): Event[] {
    return call(() => this.inner.proposeTrade(Uint8Array.from(give), Uint8Array.from(get)));
  }

  nextBot(): number {
    return this.inner.nextBot();
  }

  stepBot(): BotStep | null {
    return call(() => this.inner.stepBot());
  }

  free() {
    this.inner.free();
  }
}

/** A local match running in a Web Worker (see game/worker.ts). */
export class WorkerMatch {
  private readonly worker = new Worker(new URL('./game/worker.ts', import.meta.url), { type: 'module' });
  private nextId = 1;
  private readonly pending = new Map<number, { resolve: (s: MatchState) => void; reject: (e: Error) => void }>();

  constructor() {
    this.worker.onmessage = (e: MessageEvent<WorkerReply>) => {
      const reply = e.data;
      const p = this.pending.get(reply.id);
      if (!p) return;
      this.pending.delete(reply.id);
      if (reply.ok) p.resolve(reply.state);
      else p.reject(new EngineError(reply.error));
    };
  }

  private call(req: DistributiveOmit<WorkerRequest, 'id'>): Promise<MatchState> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.worker.postMessage({ ...req, id });
    });
  }

  create(
    players: number,
    seed: number,
    humanSeat: number | null,
    bot: BotKind,
    scenario: Scenario,
    rules: HouseRules,
  ) {
    return this.call({ op: 'create', players, seed, humanSeat, bot, scenario, rules });
  }

  apply(version: number, index: number) {
    return this.call({ op: 'apply', version, index });
  }

  proposeTrade(give: ResourceSet, get: ResourceSet) {
    return this.call({ op: 'propose', give: [...give], get: [...get] });
  }

  stepBot() {
    return this.call({ op: 'step' });
  }

  terminate() {
    this.worker.terminate();
    for (const p of this.pending.values()) p.reject(new EngineError('Terminated'));
    this.pending.clear();
  }
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

let ready: Promise<void> | undefined;
const topologies = new Map<BoardLayout, Topology>();

/** Loads the WebAssembly engine (once). */
export function loadEngine(): Promise<void> {
  ready ??= init().then(() => undefined);
  return ready;
}

/** Adjacency tables of an island. The engine must be loaded. */
export function topology(layout: BoardLayout): Topology {
  let t = topologies.get(layout);
  if (!t) {
    const raw = JSON.parse(wasm.topology(layout)) as Record<'tilePos' | 'vertexPos' | 'edgeVertices' | 'tileVertices' | 'harborEdges', number[]>;
    t = {
      layout,
      tilePos: Int16Array.from(raw.tilePos),
      vertexPos: Int16Array.from(raw.vertexPos),
      edgeVertices: Uint8Array.from(raw.edgeVertices),
      tileVertices: Uint8Array.from(raw.tileVertices),
      harborEdges: Uint8Array.from(raw.harborEdges),
    };
    topologies.set(layout, t);
  }
  return t;
}

/**
 * A recorded game, replayed by the engine on the main thread (no bots, so
 * it is cheap). Throws `EngineError` for files that do not replay.
 */
export class Replay {
  private readonly inner: wasm.Replay;
  readonly board: BoardInfo;
  readonly record: ReplayRecord;
  readonly length: number;

  constructor(json: string) {
    let record: ReplayRecord;
    try {
      record = JSON.parse(json) as ReplayRecord;
    } catch {
      throw new EngineError('BadReplay');
    }
    const c = record?.config;
    if (!c || typeof record.seed !== 'string' || !Array.isArray(record.actions) || !Array.isArray(record.players)) {
      throw new EngineError('BadReplay');
    }
    if (record.format !== 1) throw new EngineError('UnsupportedReplay');
    if (record.players.length !== c.numPlayers) throw new EngineError('BadReplayConfig');
    try {
      // Replays from before seafaring/variants default missing flags.
      const normalized = {
        ...c,
        scenario: c.scenario ?? 'Classic',
        friendlyRobber: c.friendlyRobber ?? false,
        pairedTurns: c.pairedTurns ?? false,
        eventDeck: c.eventDeck ?? false,
      };
      this.inner = wasm.Replay.fromConfigJson(
        JSON.stringify(normalized),
        record.seed,
        Uint8Array.from(record.actions),
      );
    } catch (e) {
      throw new EngineError(e instanceof Error ? e.message : String(e));
    }
    this.record = record;
    this.board = call(() => this.inner.board());
    this.length = this.inner.length();
  }

  get position(): number {
    return this.inner.position();
  }

  view(viewer: number | null): PlayerView {
    return call(() => this.inner.view(viewer ?? -1));
  }

  seek(position: number) {
    this.inner.seek(position);
  }

  /** Applies the next action; `null` at the end. */
  step(viewer: number | null): BotStep | null {
    return call(() => this.inner.step(viewer ?? -1));
  }

  free() {
    this.inner.free();
  }
}

export const runBenchmark = (games: number, seed: number) => wasm.runBenchmark(games, seed);

/** Validates and previews an `IM1:...` custom map for `(players, scenario)`. */
export function previewCustomMap(players: number, scenario: Scenario, code: string): BoardInfo {
  return call(() => wasm.previewCustomMap(players, scenario, code));
}
