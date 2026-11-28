import { Match, type Topology } from '../engine';
import { RESOURCE_NAME } from '../game/text';
import type { BoardLayout, HarborKind, Resource, Scenario, Terrain } from '../generated/proto';

export interface DraftMap {
  layout: BoardLayout;
  terrain: Terrain[];
  numbers: number[];
  harbors: HarborKind[];
}

export interface DraftStats {
  landTiles: number;
  seaTiles: number;
  fogTiles: number;
  redAdjacent: number;
  pipsByResource: Record<string, number>;
}

export interface DraftValidation {
  ok: boolean;
  errors: string[];
  warnings: string[];
  stats: DraftStats;
}

export interface MapPreset {
  id: string;
  name: string;
  description: string;
  build: (layout: BoardLayout, topo: Topology, seed?: number) => DraftMap;
}

export interface LayoutSpec {
  code: string;
  numTiles: number;
  numHarbors: number;
  label: string;
}

export const LAYOUT_SPECS: Record<BoardLayout, LayoutSpec> = {
  Base: { code: 'B', numTiles: 19, numHarbors: 9, label: '标准岛屿 (19格 · 2–4人)' },
  Extended: { code: 'E', numTiles: 30, numHarbors: 11, label: '扩展大岛 (30格 · 5–6人)' },
  Archipelago: { code: 'A', numTiles: 33, numHarbors: 9, label: '群岛海域 (33格 · 2–4人)' },
  FogIslands: { code: 'F', numTiles: 33, numHarbors: 9, label: '迷雾海域 (33格 · 2–4人)' },
  ArchipelagoExtended: { code: 'X', numTiles: 46, numHarbors: 11, label: '扩展群岛 (46格 · 5–6人)' },
  FogIslandsExtended: { code: 'Z', numTiles: 46, numHarbors: 11, label: '扩展迷雾 (46格 · 5–6人)' },
};

const CHAR_TO_LAYOUT: Record<string, BoardLayout> = {
  B: 'Base',
  E: 'Extended',
  A: 'Archipelago',
  F: 'FogIslands',
  X: 'ArchipelagoExtended',
  Z: 'FogIslandsExtended',
};

const TERRAIN_TO_CHAR: Record<Terrain, string> = {
  Forest: 'L',
  Hills: 'B',
  Pasture: 'W',
  Fields: 'G',
  Mountains: 'O',
  Desert: 'D',
  Sea: 'S',
  Gold: 'U',
  Fog: 'Q',
};

const CHAR_TO_TERRAIN: Record<string, Terrain> = {
  L: 'Forest',
  B: 'Hills',
  W: 'Pasture',
  G: 'Fields',
  O: 'Mountains',
  D: 'Desert',
  S: 'Sea',
  U: 'Gold',
  Q: 'Fog',
};

const NUMBER_TO_CHAR: Record<number, string> = {
  0: '0',
  2: '2',
  3: '3',
  4: '4',
  5: '5',
  6: '6',
  8: '8',
  9: '9',
  10: 'A',
  11: 'B',
  12: 'C',
};

const CHAR_TO_NUMBER: Record<string, number> = {
  '0': 0,
  '2': 2,
  '3': 3,
  '4': 4,
  '5': 5,
  '6': 6,
  '8': 8,
  '9': 9,
  A: 10,
  B: 11,
  C: 12,
};

const RESOURCE_TO_CHAR: Record<Resource, string> = {
  Lumber: 'L',
  Brick: 'B',
  Wool: 'W',
  Grain: 'G',
  Ore: 'O',
};

const CHAR_TO_HARBOR: Record<string, HarborKind> = {
  '3': { type: 'Generic' },
  L: { type: 'Special', value: 'Lumber' },
  B: { type: 'Special', value: 'Brick' },
  W: { type: 'Special', value: 'Wool' },
  G: { type: 'Special', value: 'Grain' },
  O: { type: 'Special', value: 'Ore' },
};

export const PIP_COUNT: readonly number[] = [0, 0, 1, 2, 3, 4, 5, 0, 5, 4, 3, 2, 1];
export const VALID_NUMBERS: readonly number[] = [2, 3, 4, 5, 6, 8, 9, 10, 11, 12];
export const GOLD_SAFE_NUMBERS: readonly number[] = [3, 4, 5, 9, 10, 11];

const BASE_NUMBER_CYCLE: readonly number[] = [
  5, 6, 8, 9, 4, 10, 3, 11, 2, 12, 4, 10, 5, 9, 3, 11, 6, 8,
];

const TERRAIN_RESOURCE: Partial<Record<Terrain, Resource>> = {
  Hills: 'Brick',
  Forest: 'Lumber',
  Pasture: 'Wool',
  Fields: 'Grain',
  Mountains: 'Ore',
};

const BASIC_RESOURCES: readonly Resource[] = ['Lumber', 'Brick', 'Wool', 'Grain', 'Ore'];

