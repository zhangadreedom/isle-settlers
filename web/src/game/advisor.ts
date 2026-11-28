// AI Setup Settlement Advisor: heuristic vertex evaluator for initial
// settlement/city placement (`Phase.type === 'SetupSettlement'`).

import type { BoardData } from '../board/BoardRenderer';
import { EMPTY, type Topology } from '../engine';
import type { PlayerView } from '../generated/proto';
import { RESOURCES, RESOURCE_SHORT } from './text';

export interface SetupAdvice {
  vertex: number;
  rank: 1 | 2 | 3;
  score: number;
  pips: number;
  tier: 'S' | 'A' | 'B';
  summary: string;
  tags: string[];
}

const ADVISOR_STORAGE_KEY = 'isle.advisor';

/** Standard 2d6 pip counts indexed by roll number `0..12` (`7` produces nothing). */
const PIPS: readonly number[] = [0, 0, 1, 2, 3, 4, 5, 0, 5, 4, 3, 2, 1];

/** Terrain numeric codes matching `BoardData.terrain` (`controller.ts`). */
const TERRAIN_FOREST = 1;
const TERRAIN_PASTURE = 2;
const TERRAIN_MOUNTAINS = 4;
const TERRAIN_DESERT = 5;
const TERRAIN_SEA = 6;
const TERRAIN_GOLD = 7;
const TERRAIN_FOG = 8;

const COMMODITY_BY_TERRAIN: Partial<Record<number, string>> = {
  [TERRAIN_FOREST]: '纸',
  [TERRAIN_PASTURE]: '布',
  [TERRAIN_MOUNTAINS]: '币',
};

function readInitialAdvisor(): boolean {
  try {
    if (typeof localStorage === 'undefined') return false;
    const raw = localStorage.getItem(ADVISOR_STORAGE_KEY);
    return raw === '1' || raw === 'true';
  } catch {
    return false;
  }
}

function persistAdvisor(on: boolean): void {
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(ADVISOR_STORAGE_KEY, on ? '1' : '0');
    }
  } catch {
    // Sandboxed iframes without `allow-same-origin` throw SecurityError.
  }
}

let advisorEnabled = readInitialAdvisor();
const advisorListeners = new Set<(on: boolean) => void>();

export function isAdvisorEnabled(): boolean {
  return advisorEnabled;
}

export function setAdvisorEnabled(on: boolean): void {
  const next = Boolean(on);
  advisorEnabled = next;
  persistAdvisor(next);
  for (const fn of advisorListeners) fn(next);
}

export function toggleAdvisor(): boolean {
  const next = !advisorEnabled;
  setAdvisorEnabled(next);
  return next;
}

export function onAdvisorChange(fn: (on: boolean) => void): () => void {
  advisorListeners.add(fn);
  return () => {
    advisorListeners.delete(fn);
  };
}

function isWaterOrFog(tCode: number | undefined): boolean {
  return tCode === undefined || tCode === TERRAIN_SEA || tCode === TERRAIN_FOG;
}

function pipsOf(num: number | undefined): number {
  if (num === undefined || num < 0 || num >= PIPS.length) return 0;
  return PIPS[num] ?? 0;
}

interface CandidateEval {
  vertex: number;
  score: number;
  pips: number;
  weightedPips: number;
  summary: string;
  tags: string[];
}

