// Game presentation: turns states from a GameSource (local WASM match or the
// online server) into Solid signals, log lines, highlights and animations.

import { batch, createSignal } from 'solid-js';
import { playForEvents, playTimerTick, playYourTurn } from '../audio/sound';
import type { BoardRenderer, BoardData, Highlights, PickKind, TileHover } from '../board/BoardRenderer';
import { topology as topologyOf } from '../engine';
import type { Action, BoardInfo, ChatLine, CommoditySet, Event, LegalActions, PlayerView, ResourceSet, Terrain } from '../generated/proto';
import type { GameSource, SourceState } from './source';
import { RESOURCES, describe, errorText } from './text';

/** Build & tactical modes on the board. */
export type BuildMode =
  | 'road'
  | 'settlement'
  | 'city'
  | 'ship'
  | 'move'
  | 'knight'
  | 'activate'
  | 'promote'
  | 'knightMove'
  | 'chase'
  | 'fishRoad'
  | 'bridge'
  | null;

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

export interface TileHoverInfo {
  tile: number;
  clientX: number;
  clientY: number;
  terrain: Terrain;
  number: number;
  pips: number;
  hasRobber: boolean;
  hasPirate: boolean;
}

const TERRAINS: Terrain[] = ['Hills', 'Forest', 'Pasture', 'Fields', 'Mountains', 'Desert', 'Sea', 'Gold', 'Fog'];
const PRODUCES = [0, 1, 2, 3, 4, -1, -1, -1, -1]; // terrain code -> resource index
const PIPS = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

export function boardData(info: BoardInfo): BoardData {
  return {
    terrain: info.terrain.map((t) => TERRAINS.indexOf(t as Terrain)),
    numbers: info.numbers,
    harbors: info.harbors.map((h) => (h.type === 'Generic' ? 5 : RESOURCES.indexOf(h.value))),
    islands: info.islands ?? info.terrain.map(() => 1),
    fishingGrounds: info.fishingGrounds,
    riverEdges: info.riverEdges,
  };
}

const sameSet = (a: readonly number[], b: readonly number[]) => a.every((x, i) => x === b[i]);