function mulberry32(seed: number): () => number {
  let s = (seed >>> 0) || 1;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function shuffleInPlace<T>(arr: T[], rand: () => number): T[] {
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
}

export function isSeafaringLayout(layout: BoardLayout): boolean {
  return (
    layout === 'Archipelago' ||
    layout === 'FogIslands' ||
    layout === 'ArchipelagoExtended' ||
    layout === 'FogIslandsExtended'
  );
}

export function isFogLayout(layout: BoardLayout): boolean {
  return layout === 'FogIslands' || layout === 'FogIslandsExtended';
}

export function isExtendedLayout(layout: BoardLayout): boolean {
  return (
    layout === 'Extended' ||
    layout === 'ArchipelagoExtended' ||
    layout === 'FogIslandsExtended'
  );
}

export function isLandTerrain(terrain: Terrain): boolean {
  return terrain !== 'Sea' && terrain !== 'Fog';
}

export function isProducingTerrain(terrain: Terrain): boolean {
  return terrain !== 'Desert' && terrain !== 'Sea' && terrain !== 'Fog';
}

export function layoutFor(scenario: Scenario, players: number): BoardLayout {
  const ext = players > 4;
  switch (scenario) {
    case 'Archipelago':
      return ext ? 'ArchipelagoExtended' : 'Archipelago';
    case 'FogIslands':
      return ext ? 'FogIslandsExtended' : 'FogIslands';
    case 'Classic':
    case 'Citadels':
    case 'Fishermen':
      return ext ? 'Extended' : 'Base';
  }
}

export function defaultScenarioPlayersForLayout(layout: BoardLayout): { scenario: Scenario; players: number } {
  switch (layout) {
    case 'Base':
      return { scenario: 'Classic', players: 4 };
    case 'Extended':
      return { scenario: 'Classic', players: 6 };
    case 'Archipelago':
      return { scenario: 'Archipelago', players: 4 };
    case 'FogIslands':
      return { scenario: 'FogIslands', players: 4 };
    case 'ArchipelagoExtended':
      return { scenario: 'Archipelago', players: 6 };
    case 'FogIslandsExtended':
      return { scenario: 'FogIslands', players: 6 };
  }
}

export function cloneHarbor(h: HarborKind): HarborKind {
  return h.type === 'Generic' ? { type: 'Generic' } : { type: 'Special', value: h.value };
}

export function cloneDraft(draft: DraftMap): DraftMap {
  return {
    layout: draft.layout,
    terrain: [...draft.terrain],
    numbers: [...draft.numbers],
    harbors: draft.harbors.map(cloneHarbor),
  };
}

/** Encodes a `DraftMap` into the canonical `IM1:<layoutChar>:<tiles>:<harbors>` wire format. */
export function encodeCustomMap(draft: DraftMap): string {
  const spec = LAYOUT_SPECS[draft.layout];
  let tiles = '';
  for (let t = 0; t < spec.numTiles; t++) {
    const ter = draft.terrain[t] ?? 'Desert';
    const rawNum = isProducingTerrain(ter) ? (draft.numbers[t] ?? 0) : 0;
    const nChar = NUMBER_TO_CHAR[rawNum] ?? '0';
    tiles += TERRAIN_TO_CHAR[ter] + nChar;
  }
  let harbors = '';
  for (let h = 0; h < spec.numHarbors; h++) {
    const hk = draft.harbors[h] ?? { type: 'Generic' };
    harbors += hk.type === 'Generic' ? '3' : RESOURCE_TO_CHAR[hk.value];
  }
  return `IM1:${spec.code}:${tiles}:${harbors}`;
}

/** Decodes an `IM1:<layoutChar>:<tiles>:<harbors>` string into a `DraftMap` or `{ error }`. */
export function decodeCustomMap(code: string): DraftMap | { error: string } {
  const trimmed = code.trim().toUpperCase();
  const parts = trimmed.split(':');
  if (parts.length !== 4 || parts[0] !== 'IM1') {
    return { error: '地图短码格式错误：应为 IM1:<规格>:<地块>:<港口>' };
  }
  const layout = CHAR_TO_LAYOUT[parts[1]];
  if (!layout) {
    return { error: `未知的棋盘规格代码「${parts[1]}」` };
  }
  const spec = LAYOUT_SPECS[layout];
  const [, , tilesStr, harborsStr] = parts;
  if (tilesStr.length !== spec.numTiles * 2) {
    return {
      error: `地块段长度不匹配（${layout} 需 ${spec.numTiles * 2} 字符，实际 ${tilesStr.length} 字符）`,
    };
  }
  if (harborsStr.length !== spec.numHarbors) {
    return {
      error: `港口段长度不匹配（${layout} 需 ${spec.numHarbors} 字符，实际 ${harborsStr.length} 字符）`,
    };
  }

  const terrain: Terrain[] = [];
  const numbers: number[] = [];
  for (let t = 0; t < spec.numTiles; t++) {
    const tc = tilesStr[t * 2];
    const nc = tilesStr[t * 2 + 1];
    const ter = CHAR_TO_TERRAIN[tc];
    if (!ter) {
      return { error: `第 ${t + 1} 格地形字符无效：「${tc}」` };
    }
    const num = CHAR_TO_NUMBER[nc];
    if (num === undefined) {
      return { error: `第 ${t + 1} 格数字字符无效：「${nc}」` };
    }
    terrain.push(ter);
    numbers.push(num);
  }

  const harbors: HarborKind[] = [];
  for (let h = 0; h < spec.numHarbors; h++) {
    const hc = harborsStr[h];
    const hk = CHAR_TO_HARBOR[hc];
    if (!hk) {
      return { error: `第 ${h + 1} 个港口字符无效：「${hc}」` };
    }
    harbors.push(cloneHarbor(hk));
  }

  return { layout, terrain, numbers, harbors };
}

/** Creates an official balanced starting `DraftMap` via the WASM match generator. */
export function createDefaultDraft(layout: BoardLayout, seed?: number): DraftMap {
  const actualSeed = seed !== undefined ? seed >>> 0 : (Math.floor(Math.random() * 0x7fffffff) || 1);
  const { scenario, players } = defaultScenarioPlayersForLayout(layout);
  const m = new Match(players, actualSeed, null, 'greedy', scenario);
  try {
    return {
      layout,
      terrain: [...m.board.terrain],
      numbers: [...m.board.numbers],
      harbors: m.board.harbors.map(cloneHarbor),
    };
  } finally {
    m.free();
  }
}

/** Builds tile-to-tile adjacency lists from shared vertices in `topo.tileVertices`. */
export function tileNeighbors(topo: Topology): number[][] {
  const numTiles = topo.tilePos.length / 2;
  const numVerts = topo.vertexPos.length / 2;
  const vertTiles: number[][] = Array.from({ length: numVerts }, () => []);
  for (let t = 0; t < numTiles; t++) {
    for (let k = 0; k < 6; k++) {
      const v = topo.tileVertices[t * 6 + k];
      if (v < numVerts) vertTiles[v].push(t);
    }
  }
  const adj: Set<number>[] = Array.from({ length: numTiles }, () => new Set<number>());
  for (let v = 0; v < numVerts; v++) {
    const touching = vertTiles[v];
    for (let i = 0; i < touching.length; i++) {
      for (let j = i + 1; j < touching.length; j++) {
        adj[touching[i]].add(touching[j]);
        adj[touching[j]].add(touching[i]);
      }
    }
  }
  return adj.map((s) => [...s].sort((a, b) => a - b));
}

/** Picks a sensible non-conflicting default number when painting a producing tile over Desert/Sea/Fog. */
export function suggestNumberForTile(
  draft: DraftMap,
  topo: Topology,
  tile: number,
  terrain: Terrain,
): number {
  if (!isProducingTerrain(terrain)) return 0;
  const counts = new Array<number>(13).fill(0);
  for (let t = 0; t < draft.numbers.length; t++) {
    if (t === tile) continue;
    const n = draft.numbers[t];
    if (n >= 2 && n <= 12) counts[n]++;
  }
  const neighbors = tileNeighbors(topo)[tile] ?? [];
  const neighborHasRed = neighbors.some((nb) => draft.numbers[nb] === 6 || draft.numbers[nb] === 8);
  const candidates = terrain === 'Gold'
    ? [5, 9, 4, 10, 3, 11]
    : [5, 9, 4, 10, 6, 8, 3, 11, 2, 12];

  let best = terrain === 'Gold' ? 5 : 4;
  let bestScore = Infinity;
  for (const n of candidates) {
    if (counts[n] >= 12) continue;
    const redPenalty = (n === 6 || n === 8) && neighborHasRed ? 100 : 0;
    const sameNeighborPenalty = neighbors.some((nb) => draft.numbers[nb] === n) ? 10 : 0;
    const score = counts[n] * 4 + redPenalty + sameNeighborPenalty;
    if (score < bestScore) {
      bestScore = score;
      best = n;
    }
  }
  return best;
}

function buildStandardNumberPool(count: number): number[] {
  const pool: number[] = [];
  const used = new Array<number>(13).fill(0);
  let idx = 0;
  const fallback = [5, 9, 4, 10, 3, 11, 2, 12, 6, 8];
  while (pool.length < count) {
    const candidate = BASE_NUMBER_CYCLE[idx % BASE_NUMBER_CYCLE.length];
    idx++;
    if (used[candidate] < 12) {
      pool.push(candidate);
      used[candidate]++;
    } else {
      const alt = fallback.find((n) => used[n] < 12);
      if (alt === undefined) break;
      pool.push(alt);
      used[alt]++;
    }
  }
  return pool;
}

function countRedAdjacent(numbers: readonly number[], neighbors: readonly number[][]): number {
  let count = 0;
  for (let t = 0; t < numbers.length; t++) {
    const a = numbers[t];
    if (a !== 6 && a !== 8) continue;
    for (const nb of neighbors[t]) {
      if (nb > t && (numbers[nb] === 6 || numbers[nb] === 8)) {
        count++;
      }
    }
  }
  return count;
}

function scoreNumberPlacement(
  terrain: readonly Terrain[],
  numbers: readonly number[],
  neighbors: readonly number[][],
): number {
  let score = countRedAdjacent(numbers, neighbors) * 1000;
  for (let t = 0; t < numbers.length; t++) {
    const n = numbers[t];
    if (n === 0) continue;
    if (terrain[t] === 'Gold' && (n === 6 || n === 8 || n === 2 || n === 12)) {
      score += 8;
    }
    for (const nb of neighbors[t]) {
      if (nb > t && numbers[nb] === n) {
        score += 3;
      }
    }
  }
  return score;
}

/**
 * Assigns balanced number tokens across all producing tiles while avoiding
 * adjacent red `6`/`8` numbers and respecting the max-12-per-number limit.
 */
export function autoBalanceNumbers(draft: DraftMap, topo: Topology, seed?: number): DraftMap {
  const next = cloneDraft(draft);
  const numTiles = topo.tilePos.length / 2;
  const neighbors = tileNeighbors(topo);
  const producing: number[] = [];
  for (let t = 0; t < numTiles; t++) {
    if (isProducingTerrain(next.terrain[t])) {
      producing.push(t);
    } else {
      next.numbers[t] = 0;
    }
  }
  if (producing.length === 0) return next;

  const currentValid = producing
    .map((t) => next.numbers[t])
    .filter((n) => VALID_NUMBERS.includes(n));
  const freq = new Array<number>(13).fill(0);
  for (const n of currentValid) freq[n]++;
  const maxFreq = Math.max(...freq);
  const redTotal = freq[6] + freq[8];
  const distinct = freq.filter((c) => c > 0).length;

  const canKeepCurrent =
    currentValid.length === producing.length &&
    maxFreq <= 12 &&
    redTotal <= Math.max(2, Math.ceil(producing.length * 0.28)) &&
    distinct >= Math.min(5, producing.length);

  const basePool = canKeepCurrent ? [...currentValid] : buildStandardNumberPool(producing.length);
  const rand = mulberry32(seed ?? ((Date.now() ^ (producing.length * 2654435761)) >>> 0));

  let bestNumbers = [...next.numbers];
  let bestScore = Infinity;

  const pool = [...basePool];
  for (let attempt = 0; attempt < 1600; attempt++) {
    shuffleInPlace(pool, rand);
    const trial = new Array<number>(numTiles).fill(0);
    for (let i = 0; i < producing.length; i++) {
      trial[producing[i]] = pool[i];
    }
    const sc = scoreNumberPlacement(next.terrain, trial, neighbors);
    if (sc < bestScore) {
      bestScore = sc;
      bestNumbers = trial;
      if (sc === 0) break;
    }
  }

  // Greedy swap & excess-red downgrade pass if needed so redAdjacent reaches 0.
  for (let guard = 0; guard < 80 && countRedAdjacent(bestNumbers, neighbors) > 0; guard++) {
    let improved = false;
    for (const i of producing) {
      if (bestNumbers[i] !== 6 && bestNumbers[i] !== 8) continue;
      if (!neighbors[i].some((nb) => bestNumbers[nb] === 6 || bestNumbers[nb] === 8)) continue;
      for (const j of producing) {
        if (i === j || bestNumbers[j] === 6 || bestNumbers[j] === 8) continue;
        [bestNumbers[i], bestNumbers[j]] = [bestNumbers[j], bestNumbers[i]];
        const sc = scoreNumberPlacement(next.terrain, bestNumbers, neighbors);
        if (sc < bestScore) {
          bestScore = sc;
          improved = true;
          break;
        }
        [bestNumbers[i], bestNumbers[j]] = [bestNumbers[j], bestNumbers[i]];
      }
      if (improved) break;
    }
    if (!improved) {
      // Too many 6/8 tokens for graph independence: replace one conflicting 6/8 with 5/9/4/10.
      const counts = new Array<number>(13).fill(0);
      for (const n of bestNumbers) if (n > 0) counts[n]++;
      const conflictTile = producing.find(
        (t) =>
          (bestNumbers[t] === 6 || bestNumbers[t] === 8) &&
          neighbors[t].some((nb) => bestNumbers[nb] === 6 || bestNumbers[nb] === 8),
      );
      const replacement = [5, 9, 4, 10, 3, 11].find((n) => counts[n] < 12);
      if (conflictTile !== undefined && replacement !== undefined) {
        bestNumbers[conflictTile] = replacement;
        bestScore = scoreNumberPlacement(next.terrain, bestNumbers, neighbors);
      } else {
        break;
      }
    }
  }

  next.numbers = bestNumbers;
  return next;
}

/**
 * Validates a `DraftMap` against exact Rust `Board::from_custom` constraints
 * and computes live pip & terrain balance diagnostics.
 */
export function validateDraftMap(draft: DraftMap, topo: Topology): DraftValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  const numTiles = topo.tilePos.length / 2;
  const numVerts = topo.vertexPos.length / 2;
  const numHarbors = topo.harborEdges.length;

  const pipsByResource: Record<string, number> = {
    Lumber: 0,
    Brick: 0,
    Wool: 0,
    Grain: 0,
    Ore: 0,
    Gold: 0,
  };

  if (draft.terrain.length !== numTiles || draft.numbers.length !== numTiles) {
    errors.push(`地块数组长度异常（期望 ${numTiles} 格）`);
  }
  if (draft.harbors.length !== numHarbors) {
    errors.push(`港口数组长度异常（期望 ${numHarbors} 处）`);
  }

  const len = Math.min(numTiles, draft.terrain.length, draft.numbers.length);
  let landTiles = 0;
  let seaTiles = 0;
  let fogTiles = 0;
  let desertTiles = 0;

  let badNonProducingNumber = false;
  let badProducingNumber = false;
  const numberCounts = new Array<number>(13).fill(0);

  for (let t = 0; t < len; t++) {
    const ter = draft.terrain[t];
    const num = draft.numbers[t];

    if (ter === 'Sea') seaTiles++;
    else if (ter === 'Fog') fogTiles++;
    else landTiles++;

    if (ter === 'Desert') desertTiles++;

    if (!isProducingTerrain(ter)) {
      if (num !== 0) badNonProducingNumber = true;
    } else if (!VALID_NUMBERS.includes(num)) {
      badProducingNumber = true;
    } else {
      numberCounts[num]++;
      const pips = PIP_COUNT[num] ?? 0;
      if (ter === 'Gold') {
        pipsByResource.Gold += pips;
      } else {
        const res = TERRAIN_RESOURCE[ter];
        if (res) pipsByResource[res] += pips;
      }
    }
  }

  const seafaring = isSeafaringLayout(draft.layout);
  const hasFog = isFogLayout(draft.layout);

  if (desertTiles < 1) {
    errors.push('至少需要 1 格荒漠作为强盗起点');
  }
  if (seafaring && seaTiles < 1) {
    errors.push('航海地图至少需要 1 格海洋安放海盗船');
  }
  if (!seafaring && seaTiles > 0) {
    errors.push('非航海陆地棋盘不能包含海洋格');
  }
  if (!hasFog && fogTiles > 0) {
    errors.push('当前玩法不支持迷雾格，请切换为「迷雾群岛」或替换为其他地形');
  }
  if (hasFog && fogTiles > 24) {
    errors.push(`迷雾格最多允许 24 格（当前 ${fogTiles} 格）`);
  }

  // Fog adjacency check: no Fog tile may share any vertex with any land tile.
  if (hasFog && fogTiles > 0) {
    const vertHasLand = new Uint8Array(numVerts);
    const vertHasFog = new Uint8Array(numVerts);
    for (let t = 0; t < len; t++) {
      const ter = draft.terrain[t];
      for (let k = 0; k < 6; k++) {
        const v = topo.tileVertices[t * 6 + k];
        if (v >= numVerts) continue;
        if (ter === 'Fog') vertHasFog[v] = 1;
        else if (isLandTerrain(ter)) vertHasLand[v] = 1;
      }
    }
    let fogTouchesLand = false;
    for (let v = 0; v < numVerts; v++) {
      if (vertHasLand[v] && vertHasFog[v]) {
        fogTouchesLand = true;
        break;
      }
    }
    if (fogTouchesLand) {
      errors.push('迷雾格必须由海洋与初始陆地隔开，不能直接毗邻陆地');
    }
  }

  if (badNonProducingNumber) {
    errors.push('荒漠、海洋与迷雾格数字必须为 0');
  }
  if (badProducingNumber) {
    errors.push('所有产出地形必须分配合法数字圆片（2–6 或 8–12）');
  }
  for (const n of VALID_NUMBERS) {
    if (numberCounts[n] > 12) {
      errors.push(`数字 ${n} 最多只能放置 12 格（当前 ${numberCounts[n]} 格）`);
    }
  }

  // Vertex-connected land components & setup vertex capacity check (matching Rust Board::from_custom).
  const neighbors = tileNeighbors(topo);
  const visitedTile = new Uint8Array(len);
  const allLandVerts = new Set<number>();
  const comps: { tileCount: number; start: number; vertCount: number }[] = [];

  for (let start = 0; start < len; start++) {
    if (visitedTile[start] || !isLandTerrain(draft.terrain[start])) continue;
    const queue = [start];
    visitedTile[start] = 1;
    let tileCount = 0;
    const compVerts = new Set<number>();
    while (queue.length > 0) {
      const cur = queue.pop()!;
      tileCount++;
      for (let k = 0; k < 6; k++) {
        const v = topo.tileVertices[cur * 6 + k];
        if (v < numVerts) {
          compVerts.add(v);
          allLandVerts.add(v);
        }
      }
      for (const nb of neighbors[cur]) {
        if (nb < len && !visitedTile[nb] && isLandTerrain(draft.terrain[nb])) {
          visitedTile[nb] = 1;
          queue.push(nb);
        }
      }
    }
    comps.push({ tileCount, start, vertCount: compVerts.size });
  }
  comps.sort((a, b) => b.tileCount - a.tileCount || a.start - b.start);
  const island1Verts = comps[0]?.vertCount ?? 0;

  const landVerts = allLandVerts.size;
  const minSetup = draft.layout.includes('Extended') || draft.layout === 'Extended' ? 45 : 29;
  const setupCap = seafaring && island1Verts >= minSetup ? island1Verts : landVerts;
  if ((!seafaring && landTiles < 2) || landTiles + seaTiles < 2 || setupCap < minSetup) {
    errors.push(`初始可建村交叉点不足，请增加陆地地块（当前 ${setupCap}/${minSetup}）`);
  }

  const redAdjacent = countRedAdjacent(draft.numbers.slice(0, len), neighbors);
  if (redAdjacent > 0) {
    warnings.push(`存在 ${redAdjacent} 处相邻红字（6/8），建议点击「一键均衡 6/8」分散高产点`);
  }

  const missingRes = BASIC_RESOURCES.filter((r) => (pipsByResource[r] ?? 0) === 0);
  if (missingRes.length > 0) {
    warnings.push(
      `缺少基础资源产出：${missingRes.map((r) => RESOURCE_NAME[r]).join('、')}（总点数为 0）`,
    );
  }

  return {
    ok: errors.length === 0,
    errors,
    warnings,
    stats: {
      landTiles,
      seaTiles,
      fogTiles,
      redAdjacent,
      pipsByResource,
    },
  };
}