export function computeSetupAdvice(
  board: BoardData,
  topo: Topology,
  view: PlayerView,
  legalVertices: number[],
  scenario: string,
): SetupAdvice[] {
  if (!legalVertices.length) return [];

  const tileCount = board.terrain.length;
  const edgeCount = Math.floor(topo.edgeVertices.length / 2);

  // 1. Build vertex -> tiles, edge -> tiles, and vertex -> edges lookup tables.
  const vertexTiles = new Map<number, number[]>();
  const tileCornerSets: Set<number>[] = [];

  for (let t = 0; t < tileCount; t++) {
    const cSet = new Set<number>();
    for (let k = 0; k < 6; k++) {
      const v = topo.tileVertices[t * 6 + k];
      if (v === undefined) continue;
      cSet.add(v);
      let list = vertexTiles.get(v);
      if (!list) {
        list = [];
        vertexTiles.set(v, list);
      }
      if (!list.includes(t)) list.push(t);
    }
    tileCornerSets.push(cSet);
  }

  const edgeTiles: number[][] = Array.from({ length: edgeCount }, () => []);
  const vertexEdges = new Map<number, number[]>();
  const pushVertexEdge = (v: number, e: number) => {
    let arr = vertexEdges.get(v);
    if (!arr) {
      arr = [];
      vertexEdges.set(v, arr);
    }
    arr.push(e);
  };

  for (let e = 0; e < edgeCount; e++) {
    const va = topo.edgeVertices[2 * e];
    const vb = topo.edgeVertices[2 * e + 1];
    pushVertexEdge(va, e);
    pushVertexEdge(vb, e);
    for (let t = 0; t < tileCount; t++) {
      if (tileCornerSets[t].has(va) && tileCornerSets[t].has(vb)) {
        edgeTiles[e].push(t);
      }
    }
  }

  // 2. Map active coastal harbors (`5` = 3:1 Generic, `0..4` = 2:1 Resource) to vertices.
  const vertexHarbor = new Map<number, number>();
  const harborLimit = Math.min(board.harbors.length, topo.harborEdges.length);
  for (let slot = 0; slot < harborLimit; slot++) {
    const e = topo.harborEdges[slot];
    if (e === undefined || e >= edgeCount) continue;
    const adj = edgeTiles[e];
    const hasLand = adj.some((t) => !isWaterOrFog(board.terrain[t]));
    const hasWaterSide = adj.length <= 1 || adj.some((t) => isWaterOrFog(board.terrain[t]));
    if (!hasLand || !hasWaterSide) continue;
    const hCode = board.harbors[slot];
    if (hCode === undefined || hCode < 0) continue;
    const va = topo.edgeVertices[2 * e];
    const vb = topo.edgeVertices[2 * e + 1];
    // Prefer 2:1 special harbor label if two slots ever shared a corner, else store code.
    const prevA = vertexHarbor.get(va);
    if (prevA === undefined || prevA === 5) vertexHarbor.set(va, hCode);
    const prevB = vertexHarbor.get(vb);
    if (prevB === undefined || prevB === 5) vertexHarbor.set(vb, hCode);
  }

  // 3. Board-wide resource scarcity across all standard resource tiles (`0..4`).
  const boardResPips = [0, 0, 0, 0, 0];
  for (let t = 0; t < tileCount; t++) {
    const tCode = board.terrain[t];
    if (tCode >= 0 && tCode <= 4) {
      boardResPips[tCode] += pipsOf(board.numbers[t]);
    }
  }
  const totalBoardPips = boardResPips.reduce((a, b) => a + b, 0);
  const avgResPips = Math.max(1, totalBoardPips / 5);
  // Scarcity index in `[0, 1]`: > 0.24 signals a drought resource on this map.
  const resScarcity = boardResPips.map((p) =>
    p > 0 ? Math.max(0, (avgResPips - p) / avgResPips) : 0,
  );

  // 4. Inspect what `view.me` already covers from their 1st setup settlement.
  const mySeat = view.me?.seat ?? view.viewer ?? view.toMove;
  const myVertices: number[] = [];
  if (mySeat !== null && mySeat !== undefined) {
    for (let v = 0; v < view.buildings.length; v++) {
      const b = view.buildings[v];
      if (b !== undefined && b !== EMPTY && (b & 0x7f) === mySeat) {
        myVertices.push(v);
      }
    }
  }
  const isSecondPlacement = myVertices.length >= 1;
  const existingPips = [0, 0, 0, 0, 0];
  let existingGoldPips = 0;
  const existingSpecialHarbors = new Set<number>();

  for (const mv of myVertices) {
    const mh = vertexHarbor.get(mv);
    if (mh !== undefined && mh >= 0 && mh <= 4) existingSpecialHarbors.add(mh);
    for (const t of vertexTiles.get(mv) ?? []) {
      const tc = board.terrain[t];
      const p = pipsOf(board.numbers[t]);
      if (tc >= 0 && tc <= 4) existingPips[tc] += p;
      else if (tc === TERRAIN_GOLD) existingGoldPips += p;
    }
  }

  const isSeafaringScenario = scenario === 'Archipelago' || scenario === 'FogIslands';
  const isCitadelsCityPlacement = scenario === 'Citadels' && isSecondPlacement;

  // Helper: check sea-edge access and proximity (1-2 steps) to outer islands / fog.
  function evaluateSeaAccess(v: number): { hasSeaEdge: boolean; nearOuterOrFog: boolean } {
    const vTiles = vertexTiles.get(v) ?? [];
    const homeIslands = new Set(
      vTiles.map((t) => board.islands[t] ?? 0).filter((id) => id > 0),
    );
    let hasSeaEdge = false;
    let nearOuterOrFog = false;

    for (const e of vertexEdges.get(v) ?? []) {
      const eTiles = edgeTiles[e] ?? [];
      const touchesSeaOrFog = eTiles.some(
        (t) => board.terrain[t] === TERRAIN_SEA || board.terrain[t] === TERRAIN_FOG,
      );
      if (!touchesSeaOrFog) continue;
      hasSeaEdge = true;

      if (eTiles.some((t) => board.terrain[t] === TERRAIN_FOG)) {
        nearOuterOrFog = true;
      }

      // Look 1 hop further across the water vertex to detect nearby fog or new islands.
      const otherV = topo.edgeVertices[2 * e] === v ? topo.edgeVertices[2 * e + 1] : topo.edgeVertices[2 * e];
      for (const t2 of vertexTiles.get(otherV) ?? []) {
        if (board.terrain[t2] === TERRAIN_FOG) nearOuterOrFog = true;
        const isl = board.islands[t2] ?? 0;
        if (isl > 0 && !homeIslands.has(isl)) nearOuterOrFog = true;
      }
    }
    return { hasSeaEdge, nearOuterOrFog };
  }

  // 5. Score every unique legal vertex.
  const uniqueLegal = [...new Set(legalVertices)];
  const candidates: CandidateEval[] = [];

  for (const v of uniqueLegal) {
    const adjTiles = vertexTiles.get(v) ?? [];
    const resPips = [0, 0, 0, 0, 0];
    let rawPips = 0;
    let weightedPips = 0;
    let goldPips = 0;
    let extraTerrainPips = 0;
    const distinctNumbers = new Set<number>();
    const commodityNames: string[] = [];
    let commodityPips = 0;
    let forestPips = 0;

    for (const t of adjTiles) {
      const tc = board.terrain[t];
      const num = board.numbers[t] ?? 0;
      const p = pipsOf(num);
      if (p <= 0) continue;

      if (tc >= 0 && tc <= 4) {
        resPips[tc] += p;
        rawPips += p;
        weightedPips += p;
        distinctNumbers.add(num);

        const comm = COMMODITY_BY_TERRAIN[tc];
        if (comm) {
          commodityPips += p;
          if (!commodityNames.includes(comm)) commodityNames.push(comm);
        }
        if (tc === TERRAIN_FOREST) forestPips += p;
      } else if (tc === TERRAIN_GOLD) {
        goldPips += p;
        rawPips += p;
        weightedPips += p * 1.25; // Gold weighted 1.25x pips!
        distinctNumbers.add(num);
      } else if (tc !== TERRAIN_DESERT && tc !== TERRAIN_SEA && tc !== TERRAIN_FOG) {
        // Graceful fallback for any future producing terrain (e.g. Fishermen lake/shoal).
        extraTerrainPips += p;
        rawPips += p;
        weightedPips += p * 1.1;
        distinctNumbers.add(num);
      }
    }

    // (A) Expected Pip Yield
    let score = weightedPips * 1.18 + distinctNumbers.size * 0.35;

    // (B) Resource Diversity & Synergy
    const vertexResIndices = [0, 1, 2, 3, 4].filter((r) => resPips[r] > 0);
    const effectiveDistinctOnV = Math.min(
      3,
      vertexResIndices.length + (goldPips > 0 ? 1 : 0) + (extraTerrainPips > 0 ? 1 : 0),
    );
    if (effectiveDistinctOnV >= 3) score += 1.85;
    else if (effectiveDistinctOnV === 2) score += 0.85;

    const combinedPips = [0, 1, 2, 3, 4].map((r) => existingPips[r] + resPips[r]);
    const missingCovered: number[] = [];

    if (isSecondPlacement) {
      for (let r = 0; r < 5; r++) {
        if (existingPips[r] === 0 && resPips[r] > 0) {
          missingCovered.push(r);
          score += 1.4 + Math.min(resPips[r], 5) * 0.24;
        } else if (
          existingPips[r] >= 5 &&
          resPips[r] > 0 &&
          vertexHarbor.get(v) !== r &&
          !existingSpecialHarbors.has(r)
        ) {
          // Mild diminishing return for heavy duplication without matching 2:1 harbor.
          score -= 0.45;
        }
      }
      if (goldPips > 0 && existingPips.some((p) => p === 0)) {
        score += 1.15;
      }
      const totalCoveredTypes =
        combinedPips.filter((p) => p > 0).length + (existingGoldPips + goldPips > 0 ? 0.5 : 0);
      if (totalCoveredTypes >= 5) score += 1.65;
      else if (totalCoveredTypes >= 4) score += 0.95;
    }

    // Brick + Lumber synergy (early roads & settlements)
    const localBrickLumber = resPips[0] > 0 && resPips[1] > 0;
    const completedBrickLumber =
      isSecondPlacement &&
      combinedPips[0] > 0 &&
      combinedPips[1] > 0 &&
      (existingPips[0] === 0 || existingPips[1] === 0);
    if (localBrickLumber) {
      score += 1.55 + 0.18 * Math.min(resPips[0], resPips[1]);
    } else if (completedBrickLumber) {
      score += 1.4 + 0.15 * Math.min(combinedPips[0], combinedPips[1]);
    }

    // Grain + Ore synergy (cities & dev cards)
    const localGrainOre = resPips[3] > 0 && resPips[4] > 0;
    const completedGrainOre =
      isSecondPlacement &&
      combinedPips[3] > 0 &&
      combinedPips[4] > 0 &&
      (existingPips[3] === 0 || existingPips[4] === 0);
    if (localGrainOre) {
      score += 1.65 + 0.2 * Math.min(resPips[3], resPips[4]);
    } else if (completedGrainOre) {
      score += 1.5 + 0.16 * Math.min(combinedPips[3], combinedPips[4]);
    }

    // Secondary dev-card / knight synergy (Wool + Grain + Ore)
    if (
      combinedPips[2] > 0 &&
      combinedPips[3] > 0 &&
      combinedPips[4] > 0 &&
      (resPips[2] > 0 || resPips[3] > 0 || resPips[4] > 0)
    ) {
      score += 0.45;
    }

    // If player already owns a 2:1 harbor from 1st spot, feeding it high pips is strong.
    for (const hr of existingSpecialHarbors) {
      if (resPips[hr] > 0) score += resPips[hr] * 0.35;
    }

    // (C) Board Scarcity Bonus
    let bestScarceRes = -1;
    let bestScarceImpact = 0;
    for (let r = 0; r < 5; r++) {
      if (resPips[r] > 0 && resScarcity[r] >= 0.24) {
        const impact = resScarcity[r] * resPips[r];
        score += impact * 0.8;
        if (impact > bestScarceImpact && resPips[r] >= 2) {
          bestScarceImpact = impact;
          bestScarceRes = r;
        }
      }
    }

    // (D) Scenario-Specific Bonuses & Harbors
    if (isCitadelsCityPlacement && commodityPips > 0) {
      score += commodityPips * 0.5 + commodityNames.length * 1.15;
      if (forestPips >= 3) score += 0.45; // Science aqueduct path boost
    } else if (scenario === 'Citadels' && !isSecondPlacement) {
      // 1st placement in Citadels benefits from Ore/Grain/Wool for city + knight defense.
      if (resPips[3] > 0 && resPips[4] > 0) score += 0.45;
      if (resPips[2] > 0 && resPips[4] > 0) score += 0.35;
    }

    const seaInfo = evaluateSeaAccess(v);
    if (isSeafaringScenario && seaInfo.hasSeaEdge) {
      score += seaInfo.nearOuterOrFog ? 1.6 : 0.95;
      if (combinedPips[1] > 0 && combinedPips[2] > 0) {
        score += 0.65; // Lumber + Wool ready for immediate ships
      }
    } else if (scenario === 'Fishermen') {
      // Score coastal fishing shoals and the inland Trout Lake (Desert on 2 & 12).
      const shoals = board.fishingGrounds ?? [];
      let fishPips = 0;
      for (const fg of shoals) {
        const a = topo.edgeVertices[2 * fg.edge];
        const b = topo.edgeVertices[2 * fg.edge + 1];
        if (a === v || b === v) fishPips += pipsOf(fg.number);
      }
      if (adjTiles.some((t) => board.terrain[t] === TERRAIN_DESERT)) fishPips += 2;
      if (fishPips > 0) score += fishPips * 0.65 + 0.6;
    } else if (scenario === 'Caravans') {
      const river = new Set(board.riverEdges ?? []);
      const nearRiver = [...river].some((e) => topo.edgeVertices[2 * e] === v || topo.edgeVertices[2 * e + 1] === v);
      const nearOasis = adjTiles.some((t) => board.terrain[t] === TERRAIN_DESERT);
      if (nearRiver) score += 1.25;
      if (nearOasis) score += 1.1;
    } else if (scenario !== 'Classic' && scenario !== 'Citadels' && seaInfo.hasSeaEdge) {
      // Gentle coastal bonus for other maritime/coastal scenarios (e.g. Fishermen).
      score += 0.65;
    }

    const hCode = vertexHarbor.get(v);
    let harborTag: string | null = null;
    if (hCode === 5) {
      score += 2.15 + (rawPips >= 8 ? 0.4 : 0);
      harborTag = '3:1港';
    } else if (hCode !== undefined && hCode >= 0 && hCode <= 4) {
      const short = RESOURCE_SHORT[RESOURCES[hCode]];
      const matchPips = combinedPips[hCode];
      if (matchPips >= 4) {
        score += 2.4 + Math.min(matchPips, 8) * 0.35;
        harborTag = `2:1${short}港绝配`;
      } else if (matchPips > 0) {
        score += 1.15 + matchPips * 0.3;
        harborTag = `2:1${short}港`;
      } else {
        score += 0.45;
        harborTag = `2:1${short}港`;
      }
    }

    // ---- Build concise Chinese tags & summary ----
    const tags: string[] = [];
    if (goldPips > 0) {
      tags.push(`金矿 ${rawPips}点`);
    } else if (rawPips >= 11) {
      tags.push(`高产 ${rawPips}点`);
    } else {
      tags.push(`产出 ${rawPips}点`);
    }

    if (isCitadelsCityPlacement && commodityNames.length > 0) {
      tags.push(
        commodityNames.length >= 2
          ? `起城产${commodityNames.join('')}`
          : `起城产商品(${commodityNames[0]})`,
      );
    }

    if (isSecondPlacement && missingCovered.length > 0) {
      const shorts = missingCovered.map((r) => RESOURCE_SHORT[RESOURCES[r]]).join('');
      if (combinedPips.every((p) => p > 0)) {
        tags.push('五源俱全');
      } else {
        tags.push(`补齐${shorts}`);
      }
    }

    if (localBrickLumber || completedBrickLumber) {
      tags.push('砖木拓荒');
    } else if (localGrainOre || completedGrainOre) {
      tags.push('麦矿升城');
    } else if (effectiveDistinctOnV >= 3 && tags.length < 3) {
      tags.push('三源均衡');
    }

    if (bestScarceRes >= 0) {
      tags.push(`稀缺${RESOURCE_SHORT[RESOURCES[bestScarceRes]]}`);
    }

    if (harborTag) {
      tags.push(harborTag);
    }

    if (isSeafaringScenario && seaInfo.hasSeaEdge) {
      tags.push(seaInfo.nearOuterOrFog ? (scenario === 'FogIslands' ? '探雾航口' : '外岛航口') : '临海造船');
    }

    const sortedResChars = [...vertexResIndices]
      .sort((a, b) => resPips[b] - resPips[a])
      .map((r) => RESOURCE_SHORT[RESOURCES[r]])
      .join('');

    const summaryParts: string[] = [`${rawPips}点${goldPips > 0 ? '含金' : ''}`];
    if (sortedResChars.length >= 3) {
      summaryParts.push(`${sortedResChars}齐全`);
    } else if (sortedResChars.length > 0) {
      summaryParts.push(`主产${sortedResChars}`);
    } else if (goldPips > 0) {
      summaryParts.push('金矿百搭');
    }

    if (isCitadelsCityPlacement && commodityNames.length > 0) {
      summaryParts.push(`起城产${commodityNames.join('/')}`);
    } else if (isSecondPlacement && missingCovered.length > 0) {
      const shorts = missingCovered.map((r) => RESOURCE_SHORT[RESOURCES[r]]).join('');
      summaryParts.push(combinedPips.every((p) => p > 0) ? '五源俱全' : `补齐${shorts}`);
    } else if (harborTag) {
      summaryParts.push(harborTag.replace('绝配', ''));
    } else if (isSeafaringScenario && seaInfo.hasSeaEdge) {
      summaryParts.push('临海出海');
    } else if (bestScarceRes >= 0) {
      summaryParts.push(`控稀缺${RESOURCE_SHORT[RESOURCES[bestScarceRes]]}`);
    }

    candidates.push({
      vertex: v,
      score,
      pips: rawPips,
      weightedPips,
      summary: summaryParts.slice(0, 3).join(' · '),
      tags: tags.slice(0, 4),
    });
  }

  // Sort descending by score, breaking ties deterministically by pips then vertex id.
  candidates.sort((a, b) => b.score - a.score || b.pips - a.pips || a.vertex - b.vertex);

  const top = candidates.slice(0, 3);
  const bestScore = top[0]?.score ?? 0;

  return top.map((c, idx) => {
    const rank = (idx + 1) as 1 | 2 | 3;
    let tier: 'S' | 'A' | 'B';
    if (
      (idx === 0 && (c.pips >= 10 || c.weightedPips >= 10.5 || c.score >= 15.5)) ||
      (c.pips >= 11 && c.score >= bestScore - 0.6 && c.score >= 16.5)
    ) {
      tier = 'S';
    } else if (c.score >= 11.5 || c.pips >= 9 || (bestScore > 0 && c.score >= bestScore * 0.84)) {
      tier = 'A';
    } else {
      tier = 'B';
    }

    return {
      vertex: c.vertex,
      rank,
      score: Math.round(c.score * 10) / 10,
      pips: c.pips,
      tier,
      summary: c.summary,
      tags: c.tags,
    };
  });
}