export function createGame(source: GameSource, renderer: BoardRenderer) {
  const board = boardData(source.board);
  const topology = topologyOf(source.board.layout);
  /** The seat this client plays (or, in a replay, watches as). */
  const me = () => source.me;

  const [view, setView] = createSignal<PlayerView | null>(null);
  const [legal, setLegal] = createSignal<LegalActions>({ version: 0, actions: [] });
  const [log, setLog] = createSignal<LogEntry[]>([]);
  const [mode, setModeSignal] = createSignal<BuildMode>(null);
  const [toast, setToast] = createSignal<string | null>(null);
  const [dice, setDice] = createSignal<{ values: [number, number]; key: number } | null>(null);
  const [flights, setFlights] = createSignal<Flight[]>([]);
  const [victimChoice, setVictimChoice] = createSignal<{ tile: number; victims: number[]; knightVertex?: number } | null>(null);
  /** Seafaring: the ship picked to move (mode `move`). */
  const [moveFrom, setMoveFrom] = createSignal<number | null>(null);
  /** Citadels: the active knight picked to move or chase robber. */
  const [knightFrom, setKnightFrom] = createSignal<number | null>(null);
  /** Seafaring: a coastal edge that could take a road or a ship. */
  const [routeChoice, setRouteChoice] = createSignal<number | null>(null);
  /** Hovered 3D hex tile metadata for tooltip inspection. */
  const [tileHover, setTileHover] = createSignal<TileHoverInfo | null>(null);
  /** Unread chat messages since log drawer was last viewed. */
  const [unreadChat, setUnreadChat] = createSignal(0);
  /** Online turn timer: seconds remaining until autopilot acts. */
  const [turnSecondsLeft, setTurnSecondsLeft] = createSignal<number | null>(null);
  let deadlineAt: number | null = null;
  const countdownTimer = window.setInterval(() => {
    if (deadlineAt === null) {
      if (turnSecondsLeft() !== null) setTurnSecondsLeft(null);
      return;
    }
    const secs = Math.max(0, Math.ceil((deadlineAt - performance.now()) / 1000));
    if (secs !== turnSecondsLeft()) {
      setTurnSecondsLeft(secs);
      const v = view();
      if (secs > 0 && secs <= 10 && me() !== null && v?.toMove === me() && v.winner === null) {
        playTimerTick(secs <= 5);
      }
    }
  }, 250);

  const seafaring =
    source.board.layout === 'Archipelago' ||
    source.board.layout === 'FogIslands' ||
    source.board.layout === 'ArchipelagoExtended' ||
    source.board.layout === 'FogIslandsExtended';
  const citadels = source.board.config.scenario === 'Citadels';
  const fishermen = source.board.config.scenario === 'Fishermen';
  const caravans = source.board.config.scenario === 'Caravans';

  let logId = 0;
  let flightId = 0;
  let toastTimer: number | undefined;
  let disposed = false;

  const name = (seat: number) => (seat === me() && !source.replay ? '你' : source.seat(seat).name);
  const actions = () => legal().actions;
  const has = (type: Action['type']) => actions().some((a) => a.type === type);

  renderer.setBoard(board, topology);
  renderer.onPick = onPick;
  renderer.onHoverTile = (h: TileHover | null) => {
    if (!h) {
      if (tileHover() !== null) setTileHover(null);
      return;
    }
    const tCode = board.terrain[h.tile];
    const terrain = TERRAINS[tCode] ?? 'Desert';
    const num = board.numbers[h.tile] ?? 0;
    const v = view();
    setTileHover({
      tile: h.tile,
      clientX: h.clientX,
      clientY: h.clientY,
      terrain,
      number: num,
      pips: PIPS[num] ?? 0,
      hasRobber: v?.robber === h.tile && terrain !== 'Fog' && terrain !== 'Sea',
      hasPirate: v?.pirate === h.tile,
    });
  };

  // ---- state sync ----

  function syncFog(v: PlayerView) {
    const f = v.fog;
    if (!f) return;
    const nextTerrain = f.terrain.map((t) => TERRAINS.indexOf(t));
    const changed =
      nextTerrain.some((t, i) => t !== board.terrain[i]) ||
      f.numbers.some((n, i) => n !== board.numbers[i]) ||
      f.islands.some((id, i) => id !== board.islands[i]);
    if (!changed) return;
    board.terrain = nextTerrain;
    board.numbers = [...f.numbers];
    board.islands = [...f.islands];
    renderer.updateBoard(board);
  }

  function onState(s: SourceState) {
    if (disposed) return;
    const prev = view();
    if (s.turnRemainingMs !== undefined) {
      deadlineAt = s.turnRemainingMs !== null && s.view.winner === null ? performance.now() + s.turnRemainingMs : null;
      setTurnSecondsLeft(deadlineAt !== null ? Math.max(0, Math.ceil((deadlineAt - performance.now()) / 1000)) : null);
    } else if (s.view.winner !== null) {
      deadlineAt = null;
      setTurnSecondsLeft(null);
    }
    syncFog(s.view);
    if (s.resync) {
      setLog([]);
      if (s.log?.length) handleEvents(s.log, s.view, false);
    } else {
      handleEvents(s.events, prev ?? s.view, true);
      playForEvents(s.events, me());
      if (
        !source.replay &&
        me() !== null &&
        prev !== null &&
        prev.toMove !== me() &&
        s.view.toMove === me() &&
        s.view.winner === null
      ) {
        playYourTurn();
      }
    }
    batch(() => {
      setView(s.view);
      setLegal(s.legal);
      if (mode() && !modeAvailable(mode(), s.legal.actions)) setModeSignal(null);
      if (mode() !== 'move') setMoveFrom(null);
      if (mode() !== 'knightMove' && mode() !== 'chase') setKnightFrom(null);
      if (s.view.phase.type !== 'MoveRobber' && mode() !== 'chase') setVictimChoice(null);
      setRouteChoice(null);
    });
    renderer.setPieces(
      {
        buildings: s.view.buildings,
        roads: s.view.roads,
        ships: s.view.ships,
        bridges: s.view.bridges,
        camels: s.view.camels,
        robber: s.view.robber,
        pirate: s.view.pirate,
        knights: s.view.citadels?.knights,
        metropolises: s.view.citadels?.metropolises,
        barbarianPos: s.view.citadels?.barbarianPos ?? null,
        barbarianSteps: s.view.citadels?.barbarianSteps,
      },
      !s.resync,
    );
    renderer.setHighlights(highlights());
  }

  const MODE_ACTION: Record<Exclude<BuildMode, null>, Action['type']> = {
    road: 'BuildRoad',
    settlement: 'BuildSettlement',
    city: 'BuildCity',
    ship: 'BuildShip',
    move: 'MoveShip',
    knight: 'RecruitKnight',
    activate: 'ActivateKnight',
    promote: 'PromoteKnight',
    knightMove: 'MoveKnight',
    chase: 'ChaseRobber',
    fishRoad: 'FishBuildRoad',
    bridge: 'BuildBridge',
  };

  function modeAvailable(m: BuildMode, list: Action[]) {
    return m !== null && list.some((a) => a.type === MODE_ACTION[m]);
  }

  function highlights(): Highlights | null {
    const v = view();
    const list = actions();
    if (!v || !list.length) return null;
    const ids = (
      type:
        | 'BuildRoad'
        | 'BuildSettlement'
        | 'BuildCity'
        | 'BuildShip'
        | 'RecruitKnight'
        | 'ActivateKnight'
        | 'PromoteKnight'
        | 'FishBuildRoad'
        | 'PlaceCamel'
        | 'BuildBridge',
    ) => list.flatMap((a) => (a.type === type ? [a.value] : []));
    switch (v.phase.type) {
      case 'SetupSettlement':
        return { kind: 'vertex', ids: ids('BuildSettlement') };
      case 'SetupRoad':
      case 'RoadBuilding':
        return { kind: 'edge', ids: [...new Set([...ids('BuildRoad'), ...ids('BuildShip'), ...ids('BuildBridge')])] };
      case 'PlaceCamel':
        return { kind: 'edge', ids: ids('PlaceCamel') };
      case 'MoveRobber':
        return { kind: 'tile', ids: [...new Set(list.flatMap((a) => (a.type === 'MoveRobber' ? [a.value.tile] : [])))] };
      case 'Main':
      case 'SpecialBuild':
      case 'PairedTurn':
        if (v.toMove !== me() || (v.phase.type === 'Main' && v.current !== me())) return null;
        if (mode() === 'road') return { kind: 'edge', ids: ids('BuildRoad') };
        if (mode() === 'fishRoad') return { kind: 'edge', ids: ids('FishBuildRoad') };
        if (mode() === 'bridge') return { kind: 'edge', ids: ids('BuildBridge') };
        if (mode() === 'settlement') return { kind: 'vertex', ids: ids('BuildSettlement') };
        if (mode() === 'city') return { kind: 'vertex', ids: ids('BuildCity') };
        if (mode() === 'ship') return { kind: 'edge', ids: ids('BuildShip') };
        if (mode() === 'knight') return { kind: 'vertex', ids: ids('RecruitKnight') };
        if (mode() === 'activate') return { kind: 'vertex', ids: ids('ActivateKnight') };
        if (mode() === 'promote') return { kind: 'vertex', ids: ids('PromoteKnight') };
        if (mode() === 'move') {
          const moves = list.flatMap((a) => (a.type === 'MoveShip' ? [a.value] : []));
          const from = moveFrom();
          return from === null
            ? { kind: 'edge', ids: [...new Set(moves.map((m) => m.from))] }
            : { kind: 'edge', ids: moves.filter((m) => m.from === from).map((m) => m.to) };
        }
        if (mode() === 'knightMove') {
          const moves = list.flatMap((a) => (a.type === 'MoveKnight' ? [a.value] : []));
          const from = knightFrom();
          return from === null
            ? { kind: 'vertex', ids: [...new Set(moves.map((m) => m.from))] }
            : { kind: 'vertex', ids: moves.filter((m) => m.from === from).map((m) => m.to) };
        }
        if (mode() === 'chase') {
          const chases = list.flatMap((a) => (a.type === 'ChaseRobber' ? [a.value] : []));
          const kv = knightFrom();
          return kv === null
            ? { kind: 'vertex', ids: [...new Set(chases.map((c) => c.vertex))] }
            : { kind: 'tile', ids: [...new Set(chases.filter((c) => c.vertex === kv).map((c) => c.tile))] };
        }
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
      if (mode() === 'chase' && knightFrom() !== null) {
        const kv = knightFrom()!;
        const options = actions().flatMap((a) =>
          a.type === 'ChaseRobber' && a.value.vertex === kv && a.value.tile === id ? [a.value.victim] : [],
        );
        const victims = options.filter((x): x is number => x !== null);
        if (victims.length > 1) setVictimChoice({ tile: id, victims, knightVertex: kv });
        else act((a) => a.type === 'ChaseRobber' && a.value.vertex === kv && a.value.tile === id);
        return;
      }
      const options = actions().flatMap((a) => (a.type === 'MoveRobber' && a.value.tile === id ? [a.value.victim] : []));
      const victims = options.filter((x): x is number => x !== null);
      if (victims.length > 1) setVictimChoice({ tile: id, victims });
      else act((a) => a.type === 'MoveRobber' && a.value.tile === id);
      return;
    }
    if (kind === 'edge') {
      if (v.phase.type === 'PlaceCamel') {
        act((a) => a.type === 'PlaceCamel' && a.value === id);
        return;
      }
      if (mode() === 'fishRoad') {
        act((a) => a.type === 'FishBuildRoad' && a.value === id);
        return;
      }
      if (mode() === 'bridge') {
        act((a) => a.type === 'BuildBridge' && a.value === id);
        return;
      }
      if (mode() === 'move') {
        const from = moveFrom();
        if (from === null) {
          setMoveFrom(id);
          renderer.setHighlights(highlights());
        } else {
          act((a) => a.type === 'MoveShip' && a.value.from === from && a.value.to === id);
        }
        return;
      }
      const bridge = actions().some((a) => a.type === 'BuildBridge' && a.value === id);
      if (bridge && !actions().some((a) => a.type === 'BuildRoad' && a.value === id)) {
        act((a) => a.type === 'BuildBridge' && a.value === id);
        return;
      }
      const road = actions().some((a) => a.type === 'BuildRoad' && a.value === id) && mode() !== 'ship';
      const ship = actions().some((a) => a.type === 'BuildShip' && a.value === id) && mode() !== 'road';
      if (road && ship) setRouteChoice(id);
      else if (ship) act((a) => a.type === 'BuildShip' && a.value === id);
      else act((a) => a.type === 'BuildRoad' && a.value === id);
      return;
    }
    if (mode() === 'knight') {
      act((a) => a.type === 'RecruitKnight' && a.value === id);
      return;
    }
    if (mode() === 'activate') {
      act((a) => a.type === 'ActivateKnight' && a.value === id);
      return;
    }
    if (mode() === 'promote') {
      act((a) => a.type === 'PromoteKnight' && a.value === id);
      return;
    }
    if (mode() === 'knightMove') {
      const from = knightFrom();
      if (from === null) {
        setKnightFrom(id);
        renderer.setHighlights(highlights());
      } else {
        act((a) => a.type === 'MoveKnight' && a.value.from === from && a.value.to === id);
      }
      return;
    }
    if (mode() === 'chase') {
      setKnightFrom(id);
      renderer.setHighlights(highlights());
      return;
    }
    if ((v.phase.type === 'Main' || v.phase.type === 'SpecialBuild' || v.phase.type === 'PairedTurn') && mode() === 'city')
      act((a) => a.type === 'BuildCity' && a.value === id);
    else act((a) => a.type === 'BuildSettlement' && a.value === id);
  }

  function setMode(m: BuildMode) {
    setModeSignal(mode() === m ? null : m);
    setMoveFrom(null);
    setKnightFrom(null);
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
        case 'TileRevealed':
          if (animate && e.value.reward !== null) {
            const r = RESOURCES.indexOf(e.value.reward);
            if (r >= 0) launchFrom(e.value.player, ([0, 0, 0, 0, 0].map((_, i) => (i === r ? 1 : 0)) as ResourceSet), [e.value.tile]);
          }
          break;
      }
    }
    if (lines.length) setLog((old) => [...old, ...lines].slice(-100));
  }

  function onChat(line: ChatLine) {
    const who = line.seat === me() && me() !== null ? '你' : line.name;
    setLog((old) => [...old, { id: ++logId, text: line.text, seat: line.seat, chat: { name: who } }].slice(-100));
    if (line.seat !== me()) setUnreadChat((n) => n + 1);
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
    window.clearInterval(countdownTimer);
    renderer.onPick = undefined;
    renderer.onHoverTile = undefined;
    renderer.setHighlights(null);
    source.dispose();
  }

  source.start({
    state: onState,
    error: (code) => showToast(errorText(code)),
    chat: onChat,
  });

  return {
    config: source.board.config,
    board,
    topology,
    get me() {
      return me();
    },
    online: source.online,
    /** Ships, the pirate and gold are in play. */
    seafaring,
    /** Commodities, city tracks, knights and barbarian raids are in play. */
    citadels,
    /** Coastal fishing grounds, fish tokens and the Old Boot are in play. */
    fishermen,
    /** Oasis camel caravans, golden river bridges and gold coins are in play. */
    caravans,
    /** Playback controls when watching a replay. */
    replay: source.replay,
    /** The finished game as a replay record (local games and online rooms). */
    exportReplay: source.exportReplay ? () => source.exportReplay!() : undefined,
    /** Shareable link to the finished game's replay (online rooms). */
    replayLink: source.replayLink ? () => source.replayLink!() : undefined,
    /** Current view (always set once the source has started). */
    view: () => view()!,
    legal,
    log,
    mode,
    thinking: () => source.thinking(),
    turnSecondsLeft,
    seat: (s: number) => source.seat(s),
    toast,
    dice,
    flights,
    victimChoice,
    moveFrom,
    knightFrom,
    routeChoice,
    clearRouteChoice: () => setRouteChoice(null),
    tileHover,
    unreadChat,
    clearUnreadChat: () => setUnreadChat(0),
    name,
    has,
    act,
    proposeTrade,
    setMode,
    chat: source.chat ? (text: string) => source.chat!(text) : undefined,
    clearVictimChoice: () => setVictimChoice(null),
    showToast,
    dispose,
    /** Finds the legal Discard/YearOfPlenty/ChooseGold action matching a set. */
    actSet(type: 'Discard' | 'PlayYearOfPlenty' | 'ChooseGold', set: ResourceSet) {
      return act((a) => a.type === type && sameSet(a.value, set));
    },
    actDiscardCitadel(resources: ResourceSet, commodities: CommoditySet) {
      return act((a) => a.type === 'DiscardCitadel' && sameSet(a.value.resources, resources) && sameSet(a.value.commodities, commodities));
    },
  };
}

export type Game = ReturnType<typeof createGame>;
