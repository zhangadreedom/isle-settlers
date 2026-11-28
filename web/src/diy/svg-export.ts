import type { Topology } from '../engine';
import { RESOURCE_SHORT, TERRAIN_NAME } from '../game/text';
import type { HarborKind, Terrain } from '../generated/proto';
import {
  encodeCustomMap,
  isLandTerrain,
  isProducingTerrain,
  LAYOUT_SPECS,
  PIP_COUNT,
  type DraftMap,
} from './custom-map';

export interface SvgExportOptions {
  hexSideMm: number;
  kerfMm: number;
  mode: 'pnp' | 'laser';
  includeTokens: boolean;
  includeHarbors: boolean;
  title?: string;
}

export const DEFAULT_SVG_EXPORT_OPTIONS: SvgExportOptions = {
  hexSideMm: 45.6,
  kerfMm: 0.12,
  mode: 'pnp',
  includeTokens: true,
  includeHarbors: true,
};

const SQRT3_2 = Math.sqrt(3) / 2;
const TOKEN_DIAMETER_MM = 25.4;
const TOKEN_RADIUS_MM = TOKEN_DIAMETER_MM / 2;

const PNP_HEX_FILL: Record<Terrain, { fill: string; inner: string; accent: string }> = {
  Forest: { fill: '#cfe5cf', inner: '#a6cfa8', accent: '#27673b' },
  Hills: { fill: '#f0cfc2', inner: '#e0a992', accent: '#a84723' },
  Pasture: { fill: '#dfefc6', inner: '#c3e099', accent: '#4e7c27' },
  Fields: { fill: '#faecc0', inner: '#f0d788', accent: '#b87d14' },
  Mountains: { fill: '#dce1e8', inner: '#bcc5d1', accent: '#4d5565' },
  Desert: { fill: '#f3e5c3', inner: '#e3cda0', accent: '#9b7b43' },
  Sea: { fill: '#cbe8f3', inner: '#9dd2e6', accent: '#1d6f8a' },
  Gold: { fill: '#f5e2b8', inner: '#e8c678', accent: '#9e6c11' },
  Fog: { fill: '#dde9f0', inner: '#c0d5e2', accent: '#537287' },
};

const HARBOR_COLOR: Record<string, string> = {
  Generic: '#4f5d6b',
  Lumber: '#2f7d4a',
  Brick: '#c0623a',
  Wool: '#6aa33b',
  Grain: '#c9961a',
  Ore: '#6a707a',
};

function fmt(n: number): string {
  return Number.isFinite(n) ? n.toFixed(3).replace(/\.?0+$/, '') : '0';
}

export function escapeXml(raw: string): string {
  return raw
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}

function latticeUnit(lx: number, ly: number): [number, number] {
  return [lx * SQRT3_2, ly * 0.5];
}

/** Computes the seaward outward unit vector `[ox, oy]` for each harbor slot. */
export function computeHarborOutward(
  topo: Topology,
  terrain: readonly Terrain[],
  tileXY: readonly [number, number][],
  vertexXY: readonly [number, number][],
): { mx: number; my: number; ax: number; ay: number; bx: number; by: number; ox: number; oy: number }[] {
  const numTiles = tileXY.length;
  const numEdges = topo.edgeVertices.length / 2;
  const edgeTiles: number[][] = Array.from({ length: numEdges }, () => []);

  for (let t = 0; t < numTiles; t++) {
    const corners = topo.tileVertices.subarray(t * 6, t * 6 + 6);
    for (let e = 0; e < numEdges; e++) {
      const va = topo.edgeVertices[2 * e];
      const vb = topo.edgeVertices[2 * e + 1];
      if (corners.includes(va) && corners.includes(vb)) {
        edgeTiles[e].push(t);
      }
    }
  }

  const boardCx = tileXY.reduce((s, p) => s + p[0], 0) / Math.max(1, numTiles);
  const boardCy = tileXY.reduce((s, p) => s + p[1], 0) / Math.max(1, numTiles);

  return Array.from(topo.harborEdges, (e) => {
    const va = topo.edgeVertices[2 * e];
    const vb = topo.edgeVertices[2 * e + 1];
    const [ax, ay] = vertexXY[va];
    const [bx, by] = vertexXY[vb];
    const mx = (ax + bx) / 2;
    const my = (ay + by) / 2;

    const touching = edgeTiles[e] ?? [];
    let refTile: number | undefined;
    if (touching.length === 1) {
      refTile = touching[0];
    } else if (touching.length >= 2) {
      const landTile = touching.find((t) => isLandTerrain(terrain[t]));
      const waterTile = touching.find((t) => !isLandTerrain(terrain[t]));
      if (landTile !== undefined && waterTile !== undefined) {
        refTile = landTile;
      } else {
        // Fallback: point away from whichever touching tile sits closer to board centroid.
        refTile = [...touching].sort(
          (t1, t2) =>
            Math.hypot(tileXY[t1][0] - boardCx, tileXY[t1][1] - boardCy) -
            Math.hypot(tileXY[t2][0] - boardCx, tileXY[t2][1] - boardCy),
        )[0];
      }
    }

    let ox = mx - boardCx;
    let oy = my - boardCy;
    if (refTile !== undefined) {
      ox = mx - tileXY[refTile][0];
      oy = my - tileXY[refTile][1];
    }
    const len = Math.hypot(ox, oy) || 1;
    return { mx, my, ax, ay, bx, by, ox: ox / len, oy: oy / len };
  });
}