// ---- Creative Preset Generators ----

function tileCentersWorld(topo: Topology): [number, number][] {
  const s = Math.sqrt(3) / 2;
  const out: [number, number][] = [];
  for (let i = 0; i < topo.tilePos.length; i += 2) {
    out.push([topo.tilePos[i] * s, topo.tilePos[i + 1] * 0.5]);
  }
  return out;
}

function buildGoldHeartPreset(layout: BoardLayout, topo: Topology, seed?: number): DraftMap {
  const draft = createDefaultDraft(layout, seed);
  const centers = tileCentersWorld(topo);
  const landIndices = draft.terrain
    .map((ter, i) => ({ ter, i }))
    .filter(({ ter }) => isLandTerrain(ter))
    .map(({ i }) => i);

  if (landIndices.length < 6) return draft;
  const cx = landIndices.reduce((acc, i) => acc + centers[i][0], 0) / landIndices.length;
  const cy = landIndices.reduce((acc, i) => acc + centers[i][1], 0) / landIndices.length;

  const byCenterDist = [...landIndices].sort((a, b) => {
    const da = Math.hypot(centers[a][0] - cx, centers[a][1] - cy);
    const db = Math.hypot(centers[b][0] - cx, centers[b][1] - cy);
    return da - db;
  });

  const goldTarget = isExtendedLayout(layout) ? 3 : 2;
  // Keep at least one Desert outside the gold core.
  const goldTiles = byCenterDist.slice(0, goldTarget);
  for (const gt of goldTiles) {
    if (draft.terrain[gt] === 'Desert') {
      const swapIdx = byCenterDist[byCenterDist.length - 1];
      draft.terrain[swapIdx] = 'Desert';
      draft.numbers[swapIdx] = 0;
    }
    draft.terrain[gt] = 'Gold';
    draft.numbers[gt] = 5;
  }

  if (!draft.terrain.includes('Desert')) {
    const fallbackDesert = byCenterDist[byCenterDist.length - 1];
    draft.terrain[fallbackDesert] = 'Desert';
    draft.numbers[fallbackDesert] = 0;
  }

  let balanced = autoBalanceNumbers(draft, topo, seed);
  const neighbors = tileNeighbors(topo);
  const preferredGoldTokens = [5, 9, 4, 10];
  goldTiles.forEach((gt, idx) => {
    const want = preferredGoldTokens[idx % preferredGoldTokens.length];
    if (balanced.numbers[gt] === want) return;
    const donor = landIndices.find(
      (i) =>
        !goldTiles.includes(i) &&
        balanced.numbers[i] === want &&
        isProducingTerrain(balanced.terrain[i]),
    );
    if (donor !== undefined) {
      [balanced.numbers[gt], balanced.numbers[donor]] = [balanced.numbers[donor], balanced.numbers[gt]];
    } else {
      balanced.numbers[gt] = want;
    }
  });
  if (countRedAdjacent(balanced.numbers, neighbors) > 0) {
    balanced = autoBalanceNumbers(balanced, topo, (seed ?? 7) + 31);
  }
  return balanced;
}

