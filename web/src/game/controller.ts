// Game presentation: turns states from a GameSource (local WASM match or the
// online server) into Solid signals, log lines, highlights and animations.

import { batch, createSignal } from 'solid-js';
import type { BoardRenderer, BoardData, Highlights, PickKind } from '../board/BoardRenderer';
import type { Topology } from '../engine';
import type { Action, BoardInfo, ChatLine, Event, LegalActions, PlayerView, ResourceSet } from '../generated/proto';
import type { GameSource, SourceState } from './source';
import { RESOURCES, describe, errorText } from './text';

export type BuildMode = 'road' | 'settlement' | 'city' | null;

export interface LogEntry {
  id: number;
  text: string;
  seat: number | null;
  /** Chat lines are shown with the sender's name. */
  chat?: { name: string };
}

/** A resource chip flying from the board to a seat's panel. */
export interface Flight {
  id: number;
  from: { x: number; y: number };
  seat: number;
  resource: number;
}

const TERRAINS = ['Hills', 'Forest', 'Pasture', 'Fields', 'Mountains', 'Desert'];
const PRODUCES = [0, 1, 2, 3, 4, -1]; // terrain code -> resource index

export function boardData(info: BoardInfo): BoardData {
  return {
    terrain: info.terrain.map((t) => TERRAINS.indexOf(t)),
    numbers: info.numbers,
    harbors: info.harbors.map((h) => (h.type === 'Generic' ? 5 : RESOURCES.indexOf(h.value))),
  };
}

const sameSet = (a: ResourceSet, b: ResourceSet) => a.every((x, i) => x === b[i]);

