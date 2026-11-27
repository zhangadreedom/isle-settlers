// Web Worker hosting a local match: the engine and the bots (including the
// tree search) run here, so thinking never blocks rendering or input.
// Requests are handled strictly in order; every reply carries the full state.

import init, { LocalMatch } from '../wasm/isle_wasm.js';
import type { BoardInfo, Event, HouseRules, LegalActions, PlayerView, ReplayRecord, Scenario } from '../generated/proto';

export type WorkerRequest =
  | { id: number; op: 'create'; players: number; seed: number; humanSeat: number | null; bot: string; scenario: Scenario; rules?: HouseRules }
  | { id: number; op: 'apply'; version: number; index: number }
  | { id: number; op: 'propose'; give: number[]; get: number[] }
  | { id: number; op: 'step' };

export interface MatchState {
  view: PlayerView;
  legal: LegalActions;
  /** Seat of the bot to move next, or -1. */
  nextBot: number;
  events: Event[];
  board?: BoardInfo;
  /** The whole game, once it is over. */
  replay?: ReplayRecord;
}

export type WorkerReply = { id: number; ok: true; state: MatchState } | { id: number; ok: false; error: string };

// Minimal typing of the worker scope (the DOM and WebWorker libs conflict).
const scope = self as unknown as {
  postMessage(msg: WorkerReply): void;
  onmessage: ((e: MessageEvent<WorkerRequest>) => void) | null;
};

let match: LocalMatch | null = null;
let queue: Promise<void> = init().then(() => undefined);

function snapshot(events: Event[]): MatchState {
  const m = match!;
  const view = JSON.parse(m.view()) as PlayerView;
  const state: MatchState = { view, legal: JSON.parse(m.legalActions()), nextBot: m.nextBot(), events };
  if (view.winner !== null) state.replay = JSON.parse(m.replay(''));
  return state;
}

function handle(r: WorkerRequest): MatchState {
  switch (r.op) {
    case 'create': {
      match?.free();
      match = LocalMatch.withRules(
        r.players,
        r.seed >>> 0,
        r.humanSeat ?? -1,
        r.bot,
        r.scenario,
        r.rules?.victoryPoints ?? 0,
        r.rules?.friendlyRobber ?? false,
        r.rules?.pairedTurns ?? false,
      );
      return { ...snapshot([]), board: JSON.parse(match.board()) };
    }
    case 'apply':
      return snapshot(JSON.parse(match!.apply(r.version, r.index)));
    case 'propose':
      return snapshot(JSON.parse(match!.proposeTrade(Uint8Array.from(r.give), Uint8Array.from(r.get))));
    case 'step': {
      const step = JSON.parse(match!.stepBot()) as { events: Event[] } | null;
      return snapshot(step?.events ?? []);
    }
  }
}

scope.onmessage = (e) => {
  const r = e.data;
  queue = queue.then(() => {
    try {
      scope.postMessage({ id: r.id, ok: true, state: handle(r) });
    } catch (err) {
      scope.postMessage({ id: r.id, ok: false, error: err instanceof Error ? err.message : String(err) });
    }
  });
};
