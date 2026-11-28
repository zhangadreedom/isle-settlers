// Three.js board renderer.
//
// * WebGPURenderer, falling back to WebGL2 automatically (and rebuilding on
//   WebGL2 if a WebGPU device is lost).
// * Render on demand: frames are drawn only while the camera moves, an
//   animation runs or the state changes; an idle board costs no GPU time.
// * Repeated objects (tiles, decorations, pieces, markers) are
//   InstancedMeshes, keeping the scene to a few dozen draw calls.
// * Picking intersects the pointer ray with the board plane and snaps to the
//   nearest highlighted spot; no mesh raycasting.

import * as THREE from 'three/webgpu';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { CITY_FLAG, EMPTY, type Topology } from '../engine';
import { isColorblindMode, onColorblindChange, SEAT_SYMBOLS, setColorblindMode, TERRAIN_SHORT_BADGE } from './a11y';
import {
  BEACH,
  METROPOLIS_COLORS,
  PLAYER_COLORS,
  RESOURCE_COLORS,
  RESOURCE_LABELS,
  SEA,
  SKY,
  TERRAIN_COLORS,
  THEME_CONFIGS,
} from './palette';

export type BoardTheme = 'day' | 'sunset' | 'night';

const THEME_STORAGE_KEY = 'isle.theme';

export function readSavedTheme(): BoardTheme {
  try {
    const raw = localStorage.getItem(THEME_STORAGE_KEY);
    if (raw === 'day' || raw === 'sunset' || raw === 'night') return raw;
  } catch {
    // Ignore storage access errors (private browsing / non-browser host).
  }
  return 'day';
}

export function saveTheme(t: BoardTheme): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, t);
  } catch {
    // Ignore storage access errors.
  }
}

/** Static layout as numeric codes (see palette for terrain/harbor codes). */
export interface BoardData {
  terrain: number[];
  numbers: number[];
  harbors: number[];
  /** Island per tile: 0 for the sea, 1.. for land. */
  islands: number[];
  /** Fishermen mode: coastal fishing grounds with edge id and roll number. */
  fishingGrounds?: { edge: number; number: number }[];
  /** Caravans mode: edges forming the Golden River. */
  riverEdges?: number[];
}

export interface KnightData {
  vertex: number;
  owner: number;
  level: number;
  active: boolean;
}

export interface PieceData {
  buildings: number[];
  roads: number[];
  /** Seafaring: ship owner per edge (or EMPTY). */
  ships?: number[];
  robber: number;
  /** Seafaring: the pirate's sea tile. */
  pirate?: number | null;
  /** Citadels: knights on vertices. */
  knights?: KnightData[];
  /** Citadels: metropolis vertex per CityTrack (`null` if unclaimed). */
  metropolises?: (number | null)[];
  /** Citadels: barbarian fleet progress (0..barbarianSteps), null/undefined when not in Citadels. */
  barbarianPos?: number | null;
  barbarianSteps?: number;
  /** Caravans mode: edge indices holding a camel caravan segment. */
  camels?: number[];
  /** Caravans mode: bridge owner per edge (or EMPTY = 255). */
  bridges?: number[];
}

export interface TileHover {
  tile: number;
  clientX: number;
  clientY: number;
}

/** Terrain code of desert (see palette). */
const DESERT_TERRAIN = 5;
/** Terrain code of the sea (see palette). */
const SEA_TERRAIN = 6;
/** Terrain code of gold field (see palette). */
const GOLD_TERRAIN = 7;
/** Terrain code of unexplored fog (see palette). */
const FOG_TERRAIN = 8;
/** Height of ships and the pirate above the water plane. */
const WATER = -0.06;

export type PickKind = 'vertex' | 'edge' | 'tile';

/** Screen space (CSS px) covered by HUD panels on each side. */
export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface Highlights {
  kind: PickKind;
  ids: number[];
}

type Backend = 'WebGPU' | 'WebGL2';

/** Minimal slice of the WebGPU device API we rely on (no @webgpu/types needed). */
interface DeviceLike {
  lost: Promise<{ reason: string; message: string }>;
}

const TILE_HEIGHT = 0.22;
const TOP = TILE_HEIGHT;
const SQRT3_2 = Math.sqrt(3) / 2;
const PIPS = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];
/** Margin (world units) kept visible around the outermost tile centres: tiles plus harbor labels. */
const MARGIN_X = 1.74;
const MARGIN_Z = 1.42;
/** Instance capacity for per-spot meshes; covers 5–6 player seafaring boards (165 edges). */
const MAX_SPOTS = 192;
/** Beach outline: distance from the outermost tile centres at the top and the foot of the sand. */
const BEACH_TOP = 1.55;
const BEACH_FOOT = 1.94;
const CAMERA_DIR = new THREE.Vector3(0, 10.5, 8.5).normalize();
const DROP_MS = 420;
const ROBBER_MS = 550;
const BARBARIAN_PULSE_MS = 2100;
/** Pick radius (world units) per target kind. */
const PICK_RADIUS: Record<PickKind, number> = { vertex: 0.38, edge: 0.32, tile: 0.9 };
const TILE_HOVER_RADIUS = 0.98;
const ACCENT = 0xf0b429;

const lattice = (x: number, y: number): [number, number] => [x * SQRT3_2, y * 0.5];
const easeOutBack = (t: number) => 1 + 2.2 * (t - 1) ** 3 + 1.2 * (t - 1) ** 2;
const easeInOut = (t: number) => (t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2);

/** Small deterministic PRNG so decorations stay put between renders. */
function prng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 4294967296;
  };
}

/** Convex hull (counter-clockwise, no collinear points) of 2D points. */
function convexHull(points: [number, number][]): [number, number][] {
  const pts = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o: [number, number], a: [number, number], b: [number, number]) =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const half = (list: [number, number][]) => {
    const out: [number, number][] = [];
    for (const p of list) {
      while (out.length >= 2 && cross(out[out.length - 2], out[out.length - 1], p) <= 1e-9) out.pop();
      out.push(p);
    }
    out.pop();
    return out;
  };
  return [...half(pts), ...half([...pts].reverse())];
}

/** Moves every edge of a convex counter-clockwise polygon outwards by `d`. */
function offsetPolygon(poly: [number, number][], d: number): [number, number][] {
  const n = poly.length;
  const normal = (i: number): [number, number] => {
    const [ax, az] = poly[i];
    const [bx, bz] = poly[(i + 1) % n];
    const len = Math.hypot(bx - ax, bz - az);
    return [(bz - az) / len, -(bx - ax) / len];
  };
  // Corners sharper than a right angle are bevelled (two points), so a
  // small island's beach does not shoot out a long spike. The choice depends
  // only on the angle, so offsets of one polygon keep the same vertex count.
  return poly.flatMap((p, i): [number, number][] => {
    const n1 = normal((i - 1 + n) % n);
    const n2 = normal(i);
    const dot = n1[0] * n2[0] + n1[1] * n2[1];
    if (dot < 0) {
      return [
        [p[0] + n1[0] * d, p[1] + n1[1] * d],
        [p[0] + n2[0] * d, p[1] + n2[1] * d],
      ];
    }
    const k = d / (1 + dot);
    return [[p[0] + (n1[0] + n2[0]) * k, p[1] + (n1[1] + n2[1]) * k]];
  });
}