export function createGame(source: GameSource, renderer: BoardRenderer, topology: Topology) {
  const board = boardData(source.board);
  const me = source.me;

  const [view, setView] = createSignal<PlayerView | null>(null);
  const [legal, setLegal] = createSignal<LegalActions>({ version: 0, actions: [] });
  const [log, setLog] = createSignal<LogEntry[]>([]);
  const [mode, setModeSignal] = createSignal<BuildMode>(null);
  const [toast, setToast] = createSignal<string | null>(null);
  const [dice, setDice] = createSignal<{ values: [number, number]; key: number } | null>(null);
  const [flights, setFlights] = createSignal<Flight[]>([]);
  const [victimChoice, setVictimChoice] = createSignal<{ tile: number; victims: number[] } | null>(null);

  let logId = 0;
  let flightId = 0;
  let toastTimer: number | undefined;
  let disposed = false;

  const name = (seat: number) => (seat === me ? '你' : source.seat(seat).name);
  const actions = () => legal().actions;
  const has = (type: Action['type']) => actions().some((a) => a.type === type);

  renderer.setBoard(board);
  renderer.onPick = onPick;

  // ---- state sync ----

  function onState(s: SourceState) {
    if (disposed) return;
    if (s.resync) {
      setLog([]);
      if (s.log?.length) handleEvents(s.log, s.view, false);
    } else {
      handleEvents(s.events, view() ?? s.view, true);
    }
    batch(() => {
      setView(s.view);
      setLegal(s.legal);
      if (mode() && !modeAvailable(mode(), s.legal.actions)) setModeSignal(null);
      if (s.view.phase.type !== 'MoveRobber') setVictimChoice(null);
    });
    renderer.setPieces({ buildings: s.view.buildings, roads: s.view.roads, robber: s.view.robber }, !s.resync);
    renderer.setHighlights(highlights());
  }

  function modeAvailable(m: BuildMode, list: Action[]) {
    const type = m === 'road' ? 'BuildRoad' : m === 'settlement' ? 'BuildSettlement' : 'BuildCity';
    return list.some((a) => a.type === type);
  }

  function highlights(): Highlights | null {
    const v = view();
    const list = actions();
    if (!v || !list.length) return null;
    const ids = (type: 'BuildRoad' | 'BuildSettlement' | 'BuildCity') =>
      list.flatMap((a) => (a.type === type ? [a.value] : []));
    switch (v.phase.type) {
      case 'SetupSettlement':
        return { kind: 'vertex', ids: ids('BuildSettlement') };
      case 'SetupRoad':
      case 'RoadBuilding':
        return { kind: 'edge', ids: ids('BuildRoad') };
      case 'MoveRobber':
        return { kind: 'tile', ids: [...new Set(list.flatMap((a) => (a.type === 'MoveRobber' ? [a.value.tile] : [])))] };
      case 'Main':
        if (v.current !== me) return null;
        if (mode() === 'road') return { kind: 'edge', ids: ids('BuildRoad') };
        if (mode() === 'settlement') return { kind: 'vertex', ids: ids('BuildSettlement') };
        if (mode() === 'city') return { kind: 'vertex', ids: ids('BuildCity') };
        return null;
      default:
        return null;
    }
  }

  // ---- human actions ----

  function act(pred: (a: Action) => boolean): boolean {
    const l = legal();
    const action = l.actions.find(pred);
    if (!action) {
      showToast('现在不能这样做');
      return false;
    }
    return source.play(action, l);
  }

  function proposeTrade(give: ResourceSet, get: ResourceSet): boolean {
    return source.play({ type: 'ProposeTrade', value: { give, get } }, legal());
  }

  function onPick(kind: PickKind, id: number) {
    const v = view();
    if (!v) return;
    if (kind === 'tile') {
      const options = actions().flatMap((a) => (a.type === 'MoveRobber' && a.value.tile === id ? [a.value.victim] : []));
      const victims = options.filter((x): x is number => x !== null);
      if (victims.length > 1) setVictimChoice({ tile: id, victims });
      else act((a) => a.type === 'MoveRobber' && a.value.tile === id);
      return;
    }
    if (kind === 'edge') {
      act((a) => a.type === 'BuildRoad' && a.value === id);
      return;
    }
    if (v.phase.type === 'Main' && mode() === 'city') act((a) => a.type === 'BuildCity' && a.value === id);
    else act((a) => a.type === 'BuildSettlement' && a.value === id);
  }

  function setMode(m: BuildMode) {
    setModeSignal(mode() === m ? null : m);
    renderer.setHighlights(highlights());
  }

  // ---- events, log, animations ----

  function handleEvents(events: Event[], v: PlayerView, animate: boolean) {
    const lines: LogEntry[] = [];
    let lastSettlement = -1;
    for (const e of events) {
      const text = describe(e, name);
      const seat = 'player' in e.value ? e.value.player : 'thief' in e.value ? e.value.thief : null;
      if (text) lines.push({ id: ++logId, text, seat });
      switch (e.type) {
        case 'DiceRolled':
          setDice({ values: e.value.dice, key: logId });
          break;
        case 'SettlementBuilt':
          lastSettlement = e.value.vertex;
          break;
        case 'ResourcesProduced':
          if (animate) {
            const sum = (dice()?.values ?? [0, 0]).reduce((a, b) => a + b, 0);
            launchProduction(e.value.player, e.value.resources, sum, v);
          }
          break;
        case 'StartingResources':
          if (animate && lastSettlement >= 0) launchFrom(e.value.player, e.value.resources, tilesAround(lastSettlement));
          break;
      }
    }
    if (lines.length) setLog((old) => [...old, ...lines].slice(-100));
  }

  function onChat(line: ChatLine) {
    const who = line.seat === me && me !== null ? '你' : line.name;
    setLog((old) => [...old, { id: ++logId, text: line.text, seat: line.seat, chat: { name: who } }].slice(-100));
  }

  function tilesAround(vertex: number): number[] {
    const out: number[] = [];
    for (let t = 0; t < board.terrain.length; t++) {
      for (let k = 0; k < 6; k++) if (topology.tileVertices[t * 6 + k] === vertex) out.push(t);
    }
    return out;
  }

  /** Chips fly from each producing tile next to the player's buildings. */
  function launchProduction(seat: number, got: ResourceSet, roll: number, v: PlayerView) {
    const tiles: number[] = [];
    for (let t = 0; t < board.terrain.length; t++) {
      if (board.numbers[t] !== roll || t === v.robber) continue;
      const corners = topology.tileVertices.slice(t * 6, t * 6 + 6);
      if (corners.some((c) => v.buildings[c] !== 255 && (v.buildings[c] & 0x7f) === seat)) tiles.push(t);
    }
    launchFrom(seat, got, tiles);
  }

  function launchFrom(seat: number, got: ResourceSet, tiles: number[]) {
    const out: Flight[] = [];
    got.forEach((count, r) => {
      const sources = tiles.filter((t) => PRODUCES[board.terrain[t]] === r);
      for (let i = 0; i < count; i++) {
        const t = sources[i % Math.max(1, sources.length)];
        const from = t === undefined ? { x: innerWidth / 2, y: innerHeight / 2 } : renderer.project('tile', t);
        out.push({ id: ++flightId, from, seat, resource: r });
      }
    });
    if (!out.length) return;
    setFlights((f) => [...f, ...out]);
    window.setTimeout(() => setFlights((f) => f.filter((x) => !out.includes(x))), 1100);
  }

  function showToast(text: string) {
    setToast(text);
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => setToast(null), 2200);
  }

  function dispose() {
    disposed = true;
    window.clearTimeout(toastTimer);
    renderer.onPick = undefined;
    renderer.setHighlights(null);
    source.dispose();
  }

  source.start({
    state: onState,
    error: (code) => showToast(errorText(code)),
    chat: onChat,
  });

  return {
    board,
    me,
    online: source.online,
    /** Current view (always set once the source has started). */
    view: () => view()!,
    legal,
    log,
    mode,
    thinking: () => source.thinking(),
    seat: (s: number) => source.seat(s),
    toast,
    dice,
    flights,
    victimChoice,
    name,
    has,
    act,
    proposeTrade,
    setMode,
    chat: source.chat ? (text: string) => source.chat!(text) : undefined,
    clearVictimChoice: () => setVictimChoice(null),
    showToast,
    dispose,
    /** Finds the legal Discard/YearOfPlenty action matching a set. */
    actSet(type: 'Discard' | 'PlayYearOfPlenty', set: ResourceSet) {
      return act((a) => a.type === type && sameSet(a.value, set));
    },
  };
}

export type Game = ReturnType<typeof createGame>;
