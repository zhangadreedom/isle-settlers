// Typed wrapper around the WebAssembly match host.
import init, * as wasm from './wasm/isle_wasm.js';
import type { Action, BoardInfo, Event, PlayerView, ResourceSet } from './generated/proto';

/** Owner value meaning "nobody" in per-spot arrays. */
export const EMPTY = 255;
/** Flag set on a building owner when the building is a city. */
export const CITY_FLAG = 0x80;

export interface Topology {
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

export type BotKind = 'greedy' | 'weighted' | 'random';

/** A match hosted entirely in WebAssembly. JS only sees the human's view. */
export class Match {
  private readonly inner: wasm.LocalMatch;
  readonly board: BoardInfo;

  constructor(players: number, seed: number, humanSeat: number | null, bot: BotKind) {
    this.inner = new wasm.LocalMatch(players, seed >>> 0, humanSeat ?? -1, bot);
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

let ready: Promise<Topology> | undefined;

export function loadEngine(): Promise<Topology> {
  ready ??= init().then(() => ({
    tilePos: wasm.tilePositions(),
    vertexPos: wasm.vertexPositions(),
    edgeVertices: wasm.edgeVertices(),
    tileVertices: wasm.tileVertices(),
    harborEdges: wasm.harborEdges(),
  }));
  return ready;
}

export const runBenchmark = (games: number, seed: number) => wasm.runBenchmark(games, seed);