/** A sand bank: flat top polygon `top` at y = 0 sloping down to `foot` at y = -height. */
function bankGeometry(top: [number, number][], foot: [number, number][], height: number) {
  const pos: number[] = [];
  const n = top.length;
  // Hull points are counter-clockwise in (x, z); seen from above (+y) that is clockwise.
  for (let i = 1; i < n - 1; i++) pos.push(top[0][0], 0, top[0][1], top[i + 1][0], 0, top[i + 1][1], top[i][0], 0, top[i][1]);
  for (let i = 0; i < n; i++) {
    const j = (i + 1) % n;
    const [a, b, c, d] = [top[i], top[j], foot[i], foot[j]];
    pos.push(a[0], 0, a[1], b[0], 0, b[1], c[0], -height, c[1]);
    pos.push(b[0], 0, b[1], d[0], -height, d[1], c[0], -height, c[1]);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.computeVertexNormals();
  return g;
}

function std(color: number, extra: Partial<THREE.MeshStandardMaterialParameters> = {}) {
  return new THREE.MeshStandardMaterial({ color, roughness: 0.85, flatShading: true, ...extra });
}

function createRenderer(forceWebGL: boolean) {
  return new THREE.WebGPURenderer({ antialias: true, forceWebGL });
}

// ---- shared geometry ----
const hexGeometry = new THREE.CylinderGeometry(0.97, 0.97, TILE_HEIGHT, 6);

function prism(width: number, height: number, depth: number) {
  const shape = new THREE.Shape();
  shape.moveTo(-width / 2, 0);
  shape.lineTo(width / 2, 0);
  shape.lineTo(0, height);
  shape.closePath();
  const g = new THREE.ExtrudeGeometry(shape, { depth, bevelEnabled: false });
  g.translate(0, 0, -depth / 2);
  return g;
}

function box(w: number, h: number, d: number, x: number, y: number, z: number) {
  return new THREE.BoxGeometry(w, h, d).translate(x, y, z).toNonIndexed();
}

const settlementGeometry = mergeGeometries([
  box(0.24, 0.18, 0.24, 0, 0.09, 0),
  prism(0.31, 0.16, 0.26).translate(0, 0.18, 0),
])!;

const cityGeometry = mergeGeometries([
  box(0.44, 0.18, 0.26, 0, 0.09, 0),
  box(0.2, 0.36, 0.26, -0.12, 0.18, 0),
  prism(0.28, 0.14, 0.26).translate(0.11, 0.18, 0),
  prism(0.25, 0.12, 0.28).translate(-0.12, 0.36, 0),
])!;

/** Citadels: armored knight figure on a hexagonal plinth. */
const knightGeometry = mergeGeometries([
  new THREE.CylinderGeometry(0.15, 0.17, 0.07, 6).translate(0, 0.035, 0).toNonIndexed(),
  new THREE.CylinderGeometry(0.085, 0.12, 0.22, 6).translate(0, 0.18, 0).toNonIndexed(),
  new THREE.SphereGeometry(0.085, 8, 6).translate(0, 0.33, 0).toNonIndexed(),
  box(0.04, 0.11, 0.14, 0, 0.4, 0),
])!;

/** Golden banner/halo marking an active knight. */
const knightBannerGeometry = mergeGeometries([
  new THREE.RingGeometry(0.18, 0.24, 6, 1, Math.PI / 6).rotateX(-Math.PI / 2).translate(0, 0.015, 0).toNonIndexed(),
  new THREE.OctahedronGeometry(0.065, 0).translate(0, 0.52, 0).toNonIndexed(),
])!;

/** Citadels: golden arch spire crowning a Metropolis city. */
const metropolisGeometry = mergeGeometries([
  box(0.48, 0.06, 0.32, 0, 0.42, 0),
  new THREE.ConeGeometry(0.14, 0.32, 6).translate(-0.12, 0.61, 0).toNonIndexed(),
  new THREE.OctahedronGeometry(0.07, 0).translate(0.11, 0.52, 0).toNonIndexed(),
])!;

const roadGeometry = new THREE.BoxGeometry(0.64, 0.09, 0.12);

/** Caravans: arched stone bridge with raised span and twin side parapets. */
const bridgeGeometry = mergeGeometries([
  box(0.11, 0.1, 0.22, -0.28, 0.05, 0),
  box(0.11, 0.1, 0.22, 0.28, 0.05, 0),
  new THREE.BoxGeometry(0.2, 0.052, 0.19).rotateZ(0.28).translate(-0.165, 0.112, 0).toNonIndexed(),
  new THREE.BoxGeometry(0.2, 0.052, 0.19).rotateZ(-0.28).translate(0.165, 0.112, 0).toNonIndexed(),
  box(0.2, 0.054, 0.19, 0, 0.145, 0),
  new THREE.BoxGeometry(0.2, 0.048, 0.032).rotateZ(0.28).translate(-0.165, 0.158, -0.086).toNonIndexed(),
  new THREE.BoxGeometry(0.2, 0.048, 0.032).rotateZ(0.28).translate(-0.165, 0.158, 0.086).toNonIndexed(),
  new THREE.BoxGeometry(0.2, 0.048, 0.032).rotateZ(-0.28).translate(0.165, 0.158, -0.086).toNonIndexed(),
  new THREE.BoxGeometry(0.2, 0.048, 0.032).rotateZ(-0.28).translate(0.165, 0.158, 0.086).toNonIndexed(),
  box(0.21, 0.05, 0.034, 0, 0.192, -0.086),
  box(0.21, 0.05, 0.034, 0, 0.192, 0.086),
  box(0.045, 0.075, 0.045, -0.285, 0.132, -0.088),
  box(0.045, 0.075, 0.045, -0.285, 0.132, 0.088),
  box(0.045, 0.075, 0.045, 0.285, 0.132, -0.088),
  box(0.045, 0.075, 0.045, 0.285, 0.132, 0.088),
  box(0.046, 0.072, 0.046, 0, 0.208, -0.088),
  box(0.046, 0.072, 0.046, 0, 0.208, 0.088),
])!;

/** Caravans: low-poly twin-hump 3D camel facing +x. */
const camelGeometry = mergeGeometries([
  box(0.036, 0.15, 0.036, 0.088, 0.075, -0.038),
  box(0.036, 0.15, 0.036, 0.088, 0.075, 0.038),
  box(0.038, 0.15, 0.036, -0.088, 0.075, -0.038),
  box(0.038, 0.15, 0.036, -0.088, 0.075, 0.038),
  box(0.245, 0.11, 0.115, 0, 0.188, 0),
  new THREE.ConeGeometry(0.056, 0.105, 6).translate(-0.058, 0.292, 0).toNonIndexed(),
  new THREE.ConeGeometry(0.056, 0.11, 6).translate(0.052, 0.295, 0).toNonIndexed(),
  new THREE.BoxGeometry(0.062, 0.145, 0.058).rotateZ(-0.26).translate(0.142, 0.248, 0).toNonIndexed(),
  box(0.105, 0.056, 0.056, 0.196, 0.332, 0),
  box(0.044, 0.042, 0.044, 0.252, 0.324, 0),
  box(0.02, 0.032, 0.02, 0.16, 0.372, -0.02),
  box(0.02, 0.032, 0.02, 0.16, 0.372, 0.02),
  new THREE.BoxGeometry(0.024, 0.085, 0.024).rotateZ(-0.22).translate(-0.134, 0.175, 0).toNonIndexed(),
])!;

/** Caravans: winding S-curved river water ribbon & golden bank bed along one hex edge (-0.5..0.5). */
const riverBankGeometry = mergeGeometries([
  new THREE.BoxGeometry(0.36, 0.024, 0.27).rotateY(-0.105).translate(-0.33, 0, 0.018).toNonIndexed(),
  new THREE.BoxGeometry(0.38, 0.024, 0.27).rotateY(0.2).translate(0, 0, 0).toNonIndexed(),
  new THREE.BoxGeometry(0.36, 0.024, 0.27).rotateY(-0.105).translate(0.33, 0, -0.018).toNonIndexed(),
])!;
const riverRibbonGeometry = mergeGeometries([
  new THREE.BoxGeometry(0.36, 0.02, 0.155).rotateY(-0.105).translate(-0.33, 0, 0.018).toNonIndexed(),
  new THREE.BoxGeometry(0.38, 0.02, 0.155).rotateY(0.2).translate(0, 0, 0).toNonIndexed(),
  new THREE.BoxGeometry(0.36, 0.02, 0.155).rotateY(-0.105).translate(0.33, 0, -0.018).toNonIndexed(),
])!;
const riverBendBankGeometry = new THREE.CylinderGeometry(0.135, 0.145, 0.024, 12);
const riverBendWaterGeometry = new THREE.CylinderGeometry(0.082, 0.088, 0.02, 12);
const oasisBankGeometry = new THREE.CylinderGeometry(0.33, 0.37, 0.02, 16);
const oasisPoolGeometry = new THREE.CylinderGeometry(0.25, 0.26, 0.024, 16);
const oasisTrunkGeometry = mergeGeometries([
  new THREE.CylinderGeometry(0.026, 0.038, 0.15, 6).translate(0, 0.075, 0).toNonIndexed(),
  new THREE.CylinderGeometry(0.022, 0.028, 0.14, 6).rotateZ(-0.16).translate(0.014, 0.21, 0).toNonIndexed(),
  new THREE.CylinderGeometry(0.018, 0.022, 0.13, 6).rotateZ(-0.28).translate(0.042, 0.33, 0).toNonIndexed(),
])!;
const oasisCrownGeometry = mergeGeometries([
  ...[0, 1, 2, 3, 4].map((i) =>
    new THREE.ConeGeometry(0.055, 0.24, 4)
      .scale(0.32, 1, 1)
      .rotateZ(-1.22)
      .translate(0.1, -0.01, 0)
      .rotateY((i * Math.PI * 2) / 5)
      .translate(0.058, 0.395, 0)
      .toNonIndexed(),
  ),
  new THREE.SphereGeometry(0.036, 6, 5).translate(0.058, 0.375, 0).toNonIndexed(),
])!;
/** Flat triangle pointing +x (a bow), `length` long, `width` wide, `thickness` high. */
function bow(width: number, length: number, thickness: number) {
  return prism(width, length, thickness).rotateX(Math.PI / 2).rotateY(Math.PI / 2);
}

const mast = (r: number, h: number, x: number, y: number) => new THREE.CylinderGeometry(r, r, h, 6).translate(x, y, 0).toNonIndexed();

/** A small boat: hull, bow, cabin, mast and sail; bow along +x. */
const shipGeometry = mergeGeometries([
  box(0.4, 0.09, 0.17, 0, 0.045, 0),
  bow(0.17, 0.12, 0.09).translate(0.2, 0.045, 0),
  box(0.12, 0.07, 0.11, -0.11, 0.125, 0),
  mast(0.012, 0.36, 0.04, 0.27),
  box(0.02, 0.22, 0.2, 0.06, 0.3, 0),
])!;
/** The pirate: a bigger, dark ship with two sails. */
const pirateHull = mergeGeometries([
  box(0.56, 0.12, 0.24, 0, 0.06, 0),
  bow(0.24, 0.16, 0.12).translate(0.28, 0.06, 0),
  box(0.16, 0.1, 0.2, -0.2, 0.17, 0),
  mast(0.015, 0.5, 0.08, 0.37),
  mast(0.013, 0.36, -0.1, 0.3),
])!;
const pirateSails = mergeGeometries([box(0.025, 0.3, 0.28, 0.1, 0.4, 0), box(0.022, 0.2, 0.2, -0.08, 0.32, 0)])!;
/** Citadels: menacing black-sail barbarian warship approaching from the outer sea. */
const barbarianHull = mergeGeometries([
  box(0.74, 0.14, 0.28, 0, 0.07, 0),
  bow(0.28, 0.22, 0.14).translate(0.37, 0.07, 0),
  box(0.05, 0.24, 0.05, 0.46, 0.2, 0),
  box(0.22, 0.13, 0.24, -0.26, 0.2, 0),
  mast(0.018, 0.62, 0.12, 0.44),
  mast(0.015, 0.46, -0.14, 0.36),
  box(0.025, 0.025, 0.38, 0.12, 0.58, 0),
  box(0.022, 0.022, 0.28, -0.14, 0.46, 0),
])!;
const barbarianSails = mergeGeometries([
  box(0.028, 0.2, 0.36, 0.14, 0.52, 0),
  box(0.026, 0.16, 0.3, 0.15, 0.35, -0.02),
  box(0.025, 0.24, 0.26, -0.12, 0.39, 0),
  box(0.16, 0.045, 0.02, 0.04, 0.74, 0),
])!;
const barbarianTrim = mergeGeometries([
  box(0.46, 0.055, 0.31, 0.02, 0.13, 0),
  new THREE.OctahedronGeometry(0.065, 0).translate(0.34, 0.22, 0).toNonIndexed(),
])!;
const barbarianRingGeometry = new THREE.RingGeometry(0.5, 0.65, 6, 1, Math.PI / 6).rotateX(-Math.PI / 2);
const seaRingGeometry = new THREE.RingGeometry(0.9, 0.97, 6, 1, Math.PI / 6).rotateX(-Math.PI / 2);
const fogPadGeometry = new THREE.CylinderGeometry(0.92, 0.96, 0.06, 6);
const tokenGeometry = new THREE.CylinderGeometry(0.25, 0.25, 0.05, 24);
const labelGeometry = new THREE.PlaneGeometry(0.5, 0.5).rotateX(-Math.PI / 2);
const vertexMarkerGeometry = new THREE.CylinderGeometry(0.14, 0.14, 0.05, 20);
const edgeMarkerGeometry = new THREE.BoxGeometry(0.5, 0.05, 0.14);
const tileMarkerGeometry = new THREE.RingGeometry(0.72, 0.86, 6, 1, Math.PI / 6).rotateX(-Math.PI / 2);
const shoalPoolGeometry = new THREE.CircleGeometry(0.35, 20).rotateX(-Math.PI / 2);
const shoalRippleGeometry = mergeGeometries([
  new THREE.RingGeometry(0.17, 0.215, 20).rotateX(-Math.PI / 2).toNonIndexed(),
  new THREE.RingGeometry(0.3, 0.355, 24).rotateX(-Math.PI / 2).toNonIndexed(),
])!;
const leapingFishGeometry = mergeGeometries([
  new THREE.ConeGeometry(0.038, 0.115, 6).rotateZ(-Math.PI / 2).translate(0.055, 0, 0).toNonIndexed(),
  new THREE.ConeGeometry(0.038, 0.135, 6).rotateZ(Math.PI / 2).translate(-0.065, 0, 0).toNonIndexed(),
  new THREE.ConeGeometry(0.05, 0.08, 4).scale(1, 1, 0.28).rotateZ(-Math.PI / 2).translate(-0.15, 0.006, 0).toNonIndexed(),
  new THREE.ConeGeometry(0.024, 0.05, 4).scale(1.2, 1, 0.25).translate(-0.008, 0.045, 0).toNonIndexed(),
])!;

interface Decoration {
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  count: number;
  scale: [number, number];
  squash?: number;
  y?: number;
  radius?: [number, number];
  /** Optional cap stacked on each instance (snow on peaks). */
  cap?: { geometry: THREE.BufferGeometry; material: THREE.Material };
}

const DECORATIONS: Record<number, Decoration> = {
  0: { geometry: new THREE.DodecahedronGeometry(0.12, 0), material: std(0x9b4a2a), count: 4, scale: [0.8, 1.2], squash: 0.6 },
  1: { geometry: new THREE.ConeGeometry(0.13, 0.42, 6).translate(0, 0.21, 0), material: std(0x1f5e36), count: 6, scale: [0.75, 1.2] },
  2: { geometry: new THREE.IcosahedronGeometry(0.075, 0).translate(0, 0.06, 0), material: std(0xf7f7f2), count: 4, scale: [0.9, 1.15] },
  3: { geometry: new THREE.ConeGeometry(0.09, 0.22, 5).translate(0, 0.11, 0), material: std(0xc99a2e), count: 5, scale: [0.85, 1.15] },
  4: {
    geometry: new THREE.ConeGeometry(0.28, 0.6, 5).translate(0, 0.3, 0),
    material: std(0x6f747d),
    count: 3,
    scale: [0.75, 1.15],
    cap: { geometry: new THREE.ConeGeometry(0.108, 0.23, 5).translate(0, 0.485, 0), material: std(0xf4f6f8) },
  },
  5: { geometry: new THREE.SphereGeometry(0.25, 8, 5), material: std(0xd8c08a), count: 2, scale: [0.8, 1.2], squash: 0.3 },
  7: {
    geometry: new THREE.OctahedronGeometry(0.1, 0).translate(0, 0.06, 0),
    material: std(0xffcf3a, { metalness: 0.55, roughness: 0.25, emissive: 0x6b4a00 }),
    count: 7,
    scale: [0.8, 1.4],
    squash: 0.75,
  },
  8: {
    geometry: new THREE.DodecahedronGeometry(0.26, 1).translate(0, 0.14, 0),
    material: std(0xdcebf5, { roughness: 0.92, emissive: 0x162b3a }),
    count: 5,
    scale: [0.85, 1.35],
    squash: 0.55,
    y: -0.05,
    radius: [0.06, 0.46],
  },
};

function canvasTexture(draw: (ctx: CanvasRenderingContext2D, size: number) => void, size = 128) {
  const c = document.createElement('canvas');
  c.width = c.height = size;
  draw(c.getContext('2d')!, size);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}

function numberTexture(n: number, terrainCode?: number) {
  if (terrainCode === undefined) {
    return canvasTexture((ctx, size) => {
      const red = n === 6 || n === 8;
      ctx.fillStyle = red ? '#b8322a' : '#2b2b2b';
      ctx.font = `bold ${red ? 64 : 58}px system-ui, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(String(n), size / 2, size / 2 - 8);
      const pips = PIPS[n];
      for (let i = 0; i < pips; i++) {
        ctx.beginPath();
        ctx.arc(size / 2 + (i - (pips - 1) / 2) * 11, size / 2 + 34, 4, 0, Math.PI * 2);
        ctx.fill();
      }
    });
  }

  const tag = TERRAIN_SHORT_BADGE[terrainCode] ?? '';
  const accentNum =
    terrainCode >= 0 && terrainCode < RESOURCE_COLORS.length
      ? RESOURCE_COLORS[terrainCode]
      : terrainCode === GOLD_TERRAIN
        ? 0xf2b938
        : 0x3d5163;
  const accent = `#${accentNum.toString(16).padStart(6, '0')}`;

  return canvasTexture((ctx, size) => {
    const c = size / 2;
    const red = n === 6 || n === 8;

    if (tag) {
      roundedPill(ctx, c - 46, 22, 92, 44, 22);
      ctx.fillStyle = '#141c24';
      ctx.fill();
      ctx.lineWidth = 4;
      ctx.strokeStyle = accent;
      ctx.stroke();

      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillStyle = '#ffffff';
      ctx.font = '900 28px system-ui, -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif';
      ctx.fillText(tag, c, 45);
    }

    ctx.fillStyle = red ? '#b8322a' : '#1f2328';
    ctx.font = `900 ${red ? 92 : 84}px system-ui, -apple-system, sans-serif`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(String(n), c, 128);

    const pips = PIPS[n] ?? 0;
    for (let i = 0; i < pips; i++) {
      ctx.beginPath();
      ctx.arc(c + (i - (pips - 1) / 2) * 18, 194, 6.5, 0, Math.PI * 2);
      ctx.fill();
    }
  }, 256);
}

function seatEmblemTexture(owner: number) {
  const sym = SEAT_SYMBOLS[owner] ?? SEAT_SYMBOLS[owner % SEAT_SYMBOLS.length];
  const borderHex = `#${(PLAYER_COLORS[owner] ?? 0xffffff).toString(16).padStart(6, '0')}`;
  return canvasTexture((ctx, size) => {
    const c = size / 2;
    roundedPill(ctx, 10, 20, size - 20, size - 40, (size - 40) / 2);
    ctx.fillStyle = '#090f14';
    ctx.fill();

    roundedPill(ctx, 16, 26, size - 32, size - 52, (size - 52) / 2);
    ctx.fillStyle = '#121b24';
    ctx.fill();
    ctx.lineWidth = 9;
    ctx.strokeStyle = borderHex;
    ctx.stroke();

    ctx.fillStyle = '#ffffff';
    ctx.font = '900 48px "Segoe UI Symbol", "Apple Symbols", "Noto Sans Symbols 2", system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'alphabetic';
    const m = ctx.measureText(sym);
    const y = m.actualBoundingBoxAscent > 0 ? c + (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 2 : c + 17;
    ctx.fillText(sym, c, y);
  }, 128);
}

function roundedPill(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const rad = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rad, y);
  ctx.arcTo(x + w, y, x + w, y + h, rad);
  ctx.arcTo(x + w, y + h, x, y + h, rad);
  ctx.arcTo(x, y + h, x, y, rad);
  ctx.arcTo(x, y, x + w, y, rad);
  ctx.closePath();
}

function harborTexture(code: number) {
  return canvasTexture((ctx, size) => {
    const c = size / 2;
    const generic = code < 0 || code >= RESOURCE_COLORS.length;
    const accent = generic ? '#3d5163' : `#${RESOURCE_COLORS[code].toString(16).padStart(6, '0')}`;

    // High-contrast dark outer medallion rim + vibrant resource/slate ring + ivory face.
    ctx.beginPath();
    ctx.arc(c, c, 122, 0, Math.PI * 2);
    ctx.fillStyle = '#182028';
    ctx.fill();

    ctx.beginPath();
    ctx.arc(c, c, 111, 0, Math.PI * 2);
    ctx.fillStyle = accent;
    ctx.fill();

    ctx.beginPath();
    ctx.arc(c, c, 92, 0, Math.PI * 2);
    ctx.fillStyle = '#fbf4e2';
    ctx.fill();
    ctx.lineWidth = 3.5;
    ctx.strokeStyle = '#182028';
    ctx.stroke();

    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = '#182028';
    ctx.font = '900 70px system-ui, -apple-system, sans-serif';
    ctx.fillText(generic ? '3:1' : '2:1', c, 86);

    if (generic) {
      // Golden `?` coin motif for 3:1 generic harbors.
      ctx.beginPath();
      ctx.arc(c, 166, 33, 0, Math.PI * 2);
      ctx.fillStyle = '#f2b938';
      ctx.fill();
      ctx.lineWidth = 5;
      ctx.strokeStyle = '#7a4f08';
      ctx.stroke();

      ctx.beginPath();
      ctx.arc(c, 166, 25, 0, Math.PI * 2);
      ctx.lineWidth = 2;
      ctx.strokeStyle = 'rgba(255, 248, 220, 0.8)';
      ctx.stroke();

      ctx.fillStyle = '#3a2200';
      ctx.font = '900 44px system-ui, -apple-system, sans-serif';
      ctx.fillText('?', c, 168);
    } else {
      // Vibrant resource-colored pill with high-contrast label (`砖`/`木`/`羊`/`麦`/`矿`).
      roundedPill(ctx, c - 78, 130, 156, 66, 33);
      ctx.fillStyle = accent;
      ctx.fill();
      ctx.lineWidth = 5;
      ctx.strokeStyle = '#182028';
      ctx.stroke();

      const darkInk = code === 2 || code === 3;
      ctx.fillStyle = darkInk ? '#182028' : '#fffdf8';
      ctx.font = '900 46px system-ui, -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif';
      ctx.fillText(RESOURCE_LABELS[code], c, 164);
    }
  }, 256);
}

function fishingTexture(n: number) {
  return canvasTexture((ctx, size) => {
    const c = size / 2;
    const red = n === 6 || n === 8;
    const rimAccent = red ? '#e0483e' : '#35cfe0';
    const ink = red ? '#c92a2a' : '#152938';

    // Crisp pill badge with dark outer silhouette, turquoise/crimson accent rim, and ivory face.
    roundedPill(ctx, 12, 46, size - 24, 164, 82);
    ctx.fillStyle = '#13222e';
    ctx.fill();

    roundedPill(ctx, 21, 55, size - 42, 146, 73);
    ctx.fillStyle = rimAccent;
    ctx.fill();

    roundedPill(ctx, 31, 65, size - 62, 126, 63);
    ctx.fillStyle = '#fbf6e8';
    ctx.fill();
    ctx.lineWidth = 3;
    ctx.strokeStyle = '#13222e';
    ctx.stroke();

    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = ink;
    ctx.font = `900 ${red ? 62 : 58}px system-ui, -apple-system, "Segoe UI Emoji", "Apple Color Emoji", sans-serif`;
    ctx.fillText(`🐟 ${n}`, c, 118);

    const pips = PIPS[n] ?? 0;
    ctx.fillStyle = ink;
    for (let i = 0; i < pips; i++) {
      ctx.beginPath();
      ctx.arc(c + (i - (pips - 1) / 2) * 14, 166, 4.8, 0, Math.PI * 2);
      ctx.fill();
    }
  }, 256);
}

/** One placed piece; `born` drives the drop-in animation. */
interface Piece {
  x: number;
  z: number;
  angle: number;
  owner: number;
  born: number;
}

export class BoardRenderer {
  /** Called when the renderer switches backend (WebGPU device loss). */
  onBackendChange?: (backend: Backend) => void;
  /** Called when a highlighted spot is clicked or tapped. */
  onPick?: (kind: PickKind, id: number) => void;
  /** Called when the pointer hovers over a board tile (or `null` when leaving tiles / dragging). */
  onHoverTile?: (hover: TileHover | null) => void;

  private _backend: Backend = 'WebGL2';
  private _theme: BoardTheme = 'day';
  private _colorblind = isColorblindMode();
  private readonly unsubColorblind: () => void;
  private recovering = false;
  private readonly scene = new THREE.Scene();
  private readonly bgColor = new THREE.Color(SKY);
  private readonly fogObj = new THREE.Fog(SKY, 22, 60);
  private readonly camera = new THREE.PerspectiveCamera(38, 1, 0.1, 120);
  private readonly controls: OrbitControls;
  private readonly board = new THREE.Group();
  private readonly roads: THREE.InstancedMesh;
  private readonly bridges: THREE.InstancedMesh;
  private readonly camels: THREE.InstancedMesh;
  private readonly ships: THREE.InstancedMesh;
  private readonly pirate = new THREE.Group();
  private readonly barbarianShip = new THREE.Group();
  private readonly barbarianSailMat: THREE.MeshStandardMaterial;
  private readonly barbarianGlowMat: THREE.MeshStandardMaterial;
  private readonly barbarianRingMat: THREE.MeshBasicMaterial;
  private readonly barbarianRing: THREE.Mesh;
  private readonly settlements: THREE.InstancedMesh;
  private readonly cities: THREE.InstancedMesh;
  private readonly knights: THREE.InstancedMesh;
  private readonly knightBanners: THREE.InstancedMesh;
  private readonly metropolises: THREE.InstancedMesh;
  private readonly seatEmblems = new THREE.Group();
  private readonly seatEmblemMaterials = new Map<number, THREE.SpriteMaterial>();
  private readonly seatEmblemSprites: THREE.Sprite[] = [];
  private readonly markers: Record<PickKind, THREE.InstancedMesh>;
  private readonly robber = new THREE.Group();
  private readonly resizeObserver: ResizeObserver;
  private readonly beach: THREE.Mesh;
  private readonly seaMaterial: THREE.MeshStandardMaterial;
  private readonly hemiLight: THREE.HemisphereLight;
  private readonly sun: THREE.DirectionalLight;
  private readonly rimLight: THREE.DirectionalLight;
  private topology: Topology | null = null;
  private lastBoardData: BoardData | null = null;
  private singleAllLand = false;
  private tileXZ: [number, number][] = [];
  private vertexXZ: [number, number][] = [];
  private edgeXZ: { x: number; z: number; angle: number }[] = [];
  /** Unit vector pointing out to sea from each harbor slot. */
  private harborOut: [number, number][] = [];
  /** Tiles on either side of each edge. */
  private edgeTiles: number[][] = [];
  /** Per edge: unit direction towards open water (zero between two sea tiles). */
  private edgeSeaward: [number, number][] = [];
  /** Half extents (world units) of the area that must stay in view. */
  private extent = { x: 5.2, z: 4.42 };
  private readonly numberTextures = new Map<string, THREE.Texture>();
  private readonly harborTextures = new Map<number, THREE.Texture>();
  private readonly fishingTextures = new Map<number, THREE.Texture>();
  private readonly raycaster = new THREE.Raycaster();
  private readonly plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), -TOP);
  private readonly pointerNdc = new THREE.Vector2();
  private readonly hitPoint = new THREE.Vector3();
  private renderQueued = false;
  private disposables: { dispose(): void }[] = [];
  private readonly sceneDisposables: { dispose(): void }[] = [];

  // Pieces keyed by spot so new ones can be animated.
  private roadPieces = new Map<number, Piece>();
  private bridgePieces = new Map<number, Piece>();
  private camelPieces = new Map<number, Piece>();
  private shipPieces = new Map<number, Piece>();
  private pirateFrom: [number, number] | null = null;
  private pirateTo: [number, number] | null = null;
  private pirateStart = 0;
  private barbarianFrom: [number, number] | null = null;
  private barbarianTo: [number, number] | null = null;
  private barbarianYaw = -0.6;
  private barbarianStart = 0;
  private barbarianWarning = false;
  private barbarianPulseStart = -1e9;
  private buildingPieces = new Map<number, Piece & { city: boolean }>();
  private knightPieces = new Map<number, Piece & { level: number; active: boolean }>();
  private metropolisPieces: { x: number; z: number; angle: number; track: number }[] = [];
  private robberFrom: [number, number] | null = null;
  private robberTo: [number, number] = [0, 0];
  private robberStart = 0;

  private insets: Insets = { top: 0, right: 0, bottom: 0, left: 0 };
  /** Camera distance that frames the island; user zoom is kept relative to it. */
  private fitDistance = 0;
  private highlights: Highlights | null = null;
  private hovered = -1;
  private hoveredTile = -1;
  private dragging = false;
  private pointerDown: { x: number; y: number; t: number } | null = null;

  /** `forceWebGL` skips WebGPU (also available as `?renderer=webgl`). */
  static async create(container: HTMLElement, forceWebGL = false) {
    const renderer = createRenderer(forceWebGL);
    await renderer.init();
    return new BoardRenderer(container, renderer);
  }

  private constructor(
    private readonly container: HTMLElement,
    private renderer: THREE.WebGPURenderer,
  ) {
    this.attachRenderer(renderer);

    this.scene.background = this.bgColor;
    this.scene.fog = this.fogObj;

    // Bound to the container (not the canvas) so a renderer swap keeps input working.
    this.controls = new OrbitControls(this.camera, container);
    this.controls.enableDamping = true;
    this.controls.enablePan = false;
    this.controls.minPolarAngle = 0.15;
    this.controls.maxPolarAngle = 1.2;
    this.controls.addEventListener('change', () => this.requestRender());

    this.hemiLight = new THREE.HemisphereLight(0xdff3ff, 0x2d4a3a, 1.3);
    this.scene.add(this.hemiLight);
    const sun = new THREE.DirectionalLight(0xfff1d6, 2.4);
    sun.position.set(-6, 12, 5);
    sun.castShadow = true;
    sun.shadow.mapSize.set(2048, 2048);
    Object.assign(sun.shadow.camera, { left: -7, right: 7, top: 7, bottom: -7, near: 1, far: 30 });
    sun.shadow.bias = -0.0005;
    this.scene.add(sun);
    this.sun = sun;

    const rim = new THREE.DirectionalLight(0x5b8cff, 0);
    rim.position.set(7, 6, -6);
    rim.castShadow = false;
    this.scene.add(rim);
    this.rimLight = rim;

    const seaGeometry = new THREE.PlaneGeometry(120, 120).rotateX(-Math.PI / 2);
    const seaMaterial = std(SEA, { roughness: 0.55, flatShading: false });
    this.seaMaterial = seaMaterial;
    const sea = new THREE.Mesh(seaGeometry, seaMaterial);
    sea.position.y = -0.14;
    sea.receiveShadow = true;
    const beachMaterial = std(BEACH);
    this.beach = new THREE.Mesh(new THREE.BufferGeometry(), beachMaterial);
    this.beach.position.y = 0;
    this.beach.receiveShadow = true;
    this.scene.add(sea, this.beach, this.board);

    this.applyTheme(readSavedTheme());

    const pieceMaterial = std(0xffffff, { roughness: 0.6 });
    const camelMaterial = std(0xdba23a, { roughness: 0.58, metalness: 0.12, emissive: 0x3b2504 });
    const goldMaterial = std(0xffcf3a, { metalness: 0.5, roughness: 0.3, emissive: 0x5c3d00 });
    const metroMaterial = std(0xffffff, { metalness: 0.45, roughness: 0.3, emissive: 0x2b2000 });
    this.roads = this.instanced(roadGeometry, pieceMaterial, MAX_SPOTS, true);
    this.bridges = this.instanced(bridgeGeometry, pieceMaterial, MAX_SPOTS, true);
    this.camels = this.instanced(camelGeometry, camelMaterial, MAX_SPOTS);
    this.ships = this.instanced(shipGeometry, pieceMaterial, MAX_SPOTS, true);
    this.scene.add(this.ships);
    this.settlements = this.instanced(settlementGeometry, pieceMaterial, MAX_SPOTS, true);
    this.cities = this.instanced(cityGeometry, pieceMaterial, MAX_SPOTS, true);
    this.knights = this.instanced(knightGeometry, pieceMaterial, MAX_SPOTS, true);
    this.knightBanners = this.instanced(knightBannerGeometry, goldMaterial, MAX_SPOTS);
    this.metropolises = this.instanced(metropolisGeometry, metroMaterial, 6, true);
    this.seatEmblems.visible = this._colorblind;
    this.scene.add(
      this.roads,
      this.bridges,
      this.camels,
      this.settlements,
      this.cities,
      this.knights,
      this.knightBanners,
      this.metropolises,
      this.seatEmblems,
    );
    this.unsubColorblind = onColorblindChange((on) => this.applyColorblind(on));

    const markerMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.85, depthWrite: false });
    this.markers = {
      vertex: this.instanced(vertexMarkerGeometry, markerMaterial, MAX_SPOTS, true),
      edge: this.instanced(edgeMarkerGeometry, markerMaterial, MAX_SPOTS, true),
      tile: this.instanced(tileMarkerGeometry, markerMaterial, MAX_SPOTS, true),
    };
    for (const m of Object.values(this.markers)) {
      m.castShadow = m.receiveShadow = false;
      m.renderOrder = 10;
      this.scene.add(m);
    }

    const robberMaterial = std(0x2d2d33, { roughness: 0.5 });
    const body = new THREE.Mesh(new THREE.CylinderGeometry(0.08, 0.13, 0.3, 10).translate(0, 0.15, 0), robberMaterial);
    const head = new THREE.Mesh(new THREE.SphereGeometry(0.085, 12, 8).translate(0, 0.36, 0), robberMaterial);
    body.castShadow = head.castShadow = true;
    this.robber.add(body, head);
    this.scene.add(this.robber);

    const pirateHullMat = std(0x24242b, { roughness: 0.6 });
    const pirateSailMat = std(0x3b3b44, { roughness: 0.8 });
    const hull = new THREE.Mesh(pirateHull, pirateHullMat);
    const sails = new THREE.Mesh(pirateSails, pirateSailMat);
    hull.castShadow = sails.castShadow = true;
    this.pirate.add(hull, sails);
    this.pirate.visible = false;
    this.scene.add(this.pirate);

    const bHullMat = std(0x18161c, { roughness: 0.62 });
    this.barbarianSailMat = std(0x221c24, { roughness: 0.78 });
    this.barbarianGlowMat = std(0xc92a2a, { roughness: 0.35, emissive: 0x7a0c12, emissiveIntensity: 0.35 });
    this.barbarianRingMat = new THREE.MeshBasicMaterial({
      color: 0xe53935,
      transparent: true,
      opacity: 0.65,
      depthWrite: false,
    });
    const bHull = new THREE.Mesh(barbarianHull, bHullMat);
    const bSails = new THREE.Mesh(barbarianSails, this.barbarianSailMat);
    const bTrim = new THREE.Mesh(barbarianTrim, this.barbarianGlowMat);
    this.barbarianRing = new THREE.Mesh(barbarianRingGeometry, this.barbarianRingMat);
    this.barbarianRing.position.y = 0.02;
    this.barbarianRing.visible = false;
    bHull.castShadow = bSails.castShadow = bTrim.castShadow = true;
    this.barbarianShip.add(bHull, bSails, bTrim, this.barbarianRing);
    this.barbarianShip.visible = false;
    this.scene.add(this.barbarianShip);

    this.sceneDisposables.push(
      seaGeometry,
      seaMaterial,
      beachMaterial,
      pieceMaterial,
      camelMaterial,
      goldMaterial,
      metroMaterial,
      markerMaterial,
      robberMaterial,
      body.geometry,
      head.geometry,
      pirateHullMat,
      pirateSailMat,
      bHullMat,
      this.barbarianSailMat,
      this.barbarianGlowMat,
      this.barbarianRingMat,
      this.roads,
      this.bridges,
      this.camels,
      this.ships,
      this.settlements,
      this.cities,
      this.knights,
      this.knightBanners,
      this.metropolises,
      ...Object.values(this.markers),
    );

    container.addEventListener('pointermove', this.onPointerMove);
    container.addEventListener('pointerdown', this.onPointerDown);
    container.addEventListener('pointerup', this.onPointerUp);
    container.addEventListener('pointerleave', this.onPointerLeave);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
    this.resize();
  }

  get backend(): Backend {
    return this._backend;
  }

  get theme(): BoardTheme {
    return this._theme;
  }

  get colorblind(): boolean {
    return this._colorblind;
  }

  /** Switches the 3D board lighting & atmosphere theme, persists preference, and queues one frame. */
  setTheme(theme: BoardTheme): void {
    this.applyTheme(theme);
    saveTheme(theme);
    this.requestRender();
  }

  /** Enables or disables colorblind-friendly terrain tags & seat-symbol emblems, queuing one frame. */
  setColorblind(enabled: boolean): void {
    const next = Boolean(enabled);
    setColorblindMode(next);
    this.applyColorblind(next);
  }

  private applyColorblind(enabled: boolean): void {
    if (this._colorblind === enabled) return;
    this._colorblind = enabled;
    this.seatEmblems.visible = enabled;
    if (this.lastBoardData && this.topology) {
      this.updateBoard(this.lastBoardData);
    }
    this.updatePieces(performance.now());
    this.requestRender();
  }

  private applyTheme(theme: BoardTheme): void {
    this._theme = theme;
    const cfg = THEME_CONFIGS[theme];
    this.bgColor.setHex(cfg.sky);
    this.scene.background = this.bgColor;
    this.fogObj.color.setHex(cfg.fog);
    this.seaMaterial.color.setHex(cfg.sea);
    this.hemiLight.color.setHex(cfg.hemiSky);
    this.hemiLight.groundColor.setHex(cfg.hemiGround);
    this.hemiLight.intensity = cfg.hemiIntensity;
    this.sun.color.setHex(cfg.sunColor);
    this.sun.intensity = cfg.sunIntensity;
    this.sun.position.set(cfg.sunPos[0], cfg.sunPos[1], cfg.sunPos[2]);
    this.rimLight.color.setHex(cfg.rimColor);
    this.rimLight.intensity = cfg.rimIntensity;
    this.rimLight.position.set(cfg.rimPos[0], cfg.rimPos[1], cfg.rimPos[2]);
    this.updateFogBounds();
  }

  private updateFogBounds(): void {
    const cfg = THEME_CONFIGS[this._theme];
    const dist = this.fitDistance || 14;
    this.fogObj.near = Math.max(cfg.fogMinNear, dist * cfg.fogNearFactor);
    this.fogObj.far = Math.max(cfg.fogMinFar, dist * cfg.fogFarFactor);
  }

  /** Island geometry: spot positions, beach, framing and shadow bounds. */
  private setTopology(topology: Topology) {
    this.topology = topology;
    this.tileXZ = [];
    for (let i = 0; i < topology.tilePos.length; i += 2) this.tileXZ.push(lattice(topology.tilePos[i], topology.tilePos[i + 1]));
    this.vertexXZ = [];
    for (let i = 0; i < topology.vertexPos.length; i += 2) this.vertexXZ.push(lattice(topology.vertexPos[i], topology.vertexPos[i + 1]));
    this.edgeXZ = [];
    for (let e = 0; e < topology.edgeVertices.length / 2; e++) {
      const [ax, az] = this.vertexXZ[topology.edgeVertices[2 * e]];
      const [bx, bz] = this.vertexXZ[topology.edgeVertices[2 * e + 1]];
      this.edgeXZ.push({ x: (ax + bx) / 2, z: (az + bz) / 2, angle: -Math.atan2(bz - az, bx - ax) });
    }
    this.edgeTiles = this.edgeXZ.map(() => []);
    for (let t = 0; t < this.tileXZ.length; t++) {
      const corners = topology.tileVertices.subarray(t * 6, t * 6 + 6);
      for (let e = 0; e < this.edgeXZ.length; e++) {
        if (corners.includes(topology.edgeVertices[2 * e]) && corners.includes(topology.edgeVertices[2 * e + 1])) this.edgeTiles[e].push(t);
      }
    }
    const maxX = Math.max(...this.tileXZ.map(([x]) => Math.abs(x)));
    const maxZ = Math.max(...this.tileXZ.map(([, z]) => Math.abs(z)));
    this.extent = { x: maxX + MARGIN_X, z: maxZ + MARGIN_Z };
    const r = Math.max(maxX, maxZ) + 3.5;
    Object.assign(this.sun.shadow.camera, { left: -r, right: r, top: r, bottom: -r });
    this.sun.shadow.camera.updateProjectionMatrix();
    this.resize();
  }

  /**
   * Land-dependent geometry: one beach per single convex all-land island, or
   * per-tile hex banks whenever seafaring, multi-island, or custom coastlines
   * are present so concave bays and straits never get bridged by convex sand.
   */
  private setLand(terrain: number[], islands: number[]) {
    const isWater = (t: number | undefined) => t === undefined || terrain[t] === SEA_TERRAIN || terrain[t] === FOG_TERRAIN;
    const seafaring = terrain.some((t) => t === SEA_TERRAIN || t === FOG_TERRAIN);
    const islandIds = [...new Set(islands.filter((i) => i > 0))];
    const singleAllLand = !seafaring && islandIds.length === 1 && islands.length >= 3 && islands.every((id) => id === islandIds[0]);
    this.singleAllLand = singleAllLand;
    const banks: THREE.BufferGeometry[] = [];
    if (!singleAllLand) {
      const hexRing = (cx: number, cz: number, r: number): [number, number][] =>
        Array.from({ length: 6 }, (_, k) => {
          const a = -Math.PI / 2 + (k * Math.PI) / 3;
          return [cx + Math.cos(a) * r, cz + Math.sin(a) * r];
        });
      terrain.forEach((t, i) => {
        if (!isWater(i) && t >= 0) {
          const [cx, cz] = this.tileXZ[i];
          banks.push(bankGeometry(hexRing(cx, cz, 1.12), hexRing(cx, cz, 1.36), 0.24));
        }
      });
    } else {
      const hull = convexHull(this.tileXZ.filter((_, t) => islands[t] === islandIds[0]));
      if (hull.length >= 3) {
        banks.push(bankGeometry(offsetPolygon(hull, BEACH_TOP), offsetPolygon(hull, BEACH_FOOT), 0.24));
      }
    }
    this.beach.geometry.dispose();
    this.beach.geometry = banks.length > 1 ? mergeGeometries(banks)! : (banks[0] ?? new THREE.BufferGeometry());
    if (banks.length > 1) banks.forEach((b) => b.dispose());

    // Away from the land tile of an edge, towards the water.
    const outward = (e: number): [number, number] => {
      const landTile = this.edgeTiles[e].find((t) => !isWater(t));
      const seaTile = this.edgeTiles[e].find((t) => isWater(t));
      const { x, z } = this.edgeXZ[e];
      if (landTile === undefined) return [0, 0];
      const [cx, cz] = this.tileXZ[landTile];
      if (seaTile === undefined && this.edgeTiles[e].length > 1) return [0, 0];
      const len = Math.hypot(x - cx, z - cz);
      return len > 1e-6 ? [(x - cx) / len, (z - cz) / len] : [0, 0];
    };
    this.edgeSeaward = this.edgeXZ.map((_, e) => outward(e));
    this.harborOut = [...this.topology!.harborEdges].map((e) => this.edgeSeaward[e] ?? [0, 0]);
  }

  /** Draw calls in the last frame (for the HUD). */
  get drawCalls() {
    return this.renderer.info.render.drawCalls;
  }

  /** Rebuilds tiles, number tokens, decorations and harbors for a new map. */
  setBoard(data: BoardData, topology: Topology) {
    if (topology !== this.topology) this.setTopology(topology);
    this.camera.position.copy(CAMERA_DIR).multiplyScalar(this.fitDistance || 14);
    this.controls.update();
    this.roadPieces.clear();
    this.bridgePieces.clear();
    this.camelPieces.clear();
    this.shipPieces.clear();
    this.buildingPieces.clear();
    this.knightPieces.clear();
    this.metropolisPieces = [];
    for (const sp of this.seatEmblemSprites) sp.visible = false;
    this.robberFrom = null;
    this.pirateFrom = this.pirateTo = null;
    this.barbarianFrom = this.barbarianTo = null;
    this.barbarianWarning = false;
    this.barbarianPulseStart = -1e9;
    this.barbarianShip.visible = false;
    if (this.hoveredTile !== -1) {
      this.hoveredTile = -1;
      this.onHoverTile?.(null);
    }
    this.updateBoard(data);
  }

  /** Refreshes board tiles, fog, beaches and tokens in place without resetting the camera or pieces. */
  updateBoard({ terrain, numbers, harbors, islands, fishingGrounds, riverEdges }: BoardData) {
    this.lastBoardData = {
      terrain: [...terrain],
      numbers: [...numbers],
      harbors: [...harbors],
      islands: [...islands],
      fishingGrounds: fishingGrounds ? [...fishingGrounds] : undefined,
      riverEdges: riverEdges ? [...riverEdges] : undefined,
    };
    this.setLand(terrain, islands);
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
    this.board.clear();

    // Land tiles are raised hexes; sea tiles inside the map are open water,
    // marked by a faint ring so the sailing grid reads; fog tiles hold a
    // soft misty pad topped by clouds.
    const land = [...terrain.keys()].filter((i) => terrain[i] !== SEA_TERRAIN && terrain[i] !== FOG_TERRAIN);
    const sea = [...terrain.keys()].filter((i) => terrain[i] === SEA_TERRAIN);
    const fog = [...terrain.keys()].filter((i) => terrain[i] === FOG_TERRAIN);
    const tileMaterial = std(0xffffff);
    const tiles = this.instanced(hexGeometry, tileMaterial, Math.max(1, land.length), true);
    tiles.castShadow = false;
    const m = new THREE.Matrix4();
    const color = new THREE.Color();
    land.forEach((t, i) => {
      const [x, z] = this.tileXZ[t];
      tiles.setMatrixAt(i, m.makeTranslation(x, TILE_HEIGHT / 2, z));
      tiles.setColorAt(i, color.setHex(TERRAIN_COLORS[terrain[t]]));
    });
    this.commit(tiles, land.length);
    this.board.add(tiles);
    this.disposables.push(tileMaterial, tiles);
    if (sea.length) {
      const ringMaterial = new THREE.MeshBasicMaterial({ color: TERRAIN_COLORS[SEA_TERRAIN], transparent: true, opacity: 0.55, depthWrite: false });
      const rings = this.instanced(seaRingGeometry, ringMaterial, sea.length);
      rings.castShadow = rings.receiveShadow = false;
      sea.forEach((t, i) => rings.setMatrixAt(i, m.makeTranslation(this.tileXZ[t][0], -0.12, this.tileXZ[t][1])));
      this.commit(rings, sea.length);
      this.board.add(rings);
      this.disposables.push(ringMaterial, rings);
    }
    if (fog.length) {
      const fogMaterial = std(TERRAIN_COLORS[FOG_TERRAIN], { transparent: true, opacity: 0.55, roughness: 0.9 });
      const pads = this.instanced(fogPadGeometry, fogMaterial, fog.length);
      pads.castShadow = false;
      fog.forEach((t, i) => pads.setMatrixAt(i, m.makeTranslation(this.tileXZ[t][0], -0.09, this.tileXZ[t][1])));
      this.commit(pads, fog.length);
      this.board.add(pads);
      this.disposables.push(fogMaterial, pads);
    }

    this.addDecorations(terrain);
    if (riverEdges?.length) this.addGoldenRiver(riverEdges, terrain);
    this.addTokens(numbers, terrain);
    this.addHarbors(harbors);
    if (fishingGrounds?.length) this.addFishingGrounds(fishingGrounds);
    this.requestRender();
  }

  /**
   * Updates roads, buildings and the robber. Pieces that were not there
   * before drop in; a moved robber walks over. Pass `animate: false` for the
   * first sync of a game.
   */
  setPieces(
    { buildings, roads, ships, robber, pirate, knights, metropolises, barbarianPos, barbarianSteps, camels, bridges }: PieceData,
    animate = true,
  ) {
    const now = animate ? performance.now() : -1e9;
    const nextKnights = new Map<number, Piece & { level: number; active: boolean }>();
    (knights ?? []).forEach((k) => {
      const prev = this.knightPieces.get(k.vertex);
      const [x, z] = this.vertexXZ[k.vertex] ?? [0, 0];
      const angle = -Math.atan2(z, x);
      const born = prev && prev.owner === k.owner && prev.level === k.level ? prev.born : now;
      nextKnights.set(k.vertex, { x, z, angle, owner: k.owner, level: k.level, active: k.active, born });
    });
    this.knightPieces = nextKnights;
    this.metropolisPieces = (metropolises ?? []).flatMap((v, track) => {
      if (v === null || v === undefined) return [];
      const [x, z] = this.vertexXZ[v] ?? [0, 0];
      return [{ x, z, angle: -Math.atan2(z, x), track }];
    });
    const nextShips = new Map<number, Piece>();
    (ships ?? []).forEach((owner, e) => {
      if (owner === EMPTY) return;
      const prev = this.shipPieces.get(e);
      const { x, z, angle } = this.edgeXZ[e];
      // Coastal ships sit off the beach, bow along the edge.
      const [ox, oz] = this.edgeSeaward[e] ?? [0, 0];
      const coastal = ox !== 0 || oz !== 0;
      const at = { x: x + ox * (coastal ? 0.42 : 0), z: z + oz * (coastal ? 0.42 : 0), angle };
      nextShips.set(e, { ...at, owner, born: prev?.owner === owner ? prev.born : now });
    });
    this.shipPieces = nextShips;

    const nextBridges = new Map<number, Piece>();
    (bridges ?? []).forEach((owner, e) => {
      if (owner === EMPTY || e < 0 || e >= this.edgeXZ.length) return;
      const prev = this.bridgePieces.get(e);
      const { x, z, angle } = this.edgeXZ[e];
      nextBridges.set(e, prev?.owner === owner ? prev : { x, z, angle, owner, born: now });
    });
    this.bridgePieces = nextBridges;

    const nextCamels = new Map<number, Piece>();
    const camelEdges = (camels ?? []).filter((e) => e >= 0 && e < this.edgeXZ.length);
    const seenEdgeCounts = new Map<number, number>();
    camelEdges.forEach((e, idx) => {
      const dup = seenEdgeCounts.get(e) ?? 0;
      seenEdgeCounts.set(e, dup + 1);
      const key = e * 16 + dup;
      const prev = this.camelPieces.get(key);
      const spot = this.edgeXZ[e];
      const tx = Math.cos(spot.angle);
      const tz = -Math.sin(spot.angle);
      const nx = Math.sin(spot.angle);
      const nz = Math.cos(spot.angle);
      const [ox, oz] = this.edgeSeaward[e] ?? [0, 0];
      const inlandSign = ox !== 0 || oz !== 0 ? (nx * ox + nz * oz > 0 ? -1 : 1) : 1;
      const side = dup % 2 === 0 ? inlandSign : -inlandSign;
      let angle = spot.angle;
      const neighbor = camelEdges[idx + 1] ?? camelEdges[idx - 1];
      if (neighbor !== undefined && neighbor !== e && this.edgeXZ[neighbor]) {
        const ns = this.edgeXZ[neighbor];
        const dirX = idx + 1 < camelEdges.length ? ns.x - spot.x : spot.x - ns.x;
        const dirZ = idx + 1 < camelEdges.length ? ns.z - spot.z : spot.z - ns.z;
        if (dirX * tx + dirZ * tz < 0) angle += Math.PI;
      }
      const x = spot.x + nx * side * 0.135;
      const z = spot.z + nz * side * 0.135;
      nextCamels.set(key, prev ? { ...prev, x, z, angle } : { x, z, angle, owner: 0, born: now });
    });
    this.camelPieces = nextCamels;

    if (pirate === undefined || pirate === null) {
      this.pirate.visible = false;
      this.pirateFrom = this.pirateTo = null;
    } else {
      const [px, pz] = this.tileXZ[pirate] ?? [0, 0];
      const to: [number, number] = [px, pz];
      const moved = this.pirateTo !== null && (to[0] !== this.pirateTo[0] || to[1] !== this.pirateTo[1]);
      if (animate && moved) {
        this.pirateFrom = [this.pirate.position.x, this.pirate.position.z];
        this.pirateStart = now;
      } else {
        this.pirateFrom = to;
        this.pirateStart = -1e9;
      }
      this.pirateTo = to;
      this.pirate.visible = true;
    }

    if (barbarianPos === undefined || barbarianPos === null) {
      this.barbarianShip.visible = false;
      this.barbarianFrom = this.barbarianTo = null;
      this.barbarianWarning = false;
    } else {
      const steps = barbarianSteps && barbarianSteps > 1 ? barbarianSteps : 7;
      const wp = this.barbarianWaypoint(barbarianPos, steps);
      const to: [number, number] = [wp.x, wp.z];
      const warning = barbarianPos >= steps - 2;
      const moved =
        this.barbarianTo !== null && (Math.abs(to[0] - this.barbarianTo[0]) > 1e-3 || Math.abs(to[1] - this.barbarianTo[1]) > 1e-3);
      if (animate && moved) {
        this.barbarianFrom = [this.barbarianShip.position.x, this.barbarianShip.position.z];
        this.barbarianStart = now;
      } else {
        this.barbarianFrom = to;
        this.barbarianStart = -1e9;
      }
      if (warning && (!this.barbarianWarning || moved)) {
        this.barbarianPulseStart = animate ? now : -1e9;
      }
      this.barbarianTo = to;
      this.barbarianYaw = wp.yaw;
      this.barbarianWarning = warning;
      this.barbarianShip.visible = true;
    }
    const nextRoads = new Map<number, Piece>();
    roads.forEach((owner, e) => {
      if (owner === EMPTY) return;
      const prev = this.roadPieces.get(e);
      const { x, z, angle } = this.edgeXZ[e];
      nextRoads.set(e, prev?.owner === owner ? prev : { x, z, angle, owner, born: now });
    });
    this.roadPieces = nextRoads;

    const nextBuildings = new Map<number, Piece & { city: boolean }>();
    buildings.forEach((b, v) => {
      if (b === EMPTY) return;
      const owner = b & ~CITY_FLAG;
      const city = (b & CITY_FLAG) !== 0;
      const prev = this.buildingPieces.get(v);
      const [x, z] = this.vertexXZ[v];
      // Face buildings towards the island centre.
      const angle = -Math.atan2(z, x);
      nextBuildings.set(v, prev && prev.owner === owner && prev.city === city ? prev : { x, z, angle, owner, city, born: now });
    });
    this.buildingPieces = nextBuildings;

    const [rx, rz] = this.tileXZ[robber] ?? [0, 0];
    const to: [number, number] = [rx + 0.42, rz + 0.12];
    if (animate && (to[0] !== this.robberTo[0] || to[1] !== this.robberTo[1]) && this.robberFrom !== null) {
      this.robberFrom = [this.robber.position.x, this.robber.position.z];
      this.robberStart = now;
    } else {
      this.robberFrom = [to[0], to[1]];
      this.robberStart = -1e9;
    }
    this.robberTo = to;
    this.updatePieces(performance.now());
    this.requestRender();
  }

  /** Keeps the island inside the screen area not covered by HUD panels. */
  setInsets(insets: Insets) {
    const same = (Object.keys(insets) as (keyof Insets)[]).every((k) => Math.abs(insets[k] - this.insets[k]) < 1);
    if (same) return;
    this.insets = { ...insets };
    this.resize();
  }

  /** Shows clickable markers on the given spots (or clears them). */
  setHighlights(h: Highlights | null) {
    this.highlights = h && h.ids.length ? h : null;
    this.hovered = -1;
    this.container.style.cursor = '';
    this.updateMarkers();
    this.requestRender();
  }

  /** Screen position (CSS pixels, relative to the container) of a spot. */
  project(kind: PickKind, id: number): { x: number; y: number } {
    const [x, z] =
      kind === 'tile' ? this.tileXZ[id] : kind === 'vertex' ? this.vertexXZ[id] : [this.edgeXZ[id].x, this.edgeXZ[id].z];
    const v = new THREE.Vector3(x, TOP + 0.1, z).project(this.camera);
    return { x: ((v.x + 1) / 2) * this.container.clientWidth, y: ((1 - v.y) / 2) * this.container.clientHeight };
  }

  requestRender() {
    if (this.renderQueued) return;
    this.renderQueued = true;
    requestAnimationFrame((t) => {
      this.renderQueued = false;
      const animating = this.updatePieces(t);
      this.controls.update(); // emits 'change' while damping, which queues the next frame
      this.renderer.render(this.scene, this.camera);
      if (animating) this.requestRender();
    });
  }

  dispose() {
    this.unsubColorblind();
    this.container.removeEventListener('pointermove', this.onPointerMove);
    this.container.removeEventListener('pointerdown', this.onPointerDown);
    this.container.removeEventListener('pointerup', this.onPointerUp);
    this.container.removeEventListener('pointerleave', this.onPointerLeave);
    this.onPick = undefined;
    this.onHoverTile = undefined;
    this.resizeObserver.disconnect();
    this.controls.dispose();
    this.beach.geometry.dispose();
    for (const d of this.disposables) d.dispose();
    for (const d of this.sceneDisposables) d.dispose();
    for (const mat of this.seatEmblemMaterials.values()) {
      mat.map?.dispose();
      mat.dispose();
    }
    this.seatEmblemMaterials.clear();
    this.seatEmblemSprites.length = 0;
    this.seatEmblems.clear();
    for (const t of this.numberTextures.values()) t.dispose();
    for (const t of this.harborTextures.values()) t.dispose();
    for (const t of this.fishingTextures.values()) t.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
  }

  // ---- input ----

  private intersectBoard(ev: PointerEvent): THREE.Vector3 | null {
    const rect = this.container.getBoundingClientRect();
    if (!rect.width || !rect.height) return null;
    this.pointerNdc.set(((ev.clientX - rect.left) / rect.width) * 2 - 1, -((ev.clientY - rect.top) / rect.height) * 2 + 1);
    this.raycaster.setFromCamera(this.pointerNdc, this.camera);
    return this.raycaster.ray.intersectPlane(this.plane, this.hitPoint);
  }

  private tileAt(hit: THREE.Vector3 | null): number {
    if (!hit) return -1;
    let best = -1;
    let bestDist = TILE_HOVER_RADIUS;
    for (let t = 0; t < this.tileXZ.length; t++) {
      const [x, z] = this.tileXZ[t];
      const d = Math.hypot(hit.x - x, hit.z - z);
      if (d < bestDist) {
        bestDist = d;
        best = t;
      }
    }
    return best;
  }

  private spotAt(hit: THREE.Vector3 | null): number {
    const h = this.highlights;
    if (!h || !hit) return -1;
    let best = -1;
    let bestDist = PICK_RADIUS[h.kind];
    for (const id of h.ids) {
      const [x, z] = h.kind === 'tile' ? this.tileXZ[id] : h.kind === 'vertex' ? this.vertexXZ[id] : [this.edgeXZ[id].x, this.edgeXZ[id].z];
      const d = Math.hypot(hit.x - x, hit.z - z);
      if (d < bestDist) {
        bestDist = d;
        best = id;
      }
    }
    return best;
  }

  private onPointerMove = (ev: PointerEvent) => {
    if (ev.pointerType === 'touch') return;
    if (this.pointerDown && !this.dragging && Math.hypot(ev.clientX - this.pointerDown.x, ev.clientY - this.pointerDown.y) > 6) {
      this.dragging = true;
    }
    if (this.dragging || (ev.buttons !== 0 && !this.pointerDown)) {
      if (this.hoveredTile !== -1) {
        this.hoveredTile = -1;
        this.onHoverTile?.(null);
      }
      return;
    }
    const hit = this.highlights || this.onHoverTile ? this.intersectBoard(ev) : null;
    if (this.onHoverTile) {
      const tile = this.tileAt(hit);
      if (tile >= 0) {
        this.hoveredTile = tile;
        this.onHoverTile({ tile, clientX: ev.clientX, clientY: ev.clientY });
      } else if (this.hoveredTile !== -1) {
        this.hoveredTile = -1;
        this.onHoverTile(null);
      }
    }
    if (!this.highlights) return;
    const id = this.spotAt(hit);
    if (id !== this.hovered) {
      this.hovered = id;
      this.container.style.cursor = id >= 0 ? 'pointer' : '';
      this.updateMarkers();
      this.requestRender();
    }
  };

  private onPointerDown = (ev: PointerEvent) => {
    this.pointerDown = { x: ev.clientX, y: ev.clientY, t: performance.now() };
    this.dragging = false;
  };

  private onPointerUp = (ev: PointerEvent) => {
    const down = this.pointerDown;
    this.pointerDown = null;
    this.dragging = false;
    if (!down || !this.highlights) return;
    // A drag rotates the camera; only short, still presses count as clicks.
    if (Math.hypot(ev.clientX - down.x, ev.clientY - down.y) > 8 || performance.now() - down.t > 600) return;
    const id = this.spotAt(this.intersectBoard(ev));
    if (id >= 0) this.onPick?.(this.highlights.kind, id);
  };

  private onPointerLeave = () => {
    this.dragging = false;
    if (this.hoveredTile !== -1) {
      this.hoveredTile = -1;
      this.onHoverTile?.(null);
    }
    if (this.hovered !== -1) {
      this.hovered = -1;
      this.container.style.cursor = '';
      this.updateMarkers();
      this.requestRender();
    }
  };

  // ---- internals ----

  private attachRenderer(renderer: THREE.WebGPURenderer) {
    const backend = renderer.backend as { isWebGPUBackend?: boolean; device?: DeviceLike };
    this._backend = backend.isWebGPUBackend ? 'WebGPU' : 'WebGL2';
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.container.appendChild(renderer.domElement);
    // A WebGPU device can be lost at any time (driver reset, GPU process
    // crash, broken adapters). Rebuild on WebGL2 instead of showing nothing.
    backend.device?.lost.then((info) => {
      if (info.reason !== 'destroyed' && renderer === this.renderer) void this.fallBackToWebGL(info.message);
    });
  }

  private async fallBackToWebGL(reason: string) {
    if (this.recovering) return;
    this.recovering = true;
    console.warn(`WebGPU device lost (${reason}); switching to WebGL2.`);
    const old = this.renderer;
    const next = createRenderer(true);
    await next.init();
    old.domElement.remove();
    try {
      old.dispose();
    } catch {
      // The lost device may refuse to release resources; nothing to do.
    }
    this.renderer = next;
    this.attachRenderer(next);
    this.recovering = false;
    this.onBackendChange?.(this._backend);
    this.resize();
  }

  /** Writes piece matrices for time `now`; returns true while animating. */
  private updatePieces(now: number): boolean {
    let animating = false;
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const color = new THREE.Color();

    const place = (mesh: THREE.InstancedMesh, i: number, piece: Piece, y: number) => {
      const t = Math.min(1, (now - piece.born) / DROP_MS);
      if (t < 1) animating = true;
      const k = Math.max(0.001, easeOutBack(t));
      q.setFromAxisAngle(up, piece.angle);
      s.set(k, k, k);
      p.set(piece.x, y + (1 - Math.min(1, t * 1.4)) * 1.2, piece.z);
      mesh.setMatrixAt(i, m.compose(p, q, s));
      mesh.setColorAt(i, color.setHex(PLAYER_COLORS[piece.owner]));
    };

    let n = 0;
    for (const piece of this.roadPieces.values()) place(this.roads, n++, piece, TOP + 0.045);
    this.commit(this.roads, n);
    let nbr = 0;
    for (const piece of this.bridgePieces.values()) place(this.bridges, nbr++, piece, TOP);
    this.commit(this.bridges, nbr);
    let ncm = 0;
    for (const piece of this.camelPieces.values()) {
      const tc = Math.min(1, (now - piece.born) / DROP_MS);
      if (tc < 1) animating = true;
      const kc = Math.max(0.001, easeOutBack(tc));
      q.setFromAxisAngle(up, piece.angle);
      s.set(kc, kc, kc);
      p.set(piece.x, TOP + (1 - Math.min(1, tc * 1.4)) * 1.1, piece.z);
      this.camels.setMatrixAt(ncm++, m.compose(p, q, s));
    }
    this.commit(this.camels, ncm);
    let nsh = 0;
    for (const piece of this.shipPieces.values()) place(this.ships, nsh++, piece, WATER);
    this.commit(this.ships, nsh);
    if (this.pirateTo) {
      const tp = Math.min(1, (now - this.pirateStart) / ROBBER_MS);
      const from = this.pirateFrom ?? this.pirateTo;
      const ep = easeInOut(tp);
      this.pirate.position.set(
        from[0] + (this.pirateTo[0] - from[0]) * ep,
        WATER + Math.sin(Math.PI * tp) * 0.25,
        from[1] + (this.pirateTo[1] - from[1]) * ep,
      );
      this.pirate.rotation.y = 0.6;
      if (tp < 1) animating = true;
    }
    if (this.barbarianTo) {
      const tb = Math.min(1, (now - this.barbarianStart) / ROBBER_MS);
      const from = this.barbarianFrom ?? this.barbarianTo;
      const eb = easeInOut(tb);
      this.barbarianShip.position.set(
        from[0] + (this.barbarianTo[0] - from[0]) * eb,
        WATER + Math.sin(Math.PI * tb) * 0.18,
        from[1] + (this.barbarianTo[1] - from[1]) * eb,
      );
      this.barbarianShip.rotation.y = this.barbarianYaw;
      if (tb < 1) animating = true;

      if (this.barbarianWarning) {
        const elapsed = now - this.barbarianPulseStart;
        const pulsing = elapsed >= 0 && elapsed < BARBARIAN_PULSE_MS;
        if (pulsing) animating = true;
        const wave = pulsing ? 0.5 + 0.5 * Math.sin((elapsed / BARBARIAN_PULSE_MS) * Math.PI * 6 - Math.PI / 2) : 0.75;
        this.barbarianSailMat.color.setHex(0x3b1218);
        this.barbarianSailMat.emissive.setHex(0x8f141c);
        this.barbarianSailMat.emissiveIntensity = 0.45 + 0.55 * wave;
        this.barbarianGlowMat.emissiveIntensity = 0.8 + 0.7 * wave;
        this.barbarianRing.visible = true;
        const rs = 0.94 + 0.22 * wave;
        this.barbarianRing.scale.set(rs, 1, rs);
        this.barbarianRingMat.opacity = 0.38 + 0.44 * wave;
      } else {
        this.barbarianSailMat.color.setHex(0x221c24);
        this.barbarianSailMat.emissive.setHex(0x000000);
        this.barbarianSailMat.emissiveIntensity = 0;
        this.barbarianGlowMat.emissiveIntensity = 0.35;
        this.barbarianRing.visible = false;
      }
    }
    let ns = 0;
    let nc = 0;
    for (const piece of this.buildingPieces.values()) {
      if (piece.city) place(this.cities, nc++, piece, TOP);
      else place(this.settlements, ns++, piece, TOP);
    }
    this.commit(this.settlements, ns);
    this.commit(this.cities, nc);

    let nk = 0;
    let nb = 0;
    for (const k of this.knightPieces.values()) {
      const tk = Math.min(1, (now - k.born) / DROP_MS);
      if (tk < 1) animating = true;
      const rankScale = 0.88 + 0.16 * k.level;
      const sc = Math.max(0.001, easeOutBack(tk)) * rankScale;
      q.setFromAxisAngle(up, k.angle);
      s.set(sc, sc, sc);
      p.set(k.x, TOP + (1 - Math.min(1, tk * 1.4)) * 1.1, k.z);
      this.knights.setMatrixAt(nk, m.compose(p, q, s));
      this.knights.setColorAt(nk, color.setHex(PLAYER_COLORS[k.owner]));
      nk++;
      if (k.active) {
        this.knightBanners.setMatrixAt(nb++, m.compose(p, q, s));
      }
    }
    this.commit(this.knights, nk);
    this.commit(this.knightBanners, nb);

    let nm = 0;
    for (const metro of this.metropolisPieces) {
      q.setFromAxisAngle(up, metro.angle);
      s.set(1, 1, 1);
      p.set(metro.x, TOP, metro.z);
      this.metropolises.setMatrixAt(nm, m.compose(p, q, s));
      this.metropolises.setColorAt(nm, color.setHex(METROPOLIS_COLORS[metro.track] ?? 0xffca3a));
      nm++;
    }
    this.commit(this.metropolises, nm);

    if (!this._colorblind) {
      this.seatEmblems.visible = false;
    } else {
      this.seatEmblems.visible = true;
      let ei = 0;
      const placeEmblem = (owner: number, x: number, y: number, z: number, scale: number) => {
        let sp = this.seatEmblemSprites[ei];
        const mat = this.getSeatEmblemMaterial(owner);
        if (!sp) {
          sp = new THREE.Sprite(mat);
          sp.renderOrder = 12;
          this.seatEmblemSprites.push(sp);
          this.seatEmblems.add(sp);
        } else if (sp.material !== mat) {
          sp.material = mat;
        }
        sp.position.set(x, y, z);
        sp.scale.set(scale, scale, 1);
        sp.visible = true;
        ei++;
      };

      for (const piece of this.buildingPieces.values()) {
        const tb = Math.min(1, (now - piece.born) / DROP_MS);
        const kb = Math.max(0.001, easeOutBack(tb));
        const dropY = (1 - Math.min(1, tb * 1.4)) * 1.2;
        const hasMetro =
          piece.city &&
          this.metropolisPieces.some((mp) => Math.abs(mp.x - piece.x) < 1e-3 && Math.abs(mp.z - piece.z) < 1e-3);
        const baseOffset = hasMetro ? 0.92 : piece.city ? 0.65 : 0.5;
        placeEmblem(piece.owner, piece.x, TOP + baseOffset + dropY, piece.z, 0.34 * kb);
      }

      for (const k of this.knightPieces.values()) {
        const tk = Math.min(1, (now - k.born) / DROP_MS);
        const kk = Math.max(0.001, easeOutBack(tk));
        const rankScale = 0.88 + 0.16 * k.level;
        const dropY = (1 - Math.min(1, tk * 1.4)) * 1.1;
        const topH = (k.active ? 0.62 : 0.48) * rankScale + 0.15;
        placeEmblem(k.owner, k.x, TOP + topH + dropY, k.z, 0.32 * kk);
      }

      for (let j = ei; j < this.seatEmblemSprites.length; j++) {
        this.seatEmblemSprites[j].visible = false;
      }
    }

    const t = Math.min(1, (now - this.robberStart) / ROBBER_MS);
    const from = this.robberFrom ?? this.robberTo;
    const e = easeInOut(t);
    this.robber.position.set(
      from[0] + (this.robberTo[0] - from[0]) * e,
      TOP + Math.sin(Math.PI * t) * 0.9,
      from[1] + (this.robberTo[1] - from[1]) * e,
    );
    if (t < 1) animating = true;
    return animating;
  }

  private updateMarkers() {
    for (const mesh of Object.values(this.markers)) mesh.count = 0;
    const h = this.highlights;
    if (!h) return;
    const mesh = this.markers[h.kind];
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const color = new THREE.Color();
    h.ids.forEach((id, i) => {
      const hover = id === this.hovered;
      const k = hover ? 1.35 : 1;
      if (h.kind === 'vertex') {
        const [x, z] = this.vertexXZ[id];
        p.set(x, TOP + 0.03, z);
        q.identity();
      } else if (h.kind === 'edge') {
        const { x, z, angle } = this.edgeXZ[id];
        p.set(x, TOP + 0.03, z);
        q.setFromAxisAngle(up, angle);
      } else {
        const [x, z] = this.tileXZ[id];
        p.set(x, TOP + 0.02, z);
        q.identity();
      }
      s.set(k, 1, k);
      mesh.setMatrixAt(i, m.compose(p, q, s));
      mesh.setColorAt(i, color.setHex(hover ? ACCENT : 0xffffff));
    });
    this.commit(mesh, h.ids.length);
  }

  /**
   * `colored` allocates per-instance colors up front: the WebGPU renderer
   * builds the material program on first use, so the attribute has to exist
   * before the first frame for colors set later to show up.
   */
  private instanced(geometry: THREE.BufferGeometry, material: THREE.Material, capacity: number, colored = false) {
    const mesh = new THREE.InstancedMesh(geometry, material, capacity);
    if (colored) mesh.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(capacity * 3).fill(1), 3);
    mesh.count = 0;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.frustumCulled = false;
    return mesh;
  }

  private commit(mesh: THREE.InstancedMesh, count: number) {
    mesh.count = count;
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  }

  private addDecorations(terrain: number[]) {
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);

    for (const [code, deco] of Object.entries(DECORATIONS)) {
      const tilesOfType = [...terrain.keys()].filter((i) => terrain[i] === Number(code));
      const capacity = tilesOfType.length * deco.count;
      if (capacity === 0) continue;
      const mesh = this.instanced(deco.geometry, deco.material, capacity);
      const cap = deco.cap && this.instanced(deco.cap.geometry, deco.cap.material, capacity);
      let n = 0;
      for (const t of tilesOfType) {
        const rand = prng(t * 7919 + Number(code) * 104729);
        const [cx, cz] = this.tileXZ[t];
        const offset = rand() * Math.PI * 2;
        const [rLo, rHi] = deco.radius ?? [0.42, 0.64];
        const baseY = deco.y ?? TOP;
        for (let k = 0; k < deco.count; k++) {
          const angle = offset + (k / deco.count) * Math.PI * 2 + (rand() - 0.5) * 0.6;
          const r = k === 0 && deco.radius ? rLo * 0.5 : rLo + rand() * (rHi - rLo);
          const sc = deco.scale[0] + rand() * (deco.scale[1] - deco.scale[0]);
          q.setFromAxisAngle(up, rand() * Math.PI * 2);
          s.set(sc, sc * (deco.squash ?? 1), sc);
          m.compose(p.set(cx + Math.cos(angle) * r, baseY, cz + Math.sin(angle) * r), q, s);
          mesh.setMatrixAt(n, m);
          cap?.setMatrixAt(n, m);
          n++;
        }
      }
      this.commit(mesh, n);
      this.board.add(mesh);
      this.disposables.push(mesh);
      if (cap) {
        this.commit(cap, n);
        this.board.add(cap);
        this.disposables.push(cap);
      }
    }
  }

  private getSeatEmblemMaterial(owner: number): THREE.SpriteMaterial {
    const idx = ((owner % PLAYER_COLORS.length) + PLAYER_COLORS.length) % PLAYER_COLORS.length;
    let mat = this.seatEmblemMaterials.get(idx);
    if (!mat) {
      const tex = seatEmblemTexture(idx);
      mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: true, depthWrite: false });
      this.seatEmblemMaterials.set(idx, mat);
    }
    return mat;
  }

  /** Renders the Caravans mode Golden River (winding turquoise ribbon + golden shimmer banks) & Desert oasis. */
  private addGoldenRiver(riverEdges: number[], terrain: number[]) {
    const valid = riverEdges.filter((e) => e >= 0 && e < this.edgeXZ.length);
    if (!valid.length || !this.topology) return;

    const bankMaterial = std(0xf2c64d, {
      metalness: 0.42,
      roughness: 0.34,
      emissive: 0x5c3e06,
    });
    const waterMaterial = std(0x1cc7d8, {
      metalness: 0.28,
      roughness: 0.2,
      emissive: 0x0b5260,
    });

    const banks = this.instanced(riverBankGeometry, bankMaterial, valid.length);
    const ribbons = this.instanced(riverRibbonGeometry, waterMaterial, valid.length);
    banks.castShadow = false;
    ribbons.castShadow = false;

    const vertexSet = new Set<number>();
    const ev = this.topology.edgeVertices;
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3(1, 1, 1);
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);

    valid.forEach((e, i) => {
      const spot = this.edgeXZ[e];
      q.setFromAxisAngle(up, spot.angle);
      banks.setMatrixAt(i, m.compose(p.set(spot.x, TOP - 0.004, spot.z), q, s));
      ribbons.setMatrixAt(i, m.compose(p.set(spot.x, TOP + 0.012, spot.z), q, s));
      if (2 * e + 1 < ev.length) {
        vertexSet.add(ev[2 * e]);
        vertexSet.add(ev[2 * e + 1]);
      }
    });

    this.commit(banks, valid.length);
    this.commit(ribbons, valid.length);
    this.board.add(banks, ribbons);
    this.disposables.push(bankMaterial, waterMaterial, banks, ribbons);

    const bendVertices = [...vertexSet].filter((v) => v >= 0 && v < this.vertexXZ.length);
    if (bendVertices.length) {
      const bendBanks = this.instanced(riverBendBankGeometry, bankMaterial, bendVertices.length);
      const bendWaters = this.instanced(riverBendWaterGeometry, waterMaterial, bendVertices.length);
      bendBanks.castShadow = false;
      bendWaters.castShadow = false;
      bendVertices.forEach((v, i) => {
        const [vx, vz] = this.vertexXZ[v];
        bendBanks.setMatrixAt(i, m.makeTranslation(vx, TOP - 0.004, vz));
        bendWaters.setMatrixAt(i, m.makeTranslation(vx, TOP + 0.012, vz));
      });
      this.commit(bendBanks, bendVertices.length);
      this.commit(bendWaters, bendVertices.length);
      this.board.add(bendBanks, bendWaters);
      this.disposables.push(bendBanks, bendWaters);
    }

    const deserts = [...terrain.keys()].filter((t) => terrain[t] === DESERT_TERRAIN);
    if (deserts.length) {
      const trunkMat = std(0x7a4e2b, { roughness: 0.82 });
      const crownMat = std(0x2a8a46, { roughness: 0.68 });
      const oasisBanks = this.instanced(oasisBankGeometry, bankMaterial, deserts.length);
      const oasisPools = this.instanced(oasisPoolGeometry, waterMaterial, deserts.length);
      const palmsPerTile = 3;
      const trunks = this.instanced(oasisTrunkGeometry, trunkMat, deserts.length * palmsPerTile);
      const crowns = this.instanced(oasisCrownGeometry, crownMat, deserts.length * palmsPerTile);
      oasisBanks.castShadow = false;
      oasisPools.castShadow = false;

      const palmAngles = [2.15, 3.55, -1.35];
      let nPalms = 0;
      deserts.forEach((t, i) => {
        const [cx, cz] = this.tileXZ[t];
        const ox = cx - 0.04;
        const oz = cz - 0.02;
        oasisBanks.setMatrixAt(i, m.makeTranslation(ox, TOP + 0.004, oz));
        oasisPools.setMatrixAt(i, m.makeTranslation(ox, TOP + 0.012, oz));

        palmAngles.forEach((ang, k) => {
          const r = 0.27 + (k % 2) * 0.03;
          const px = ox + Math.cos(ang) * r;
          const pz = oz + Math.sin(ang) * r;
          const sc = 0.92 + k * 0.08;
          q.setFromAxisAngle(up, -ang);
          s.set(sc, sc, sc);
          m.compose(p.set(px, TOP + 0.006, pz), q, s);
          trunks.setMatrixAt(nPalms, m);
          crowns.setMatrixAt(nPalms, m);
          nPalms++;
        });
      });

      this.commit(oasisBanks, deserts.length);
      this.commit(oasisPools, deserts.length);
      this.commit(trunks, nPalms);
      this.commit(crowns, nPalms);
      this.board.add(oasisBanks, oasisPools, trunks, crowns);
      this.disposables.push(trunkMat, crownMat, oasisBanks, oasisPools, trunks, crowns);
    }
  }

  private addTokens(numbers: number[], terrain: number[]) {
    const discMaterial = std(0xf3ead2, { roughness: 0.7 });
    this.disposables.push(discMaterial);
    numbers.forEach((n, i) => {
      if (n === 0) return;
      const [x, z] = this.tileXZ[i];
      const disc = new THREE.Mesh(tokenGeometry, discMaterial);
      disc.position.set(x, TOP + 0.025, z);
      disc.castShadow = true;
      const tCode = this._colorblind ? terrain[i] : undefined;
      const key = tCode === undefined ? String(n) : `cb:${n}:${tCode}`;
      let tex = this.numberTextures.get(key);
      if (!tex) this.numberTextures.set(key, (tex = numberTexture(n, tCode)));
      const labelMaterial = new THREE.MeshStandardMaterial({ map: tex, transparent: true, roughness: 0.7 });
      const label = new THREE.Mesh(labelGeometry, labelMaterial);
      label.position.set(x, TOP + 0.052, z);
      this.board.add(disc, label);
      this.disposables.push(labelMaterial);
    });
  }

  /** Sea approach waypoint from the northwest horizon (`pos = 0`) to right off the shore (`pos = steps - 1`). */
  private barbarianWaypoint(pos: number, steps: number): { x: number; z: number; yaw: number } {
    const u = Math.min(1, Math.max(0, pos) / Math.max(1, steps - 1));
    const dirNear = [-0.84, -0.5426] as const;
    const dirFar = [-0.8, -0.6] as const;
    const coastProj = this.tileXZ.reduce((mx, [tx, tz]) => Math.max(mx, tx * dirNear[0] + tz * dirNear[1]), 2.8);
    const farX = dirFar[0] * (coastProj + 4.15);
    const farZ = dirFar[1] * (coastProj + 4.15);
    const nearX = dirNear[0] * (coastProj + 2.22);
    const nearZ = dirNear[1] * (coastProj + 2.22);
    const x = farX + (nearX - farX) * u;
    const z = farZ + (nearZ - farZ) * u;
    return { x, z, yaw: -Math.atan2(-z, -x) };
  }

  private addHarbors(harbors: number[]) {
    if (!harbors.length || !this.topology) return;
    const plankMaterial = std(0x8a5a35);
    const planks = this.instanced(new THREE.BoxGeometry(1, 0.055, 0.11), plankMaterial, Math.max(1, harbors.length * 2));
    const docks = this.instanced(new THREE.BoxGeometry(0.38, 0.09, 0.38), plankMaterial, Math.max(1, harbors.length));
    this.disposables.push(plankMaterial, planks, docks, planks.geometry, docks.geometry);
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const topology = this.topology;
    const ev = topology.edgeVertices;

    let nDocks = 0;
    let nPlanks = 0;
    harbors.forEach((code, slot) => {
      if (slot >= topology.harborEdges.length) return;
      const e = topology.harborEdges[slot];
      if (2 * e + 1 >= ev.length) return;
      const [ox, oz] = this.harborOut[slot] ?? [0, 0];
      if (ox === 0 && oz === 0) return;
      const a = this.vertexXZ[ev[2 * e]];
      const b = this.vertexXZ[ev[2 * e + 1]];
      if (!a || !b) return;
      const mx = (a[0] + b[0]) / 2;
      const mz = (a[1] + b[1]) / 2;
      const dock: [number, number] = [mx + ox * 0.52, mz + oz * 0.52];
      docks.setMatrixAt(nDocks++, m.makeTranslation(dock[0], -0.015, dock[1]));
      for (const v of [a, b]) {
        const dx = dock[0] - v[0];
        const dz = dock[1] - v[1];
        q.setFromAxisAngle(up, -Math.atan2(dz, dx));
        s.set(Math.hypot(dx, dz), 1, 1);
        planks.setMatrixAt(nPlanks++, m.compose(p.set((v[0] + dock[0]) / 2, 0.025, (v[1] + dock[1]) / 2), q, s));
      }
      let tex = this.harborTextures.get(code);
      if (!tex) this.harborTextures.set(code, (tex = harborTexture(code)));
      const mat = new THREE.SpriteMaterial({ map: tex, depthTest: true });
      const label = new THREE.Sprite(mat);
      label.scale.set(0.68, 0.68, 1);
      label.position.set(dock[0], 0.48, dock[1]);
      this.board.add(label);
      this.disposables.push(mat);
    });
    this.commit(planks, nPlanks);
    this.commit(docks, nDocks);
    this.board.add(planks, docks);
  }

  /** Renders 3D coastal fishing shoals (shimmering ripples, leaping silver fish & `🐟 N` badge). */
  private addFishingGrounds(fishingGrounds: { edge: number; number: number }[]) {
    const valid = fishingGrounds.filter((fg) => fg.edge >= 0 && fg.edge < this.edgeXZ.length);
    if (!valid.length) return;

    const poolMaterial = new THREE.MeshStandardMaterial({
      color: 0x1fc4d6,
      emissive: 0x0a4b58,
      roughness: 0.25,
      metalness: 0.2,
      transparent: true,
      opacity: 0.7,
      depthWrite: false,
    });
    const rippleMaterial = new THREE.MeshStandardMaterial({
      color: 0xc2f9ff,
      emissive: 0x3baec2,
      roughness: 0.2,
      metalness: 0.55,
      transparent: true,
      opacity: 0.9,
      depthWrite: false,
    });
    const fishMaterial = std(0xdbe9f5, {
      metalness: 0.65,
      roughness: 0.22,
      emissive: 0x1e384a,
    });

    const pools = this.instanced(shoalPoolGeometry, poolMaterial, valid.length);
    const ripples = this.instanced(shoalRippleGeometry, rippleMaterial, valid.length);
    const fishes = this.instanced(leapingFishGeometry, fishMaterial, valid.length * 2);
    pools.castShadow = pools.receiveShadow = false;
    ripples.castShadow = ripples.receiveShadow = false;

    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const euler = new THREE.Euler(0, 0, 0, 'YXZ');
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();

    // On convex all-land boards, the coastal sand shelf sits at y = 0 out to 1.55;
    // on seafaring / per-hex bank boards, 0.62 seaward sits over open water at y = -0.14.
    const surfaceY = this.singleAllLand ? 0.014 : -0.115;
    let nShoals = 0;
    let nFish = 0;

    for (const fg of valid) {
      const spot = this.edgeXZ[fg.edge];
      if (!spot) continue;
      let [ox, oz] = this.edgeSeaward[fg.edge] ?? [0, 0];
      if (ox === 0 && oz === 0) {
        const d = Math.hypot(spot.x, spot.z);
        if (d > 1e-6) {
          ox = spot.x / d;
          oz = spot.z / d;
        }
      }

      const sx = spot.x + ox * 0.62;
      const sz = spot.z + oz * 0.62;

      pools.setMatrixAt(nShoals, m.makeTranslation(sx, surfaceY, sz));
      ripples.setMatrixAt(nShoals, m.makeTranslation(sx, surfaceY + 0.006, sz));
      nShoals++;

      // Tangent direction along the coastal edge so the two silver fish arc gracefully across the shoal.
      const tx = Math.cos(spot.angle);
      const tz = -Math.sin(spot.angle);

      // Fish 1: cresting forward arc.
      euler.set(0, spot.angle + 0.28, -0.34);
      q.setFromEuler(euler);
      s.set(1, 1, 1);
      p.set(sx - tx * 0.1 + ox * 0.04, surfaceY + 0.11, sz - tz * 0.1 + oz * 0.04);
      fishes.setMatrixAt(nFish++, m.compose(p, q, s));

      // Fish 2: companion fish arcing counter-parallel slightly further out.
      euler.set(0, spot.angle + Math.PI - 0.24, -0.42);
      q.setFromEuler(euler);
      s.set(0.86, 0.86, 0.86);
      p.set(sx + tx * 0.11 - ox * 0.05, surfaceY + 0.085, sz + tz * 0.11 - oz * 0.05);
      fishes.setMatrixAt(nFish++, m.compose(p, q, s));

      let tex = this.fishingTextures.get(fg.number);
      if (!tex) this.fishingTextures.set(fg.number, (tex = fishingTexture(fg.number)));
      const badgeMat = new THREE.SpriteMaterial({ map: tex, depthTest: true });
      const badge = new THREE.Sprite(badgeMat);
      badge.scale.set(0.68, 0.68, 1);
      badge.position.set(sx, surfaceY + 0.44, sz);
      this.board.add(badge);
      this.disposables.push(badgeMat);
    }

    this.commit(pools, nShoals);
    this.commit(ripples, nShoals);
    this.commit(fishes, nFish);
    this.board.add(pools, ripples, fishes);
    this.disposables.push(poolMaterial, rippleMaterial, fishMaterial, pools, ripples, fishes);
  }

  private resize() {
    const w = Math.max(1, this.container.clientWidth);
    const h = Math.max(1, this.container.clientHeight);
    this.renderer.setSize(w, h);
    this.camera.aspect = w / h;
    // Fit the island into the free area (at least 45% of the screen each way)
    // and shift the projection centre into the middle of that area.
    const { top, right, bottom, left } = this.insets;
    const safeW = Math.max(w * 0.45, w - left - right);
    const safeH = Math.max(h * 0.45, h - top - bottom);
    const tanV = Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2));
    const tanH = tanV * this.camera.aspect;
    const distance = Math.max(this.extent.x / (tanH * (safeW / w)), this.extent.z / (tanV * (safeH / h)));
    const shiftX = Math.min(left, w - safeW) + safeW / 2 - w / 2;
    const shiftY = Math.min(top, h - safeH) + safeH / 2 - h / 2;
    this.camera.setViewOffset(w, h, -shiftX, -shiftY, w, h);
    // Keep the user's viewing angle and relative zoom across refits.
    const current = this.camera.position.length();
    const zoom = this.fitDistance > 0 && current > 0 ? current / this.fitDistance : 1;
    const dir = current > 0 && this.fitDistance > 0 ? this.camera.position.clone().normalize() : CAMERA_DIR;
    this.fitDistance = distance;
    this.camera.position.copy(dir).multiplyScalar(distance * zoom);
    this.controls.minDistance = distance * 0.45;
    this.controls.maxDistance = distance * 1.6;
    this.updateFogBounds();
    this.camera.updateProjectionMatrix();
    this.controls.update();
    this.requestRender();
  }
}