/**
 * Vector terrain illustration centered at `(cx, cy)` with characteristic radius `s` (mm).
 * In `'laser'` mode, produces pure `#111111` stroke paths with `fill="none"`.
 */
export function renderTerrainArtSvg(
  terrain: Terrain,
  cx: number,
  cy: number,
  s: number,
  laser: boolean,
): string {
  const stroke = laser ? '#111111' : PNP_HEX_FILL[terrain].accent;
  const sw = fmt(Math.max(0.28, s * 0.08));
  const tr = `translate(${fmt(cx)},${fmt(cy)})`;

  switch (terrain) {
    case 'Forest': {
      // Three pine trees
      const tree = (tx: number, ty: number, sc: number) => {
        const w = s * 0.44 * sc;
        const h = s * 0.72 * sc;
        const fillAttr = laser ? 'fill="none"' : 'fill="#2d7a47"';
        const trunkFill = laser ? 'fill="none"' : 'fill="#6e4726"';
        return (
          `<path d="M ${fmt(tx)} ${fmt(ty - h)} L ${fmt(tx - w * 0.65)} ${fmt(ty - h * 0.35)} L ${fmt(tx - w * 0.32)} ${fmt(ty - h * 0.35)} L ${fmt(tx - w)} ${fmt(ty + h * 0.25)} L ${fmt(tx + w)} ${fmt(ty + h * 0.25)} L ${fmt(tx + w * 0.32)} ${fmt(ty - h * 0.35)} L ${fmt(tx + w * 0.65)} ${fmt(ty - h * 0.35)} Z" ${fillAttr} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
          `<rect x="${fmt(tx - w * 0.2)}" y="${fmt(ty + h * 0.25)}" width="${fmt(w * 0.4)}" height="${fmt(h * 0.28)}" ${trunkFill} stroke="${stroke}" stroke-width="${sw}"/>`
        );
      };
      return `<g transform="${tr}">${tree(-s * 0.52, s * 0.1, 0.78)}${tree(s * 0.52, s * 0.1, 0.78)}${tree(0, -s * 0.04, 1)}</g>`;
    }

    case 'Hills': {
      // Rolling hills + stacked clay bricks
      const hillFill = laser ? 'fill="none"' : 'fill="#d87a56"';
      const brickFill = laser ? 'fill="none"' : 'fill="#b84f29"';
      const bw = s * 0.44;
      const bh = s * 0.2;
      return (
        `<g transform="${tr}">` +
        `<path d="M ${fmt(-s * 0.9)} ${fmt(s * 0.22)} Q ${fmt(-s * 0.42)} ${fmt(-s * 0.55)} ${fmt(s * 0.08)} ${fmt(s * 0.15)} Q ${fmt(s * 0.5)} ${fmt(-s * 0.42)} ${fmt(s * 0.9)} ${fmt(s * 0.22)}" ${hillFill} stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>` +
        `<rect x="${fmt(-bw - s * 0.04)}" y="${fmt(s * 0.28)}" width="${fmt(bw)}" height="${fmt(bh)}" rx="${fmt(bh * 0.15)}" ${brickFill} stroke="${stroke}" stroke-width="${sw}"/>` +
        `<rect x="${fmt(s * 0.04)}" y="${fmt(s * 0.28)}" width="${fmt(bw)}" height="${fmt(bh)}" rx="${fmt(bh * 0.15)}" ${brickFill} stroke="${stroke}" stroke-width="${sw}"/>` +
        `<rect x="${fmt(-bw * 0.5)}" y="${fmt(s * 0.04)}" width="${fmt(bw)}" height="${fmt(bh)}" rx="${fmt(bh * 0.15)}" ${brickFill} stroke="${stroke}" stroke-width="${sw}"/>` +
        `</g>`
      );
    }

    case 'Pasture': {
      // Fluffy sheep puff + head & legs
      const woolFill = laser ? 'fill="none"' : 'fill="#ffffff"';
      const headFill = laser ? 'fill="none"' : 'fill="#3c463d"';
      return (
        `<g transform="${tr}">` +
        `<line x1="${fmt(-s * 0.36)}" y1="${fmt(s * 0.28)}" x2="${fmt(-s * 0.36)}" y2="${fmt(s * 0.58)}" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>` +
        `<line x1="${fmt(s * 0.22)}" y1="${fmt(s * 0.28)}" x2="${fmt(s * 0.22)}" y2="${fmt(s * 0.58)}" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>` +
        `<path d="M ${fmt(-s * 0.58)} ${fmt(s * 0.12)} C ${fmt(-s * 0.78)} ${fmt(-s * 0.15)}, ${fmt(-s * 0.52)} ${fmt(-s * 0.48)}, ${fmt(-s * 0.22)} ${fmt(-s * 0.38)} C ${fmt(-s * 0.08)} ${fmt(-s * 0.62)}, ${fmt(s * 0.28)} ${fmt(-s * 0.58)}, ${fmt(s * 0.38)} ${fmt(-s * 0.32)} C ${fmt(s * 0.68)} ${fmt(-s * 0.28)}, ${fmt(s * 0.68)} ${fmt(s * 0.16)}, ${fmt(s * 0.42)} ${fmt(s * 0.25)} C ${fmt(s * 0.18)} ${fmt(s * 0.38)}, ${fmt(-s * 0.32)} ${fmt(s * 0.38)}, ${fmt(-s * 0.58)} ${fmt(s * 0.12)} Z" ${woolFill} stroke="${stroke}" stroke-width="${sw}"/>` +
        `<ellipse cx="${fmt(s * 0.56)}" cy="${fmt(-s * 0.06)}" rx="${fmt(s * 0.22)}" ry="${fmt(s * 0.18)}" ${headFill} stroke="${stroke}" stroke-width="${sw}"/>` +
        `</g>`
      );
    }

    case 'Fields': {
      // Twin wheat stalks with kernels
      const grainFill = laser ? 'fill="none"' : 'fill="#f2c138"';
      const stalk = (ox: number, tilt: number) => {
        let kernels = '';
        for (let k = 0; k < 3; k++) {
          const ky = -s * 0.46 + k * s * 0.26;
          kernels +=
            `<ellipse cx="${fmt(ox - s * 0.15)}" cy="${fmt(ky)}" rx="${fmt(s * 0.12)}" ry="${fmt(s * 0.07)}" transform="rotate(-32 ${fmt(ox - s * 0.15)} ${fmt(ky)})" ${grainFill} stroke="${stroke}" stroke-width="${sw}"/>` +
            `<ellipse cx="${fmt(ox + s * 0.15)}" cy="${fmt(ky)}" rx="${fmt(s * 0.12)}" ry="${fmt(s * 0.07)}" transform="rotate(32 ${fmt(ox + s * 0.15)} ${fmt(ky)})" ${grainFill} stroke="${stroke}" stroke-width="${sw}"/>`;
        }
        return (
          `<g transform="rotate(${tilt} ${fmt(ox)} ${fmt(s * 0.45)})">` +
          `<line x1="${fmt(ox)}" y1="${fmt(-s * 0.64)}" x2="${fmt(ox)}" y2="${fmt(s * 0.58)}" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>` +
          kernels +
          `</g>`
        );
      };
      return `<g transform="${tr}">${stalk(-s * 0.3, -10)}${stalk(s * 0.3, 10)}</g>`;
    }

    case 'Mountains': {
      // Twin alpine peaks with snow caps
      const rockFill = laser ? 'fill="none"' : 'fill="#7b8394"';
      const snowFill = laser ? 'fill="none"' : 'fill="#f7fafc"';
      return (
        `<g transform="${tr}">` +
        `<polygon points="${fmt(-s * 0.92)},${fmt(s * 0.48)} ${fmt(-s * 0.28)},${fmt(-s * 0.58)} ${fmt(s * 0.32)},${fmt(s * 0.48)}" ${rockFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `<polygon points="${fmt(-s * 0.15)},${fmt(s * 0.48)} ${fmt(s * 0.42)},${fmt(-s * 0.42)} ${fmt(s * 0.94)},${fmt(s * 0.48)}" ${rockFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `<polygon points="${fmt(-s * 0.28)},${fmt(-s * 0.58)} ${fmt(-s * 0.5)},${fmt(-s * 0.2)} ${fmt(-s * 0.28)},${fmt(-s * 0.12)} ${fmt(-s * 0.06)},${fmt(-s * 0.2)}" ${snowFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `<polygon points="${fmt(s * 0.42)},${fmt(-s * 0.42)} ${fmt(s * 0.24)},${fmt(-s * 0.1)} ${fmt(s * 0.42)},${fmt(-s * 0.04)} ${fmt(s * 0.6)},${fmt(-s * 0.1)}" ${snowFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `</g>`
      );
    }

    case 'Desert': {
      // Sand dune wave + saguaro cactus
      const cactusFill = laser ? 'fill="none"' : 'fill="#588b48"';
      return (
        `<g transform="${tr}">` +
        `<path d="M ${fmt(-s * 0.92)} ${fmt(s * 0.45)} Q ${fmt(-s * 0.3)} ${fmt(s * 0.18)} ${fmt(s * 0.25)} ${fmt(s * 0.4)} T ${fmt(s * 0.92)} ${fmt(s * 0.38)}" fill="none" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>` +
        `<path d="M ${fmt(-s * 0.1)} ${fmt(s * 0.38)} L ${fmt(-s * 0.1)} ${fmt(-s * 0.55)} A ${fmt(s * 0.1)} ${fmt(s * 0.1)} 0 0 1 ${fmt(s * 0.1)} ${fmt(-s * 0.55)} L ${fmt(s * 0.1)} ${fmt(s * 0.38)} Z" ${cactusFill} stroke="${stroke}" stroke-width="${sw}"/>` +
        `<path d="M ${fmt(-s * 0.1)} ${fmt(-s * 0.05)} L ${fmt(-s * 0.38)} ${fmt(-s * 0.05)} L ${fmt(-s * 0.38)} ${fmt(-s * 0.36)} M ${fmt(s * 0.1)} ${fmt(-s * 0.16)} L ${fmt(s * 0.38)} ${fmt(-s * 0.16)} L ${fmt(s * 0.38)} ${fmt(-s * 0.44)}" fill="none" stroke="${stroke}" stroke-width="${fmt(Math.max(0.35, s * 0.11))}" stroke-linecap="round" stroke-linejoin="round"/>` +
        `</g>`
      );
    }

    case 'Sea': {
      // Three ocean wave crests
      const wave = (wy: number, w: number) =>
        `<path d="M ${fmt(-s * w)} ${fmt(wy)} Q ${fmt(-s * w * 0.5)} ${fmt(wy - s * 0.2)} 0 ${fmt(wy)} T ${fmt(s * w)} ${fmt(wy)}" fill="none" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>`;
      return `<g transform="${tr}">${wave(-s * 0.32, 0.72)}${wave(0, 0.88)}${wave(s * 0.32, 0.68)}</g>`;
    }

    case 'Gold': {
      // Faceted gold nuggets + river shimmer + 4-point star
      const nugFill = laser ? 'fill="none"' : 'fill="#f4be2a"';
      const starFill = laser ? 'fill="none"' : 'fill="#fff3b0"';
      return (
        `<g transform="${tr}">` +
        `<path d="M ${fmt(-s * 0.85)} ${fmt(s * 0.36)} Q ${fmt(-s * 0.2)} ${fmt(s * 0.12)} ${fmt(s * 0.85)} ${fmt(s * 0.36)}" fill="none" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round"/>` +
        `<polygon points="${fmt(-s * 0.58)},${fmt(s * 0.18)} ${fmt(-s * 0.42)},${fmt(-s * 0.22)} ${fmt(-s * 0.05)},${fmt(-s * 0.28)} ${fmt(s * 0.12)},${fmt(s * 0.16)}" ${nugFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `<polygon points="${fmt(s * 0.15)},${fmt(s * 0.2)} ${fmt(s * 0.28)},${fmt(-s * 0.12)} ${fmt(s * 0.6)},${fmt(-s * 0.08)} ${fmt(s * 0.68)},${fmt(s * 0.22)}" ${nugFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `<polygon points="${fmt(s * 0.28)},${fmt(-s * 0.62)} ${fmt(s * 0.36)},${fmt(-s * 0.42)} ${fmt(s * 0.56)},${fmt(-s * 0.34)} ${fmt(s * 0.36)},${fmt(-s * 0.26)} ${fmt(s * 0.28)},${fmt(-s * 0.06)} ${fmt(s * 0.2)},${fmt(-s * 0.26)} ${fmt(0)},${fmt(-s * 0.34)} ${fmt(s * 0.2)},${fmt(-s * 0.42)}" ${starFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `</g>`
      );
    }

    case 'Fog': {
      // Mist cloud + mystery question mark
      const cloudFill = laser ? 'fill="none"' : 'fill="#f1f7fb"';
      return (
        `<g transform="${tr}">` +
        `<path d="M ${fmt(-s * 0.68)} ${fmt(s * 0.28)} C ${fmt(-s * 0.88)} ${fmt(s * 0.28)}, ${fmt(-s * 0.9)} ${fmt(-s * 0.08)}, ${fmt(-s * 0.56)} ${fmt(-s * 0.12)} C ${fmt(-s * 0.44)} ${fmt(-s * 0.5)}, ${fmt(s * 0.14)} ${fmt(-s * 0.52)}, ${fmt(s * 0.28)} ${fmt(-s * 0.18)} C ${fmt(s * 0.66)} ${fmt(-s * 0.22)}, ${fmt(s * 0.86)} ${fmt(s * 0.28)}, ${fmt(s * 0.54)} ${fmt(s * 0.28)} Z" ${cloudFill} stroke="${stroke}" stroke-width="${sw}" stroke-linejoin="round"/>` +
        `<text x="0" y="${fmt(s * 0.18)}" text-anchor="middle" font-family="system-ui, sans-serif" font-size="${fmt(s * 0.72)}" font-weight="800" fill="${laser ? 'none' : stroke}" ${laser ? `stroke="${stroke}" stroke-width="${sw}"` : ''}>?</text>` +
        `</g>`
      );
    }
  }
}

function renderTokenArtworkSvg(cx: number, cy: number, num: number, laser: boolean): string {
  const pips = PIP_COUNT[num] ?? 0;
  const isRed = num === 6 || num === 8;
  const textColor = laser ? '#111111' : isRed ? '#c92a2a' : '#1e242b';
  const ringStroke = laser ? '#111111' : isRed ? '#c92a2a' : '#b9a67d';

  let out = '';
  if (!laser) {
    out += `<circle cx="${fmt(cx)}" cy="${fmt(cy)}" r="${fmt(TOKEN_RADIUS_MM)}" fill="#fbf5e6" stroke="${ringStroke}" stroke-width="0.55"/>`;
    out += `<circle cx="${fmt(cx)}" cy="${fmt(cy)}" r="${fmt(TOKEN_RADIUS_MM - 1.5)}" fill="none" stroke="${ringStroke}" stroke-width="0.22" stroke-dasharray="1.2 0.9"/>`;
  } else {
    out += `<circle cx="${fmt(cx)}" cy="${fmt(cy)}" r="${fmt(TOKEN_RADIUS_MM - 1.6)}" fill="none" stroke="#111111" stroke-width="0.2"/>`;
  }

  // Number text
  if (laser) {
    out += `<text x="${fmt(cx)}" y="${fmt(cy + 1.5)}" text-anchor="middle" font-family="system-ui, 'PingFang SC', sans-serif" font-size="10.2" font-weight="700" fill="none" stroke="#111111" stroke-width="0.32">${num}</text>`;
  } else {
    out += `<text x="${fmt(cx)}" y="${fmt(cy + 1.6)}" text-anchor="middle" font-family="system-ui, 'PingFang SC', sans-serif" font-size="10.4" font-weight="800" fill="${textColor}">${num}</text>`;
  }

  // Orientation bar under 6 & 9 so physical tokens never read upside-down
  if (num === 6 || num === 9) {
    out += `<line x1="${fmt(cx - 3.2)}" y1="${fmt(cy + 3.4)}" x2="${fmt(cx + 3.2)}" y2="${fmt(cy + 3.4)}" stroke="${textColor}" stroke-width="0.5" stroke-linecap="round"/>`;
  }

  // Pip dots (•)
  const spacing = 2.6;
  const startX = cx - ((pips - 1) * spacing) / 2;
  const pipY = cy + 6.7;
  for (let i = 0; i < pips; i++) {
    const px = startX + i * spacing;
    if (laser) {
      out += `<circle cx="${fmt(px)}" cy="${fmt(pipY)}" r="0.82" fill="none" stroke="#111111" stroke-width="0.28"/>`;
    } else {
      out += `<circle cx="${fmt(px)}" cy="${fmt(pipY)}" r="0.92" fill="${textColor}"/>`;
    }
  }
  return out;
}

function harborLabelInfo(hk: HarborKind): { ratio: string; sub: string; colorKey: string } {
  if (hk.type === 'Generic') {
    return { ratio: '3:1', sub: '通用港', colorKey: 'Generic' };
  }
  return {
    ratio: '2:1',
    sub: `${RESOURCE_SHORT[hk.value]}港`,
    colorKey: hk.value,
  };
}

/**
 * Generates a 1:1 physical-scale (millimeter) printable PnP or laser-cut SVG blueprint.
 */
export function generateDiySvg(
  draft: DraftMap,
  topo: Topology,
  opts: SvgExportOptions = DEFAULT_SVG_EXPORT_OPTIONS,
): string {
  const hexSide = Math.max(10, opts.hexSideMm || 45.6);
  const laser = opts.mode === 'laser';
  const kerfMm = laser ? Math.max(0, opts.kerfMm) : 0;
  const kerfHalf = kerfMm / 2;

  const numTiles = topo.tilePos.length / 2;
  const numVerts = topo.vertexPos.length / 2;

  const rawTile = Array.from({ length: numTiles }, (_, t) =>
    latticeUnit(topo.tilePos[2 * t], topo.tilePos[2 * t + 1]),
  );
  const rawVert = Array.from({ length: numVerts }, (_, v) =>
    latticeUnit(topo.vertexPos[2 * v], topo.vertexPos[2 * v + 1]),
  );

  // Verify circumradius from tileVertices distance to tilePos so every hex side is strictly `hexSide` mm.
  const v0Idx = topo.tileVertices[0] ?? 0;
  const unitCircumradius =
    numTiles > 0
      ? Math.hypot(rawVert[v0Idx][0] - rawTile[0][0], rawVert[v0Idx][1] - rawTile[0][1])
      : 1;
  const mmPerUnit = hexSide / (unitCircumradius || 1);

  const tileMm: [number, number][] = rawTile.map(([x, y]) => [x * mmPerUnit, y * mmPerUnit]);
  const vertMm: [number, number][] = rawVert.map(([x, y]) => [x * mmPerUnit, y * mmPerUnit]);

  // Compute harbor geometries in mm coordinates prior to global sheet offset.
  const harborDirs = computeHarborOutward(topo, draft.terrain, tileMm, vertMm);
  const wedgeDepth = hexSide * 0.56;

  // Compute bounding box of board + harbor wedges.
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;

  const expandBounds = (x: number, y: number, pad = 0) => {
    if (x - pad < minX) minX = x - pad;
    if (x + pad > maxX) maxX = x + pad;
    if (y - pad < minY) minY = y - pad;
    if (y + pad > maxY) maxY = y + pad;
  };

  for (const [vx, vy] of vertMm) expandBounds(vx, vy, kerfHalf + 1);
  if (opts.includeHarbors) {
    for (const h of harborDirs) {
      expandBounds(h.mx + h.ox * wedgeDepth, h.my + h.oy * wedgeDepth, hexSide * 0.25);
    }
  }

  const padMm = 14;
  const headerMm = 18;
  const shiftX = -minX + padMm;
  const shiftY = -minY + padMm + headerMm;

  const boardWidth = maxX - minX + padMm * 2;
  const minSheetWidth = 220;
  const widthMm = Math.max(boardWidth, minSheetWidth);
  const extraCenterShiftX = (widthMm - boardWidth) / 2;
  const ox = shiftX + extraCenterShiftX;
  const oy = shiftY;

  // Producing tokens list for dedicated Ø25.4mm cut-out chits tray (prevents laser cutting holes inside hexes!)
  const producingTokens: { tile: number; num: number; terrain: Terrain }[] = [];
  for (let t = 0; t < numTiles; t++) {
    const ter = draft.terrain[t];
    const num = draft.numbers[t];
    if (isProducingTerrain(ter) && num > 0) {
      producingTokens.push({ tile: t, num, terrain: ter });
    }
  }

  const tokenPitch = TOKEN_DIAMETER_MM + 4.6;
  const usableTrayWidth = widthMm - padMm * 2;
  const tokensPerRow = Math.max(1, Math.floor((usableTrayWidth + 4.6) / tokenPitch));
  const tokenRows = opts.includeTokens && producingTokens.length > 0
    ? Math.ceil(producingTokens.length / tokensPerRow)
    : 0;
  const tokenTrayStartY = maxY + oy + 14;
  const tokenTrayHeight = tokenRows > 0 ? 10 + tokenRows * tokenPitch + 4 : 0;

  const footerStartY = tokenTrayStartY + tokenTrayHeight + 6;
  const footerHeight = 36;
  const heightMm = footerStartY + footerHeight;

  const cutParts: string[] = [];
  const artParts: string[] = [];

  // Sheet background (PnP only; laser keeps background completely clear for LightBurn import)
  if (!laser) {
    artParts.push(
      `<rect x="0" y="0" width="${fmt(widthMm)}" height="${fmt(heightMm)}" fill="#faf7f0"/>`,
    );
  }

  // Sheet header title
  const spec = LAYOUT_SPECS[draft.layout];
  const sheetTitle = escapeXml(
    opts.title?.trim() || `Isle Settlers · 1:1 实体桌游图纸 (${spec.label})`,
  );
  const flatToFlat = hexSide * Math.sqrt(3);
  if (laser) {
    artParts.push(
      `<text x="${fmt(padMm)}" y="${fmt(padMm + 4)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="5.6" fill="none" stroke="#111111" stroke-width="0.22">${sheetTitle}  [Laser Kerf ${fmt(kerfMm)}mm · Hex ${fmt(hexSide)}mm]</text>`,
    );
  } else {
    artParts.push(
      `<text x="${fmt(padMm)}" y="${fmt(padMm + 4)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="6.2" font-weight="700" fill="#1b2a34">${sheetTitle}</text>`,
      `<text x="${fmt(widthMm - padMm)}" y="${fmt(padMm + 4)}" text-anchor="end" font-family="ui-monospace, monospace" font-size="4.2" fill="#5b6b77">1:1 PnP · 边长 ${fmt(hexSide)}mm · 对边 ${fmt(flatToFlat)}mm</text>`,
    );
  }

  // Radial vertex delta when offsetting hex edges perpendicularly outward by `kerfHalf = kerfMm / 2`
  const radialKerfDelta = kerfHalf / SQRT3_2;

  // 1. Hex tiles: cut outlines + artwork
  for (let t = 0; t < numTiles; t++) {
    const ter = draft.terrain[t] ?? 'Desert';
    const num = draft.numbers[t] ?? 0;
    const cx = tileMm[t][0] + ox;
    const cy = tileMm[t][1] + oy;

    const exactPts: string[] = [];
    const cutPts: string[] = [];
    const innerPts: string[] = [];

    for (let k = 0; k < 6; k++) {
      const vi = topo.tileVertices[t * 6 + k];
      const vx = vertMm[vi][0] + ox;
      const vy = vertMm[vi][1] + oy;
      const dx = vx - cx;
      const dy = vy - cy;
      const dLen = Math.hypot(dx, dy) || 1;
      const ux = dx / dLen;
      const uy = dy / dLen;

      exactPts.push(`${fmt(vx)},${fmt(vy)}`);
      cutPts.push(`${fmt(vx + ux * radialKerfDelta)},${fmt(vy + uy * radialKerfDelta)}`);
      innerPts.push(`${fmt(vx - ux * 2.4)},${fmt(vy - uy * 2.4)}`);
    }

    cutParts.push(`<polygon points="${cutPts.join(' ')}" />`);

    if (!laser) {
      const pal = PNP_HEX_FILL[ter];
      artParts.push(
        `<polygon points="${exactPts.join(' ')}" fill="${pal.fill}" stroke="${pal.accent}" stroke-width="0.35" stroke-linejoin="round"/>`,
        `<polygon points="${innerPts.join(' ')}" fill="none" stroke="${pal.inner}" stroke-width="0.6" stroke-linejoin="round"/>`,
      );
    } else {
      artParts.push(
        `<polygon points="${innerPts.join(' ')}" fill="none" stroke="#111111" stroke-width="0.18" stroke-dasharray="2 1.2"/>`,
      );
    }

    // Terrain vector illustration:
    // On PnP with on-tile token shown, place illustration in the upper half of the hex;
    // on Laser (where wooden hexes stay solid without center cut-out) or non-producing tiles, center the icon cleanly.
    const showOnTileToken = !laser && opts.includeTokens && isProducingTerrain(ter) && num > 0;
    const artCy = showOnTileToken ? cy - hexSide * 0.44 : cy - hexSide * 0.04;
    const artSize = showOnTileToken ? hexSide * 0.24 : hexSide * 0.34;
    artParts.push(renderTerrainArtSvg(ter, cx, artCy, artSize, laser));

    // Small terrain caption at bottom of hex
    const labelY = cy + hexSide * 0.72;
    if (laser) {
      artParts.push(
        `<text x="${fmt(cx)}" y="${fmt(labelY)}" text-anchor="middle" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.6" fill="none" stroke="#111111" stroke-width="0.18">${TERRAIN_NAME[ter]}</text>`,
      );
    } else {
      artParts.push(
        `<text x="${fmt(cx)}" y="${fmt(labelY)}" text-anchor="middle" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.8" font-weight="600" fill="${PNP_HEX_FILL[ter].accent}" opacity="0.85">${TERRAIN_NAME[ter]}</text>`,
      );
    }

    if (showOnTileToken) {
      artParts.push(renderTokenArtworkSvg(cx, cy + hexSide * 0.08, num, false));
    }
  }

  // 2. Harbor wedges (cut outlines + artwork)
  if (opts.includeHarbors) {
    harborDirs.forEach((h, slot) => {
      const hk = draft.harbors[slot] ?? { type: 'Generic' };
      const ax = h.ax + ox;
      const ay = h.ay + oy;
      const bx = h.bx + ox;
      const by = h.by + oy;
      const mx = h.mx + ox;
      const my = h.my + oy;

      // Tangent unit vector along edge a -> b
      const ex = bx - ax;
      const ey = by - ay;
      const eLen = Math.hypot(ex, ey) || 1;
      const tx = ex / eLen;
      const ty = ey / eLen;

      // Trapezoidal harbor wedge extending seaward from coastal edge
      const baseHalf = Math.min(eLen * 0.42, hexSide * 0.42) + kerfHalf;
      const tipHalf = Math.min(eLen * 0.24, hexSide * 0.24) + kerfHalf;
      const dOut = wedgeDepth + kerfHalf;

      const p1x = mx - tx * baseHalf + h.ox * 1.1;
      const p1y = my - ty * baseHalf + h.oy * 1.1;
      const p2x = mx + tx * baseHalf + h.ox * 1.1;
      const p2y = my + ty * baseHalf + h.oy * 1.1;
      const p3x = mx + tx * tipHalf + h.ox * dOut;
      const p3y = my + ty * tipHalf + h.oy * dOut;
      const p4x = mx - tx * tipHalf + h.ox * dOut;
      const p4y = my - ty * tipHalf + h.oy * dOut;

      const wedgePts = `${fmt(p1x)},${fmt(p1y)} ${fmt(p2x)},${fmt(p2y)} ${fmt(p3x)},${fmt(p3y)} ${fmt(p4x)},${fmt(p4y)}`;
      cutParts.push(`<polygon points="${wedgePts}" />`);

      const cx = mx + h.ox * (wedgeDepth * 0.56);
      const cy = my + h.oy * (wedgeDepth * 0.56);
      const info = harborLabelInfo(hk);
      const color = HARBOR_COLOR[info.colorKey] ?? '#4f5d6b';

      if (laser) {
        artParts.push(
          `<line x1="${fmt(p1x + tx * 2)}" y1="${fmt(p1y + ty * 2)}" x2="${fmt(cx)}" y2="${fmt(cy)}" stroke="#111111" stroke-width="0.25"/>`,
          `<line x1="${fmt(p2x - tx * 2)}" y1="${fmt(p2y - ty * 2)}" x2="${fmt(cx)}" y2="${fmt(cy)}" stroke="#111111" stroke-width="0.25"/>`,
          `<circle cx="${fmt(cx)}" cy="${fmt(cy)}" r="${fmt(hexSide * 0.19)}" fill="none" stroke="#111111" stroke-width="0.25"/>`,
          `<text x="${fmt(cx)}" y="${fmt(cy - 0.5)}" text-anchor="middle" font-family="system-ui, sans-serif" font-size="4.4" font-weight="700" fill="none" stroke="#111111" stroke-width="0.22">${info.ratio}</text>`,
          `<text x="${fmt(cx)}" y="${fmt(cy + 3.8)}" text-anchor="middle" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.4" fill="none" stroke="#111111" stroke-width="0.18">${info.sub}</text>`,
        );
      } else {
        artParts.push(
          `<polygon points="${wedgePts}" fill="#efe3c8" stroke="#8a6b43" stroke-width="0.35" stroke-linejoin="round"/>`,
          `<line x1="${fmt(ax)}" y1="${fmt(ay)}" x2="${fmt(cx)}" y2="${fmt(cy)}" stroke="#8a5a35" stroke-width="1.1" stroke-linecap="round"/>`,
          `<line x1="${fmt(bx)}" y1="${fmt(by)}" x2="${fmt(cx)}" y2="${fmt(cy)}" stroke="#8a5a35" stroke-width="1.1" stroke-linecap="round"/>`,
          `<circle cx="${fmt(cx)}" cy="${fmt(cy)}" r="${fmt(hexSide * 0.2)}" fill="#ffffff" stroke="${color}" stroke-width="0.8"/>`,
          `<text x="${fmt(cx)}" y="${fmt(cy - 0.4)}" text-anchor="middle" font-family="system-ui, sans-serif" font-size="4.6" font-weight="800" fill="${color}">${info.ratio}</text>`,
          `<text x="${fmt(cx)}" y="${fmt(cy + 3.7)}" text-anchor="middle" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.5" font-weight="700" fill="#2b343b">${info.sub}</text>`,
        );
      }
    });
  }

  // 3. Dedicated Ø25.4mm circular number token tray (cut circles + engraved/printed faces)
  if (opts.includeTokens && producingTokens.length > 0) {
    const captionColor = laser ? '#111111' : '#3a4a56';
    artParts.push(
      laser
        ? `<text x="${fmt(padMm)}" y="${fmt(tokenTrayStartY + 4)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="4.2" fill="none" stroke="#111111" stroke-width="0.2">独立数字圆片 (Ø25.4mm × ${producingTokens.length} 枚)</text>`
        : `<text x="${fmt(padMm)}" y="${fmt(tokenTrayStartY + 4)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="4.4" font-weight="700" fill="${captionColor}">独立数字圆片切割区 · Ø25.4mm × ${producingTokens.length} 枚</text>`,
    );

    const cutTokenRadius = TOKEN_RADIUS_MM + kerfHalf;
    producingTokens.forEach((tok, idx) => {
      const col = idx % tokensPerRow;
      const row = Math.floor(idx / tokensPerRow);
      const tcx = padMm + TOKEN_RADIUS_MM + col * tokenPitch;
      const tcy = tokenTrayStartY + 9 + TOKEN_RADIUS_MM + row * tokenPitch;

      cutParts.push(`<circle cx="${fmt(tcx)}" cy="${fmt(tcy)}" r="${fmt(cutTokenRadius)}" />`);
      artParts.push(renderTokenArtworkSvg(tcx, tcy, tok.num, laser));
    });
  }

  // 4. Calibration Scale Bar (strictly 50.00mm) + Bill of Materials (BOM) summary caption
  const rulerX = padMm;
  const rulerY = footerStartY + 8;
  const rulerStroke = laser ? '#111111' : '#1b2a34';

  artParts.push(
    `<g id="calibration-scale-50mm">`,
    `<line x1="${fmt(rulerX)}" y1="${fmt(rulerY)}" x2="${fmt(rulerX + 50)}" y2="${fmt(rulerY)}" stroke="${rulerStroke}" stroke-width="0.5"/>`,
  );
  for (let mm = 0; mm <= 50; mm += 10) {
    const tx = rulerX + mm;
    const tickH = mm === 0 || mm === 50 ? 3.6 : 2.2;
    artParts.push(
      `<line x1="${fmt(tx)}" y1="${fmt(rulerY - tickH)}" x2="${fmt(tx)}" y2="${fmt(rulerY + 0.8)}" stroke="${rulerStroke}" stroke-width="0.35"/>`,
    );
    if (mm === 0 || mm === 25 || mm === 50) {
      artParts.push(
        laser
          ? `<text x="${fmt(tx)}" y="${fmt(rulerY - 4.2)}" text-anchor="middle" font-family="ui-monospace, monospace" font-size="3.1" fill="none" stroke="#111111" stroke-width="0.16">${mm}</text>`
          : `<text x="${fmt(tx)}" y="${fmt(rulerY - 4.2)}" text-anchor="middle" font-family="ui-monospace, monospace" font-size="3.2" fill="#3a4a56">${mm}</text>`,
      );
    }
  }
  artParts.push(
    laser
      ? `<text x="${fmt(rulerX + 54)}" y="${fmt(rulerY + 0.5)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.8" fill="none" stroke="#111111" stroke-width="0.18">50.0 mm 比例校准尺（请以 100% 实际尺寸打印/导入 LightBurn）</text>`
      : `<text x="${fmt(rulerX + 54)}" y="${fmt(rulerY + 0.5)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.9" font-weight="600" fill="#1b2a34">50.0 mm 比例校准尺（打印请关闭「适应纸张」，保持 100% 实际比例）</text>`,
    `</g>`,
  );

  // Bill of Materials counts
  const terrainCounts = new Map<Terrain, number>();
  for (const ter of draft.terrain) {
    terrainCounts.set(ter, (terrainCounts.get(ter) ?? 0) + 1);
  }
  const bomTerrain = [...terrainCounts.entries()]
    .map(([ter, n]) => `${TERRAIN_NAME[ter]}×${n}`)
    .join(' · ');
  const genericHarbors = draft.harbors.filter((h) => h.type === 'Generic').length;
  const specialHarbors = draft.harbors.length - genericHarbors;
  const mapCode = encodeCustomMap(draft);
  const bomLine1 = `物料清单 (BOM)：六角地块共 ${numTiles} 枚（${bomTerrain}） | Ø25.4mm 数字圆片 ${producingTokens.length} 枚 | 港口配件 ${draft.harbors.length} 枚（3:1×${genericHarbors}, 2:1×${specialHarbors}）`;
  const bomLine2 = `规格参数：边长 ${fmt(hexSide)}mm · 对边宽 ${fmt(flatToFlat)}mm · 刀缝补偿 ${fmt(kerfMm)}mm · 短码 ${escapeXml(mapCode.slice(0, 68))}${mapCode.length > 68 ? '…' : ''}`;

  if (laser) {
    artParts.push(
      `<text x="${fmt(padMm)}" y="${fmt(rulerY + 9)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.5" fill="none" stroke="#111111" stroke-width="0.17">${bomLine1}</text>`,
      `<text x="${fmt(padMm)}" y="${fmt(rulerY + 15)}" font-family="ui-monospace, monospace" font-size="3.2" fill="none" stroke="#111111" stroke-width="0.16">${bomLine2}</text>`,
    );
  } else {
    artParts.push(
      `<text x="${fmt(padMm)}" y="${fmt(rulerY + 9)}" font-family="system-ui, 'PingFang SC', sans-serif" font-size="3.7" fill="#32424e">${bomLine1}</text>`,
      `<text x="${fmt(padMm)}" y="${fmt(rulerY + 15)}" font-family="ui-monospace, monospace" font-size="3.3" fill="#5b6b77">${bomLine2}</text>`,
    );
  }

  return [
    `<?xml version="1.0" encoding="UTF-8"?>`,
    `<svg xmlns="http://www.w3.org/2000/svg" width="${fmt(widthMm)}mm" height="${fmt(heightMm)}mm" viewBox="0 0 ${fmt(widthMm)} ${fmt(heightMm)}">`,
    `  <g id="artwork-engrave">`,
    `    ${artParts.join('\n    ')}`,
    `  </g>`,
    `  <g id="laser-cut" stroke="#ff0000" stroke-width="0.1" fill="none">`,
    `    ${cutParts.join('\n    ')}`,
    `  </g>`,
    `</svg>`,
  ].join('\n');
}

/** Triggers a browser download of the generated SVG blueprint (`image/svg+xml;charset=utf-8`). */
export function downloadDiySvg(svg: string, filename: string): void {
  const safeName = filename.endsWith('.svg') ? filename : `${filename}.svg`;
  const blob = new Blob([svg], { type: 'image/svg+xml;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = safeName;
  a.style.display = 'none';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1500);
}
