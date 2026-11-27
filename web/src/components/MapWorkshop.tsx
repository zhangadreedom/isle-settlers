import { batch, createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { loadEngine, topology, type Topology } from '../engine';
import { RESOURCES, RESOURCE_SHORT } from '../game/text';
import type { BoardLayout, HarborKind, Resource, Scenario, Terrain } from '../generated/proto';
import {
  autoBalanceNumbers,
  cloneDraft,
  cloneHarbor,
  createDefaultDraft,
  decodeCustomMap,
  defaultScenarioPlayersForLayout,
  encodeCustomMap,
  isFogLayout,
  isLandTerrain,
  isProducingTerrain,
  isSeafaringLayout,
  LAYOUT_SPECS,
  layoutFor,
  MAP_PRESETS,
  PIP_COUNT,
  suggestNumberForTile,
  tileNeighbors,
  VALID_NUMBERS,
  validateDraftMap,
  type DraftMap,
} from '../diy/custom-map';
import {
  computeHarborOutward,
  downloadDiySvg,
  generateDiySvg,
  type SvgExportOptions,
} from '../diy/svg-export';
import { Modal, ResIcon } from './bits';

export interface MapWorkshopModalProps {
  initialScenario: Scenario;
  initialPlayers: number;
  initialCode?: string | null;
  embedded?: boolean;
  onClose: () => void;
  onPlayLocal: (scenario: Scenario, players: number, code: string) => void;
  onUseInRoom?: (scenario: Scenario, players: number, code: string) => void;
}

type ToolTab = 'terrain' | 'number' | 'harbor';

const SCENARIO_ITEMS: { id: Scenario; label: string }[] = [
  { id: 'Classic', label: '经典岛屿' },
  { id: 'Archipelago', label: '群岛开拓' },
  { id: 'FogIslands', label: '迷雾群岛' },
  { id: 'Citadels', label: '城邦与铁卫' },
];

const TERRAIN_BRUSHES: { id: Terrain; label: string; color: string; hint: string }[] = [
  { id: 'Forest', label: '森林', color: '#2f7d4a', hint: '产木材' },
  { id: 'Hills', label: '丘陵', color: '#c0623a', hint: '产砖块' },
  { id: 'Pasture', label: '牧场', color: '#7eb856', hint: '产羊毛' },
  { id: 'Fields', label: '麦田', color: '#d8aa38', hint: '产小麦' },
  { id: 'Mountains', label: '山地', color: '#7d838f', hint: '产矿石' },
  { id: 'Desert', label: '荒漠', color: '#d7c18c', hint: '强盗起点' },
  { id: 'Gold', label: '金矿', color: '#c9931b', hint: '任选资源' },
  { id: 'Sea', label: '海洋', color: '#257992', hint: '航线/海盗' },
  { id: 'Fog', label: '迷雾', color: '#7c9eb4', hint: '探索未知格' },
];

const CANVAS_HEX_FILL: Record<Terrain, string> = {
  Forest: '#2f7d4a',
  Hills: '#c0623a',
  Pasture: '#78b856',
  Fields: '#dfb343',
  Mountains: '#7f8692',
  Desert: '#dfd0a3',
  Sea: '#1f6c86',
  Gold: '#9c752b',
  Fog: '#7e9fb6',
};

const HARBOR_BRUSHES: { label: string; short: string; kind: HarborKind; color: string }[] = [
  { label: '3:1 通用', short: '3:1', kind: { type: 'Generic' }, color: '#536878' },
  { label: '2:1 木', short: '2:1木', kind: { type: 'Special', value: 'Lumber' }, color: '#2f7d4a' },
  { label: '2:1 砖', short: '2:1砖', kind: { type: 'Special', value: 'Brick' }, color: '#c0623a' },
  { label: '2:1 羊', short: '2:1羊', kind: { type: 'Special', value: 'Wool' }, color: '#6da642' },
  { label: '2:1 麦', short: '2:1麦', kind: { type: 'Special', value: 'Grain' }, color: '#cc9a22' },
  { label: '2:1 矿', short: '2:1矿', kind: { type: 'Special', value: 'Ore' }, color: '#767c88' },
];

const SQRT3_2 = Math.sqrt(3) / 2;

function sameHarbor(a: HarborKind, b: HarborKind): boolean {
  if (a.type !== b.type) return false;
  return a.type === 'Generic' || (b.type === 'Special' && a.value === b.value);
}

function harborBadgeText(hk: HarborKind): { top: string; bottom: string; fill: string } {
  if (hk.type === 'Generic') {
    return { top: '3:1', bottom: '通用', fill: '#354856' };
  }
  const colorMap: Record<Resource, string> = {
    Lumber: '#286c3f',
    Brick: '#a8512c',
    Wool: '#578d2e',
    Grain: '#b38314',
    Ore: '#5c626d',
  };
  return {
    top: '2:1',
    bottom: RESOURCE_SHORT[hk.value],
    fill: colorMap[hk.value],
  };
}

function resolveScenarioPlayersForImportedLayout(
  layout: BoardLayout,
  curScenario: Scenario,
  curPlayers: number,
): { scenario: Scenario; players: number } {
  if (layoutFor(curScenario, curPlayers) === layout) {
    return { scenario: curScenario, players: curPlayers };
  }
  const wantExt = layout.includes('Extended') || layout === 'Extended';
  const adjustedPlayers = wantExt
    ? curPlayers > 4
      ? curPlayers
      : 6
    : curPlayers <= 4
      ? Math.max(2, curPlayers)
      : 4;
  if (layoutFor(curScenario, adjustedPlayers) === layout) {
    return { scenario: curScenario, players: adjustedPlayers };
  }
  const def = defaultScenarioPlayersForLayout(layout);
  return { scenario: def.scenario, players: adjustedPlayers };
}

export function MapWorkshopModal(props: MapWorkshopModalProps) {
  const clampPlayers = (n: number) => Math.min(6, Math.max(2, Math.round(n) || 4));
  const [engineReady, setEngineReady] = createSignal(false);
  const [scenario, setScenario] = createSignal<Scenario>(props.initialScenario);
  const [players, setPlayers] = createSignal<number>(clampPlayers(props.initialPlayers));
  const [presetId, setPresetId] = createSignal<string>('standard');
  const [draft, setDraft] = createSignal<DraftMap | null>(null);

  // Interactive brush state
  const [toolTab, setToolTab] = createSignal<ToolTab>('terrain');
  const [terrainBrush, setTerrainBrush] = createSignal<Terrain>('Forest');
  const [numberBrush, setNumberBrush] = createSignal<number>(6);
  const [swapMode, setSwapMode] = createSignal<boolean>(false);
  const [swapFirst, setSwapFirst] = createSignal<number | null>(null);
  const [harborBrush, setHarborBrush] = createSignal<HarborKind>({ type: 'Generic' });
  const [painting, setPainting] = createSignal<boolean>(false);

  // Shareable short code state
  const [codeInput, setCodeInput] = createSignal<string>('');
  const [codeFeedback, setCodeFeedback] = createSignal<{ ok: boolean; msg: string } | null>(null);

  // Physical DIY SVG Export state
  const [exportMode, setExportMode] = createSignal<'pnp' | 'laser'>('pnp');
  const [hexSideMm, setHexSideMm] = createSignal<number>(45.6);
  const [kerfMm, setKerfMm] = createSignal<number>(0.12);
  const [includeTokens, setIncludeTokens] = createSignal<boolean>(true);
  const [includeHarbors, setIncludeHarbors] = createSignal<boolean>(true);
  const [exportTitle, setExportTitle] = createSignal<string>('');
  const [showSvgPreview, setShowSvgPreview] = createSignal<boolean>(false);

  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;
  const flashCodeFeedback = (ok: boolean, msg: string) => {
    setCodeFeedback({ ok, msg });
    if (feedbackTimer) clearTimeout(feedbackTimer);
    feedbackTimer = setTimeout(() => setCodeFeedback(null), 3200);
  };

  onMount(() => {
    const endPaint = () => setPainting(false);
    window.addEventListener('pointerup', endPaint);
    window.addEventListener('pointercancel', endPaint);
    onCleanup(() => {
      window.removeEventListener('pointerup', endPaint);
      window.removeEventListener('pointercancel', endPaint);
      if (feedbackTimer) clearTimeout(feedbackTimer);
    });

    void loadEngine().then(() => {
      const initCode = props.initialCode?.trim();
      if (initCode) {
        const parsed = decodeCustomMap(initCode);
        if (!('error' in parsed)) {
          const resolved = resolveScenarioPlayersForImportedLayout(
            parsed.layout,
            props.initialScenario,
            clampPlayers(props.initialPlayers),
          );
          batch(() => {
            setScenario(resolved.scenario);
            setPlayers(resolved.players);
            setDraft(parsed);
            setEngineReady(true);
          });
          return;
        }
      }
      const initialLayout = layoutFor(props.initialScenario, clampPlayers(props.initialPlayers));
      batch(() => {
        setDraft(createDefaultDraft(initialLayout));
        setEngineReady(true);
      });
    });
  });

  const currentTopo = createMemo<Topology | null>(() => {
    if (!engineReady()) return null;
    const d = draft();
    return d ? topology(d.layout) : null;
  });

  const liveCode = createMemo<string>(() => {
    const d = draft();
    return d ? encodeCustomMap(d) : '';
  });

  createEffect(() => {
    const code = liveCode();
    if (code) setCodeInput(code);
  });

  const validation = createMemo(() => {
    const d = draft();
    const t = currentTopo();
    if (!d || !t) {
      return {
        ok: false,
        errors: ['正在加载地图拓扑…'],
        warnings: [] as string[],
        stats: { landTiles: 0, seaTiles: 0, fogTiles: 0, redAdjacent: 0, pipsByResource: {} },
      };
    }
    return validateDraftMap(d, t);
  });

  // Switch scenario / player count while preserving draft when layout stays identical
  const applyScenarioPlayers = (nextScenario: Scenario, nextPlayers: number) => {
    const clamped = clampPlayers(nextPlayers);
    const nextLayout = layoutFor(nextScenario, clamped);
    const cur = draft();
    batch(() => {
      setScenario(nextScenario);
      setPlayers(clamped);
      setSwapFirst(null);
      if (!isSeafaringLayout(nextLayout) && (terrainBrush() === 'Sea' || terrainBrush() === 'Fog')) {
        setTerrainBrush('Forest');
      } else if (!isFogLayout(nextLayout) && terrainBrush() === 'Fog') {
        setTerrainBrush('Sea');
      }
      if (!cur || cur.layout !== nextLayout) {
        const nextTopo = topology(nextLayout);
        const preset = MAP_PRESETS.find((p) => p.id === presetId()) ?? MAP_PRESETS[0];
        setDraft(preset.build(nextLayout, nextTopo));
      }
    });
  };

  const applyPreset = (id: string) => {
    const d = draft();
    const t = currentTopo();
    if (!d || !t) return;
    const preset = MAP_PRESETS.find((p) => p.id === id) ?? MAP_PRESETS[0];
    const seed = Math.floor(Math.random() * 0x7fffffff) || 1;
    batch(() => {
      setPresetId(preset.id);
      setSwapFirst(null);
      setDraft(preset.build(d.layout, t, seed));
    });
  };

  const rerollCurrent = () => {
    applyPreset(presetId());
  };

  const balanceRedNumbers = () => {
    const d = draft();
    const t = currentTopo();
    if (!d || !t) return;
    setSwapFirst(null);
    setDraft(autoBalanceNumbers(d, t));
  };

  // ---- Canvas Painting Handlers ----

  const paintTile = (tileIdx: number, isDrag = false) => {
    const d = draft();
    const t = currentTopo();
    if (!d || !t || tileIdx < 0 || tileIdx >= d.terrain.length) return;

    if (toolTab() === 'terrain') {
      const brush = terrainBrush();
      if (!isSeafaringLayout(d.layout) && brush === 'Sea') return;
      if (!isFogLayout(d.layout) && brush === 'Fog') return;
      if (d.terrain[tileIdx] === brush) return;

      const next = cloneDraft(d);
      const prevNum = next.numbers[tileIdx];
      next.terrain[tileIdx] = brush;
      if (!isProducingTerrain(brush)) {
        next.numbers[tileIdx] = 0;
      } else if (prevNum === 0 || !VALID_NUMBERS.includes(prevNum)) {
        next.numbers[tileIdx] = suggestNumberForTile(next, t, tileIdx, brush);
      } else if (brush === 'Gold' && (prevNum === 6 || prevNum === 8 || prevNum === 2 || prevNum === 12)) {
        next.numbers[tileIdx] = suggestNumberForTile(next, t, tileIdx, brush);
      }
      setDraft(next);
      return;
    }

    if (toolTab() === 'number') {
      if (!isProducingTerrain(d.terrain[tileIdx])) return;
      if (swapMode()) {
        if (isDrag) return;
        const first = swapFirst();
        if (first === null) {
          setSwapFirst(tileIdx);
        } else if (first === tileIdx) {
          setSwapFirst(null);
        } else {
          const next = cloneDraft(d);
          [next.numbers[first], next.numbers[tileIdx]] = [next.numbers[tileIdx], next.numbers[first]];
          setSwapFirst(null);
          setDraft(next);
        }
      } else {
        const want = numberBrush();
        if (d.numbers[tileIdx] === want) return;
        const next = cloneDraft(d);
        next.numbers[tileIdx] = want;
        setDraft(next);
      }
    }
  };

  const paintHarbor = (slot: number) => {
    const d = draft();
    if (!d || slot < 0 || slot >= d.harbors.length) return;
    if (toolTab() !== 'harbor') {
      setToolTab('harbor');
    }
    const next = cloneDraft(d);
    next.harbors[slot] = cloneHarbor(harborBrush());
    setDraft(next);
  };

  const shuffleHarbors = () => {
    const d = draft();
    if (!d) return;
    const next = cloneDraft(d);
    for (let i = next.harbors.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [next.harbors[i], next.harbors[j]] = [next.harbors[j], next.harbors[i]];
    }
    setDraft(next);
  };

  // ---- Interactive 2D SVG Board Geometry ----

  const canvasGeometry = createMemo(() => {
    const d = draft();
    const t = currentTopo();
    if (!d || !t) return null;

    const R = 36;
    const numTiles = t.tilePos.length / 2;
    const numVerts = t.vertexPos.length / 2;
    const tileXY: [number, number][] = Array.from({ length: numTiles }, (_, i) => [
      t.tilePos[2 * i] * SQRT3_2 * R,
      t.tilePos[2 * i + 1] * 0.5 * R,
    ]);
    const vertXY: [number, number][] = Array.from({ length: numVerts }, (_, i) => [
      t.vertexPos[2 * i] * SQRT3_2 * R,
      t.vertexPos[2 * i + 1] * 0.5 * R,
    ]);

    const harbors = computeHarborOutward(t, d.terrain, tileXY, vertXY);
    const neighbors = tileNeighbors(t);

    // Highlight red-adjacent conflict tiles & fog-land conflict tiles right on the canvas
    const redConflict = new Set<number>();
    for (let i = 0; i < numTiles; i++) {
      if (d.numbers[i] !== 6 && d.numbers[i] !== 8) continue;
      if (neighbors[i].some((nb) => d.numbers[nb] === 6 || d.numbers[nb] === 8)) {
        redConflict.add(i);
      }
    }

    const fogConflict = new Set<number>();
    if (isFogLayout(d.layout)) {
      for (let i = 0; i < numTiles; i++) {
        if (d.terrain[i] !== 'Fog') continue;
        if (neighbors[i].some((nb) => isLandTerrain(d.terrain[nb]))) {
          fogConflict.add(i);
        }
      }
    }

    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;
    for (const [vx, vy] of vertXY) {
      if (vx < minX) minX = vx;
      if (vx > maxX) maxX = vx;
      if (vy < minY) minY = vy;
      if (vy > maxY) maxY = vy;
    }
    const harborDist = R * 0.68;
    for (const h of harbors) {
      const hx = h.mx + h.ox * harborDist;
      const hy = h.my + h.oy * harborDist;
      if (hx < minX) minX = hx;
      if (hx > maxX) maxX = hx;
      if (hy < minY) minY = hy;
      if (hy > maxY) maxY = hy;
    }

    const pad = 26;
    const viewBox = `${(minX - pad).toFixed(1)} ${(minY - pad).toFixed(1)} ${(maxX - minX + pad * 2).toFixed(1)} ${(maxY - minY + pad * 2).toFixed(1)}`;

    const tiles = Array.from({ length: numTiles }, (_, i) => {
      const pts: string[] = [];
      for (let k = 0; k < 6; k++) {
        const vi = t.tileVertices[i * 6 + k];
        pts.push(`${vertXY[vi][0].toFixed(1)},${vertXY[vi][1].toFixed(1)}`);
      }
      return {
        index: i,
        cx: tileXY[i][0],
        cy: tileXY[i][1],
        points: pts.join(' '),
        terrain: d.terrain[i],
        number: d.numbers[i],
        hasRedConflict: redConflict.has(i),
        hasFogConflict: fogConflict.has(i),
      };
    });

    const harborItems = harbors.map((h, slot) => ({
      slot,
      ax: h.ax,
      ay: h.ay,
      bx: h.bx,
      by: h.by,
      cx: h.mx + h.ox * harborDist,
      cy: h.my + h.oy * harborDist,
      kind: d.harbors[slot] ?? { type: 'Generic' as const },
    }));

    return { R, viewBox, tiles, harborItems };
  });

  // ---- Short Code Copy / Import ----

  const copyShortCode = async () => {
    const code = liveCode();
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code);
      flashCodeFeedback(true, '已复制短码 ✓');
    } catch {
      flashCodeFeedback(false, '请手动全选文本框复制');
    }
  };

  const applyImportedCode = (raw: string) => {
    const parsed = decodeCustomMap(raw);
    if ('error' in parsed) {
      flashCodeFeedback(false, parsed.error);
      return;
    }
    const resolved = resolveScenarioPlayersForImportedLayout(parsed.layout, scenario(), players());
    batch(() => {
      setScenario(resolved.scenario);
      setPlayers(resolved.players);
      setSwapFirst(null);
      setDraft(parsed);
    });
    flashCodeFeedback(true, `已导入 ${LAYOUT_SPECS[parsed.layout].label} ✓`);
  };

  const pasteAndImportCode = async () => {
    try {
      const clip = (await navigator.clipboard.readText()).trim();
      if (clip.startsWith('IM1:')) {
        setCodeInput(clip);
        applyImportedCode(clip);
        return;
      }
    } catch {
      // Clipboard read denied or unavailable — fall back to input box content.
    }
    applyImportedCode(codeInput());
  };

  // ---- SVG Blueprint Generation & Preview ----

  const svgExportOpts = createMemo<SvgExportOptions>(() => ({
    hexSideMm: Math.min(120, Math.max(15, Number(hexSideMm()) || 45.6)),
    kerfMm: exportMode() === 'laser' ? Math.min(2, Math.max(0, Number(kerfMm()) || 0)) : 0,
    mode: exportMode(),
    includeTokens: includeTokens(),
    includeHarbors: includeHarbors(),
    title: exportTitle().trim() || undefined,
  }));

  const previewDataUrl = createMemo<string | null>(() => {
    if (!showSvgPreview()) return null;
    const d = draft();
    const t = currentTopo();
    if (!d || !t) return null;
    const svg = generateDiySvg(d, t, svgExportOpts());
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  });

  const handleDownloadSvg = () => {
    const d = draft();
    const t = currentTopo();
    if (!d || !t) return;
    const opts = svgExportOpts();
    const svg = generateDiySvg(d, t, opts);
    const suffix = opts.mode === 'laser' ? `laser-kerf${opts.kerfMm}mm` : 'pnp-color';
    downloadDiySvg(svg, `isle-settlers-${d.layout.toLowerCase()}-${opts.hexSideMm}mm-${suffix}.svg`);
  };

  const availableTerrains = createMemo(() => {
    const d = draft();
    if (!d) return TERRAIN_BRUSHES.slice(0, 7);
    return TERRAIN_BRUSHES.filter((b) => {
      if (b.id === 'Sea') return isSeafaringLayout(d.layout);
      if (b.id === 'Fog') return isFogLayout(d.layout);
      return true;
    });
  });

  // Resource pip summary in order: Lumber, Brick, Wool, Grain, Ore (matching ResIcon indices 1,0,2,3,4)
  const pipItems: { resIndex: number; key: Resource; label: string }[] = [
    { resIndex: 1, key: 'Lumber', label: '木' },
    { resIndex: 0, key: 'Brick', label: '砖' },
    { resIndex: 2, key: 'Wool', label: '羊' },
    { resIndex: 3, key: 'Grain', label: '麦' },
    { resIndex: 4, key: 'Ore', label: '矿' },
  ];

  const handleSvgPointerMove = (e: PointerEvent) => {
    if (!painting()) return;
    const target = document.elementFromPoint(e.clientX, e.clientY);
    const tileAttr = target?.closest('[data-tile-idx]')?.getAttribute('data-tile-idx');
    if (tileAttr !== null && tileAttr !== undefined) {
      paintTile(Number(tileAttr), true);
    }
  };

  return (
    <Modal title="DIY 地图工坊 & 实体图纸导出" wide onClose={props.onClose}>
      <style>{`
        .modal.wide:has(.diy-workshop) {
          width: min(960px, calc(100vw - 24px));
        }
        .diy-workshop {
          display: flex;
          flex-direction: column;
          gap: 12px;
        }
        .diy-topbar {
          display: flex;
          flex-wrap: wrap;
          gap: 8px 12px;
          align-items: center;
          padding: 10px 12px;
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid var(--panel-border);
          border-radius: 10px;
        }
        .diy-grid {
          display: grid;
          grid-template-columns: minmax(0, 1.35fr) minmax(290px, 1fr);
          gap: 12px;
          align-items: start;
        }
        @media (max-width: 780px) {
          .diy-grid {
            grid-template-columns: 1fr;
          }
        }
        .diy-canvas-card {
          position: relative;
          border-radius: 12px;
          border: 1px solid var(--panel-border);
          background: radial-gradient(circle at 50% 45%, #16516d 0%, #0b2c3d 75%, #071e2b 100%);
          padding: 8px;
          user-select: none;
          touch-action: none;
        }
        .diy-canvas-card svg {
          display: block;
          width: 100%;
          max-height: 450px;
          height: auto;
        }
        .diy-hex {
          cursor: pointer;
          transition: filter 0.12s;
        }
        .diy-hex:hover {
          filter: brightness(1.12);
        }
        .diy-harbor {
          cursor: pointer;
        }
        .diy-harbor:hover circle {
          stroke: var(--accent);
          stroke-width: 2.2px;
        }
        .diy-side {
          display: flex;
          flex-direction: column;
          gap: 10px;
        }
        .diy-box {
          padding: 10px 12px;
          border-radius: 10px;
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid var(--panel-border);
          display: flex;
          flex-direction: column;
          gap: 8px;
        }
        .diy-palette {
          display: grid;
          grid-template-columns: repeat(3, minmax(0, 1fr));
          gap: 6px;
        }
        .diy-num-grid {
          display: grid;
          grid-template-columns: repeat(5, minmax(0, 1fr));
          gap: 6px;
        }
        .diy-swatch {
          display: inline-block;
          width: 12px;
          height: 12px;
          border-radius: 3px;
          border: 1px solid rgba(255,255,255,0.35);
          flex: none;
        }
        .diy-pips-row {
          display: flex;
          flex-wrap: wrap;
          gap: 6px;
          align-items: center;
        }
        .diy-pip-chip {
          display: inline-flex;
          align-items: center;
          gap: 4px;
          padding: 3px 7px;
          border-radius: 7px;
          background: rgba(255, 255, 255, 0.06);
          font-size: 12px;
          font-variant-numeric: tabular-nums;
        }
        .diy-status-ok {
          color: #6ee787;
          font-weight: 600;
          font-size: 13px;
        }
        .diy-status-err {
          color: #ff7b7e;
          font-size: 12.5px;
          margin: 0;
          padding-left: 18px;
        }
        .diy-status-warn {
          color: #f7c64a;
          font-size: 12px;
          margin: 0;
          padding-left: 18px;
        }
        .diy-preview-frame {
          border-radius: 8px;
          border: 1px solid var(--panel-border);
          background: #faf7f0;
          padding: 6px;
          max-height: 320px;
          overflow: auto;
        }
        .diy-preview-frame img {
          display: block;
          width: 100%;
          height: auto;
        }
        .diy-select {
          font: inherit;
          color: var(--text);
          background: var(--button);
          border: 1px solid var(--panel-border);
          border-radius: 8px;
          padding: 6px 10px;
        }
      `}</style>

      <div class="diy-workshop">
        {/* Top Controls: Scenario, Player Scale, Presets, Quick Shuffle & 6/8 Auto-Balance */}
        <div class="diy-topbar">
          <div class="row" style={{ gap: '6px' }}>
            <span class="muted small">剧本</span>
            <div class="seg small">
              <For each={SCENARIO_ITEMS}>
                {(item) => (
                  <button
                    classList={{ on: scenario() === item.id }}
                    onClick={() => applyScenarioPlayers(item.id, players())}
                  >
                    {item.label}
                  </button>
                )}
              </For>
            </div>
          </div>

          <div class="row" style={{ gap: '6px' }}>
            <span class="muted small">人数规模</span>
            <div class="seg small">
              <For each={[2, 3, 4, 5, 6]}>
                {(n) => (
                  <button
                    classList={{ on: players() === n }}
                    title={n <= 4 ? '2–4人标准盘' : '5–6人扩展盘'}
                    onClick={() => applyScenarioPlayers(scenario(), n)}
                  >
                    {n}人
                  </button>
                )}
              </For>
            </div>
            <span class="badge">
              {players() <= 4 ? '2–4人标准盘' : '5–6人扩展盘'}
            </span>
          </div>

          <div class="row" style={{ 'margin-left': 'auto', gap: '6px' }}>
            <select
              class="diy-select small"
              value={presetId()}
              aria-label="预设地图模板"
              onChange={(e) => applyPreset(e.currentTarget.value)}
            >
              <For each={MAP_PRESETS}>
                {(p) => <option value={p.id}>预设：{p.name}</option>}
              </For>
            </select>
            <button class="small" onClick={rerollCurrent} title="按当前预设模板重新随机生成">
              随机重排
            </button>
            <button class="small" onClick={balanceRedNumbers} title="保持产出地块并自动分散相邻的 6 与 8">
              一键均衡 6/8
            </button>
          </div>
        </div>

        {/* Main Studio Grid: Interactive 2D Hex Canvas + Right Tool & Diagnostics Column */}
        <div class="diy-grid">
          <div class="diy-canvas-card" onPointerMove={handleSvgPointerMove}>
            <Show
              when={canvasGeometry()}
              fallback={<div class="muted" style={{ padding: '48px', 'text-align': 'center' }}>正在初始化六角网格拓扑…</div>}
            >
              {(geo) => (
                <>
                  <svg viewBox={geo().viewBox}>
                    {/* Coastal harbor piers & clickable badges */}
                    <For each={geo().harborItems}>
                      {(hb) => {
                        const info = harborBadgeText(hb.kind);
                        return (
                          <g
                            class="diy-harbor"
                            onClick={() => paintHarbor(hb.slot)}
                          >
                            <line
                              x1={hb.ax}
                              y1={hb.ay}
                              x2={hb.cx}
                              y2={hb.cy}
                              stroke="#b68655"
                              stroke-width="2.6"
                              stroke-linecap="round"
                            />
                            <line
                              x1={hb.bx}
                              y1={hb.by}
                              x2={hb.cx}
                              y2={hb.cy}
                              stroke="#b68655"
                              stroke-width="2.6"
                              stroke-linecap="round"
                            />
                            <circle
                              cx={hb.cx}
                              cy={hb.cy}
                              r={12.8}
                              fill={info.fill}
                              stroke={toolTab() === 'harbor' ? '#f0b429' : 'rgba(255,255,255,0.72)'}
                              stroke-width={toolTab() === 'harbor' ? '1.8' : '1.2'}
                            />
                            <text
                              x={hb.cx}
                              y={hb.cy - 1.5}
                              text-anchor="middle"
                              font-size="8.2"
                              font-weight="800"
                              fill="#ffffff"
                              pointer-events="none"
                            >
                              {info.top}
                            </text>
                            <text
                              x={hb.cx}
                              y={hb.cy + 7.2}
                              text-anchor="middle"
                              font-size="7.4"
                              font-weight="700"
                              fill="#f5efe0"
                              pointer-events="none"
                            >
                              {info.bottom}
                            </text>
                          </g>
                        );
                      }}
                    </For>

                    {/* Hex tiles */}
                    <For each={geo().tiles}>
                      {(tile) => {
                        const isSwapSelected = () => toolTab() === 'number' && swapMode() && swapFirst() === tile.index;
                        const isRed = tile.number === 6 || tile.number === 8;
                        const pips = PIP_COUNT[tile.number] ?? 0;
                        const strokeColor = () => {
                          if (isSwapSelected()) return '#f0b429';
                          if (tile.hasFogConflict) return '#ff4d4f';
                          if (tile.hasRedConflict) return '#ff8787';
                          return 'rgba(12, 32, 44, 0.82)';
                        };
                        return (
                          <g
                            class="diy-hex"
                            data-tile-idx={tile.index}
                            onPointerDown={(e) => {
                              e.preventDefault();
                              setPainting(true);
                              paintTile(tile.index, false);
                            }}
                            onPointerEnter={() => {
                              if (painting()) paintTile(tile.index, true);
                            }}
                          >
                            <polygon
                              points={tile.points}
                              fill={CANVAS_HEX_FILL[tile.terrain]}
                              stroke={strokeColor()}
                              stroke-width={isSwapSelected() || tile.hasFogConflict || tile.hasRedConflict ? '3' : '1.6'}
                              stroke-dasharray={tile.hasFogConflict ? '4 2' : undefined}
                              stroke-linejoin="round"
                            />
                            {/* Subtle terrain glyph/label */}
                            <text
                              x={tile.cx}
                              y={isProducingTerrain(tile.terrain) && tile.number > 0 ? tile.cy - 15 : tile.cy + 3}
                              text-anchor="middle"
                              font-size={isProducingTerrain(tile.terrain) ? '8.5' : '10.5'}
                              font-weight="700"
                              fill="rgba(255,255,255,0.9)"
                              style={{ 'text-shadow': '0 1px 2px rgba(0,0,0,0.55)' }}
                              pointer-events="none"
                            >
                              {TERRAIN_BRUSHES.find((b) => b.id === tile.terrain)?.label ?? tile.terrain}
                            </text>

                            {/* Number token badge on producing tiles */}
                            <Show when={isProducingTerrain(tile.terrain) && tile.number > 0}>
                              <circle
                                cx={tile.cx}
                                cy={tile.cy + 4}
                                r={12.5}
                                fill="#f8f1dd"
                                stroke={tile.hasRedConflict ? '#e5484d' : isRed ? '#c92a2a' : '#b7a37a'}
                                stroke-width={tile.hasRedConflict ? '2' : '1.1'}
                                pointer-events="none"
                              />
                              <text
                                x={tile.cx}
                                y={tile.cy + 7}
                                text-anchor="middle"
                                font-size="11.5"
                                font-weight="800"
                                fill={isRed ? '#c92a2a' : '#1f2429'}
                                pointer-events="none"
                              >
                                {tile.number}
                              </text>
                              <text
                                x={tile.cx}
                                y={tile.cy + 13.8}
                                text-anchor="middle"
                                font-size="7.2"
                                letter-spacing="0.5"
                                fill={isRed ? '#c92a2a' : '#495057'}
                                pointer-events="none"
                              >
                                {'•'.repeat(pips)}
                              </text>
                            </Show>
                          </g>
                        );
                      }}
                    </For>
                  </svg>
                  <div class="row muted small" style={{ 'justify-content': 'space-between', padding: '4px 6px 0' }}>
                    <span>
                      {draft() ? LAYOUT_SPECS[draft()!.layout].label : ''}
                    </span>
                    <span>
                      {toolTab() === 'terrain'
                        ? '点按或按住拖拽即可连续涂刷地形'
                        : toolTab() === 'number'
                          ? swapMode()
                            ? swapFirst() === null
                              ? '交换模式：点击第 1 格数字圆片'
                              : '交换模式：再点击第 2 格完成对调'
                            : '点击或拖拽资源格放置所选数字圆片'
                          : '点击海岸边上的港口圆章即可替换港口类型'}
                    </span>
                  </div>
                </>
              )}
            </Show>
          </div>

          {/* Right Panel: Tool Tabs + Live Validation + Pip Stats */}
          <div class="diy-side">
            <div class="diy-box">
              <div class="tabs">
                <button
                  class="small"
                  style={{ flex: 1 }}
                  classList={{ on: toolTab() === 'terrain' }}
                  onClick={() => {
                    setToolTab('terrain');
                    setSwapFirst(null);
                  }}
                >
                  涂刷地形
                </button>
                <button
                  class="small"
                  style={{ flex: 1 }}
                  classList={{ on: toolTab() === 'number' }}
                  onClick={() => setToolTab('number')}
                >
                  放置数字圆片
                </button>
                <button
                  class="small"
                  style={{ flex: 1 }}
                  classList={{ on: toolTab() === 'harbor' }}
                  onClick={() => {
                    setToolTab('harbor');
                    setSwapFirst(null);
                  }}
                >
                  配置港口
                </button>
              </div>

              {/* Tab 1: Terrain Brush */}
              <Show when={toolTab() === 'terrain'}>
                <div class="diy-palette">
                  <For each={availableTerrains()}>
                    {(item) => (
                      <button
                        class="choice small"
                        style={{ padding: '6px 8px' }}
                        classList={{ on: terrainBrush() === item.id }}
                        title={item.hint}
                        onClick={() => setTerrainBrush(item.id)}
                      >
                        <span class="diy-swatch" style={{ background: item.color }} />
                        <span>{item.label}</span>
                      </button>
                    )}
                  </For>
                </div>
              </Show>

              {/* Tab 2: Number Tokens + Swap Mode */}
              <Show when={toolTab() === 'number'}>
                <div class="row" style={{ 'justify-content': 'space-between' }}>
                  <span class="muted small">选择点数或对调两格</span>
                  <button
                    class="small"
                    classList={{ on: swapMode() }}
                    onClick={() => {
                      setSwapMode(!swapMode());
                      setSwapFirst(null);
                    }}
                  >
                    ⇄ 交换两格数字
                  </button>
                </div>
                <div class="diy-num-grid">
                  <For each={VALID_NUMBERS}>
                    {(num) => {
                      const red = num === 6 || num === 8;
                      return (
                        <button
                          class="small"
                          style={{
                            padding: '5px 4px',
                            color: red ? '#ff7b7e' : undefined,
                            'font-weight': red ? '700' : '600',
                          }}
                          classList={{ on: !swapMode() && numberBrush() === num }}
                          onClick={() => {
                            setSwapMode(false);
                            setSwapFirst(null);
                            setNumberBrush(num);
                          }}
                        >
                          <div>{num}</div>
                          <div style={{ 'font-size': '9px', 'line-height': '1', opacity: 0.8 }}>
                            {'•'.repeat(PIP_COUNT[num] ?? 0)}
                          </div>
                        </button>
                      );
                    }}
                  </For>
                </div>
              </Show>

              {/* Tab 3: Harbor Configuration */}
              <Show when={toolTab() === 'harbor'}>
                <div class="diy-palette">
                  <For each={HARBOR_BRUSHES}>
                    {(hb) => (
                      <button
                        class="choice small"
                        style={{ padding: '6px 8px' }}
                        classList={{ on: sameHarbor(harborBrush(), hb.kind) }}
                        onClick={() => setHarborBrush(cloneHarbor(hb.kind))}
                      >
                        <span class="diy-swatch" style={{ background: hb.color }} />
                        <span>{hb.label}</span>
                      </button>
                    )}
                  </For>
                </div>
                <div class="row" style={{ 'justify-content': 'space-between' }}>
                  <span class="muted small">选中港口后点击海边圆章应用</span>
                  <button class="small" onClick={shuffleHarbors}>
                    随机打乱港口
                  </button>
                </div>
              </Show>
            </div>

            {/* Live Validation & Resource Pip Balance */}
            <div class="diy-box">
              <Show
                when={validation().ok}
                fallback={
                  <ul class="diy-status-err">
                    <For each={validation().errors}>{(err) => <li>{err}</li>}</For>
                  </ul>
                }
              >
                <div class="diy-status-ok">✓ 地图合法，可直接开局</div>
              </Show>

              <Show when={validation().warnings.length > 0}>
                <ul class="diy-status-warn">
                  <For each={validation().warnings}>{(w) => <li>{w}</li>}</For>
                </ul>
              </Show>

              <div class="diy-pips-row">
                <For each={pipItems}>
                  {(item) => (
                    <span class="diy-pip-chip" title={`${RESOURCES[item.resIndex]} 总概率点数`}>
                      <ResIcon r={item.resIndex} size="sm" />
                      <b>{validation().stats.pipsByResource[item.key] ?? 0}</b>
                      <span class="muted">点</span>
                    </span>
                  )}
                </For>
                <Show when={(validation().stats.pipsByResource.Gold ?? 0) > 0}>
                  <span class="diy-pip-chip" title="金矿总概率点数">
                    <span class="diy-swatch" style={{ background: '#e2a922', 'border-radius': '50%' }} />
                    <b>{validation().stats.pipsByResource.Gold}</b>
                    <span class="muted">金点</span>
                  </span>
                </Show>
              </div>

              <div class="muted small">
                陆地 {validation().stats.landTiles} 格
                <Show when={validation().stats.seaTiles > 0}> · 海洋 {validation().stats.seaTiles} 格</Show>
                <Show when={validation().stats.fogTiles > 0}> · 迷雾 {validation().stats.fogTiles} 格</Show>
                {' · '}相邻红字 {validation().stats.redAdjacent} 处
              </div>
            </div>

            {/* Shareable IM1:... Short Code */}
            <div class="diy-box">
              <div class="row" style={{ 'justify-content': 'space-between' }}>
                <span class="small" style={{ 'font-weight': 600 }}>地图短码分享 (IM1)</span>
                <Show when={codeFeedback()}>
                  {(fb) => (
                    <span
                      class="small"
                      style={{ color: fb().ok ? '#6ee787' : '#ff7b7e' }}
                    >
                      {fb().msg}
                    </span>
                  )}
                </Show>
              </div>
              <div class="row" style={{ gap: '6px' }}>
                <input
                  class="grow small"
                  style={{ 'font-family': 'ui-monospace, monospace', 'font-size': '12px' }}
                  value={codeInput()}
                  onInput={(e) => setCodeInput(e.currentTarget.value)}
                  placeholder="粘贴 IM1:... 地图短码"
                  aria-label="IM1 地图短码"
                />
                <button class="small" onClick={() => void copyShortCode()}>
                  复制短码
                </button>
                <button class="small" onClick={() => void pasteAndImportCode()}>
                  粘贴导入
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* Physical 1:1 DIY Laser-Cut & PnP Export Panel */}
        <details class="diy-box" open={showSvgPreview()}>
          <summary style={{ cursor: 'pointer', 'font-weight': 600 }}>
            1:1 实体 DIY 图纸生成（彩色打印 PnP / 3mm 椴木激光切割 SVG）
          </summary>
          <div style={{ display: 'flex', 'flex-direction': 'column', gap: '10px', 'margin-top': '8px' }}>
            <div class="row" style={{ gap: '10px 14px' }}>
              <div class="seg small">
                <button
                  classList={{ on: exportMode() === 'pnp' }}
                  onClick={() => setExportMode('pnp')}
                >
                  彩色打印 PnP
                </button>
                <button
                  classList={{ on: exportMode() === 'laser' }}
                  onClick={() => {
                    setExportMode('laser');
                    if (kerfMm() <= 0) setKerfMm(0.12);
                  }}
                >
                  3mm 椴木激光切割图纸
                </button>
              </div>

              <label class="row small" style={{ gap: '5px' }}>
                <span class="muted">六角边长 (mm)</span>
                <input
                  type="number"
                  step="0.1"
                  min="15"
                  max="120"
                  style={{ width: '74px', padding: '4px 7px' }}
                  value={hexSideMm()}
                  onInput={(e) => setHexSideMm(Number(e.currentTarget.value) || 45.6)}
                />
                <span class="muted">（对边 {(hexSideMm() * Math.sqrt(3)).toFixed(1)}mm）</span>
              </label>

              <Show when={exportMode() === 'laser'}>
                <label class="row small" style={{ gap: '5px' }}>
                  <span class="muted">激光刀缝 Kerf (mm)</span>
                  <input
                    type="number"
                    step="0.01"
                    min="0"
                    max="2"
                    style={{ width: '68px', padding: '4px 7px' }}
                    value={kerfMm()}
                    onInput={(e) => setKerfMm(Math.max(0, Number(e.currentTarget.value) || 0))}
                  />
                </label>
              </Show>
            </div>

            <div class="row" style={{ 'justify-content': 'space-between', gap: '8px' }}>
              <div class="row small" style={{ gap: '12px' }}>
                <label class="row" style={{ gap: '5px', cursor: 'pointer' }}>
                  <input
                    type="checkbox"
                    checked={includeTokens()}
                    onChange={(e) => setIncludeTokens(e.currentTarget.checked)}
                  />
                  <span>含 Ø25.4mm 数字圆片</span>
                </label>
                <label class="row" style={{ gap: '5px', cursor: 'pointer' }}>
                  <input
                    type="checkbox"
                    checked={includeHarbors()}
                    onChange={(e) => setIncludeHarbors(e.currentTarget.checked)}
                  />
                  <span>含海岸港口配件</span>
                </label>
                <input
                  class="small"
                  style={{ width: '190px', padding: '4px 8px' }}
                  value={exportTitle()}
                  onInput={(e) => setExportTitle(e.currentTarget.value)}
                  placeholder="可选图纸标题…"
                  aria-label="图纸标题"
                />
              </div>

              <div class="row" style={{ gap: '8px' }}>
                <button
                  class="small"
                  classList={{ on: showSvgPreview() }}
                  onClick={() => setShowSvgPreview(!showSvgPreview())}
                >
                  {showSvgPreview() ? '收起预览' : '预览图纸'}
                </button>
                <Show when={!props.embedded}>
                  <button class="primary small" disabled={!draft()} onClick={handleDownloadSvg}>
                    下载 1:1 SVG 图纸
                  </button>
                </Show>
              </div>
            </div>

            <Show when={showSvgPreview() && previewDataUrl()}>
              {(url) => (
                <div class="diy-preview-frame">
                  <img src={url()} alt="1:1 DIY SVG 实体图纸预览" />
                </div>
              )}
            </Show>
          </div>
        </details>

        {/* Modal Footer Actions */}
        <footer>
          <span class="muted small">
            {validation().ok
              ? `当前出战设置：${SCENARIO_ITEMS.find((s) => s.id === scenario())?.label} · ${players()} 人`
              : '请先修正上方红字规则提示后再开局'}
          </span>
          <button class="ghost" onClick={props.onClose}>
            取消
          </button>
          <Show when={!props.embedded && props.onUseInRoom}>
            <button
              disabled={!validation().ok}
              onClick={() => props.onUseInRoom?.(scenario(), players(), liveCode())}
            >
              用此地图创建联机房
            </button>
          </Show>
          <button
            class="primary"
            disabled={!validation().ok}
            onClick={() => props.onPlayLocal(scenario(), players(), liveCode())}
          >
            用此地图单机开局
          </button>
        </footer>
      </div>
    </Modal>
  );
}