function buildTwinIslandsPreset(layout: BoardLayout, topo: Topology, seed?: number): DraftMap {
  const base = createDefaultDraft(layout, seed);
  const numTiles = topo.tilePos.length / 2;
  const rand = mulberry32(seed ?? 202603);

  if (!isSeafaringLayout(layout)) {
    // Land layouts (Base / Extended): central desert rift & twin gold peaks flanked by mirrored rich lands.
    const centers = tileCentersWorld(topo);
    const left: number[] = [];
    const right: number[] = [];
    const middle: number[] = [];
    for (let t = 0; t < numTiles; t++) {
      const x = centers[t][0];
      if (Math.abs(x) < 0.45) middle.push(t);
      else if (x < 0) left.push(t);
      else right.push(t);
    }
    middle.sort((a, b) => centers[a][1] - centers[b][1]);
    if (middle.length >= 3) {
      base.terrain[middle[0]] = 'Gold';
      base.terrain[middle[Math.floor(middle.length / 2)]] = 'Desert';
      base.terrain[middle[middle.length - 1]] = 'Gold';
      for (let i = 1; i < middle.length - 1; i++) {
        if (i !== Math.floor(middle.length / 2)) {
          base.terrain[middle[i]] = i % 2 === 0 ? 'Mountains' : 'Fields';
        }
      }
    }
    const richCycle: Terrain[] = ['Forest', 'Hills', 'Pasture', 'Fields', 'Mountains', 'Fields', 'Mountains', 'Forest', 'Pasture', 'Hills'];
    const offset = Math.floor(rand() * richCycle.length);
    left.sort((a, b) => centers[a][1] - centers[b][1] || centers[a][0] - centers[b][0]);
    right.sort((a, b) => centers[a][1] - centers[b][1] || -centers[a][0] + centers[b][0]);
    left.forEach((t, idx) => {
      base.terrain[t] = richCycle[(idx + offset) % richCycle.length];
    });
    right.forEach((t, idx) => {
      base.terrain[t] = richCycle[(idx + offset) % richCycle.length];
    });
    for (let t = 0; t < numTiles; t++) {
      base.numbers[t] = isProducingTerrain(base.terrain[t]) ? 5 : 0;
    }
    return autoBalanceNumbers(base, topo, seed);
  }

  if (isFogLayout(layout)) {
    // Fog layouts: split the western crescent into North & South twin starting islands across a central strait,
    // keeping the eastern sea buffer so no Fog tile ever touches land.
    const ys = Array.from({ length: numTiles }, (_, t) => topo.tilePos[2 * t + 1]);
    const xs = Array.from({ length: numTiles }, (_, t) => topo.tilePos[2 * t]);
    for (let t = 0; t < numTiles; t++) {
      if (base.terrain[t] === 'Fog' || base.terrain[t] === 'Sea') continue;
      if (ys[t] === 0 && xs[t] >= -4) {
        base.terrain[t] = 'Sea';
        base.numbers[t] = 0;
      }
    }
    const landLeft = Array.from({ length: numTiles }, (_, t) => t).filter((t) => isLandTerrain(base.terrain[t]));
    if (!landLeft.some((t) => base.terrain[t] === 'Desert') && landLeft.length > 0) {
      base.terrain[landLeft[0]] = 'Desert';
      base.numbers[landLeft[0]] = 0;
    }
    const northTip = landLeft.find((t) => base.terrain[t] !== 'Desert' && ys[t] < 0 && xs[t] >= -2);
    const southTip = [...landLeft].reverse().find((t) => base.terrain[t] !== 'Desert' && ys[t] > 0 && xs[t] >= -2);
    if (northTip !== undefined) base.terrain[northTip] = 'Gold';
    if (southTip !== undefined && southTip !== northTip) base.terrain[southTip] = 'Gold';
    for (let t = 0; t < numTiles; t++) {
      if (isProducingTerrain(base.terrain[t]) && base.numbers[t] === 0) base.numbers[t] = 5;
    }
    const candidate = autoBalanceNumbers(base, topo, seed);
    return validateDraftMap(candidate, topo).ok ? candidate : createDefaultDraft(layout, seed);
  }

  // Archipelago / ArchipelagoExtended: continuous 1-hex central sea channel splitting West & East twin continents!
  const west: number[] = [];
  const east: number[] = [];
  const strait: number[] = [];
  for (let t = 0; t < numTiles; t++) {
    const lx = topo.tilePos[2 * t];
    const evenRow = (lx & 1) === 0;
    if ((evenRow && lx === 0) || (!evenRow && lx === 1)) {
      strait.push(t);
    } else if (lx < 0) {
      west.push(t);
    } else {
      east.push(t);
    }
  }

  for (const t of strait) {
    base.terrain[t] = 'Sea';
    base.numbers[t] = 0;
  }

  const cycle: Terrain[] = [
    'Forest',
    'Fields',
    'Mountains',
    'Hills',
    'Pasture',
    'Forest',
    'Fields',
    'Pasture',
    'Mountains',
    'Hills',
    'Forest',
    'Pasture',
    'Fields',
    'Mountains',
  ];
  const shift = Math.floor(rand() * cycle.length);
  west.forEach((t, i) => {
    base.terrain[t] = cycle[(i + shift) % cycle.length];
    base.numbers[t] = 5;
  });
  east.forEach((t, i) => {
    base.terrain[t] = cycle[(i + shift + 2) % cycle.length];
    base.numbers[t] = 5;
  });

  // Place Deserts & strait-side Gold outposts on both islands.
  if (west.length > 0) {
    const westDesert = west[Math.floor(west.length / 2)];
    base.terrain[westDesert] = 'Desert';
    base.numbers[westDesert] = 0;
  }
  if (isExtendedLayout(layout) && east.length > 2) {
    const eastDesert = east[Math.floor(east.length / 2)];
    base.terrain[eastDesert] = 'Desert';
    base.numbers[eastDesert] = 0;
  }

  const westCoast = west.filter((t) => topo.tilePos[2 * t] >= -2 && base.terrain[t] !== 'Desert');
  const eastCoast = east.filter((t) => topo.tilePos[2 * t] <= 3 && base.terrain[t] !== 'Desert');
  if (westCoast.length > 0) base.terrain[westCoast[0]] = 'Gold';
  if (eastCoast.length > 0) base.terrain[eastCoast[eastCoast.length - 1]] = 'Gold';
  if (isExtendedLayout(layout) && eastCoast.length > 2) base.terrain[eastCoast[0]] = 'Gold';

  const balanced = autoBalanceNumbers(base, topo, seed);
  // Give Gold outposts 5 / 9 tokens when safe.
  const goldList = Array.from({ length: numTiles }, (_, t) => t).filter((t) => balanced.terrain[t] === 'Gold');
  goldList.forEach((gt, idx) => {
    const target = idx % 2 === 0 ? 5 : 9;
    const swapper = Array.from({ length: numTiles }, (_, i) => i).find(
      (i) => balanced.terrain[i] !== 'Gold' && balanced.numbers[i] === target,
    );
    if (swapper !== undefined) {
      [balanced.numbers[gt], balanced.numbers[swapper]] = [balanced.numbers[swapper], balanced.numbers[gt]];
    }
  });
  return validateDraftMap(balanced, topo).ok ? balanced : createDefaultDraft(layout, seed);
}

function buildIndustrialPreset(layout: BoardLayout, topo: Topology, seed?: number): DraftMap {
  const draft = createDefaultDraft(layout, seed);
  const numTiles = topo.tilePos.length / 2;

  const counts: Record<Terrain, number[]> = {
    Hills: [],
    Forest: [],
    Pasture: [],
    Fields: [],
    Mountains: [],
    Desert: [],
    Sea: [],
    Gold: [],
    Fog: [],
  };
  for (let t = 0; t < numTiles; t++) {
    counts[draft.terrain[t]].push(t);
  }

  const convertTarget = isExtendedLayout(layout) ? 4 : 2;
  let converted = 0;
  const donors: Terrain[] = ['Pasture', 'Forest', 'Hills'];
  for (const donor of donors) {
    while (counts[donor].length > 2 && converted < convertTarget) {
      const idx = counts[donor].pop()!;
      const nextTer: Terrain = converted % 2 === 0 ? 'Mountains' : 'Fields';
      draft.terrain[idx] = nextTer;
      counts[nextTer].push(idx);
      converted++;
    }
  }

  const balanced = autoBalanceNumbers(draft, topo, seed);
  // Route top-pip non-red numbers (5/9) toward Mountains & Fields while preserving red separation.
  const neighbors = tileNeighbors(topo);
  const industrialTiles = Array.from({ length: numTiles }, (_, t) => t).filter(
    (t) => balanced.terrain[t] === 'Mountains' || balanced.terrain[t] === 'Fields',
  );
  const otherTiles = Array.from({ length: numTiles }, (_, t) => t).filter(
    (t) =>
      balanced.terrain[t] === 'Pasture' ||
      balanced.terrain[t] === 'Forest' ||
      balanced.terrain[t] === 'Hills',
  );

  for (const ind of industrialTiles) {
    const curPips = PIP_COUNT[balanced.numbers[ind]] ?? 0;
    if (curPips >= 4) continue;
    const richOther = otherTiles.find((o) => {
      const n = balanced.numbers[o];
      return (n === 5 || n === 9) && (PIP_COUNT[n] ?? 0) > curPips;
    });
    if (richOther !== undefined) {
      [balanced.numbers[ind], balanced.numbers[richOther]] = [
        balanced.numbers[richOther],
        balanced.numbers[ind],
      ];
    }
  }

  if (countRedAdjacent(balanced.numbers, neighbors) > 0) {
    return autoBalanceNumbers(balanced, topo, (seed ?? 11) + 97);
  }
  return balanced;
}

export const MAP_PRESETS: readonly MapPreset[] = [
  {
    id: 'standard',
    name: '随机均衡标准图',
    description: '官方标准地形与港口配比，6/8 红字自动互不相邻',
    build: (layout, _topo, seed) => createDefaultDraft(layout, seed),
  },
  {
    id: 'gold-heart',
    name: '黄金中心岛',
    description: '岛屿腹地富集 5/9 点金矿脉，外圈环绕均衡五系资源',
    build: buildGoldHeartPreset,
  },
  {
    id: 'twin-strait',
    name: '双子对称岛 / 海峡争锋',
    description: '狭长海峡或裂谷将版图一分为二，海岸金矿哨站兵家必争',
    build: buildTwinIslandsPreset,
  },
  {
    id: 'industrial',
    name: '富矿工业岛',
    description: '丰沛山脉矿石与肥沃麦田，专为快速起城与「城邦与铁卫」打造',
    build: buildIndustrialPreset,
  },
];
