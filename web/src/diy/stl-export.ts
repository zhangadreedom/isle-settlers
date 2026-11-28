import type { Topology } from '../engine';
import type { Terrain } from '../generated/proto';
import type { DraftMap } from './custom-map';

export interface StlExportOptions {
  /** Outer hex side length in mm (official Catan = 45.6 mm). */
  hexSideMm: number;
  /** Base tile thickness in mm (default 6.0 mm). */
  baseHeightMm: number;
  /** Relief height above base in mm (default 3.2 mm). */
  reliefHeightMm: number;
  /** Include recessed side pockets (Ø5.2mm x 2.2mm) for 5x2mm cylindrical magnets. */
  magnetSlots: boolean;
  /** Include Ø26.0mm center recess for circular number token. */
  tokenRecess: boolean;
  /** Optional inter-tile clearance gap in mm for full-board prints (default 0.6 mm; 0 = fused diorama). */
  clearanceMm?: number;
}

export const DEFAULT_STL_OPTIONS: StlExportOptions = {
  hexSideMm: 45.6,
  baseHeightMm: 6.0,
  reliefHeightMm: 3.2,
  magnetSlots: true,
  tokenRecess: true,
};

type Vec3 = [number, number, number];

const SQRT3_2 = Math.sqrt(3) / 2;
const TAU = Math.PI * 2;
const REFERENCE_HEX_SIDE_MM = 45.6;

/** Accumulates oriented outward-facing triangles (9 floats per triangle). */
class TriangleMesh {
  readonly coords: number[] = [];

  get triangleCount(): number {
    return this.coords.length / 9;
  }

  tri(a: Vec3, b: Vec3, c: Vec3): void {
    const abx = b[0] - a[0];
    const aby = b[1] - a[1];
    const abz = b[2] - a[2];
    const acx = c[0] - a[0];
    const acy = c[1] - a[1];
    const acz = c[2] - a[2];
    const nx = aby * acz - abz * acy;
    const ny = abz * acx - abx * acz;
    const nz = abx * acy - aby * acx;
    if (nx * nx + ny * ny + nz * nz <= 1e-16) return;
    this.coords.push(a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]);
  }

  /** Emits two triangles for counter-clockwise quad `a -> b -> c -> d`. */
  quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3): void {
    this.tri(a, b, c);
    this.tri(a, c, d);
  }

  appendTranslated(other: TriangleMesh, dx: number, dy: number, dz: number): void {
    const src = other.coords;
    for (let i = 0; i < src.length; i += 3) {
      this.coords.push(src[i] + dx, src[i + 1] + dy, src[i + 2] + dz);
    }
  }
}

function resolveOptions(opts?: Partial<StlExportOptions>): Required<StlExportOptions> {
  return {
    hexSideMm: Math.max(10, opts?.hexSideMm ?? DEFAULT_STL_OPTIONS.hexSideMm),
    baseHeightMm: Math.max(2, opts?.baseHeightMm ?? DEFAULT_STL_OPTIONS.baseHeightMm),
    reliefHeightMm: Math.max(0.4, opts?.reliefHeightMm ?? DEFAULT_STL_OPTIONS.reliefHeightMm),
    magnetSlots: opts?.magnetSlots ?? DEFAULT_STL_OPTIONS.magnetSlots,
    tokenRecess: opts?.tokenRecess ?? DEFAULT_STL_OPTIONS.tokenRecess,
    clearanceMm: Math.max(0, opts?.clearanceMm ?? 0.6),
  };
}

/**
 * Packs a `TriangleMesh` into standard Binary STL format:
 * 80-byte ASCII header + uint32 little-endian triangle count + 50 bytes per triangle.
 */
function packBinaryStl(mesh: TriangleMesh, label: string): ArrayBuffer {
  const coords = mesh.coords;
  const numTriangles = Math.floor(coords.length / 9);
  const buffer = new ArrayBuffer(84 + numTriangles * 50);
  const bytes = new Uint8Array(buffer);
  const header = `Isle Settlers 1:1 Printable STL - ${label}`.slice(0, 79);
  for (let i = 0; i < header.length; i++) {
    bytes[i] = header.charCodeAt(i) & 0x7f;
  }

  const view = new DataView(buffer);
  view.setUint32(80, numTriangles, true);

  let offset = 84;
  for (let t = 0; t < numTriangles; t++) {
    const base = t * 9;
    const ax = coords[base];
    const ay = coords[base + 1];
    const az = coords[base + 2];
    const bx = coords[base + 3];
    const by = coords[base + 4];
    const bz = coords[base + 5];
    const cx = coords[base + 6];
    const cy = coords[base + 7];
    const cz = coords[base + 8];

    const abx = bx - ax;
    const aby = by - ay;
    const abz = bz - az;
    const acx = cx - ax;
    const acy = cy - ay;
    const acz = cz - az;

    let nx = aby * acz - abz * acy;
    let ny = abz * acx - abx * acz;
    let nz = abx * acy - aby * acx;
    const len = Math.hypot(nx, ny, nz) || 1;
    nx /= len;
    ny /= len;
    nz /= len;

    view.setFloat32(offset, nx, true);
    view.setFloat32(offset + 4, ny, true);
    view.setFloat32(offset + 8, nz, true);

    view.setFloat32(offset + 12, ax, true);
    view.setFloat32(offset + 16, ay, true);
    view.setFloat32(offset + 20, az, true);

    view.setFloat32(offset + 24, bx, true);
    view.setFloat32(offset + 28, by, true);
    view.setFloat32(offset + 32, bz, true);

    view.setFloat32(offset + 36, cx, true);
    view.setFloat32(offset + 40, cy, true);
    view.setFloat32(offset + 44, cz, true);

    view.setUint16(offset + 48, 0, true);
    offset += 50;
  }

  return buffer;
}

/**
 * Builds the watertight hexagonal base prism (circumradius = `hexSideMm`, height = `baseHeightMm`),
 * optionally carving a 12-sided Ø26.0mm × 1.5mm top number-token recess and 6 octagonal
 * 5.4mm × 2.4mm × 2.2mm side magnet pockets with zero T-junctions.
 */
function addHexBase(
  mesh: TriangleMesh,
  R: number,
  H: number,
  hasRecess: boolean,
  magnetSlots: boolean,
): void {
  // Pointy-top hex vertices going counter-clockwise when viewed from +Z:
  // k=0 at +90° (top corner), k=1 at +150°, ..., k=5 at +30°.
  const vx: number[] = [];
  const vy: number[] = [];
  for (let k = 0; k < 6; k++) {
    const a = Math.PI / 2 + (k * Math.PI) / 3;
    vx.push(R * Math.cos(a));
    vy.push(R * Math.sin(a));
  }

  const botVerts: Vec3[] = vx.map((x, k) => [x, vy[k], 0]);
  const topVerts: Vec3[] = vx.map((x, k) => [x, vy[k], H]);

  // 1. Flat bottom face at z = 0 (outward normal = -Z).
  const botCenter: Vec3 = [0, 0, 0];
  for (let k = 0; k < 6; k++) {
    mesh.tri(botCenter, botVerts[(k + 1) % 6], botVerts[k]);
  }

  // 2. Top face at z = H (with optional 12-sided circular token recess).
  if (!hasRecess) {
    const topCenter: Vec3 = [0, 0, H];
    for (let k = 0; k < 6; k++) {
      mesh.tri(topCenter, topVerts[k], topVerts[(k + 1) % 6]);
    }
  } else {
    const recessR = Math.min(13.0, R * 0.56);
    const recessD = Math.min(1.5, H * 0.55);
    const zFloor = H - recessD;

    const ringTop: Vec3[] = [];
    const ringBot: Vec3[] = [];
    for (let j = 0; j < 12; j++) {
      const a = Math.PI / 2 + (j * Math.PI) / 6;
      const rx = recessR * Math.cos(a);
      const ry = recessR * Math.sin(a);
      ringTop.push([rx, ry, H]);
      ringBot.push([rx, ry, zFloor]);
    }

    // Outer hexagonal annulus stitched to the 12-sided recess (3 triangles per sector, zero T-junctions).
    for (let k = 0; k < 6; k++) {
      const kNext = (k + 1) % 6;
      const j0 = 2 * k;
      const j1 = 2 * k + 1;
      const j2 = (2 * k + 2) % 12;
      mesh.tri(ringTop[j0], topVerts[k], ringTop[j1]);
      mesh.tri(ringTop[j1], topVerts[k], topVerts[kNext]);
      mesh.tri(ringTop[j1], topVerts[kNext], ringTop[j2]);
    }

    // Vertical recess step wall (outward normal points radially inward toward recess center).
    for (let j = 0; j < 12; j++) {
      const jn = (j + 1) % 12;
      mesh.quad(ringTop[j], ringTop[jn], ringBot[jn], ringBot[j]);
    }

    // Recess floor at z = H - recessD (outward normal = +Z).
    const recessCenter: Vec3 = [0, 0, zFloor];
    for (let j = 0; j < 12; j++) {
      mesh.tri(recessCenter, ringBot[j], ringBot[(j + 1) % 12]);
    }
  }

  // 3. Six vertical hex side faces (with optional centered 5.4 x 2.4 x 2.2 mm octagonal magnet pockets).
  const pw = Math.min(5.4, R * 0.52);
  const ph = Math.min(2.4, H * 0.65);
  const pd = Math.min(2.2, R * 0.22);
  const usePockets = magnetSlots && pw >= 2.0 && ph >= 1.2 && pd >= 0.8;

  for (let k = 0; k < 6; k++) {
    const kn = (k + 1) % 6;
    const o0 = botVerts[k];
    const o1 = botVerts[kn];
    const o2 = topVerts[kn];
    const o3 = topVerts[k];

    if (!usePockets) {
      mesh.quad(o0, o1, o2, o3);
      continue;
    }

    const dx = vx[kn] - vx[k];
    const dy = vy[kn] - vy[k];
    const edgeLen = Math.hypot(dx, dy) || 1;
    const tx = dx / edgeLen;
    const ty = dy / edgeLen;
    // Outward side normal: T x +Z = (ty, -tx, 0).
    const nx = ty;
    const ny = -tx;
    const mx = 0.5 * (vx[k] + vx[kn]);
    const my = 0.5 * (vy[k] + vy[kn]);
    const zc = 0.5 * H;

    const wallPt = (u: number, z: number, inset: number): Vec3 => [
      mx + tx * u - nx * inset,
      my + ty * u - ny * inset,
      z,
    ];

    const hw = pw * 0.5;
    const hh = ph * 0.5;
    const ch = Math.min(0.45, hw * 0.25, hh * 0.35);

    // Octagonal pocket mouth in local wall (u, z) coordinates (CCW from outside).
    const pocketUZ: [number, number][] = [
      [-hw + ch, zc - hh],
      [hw - ch, zc - hh],
      [hw, zc - hh + ch],
      [hw, zc + hh - ch],
      [hw - ch, zc + hh],
      [-hw + ch, zc + hh],
      [-hw, zc + hh - ch],
      [-hw, zc - hh + ch],
    ];

    const mouth = pocketUZ.map(([u, z]) => wallPt(u, z, 0));
    const back = pocketUZ.map(([u, z]) => wallPt(u, z, pd));
    const outer: Vec3[] = [o0, o1, o2, o3];

    // Connect 4 outer quad corners to 8 octagonal pocket vertices (12 triangles, zero T-junctions).
    for (let q = 0; q < 4; q++) {
      const oa = outer[q];
      const ob = outer[(q + 1) % 4];
      const pa = mouth[2 * q];
      const pb = mouth[2 * q + 1];
      const pPrev = mouth[(2 * q + 7) % 8];
      mesh.tri(oa, pa, pPrev);
      mesh.tri(oa, ob, pb);
      mesh.tri(oa, pb, pa);
    }

    // Recessed pocket tunnel walls.
    for (let j = 0; j < 8; j++) {
      const jn = (j + 1) % 8;
      mesh.quad(mouth[j], mouth[jn], back[jn], back[j]);
    }

    // Recessed pocket back face (outward normal = +N_out, facing out of the pocket).
    const backCenter = wallPt(0, zc, pd);
    for (let j = 0; j < 8; j++) {
      mesh.tri(backCenter, back[j], back[(j + 1) % 8]);
    }
  }
}

// ---- Watertight low-poly 3D relief primitives ----

/** Closed multi-ring lofted solid (generalizes cones, pyramids, frustums, domes, and spires). */
function addLoftedRings(
  mesh: TriangleMesh,
  rings: { cx: number; cy: number; z: number; rx: number; ry: number }[],
  sides: number,
  rot = 0,
  apex?: Vec3,
): void {
  if (rings.length === 0) return;
  const cosR = Math.cos(rot);
  const sinR = Math.sin(rot);

  const ringVerts: Vec3[][] = rings.map((r) => {
    const pts: Vec3[] = [];
    for (let i = 0; i < sides; i++) {
      const a = (i / sides) * TAU;
      const lx = Math.cos(a) * r.rx;
      const ly = Math.sin(a) * r.ry;
      pts.push([r.cx + lx * cosR - ly * sinR, r.cy + lx * sinR + ly * cosR, r.z]);
    }
    return pts;
  });

  // Bottom cap (normal = -Z).
  const first = rings[0];
  const botCenter: Vec3 = [first.cx, first.cy, first.z];
  const botRing = ringVerts[0];
  for (let i = 0; i < sides; i++) {
    mesh.tri(botCenter, botRing[(i + 1) % sides], botRing[i]);
  }

  // Side bands between consecutive rings.
  for (let b = 0; b < ringVerts.length - 1; b++) {
    const lower = ringVerts[b];
    const upper = ringVerts[b + 1];
    for (let i = 0; i < sides; i++) {
      const next = (i + 1) % sides;
      mesh.quad(lower[i], lower[next], upper[next], upper[i]);
    }
  }

  // Top cap or sharp apex.
  const topRing = ringVerts[ringVerts.length - 1];
  if (apex) {
    for (let i = 0; i < sides; i++) {
      mesh.tri(topRing[i], topRing[(i + 1) % sides], apex);
    }
  } else {
    const last = rings[rings.length - 1];
    const topCenter: Vec3 = [last.cx, last.cy, last.z];
    for (let i = 0; i < sides; i++) {
      mesh.tri(topCenter, topRing[i], topRing[(i + 1) % sides]);
    }
  }
}

function addFrustum(
  mesh: TriangleMesh,
  cx: number,
  cy: number,
  z0: number,
  z1: number,
  rBot: number,
  rTop: number,
  sides: number,
  rot = 0,
  topShiftX = 0,
  topShiftY = 0,
): void {
  if (rTop <= 1e-4) {
    addLoftedRings(mesh, [{ cx, cy, z: z0, rx: rBot, ry: rBot }], sides, rot, [
      cx + topShiftX,
      cy + topShiftY,
      z1,
    ]);
  } else {
    addLoftedRings(
      mesh,
      [
        { cx, cy, z: z0, rx: rBot, ry: rBot },
        { cx: cx + topShiftX, cy: cy + topShiftY, z: z1, rx: rTop, ry: rTop },
      ],
      sides,
      rot,
    );
  }
}

function addDomedMound(
  mesh: TriangleMesh,
  cx: number,
  cy: number,
  z0: number,
  height: number,
  rx: number,
  ry: number,
  sides = 10,
  rot = 0,
): void {
  addLoftedRings(
    mesh,
    [
      { cx, cy, z: z0, rx, ry },
      { cx, cy, z: z0 + height * 0.44, rx: rx * 0.88, ry: ry * 0.88 },
      { cx, cy, z: z0 + height * 0.8, rx: rx * 0.55, ry: ry * 0.55 },
    ],
    sides,
    rot,
    [cx, cy, z0 + height],
  );
}

/**
 * Closed watertight linear ridge or trapezoidal bar from `(x0, y0)` to `(x1, y1)`.
 * When `halfWidthTop <= 0`, forms a hip-roof triangular ridge prism; otherwise a sloped prism bar.
 */
function addRidgePrism(
  mesh: TriangleMesh,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  z0: number,
  height: number,
  halfWidthBot: number,
  halfWidthTop = 0,
  endInset = 0,
): void {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len = Math.hypot(dx, dy);
  if (len < 1e-3) return;
  const ux = dx / len;
  const uy = dy / len;
  // Left unit normal v = (-uy, ux) so u x v = +Z.
  const vx = -uy;
  const vy = ux;

  const b0: Vec3 = [x0 - vx * halfWidthBot, y0 - vy * halfWidthBot, z0];
  const b1: Vec3 = [x1 - vx * halfWidthBot, y1 - vy * halfWidthBot, z0];
  const b2: Vec3 = [x1 + vx * halfWidthBot, y1 + vy * halfWidthBot, z0];
  const b3: Vec3 = [x0 + vx * halfWidthBot, y0 + vy * halfWidthBot, z0];

  const inset = Math.min(endInset, len * 0.35);
  const px0 = x0 + ux * inset;
  const py0 = y0 + uy * inset;
  const px1 = x1 - ux * inset;
  const py1 = y1 - uy * inset;
  const z1 = z0 + height;

  // Bottom face (normal = -Z).
  mesh.quad(b0, b3, b2, b1);

  if (halfWidthTop <= 1e-4) {
    const c0: Vec3 = [px0, py0, z1];
    const c1: Vec3 = [px1, py1, z1];
    mesh.quad(b0, b1, c1, c0);
    mesh.tri(b1, b2, c1);
    mesh.quad(b2, b3, c0, c1);
    mesh.tri(b3, b0, c0);
  } else {
    const t0: Vec3 = [px0 - vx * halfWidthTop, py0 - vy * halfWidthTop, z1];
    const t1: Vec3 = [px1 - vx * halfWidthTop, py1 - vy * halfWidthTop, z1];
    const t2: Vec3 = [px1 + vx * halfWidthTop, py1 + vy * halfWidthTop, z1];
    const t3: Vec3 = [px0 + vx * halfWidthTop, py0 + vy * halfWidthTop, z1];
    mesh.quad(t0, t1, t2, t3);
    mesh.quad(b0, b1, t1, t0);
    mesh.quad(b1, b2, t2, t1);
    mesh.quad(b2, b3, t3, t2);
    mesh.quad(b3, b0, t0, t3);
  }
}

/**
 * Closed watertight curved crescent dune / ocean wave crest tapering smoothly to tips at both ends.
 * `angEnd` must be strictly greater than `angStart`.
 */
function addCurvedCrest(
  mesh: TriangleMesh,
  cx: number,
  cy: number,
  arcRadius: number,
  angStart: number,
  angEnd: number,
  segments: number,
  z0: number,
  peakHeight: number,
  halfWidth: number,
  radialSkew = 0,
): void {
  const nSeg = Math.max(3, segments);
  const tipStart: Vec3 = [
    cx + Math.cos(angStart) * arcRadius,
    cy + Math.sin(angStart) * arcRadius,
    z0,
  ];
  const tipEnd: Vec3 = [
    cx + Math.cos(angEnd) * arcRadius,
    cy + Math.sin(angEnd) * arcRadius,
    z0,
  ];

  const rights: Vec3[] = [];
  const lefts: Vec3[] = [];
  const crests: Vec3[] = [];

  for (let s = 1; s < nSeg; s++) {
    const t = s / nSeg;
    const env = Math.sin(Math.PI * t);
    const a = angStart + (angEnd - angStart) * t;
    const cosA = Math.cos(a);
    const sinA = Math.sin(a);
    const w = Math.max(0.2, halfWidth * (0.3 + 0.7 * env));
    const rOuter = arcRadius + w;
    const rInner = Math.max(0.5, arcRadius - w);
    const rCrest = arcRadius + radialSkew * w;

    rights.push([cx + cosA * rOuter, cy + sinA * rOuter, z0]);
    lefts.push([cx + cosA * rInner, cy + sinA * rInner, z0]);
    crests.push([cx + cosA * rCrest, cy + sinA * rCrest, z0 + peakHeight * env]);
  }

  // Start cap.
  mesh.tri(tipStart, lefts[0], rights[0]);
  mesh.tri(tipStart, rights[0], crests[0]);
  mesh.tri(tipStart, crests[0], lefts[0]);

  // Interior bands.
  for (let i = 0; i < rights.length - 1; i++) {
    const j = i + 1;
    mesh.quad(rights[i], lefts[i], lefts[j], rights[j]);
    mesh.quad(rights[i], rights[j], crests[j], crests[i]);
    mesh.quad(crests[i], crests[j], lefts[j], lefts[i]);
  }

  // End cap.
  const last = rights.length - 1;
  mesh.tri(rights[last], lefts[last], tipEnd);
  mesh.tri(rights[last], tipEnd, crests[last]);
  mesh.tri(crests[last], tipEnd, lefts[last]);
}

/** Support-free single-shell 3D pine tree (trunk + beveled pine cone tiers). */
function addPineTree(
  mesh: TriangleMesh,
  cx: number,
  cy: number,
  z0: number,
  totalHeight: number,
  radius: number,
  rot: number,
): void {
  const trunkR = radius * 0.32;
  addLoftedRings(
    mesh,
    [
      { cx, cy, z: z0, rx: trunkR * 1.15, ry: trunkR * 1.15 },
      { cx, cy, z: z0 + totalHeight * 0.16, rx: trunkR, ry: trunkR },
      // 45° support-free flare out to lower pine skirt.
      { cx, cy, z: z0 + totalHeight * 0.34, rx: radius, ry: radius },
      { cx, cy, z: z0 + totalHeight * 0.62, rx: radius * 0.44, ry: radius * 0.44 },
      // Second pine tier flare + crown.
      { cx, cy, z: z0 + totalHeight * 0.7, rx: radius * 0.76, ry: radius * 0.76 },
    ],
    6,
    rot,
    [cx, cy, z0 + totalHeight],
  );
}

/** Faceted hexagonal crystal spire with tilted shaft and sharp pyramidal tip. */
function addCrystalSpire(
  mesh: TriangleMesh,
  cx: number,
  cy: number,
  z0: number,
  height: number,
  radius: number,
  tiltX: number,
  tiltY: number,
  rot: number,
): void {
  const shoulderZ = z0 + height * 0.66;
  addLoftedRings(
    mesh,
    [
      { cx, cy, z: z0, rx: radius * 1.12, ry: radius * 1.12 },
      {
        cx: cx + tiltX * 0.66,
        cy: cy + tiltY * 0.66,
        z: shoulderZ,
        rx: radius * 0.88,
        ry: radius * 0.88,
      },
    ],
    6,
    rot,
    [cx + tiltX, cy + tiltY, z0 + height],
  );
}

/** Adds distinct low-poly 3D terrain relief onto the tile top surface. */
function addTerrainRelief(
  mesh: TriangleMesh,
  terrain: Terrain,
  R: number,
  H: number,
  rh: number,
  hasRecess: boolean,
): void {
  const s = R / REFERENCE_HEX_SIDE_MM;
  // Embed relief bases 0.1mm into the solid hex top so slicers union shells cleanly without coplanar z-fighting.
  const z0 = Math.max(0.4, H - 0.1);
  const topH = rh + 0.1;
  const recessR = hasRecess ? Math.min(13.0, R * 0.56) : 0;
  const rRingMid = hasRecess ? (recessR + 2.2 * s + R * 0.74) * 0.5 : R * 0.52;

  switch (terrain) {
    case 'Forest': {
      // 6 pine tree cones on trunks arranged around the outer ring.
      const treeSpecs = [
        { a: 0.12, r: 0.98, h: 1.0, rad: 4.1 },
        { a: 1.12, r: 1.05, h: 0.84, rad: 3.7 },
        { a: 2.15, r: 0.96, h: 0.94, rad: 4.0 },
        { a: 3.22, r: 1.04, h: 0.82, rad: 3.6 },
        { a: 4.24, r: 0.98, h: 0.96, rad: 4.0 },
        { a: 5.26, r: 1.03, h: 0.88, rad: 3.8 },
      ];
      for (const spec of treeSpecs) {
        const dist = rRingMid * spec.r;
        const tx = Math.cos(spec.a) * dist;
        const ty = Math.sin(spec.a) * dist;
        addPineTree(mesh, tx, ty, z0, topH * spec.h, spec.rad * s, spec.a);
      }
      break;
    }

    case 'Hills': {
      // 4 stepped brick/hill mesas + clay brick stacks on the outer ring.
      const mesaAngles = [0.42, 1.98, 3.56, 5.12];
      mesaAngles.forEach((a, idx) => {
        const dist = rRingMid * (idx % 2 === 0 ? 0.98 : 1.03);
        const mx = Math.cos(a) * dist;
        const my = Math.sin(a) * dist;
        const hScale = idx % 2 === 0 ? 0.95 : 0.82;
        // Lower mesa terrace.
        addFrustum(mesh, mx, my, z0, z0 + topH * 0.54 * hScale, 5.6 * s, 4.1 * s, 6, a);
        // Upper stepped mesa plateau.
        addFrustum(
          mesh,
          mx + Math.cos(a + 0.6) * 0.7 * s,
          my + Math.sin(a + 0.6) * 0.7 * s,
          z0 + topH * 0.46 * hScale,
          z0 + topH * hScale,
          3.1 * s,
          2.1 * s,
          6,
          a + 0.25,
        );

        // Clay brick stack tucked beside each pair of mesas.
        const ba = a + 0.76;
        const bDist = rRingMid * 0.98;
        const bx = Math.cos(ba) * bDist;
        const by = Math.sin(ba) * bDist;
        const tx = -Math.sin(ba) * 2.4 * s;
        const ty = Math.cos(ba) * 2.4 * s;
        addRidgePrism(mesh, bx - tx, by - ty, bx + tx, by + ty, z0, topH * 0.46, 1.5 * s, 1.2 * s, 0.3 * s);
      });
      break;
    }

    case 'Pasture': {
      // Gentle rounded pasture mounds + wooden pasture fence bars around the outer ring.
      const moundAngles = [0.75, 2.35, 3.9, 5.48];
      moundAngles.forEach((a, idx) => {
        const dist = rRingMid * 0.98;
        const mx = Math.cos(a) * dist;
        const my = Math.sin(a) * dist;
        const hScale = idx % 2 === 0 ? 0.78 : 0.66;
        addDomedMound(mesh, mx, my, z0, topH * hScale, 5.8 * s, 4.1 * s, 10, a + Math.PI / 2);
      });

      // Two rustic pasture fence bars at top-right/bottom-left sectors.
      for (const fa of [0.0, Math.PI]) {
        const fcDist = rRingMid * 1.02;
        const fcx = Math.cos(fa) * fcDist;
        const fcy = Math.sin(fa) * fcDist;
        const span = 4.6 * s;
        const tx = -Math.sin(fa);
        const ty = Math.cos(fa);
        const x0 = fcx - tx * span;
        const y0 = fcy - ty * span;
        const x1 = fcx + tx * span;
        const y1 = fcy + ty * span;

        // Horizontal fence bar + 3 fence posts.
        addRidgePrism(mesh, x0, y0, x1, y1, z0, topH * 0.52, 0.85 * s, 0.55 * s, 0.2 * s);
        for (const u of [-span * 0.82, 0, span * 0.82]) {
          addFrustum(
            mesh,
            fcx + tx * u,
            fcy + ty * u,
            z0,
            z0 + topH * 0.76,
            1.15 * s,
            0.9 * s,
            6,
            fa,
          );
        }
      }
      break;
    }

    case 'Fields': {
      // Parallel raised wheat-furrow ridges clipped cleanly to the outer top ring.
      const furrowAngle = 0.36;
      const cosA = Math.cos(furrowAngle);
      const sinA = Math.sin(furrowAngle);
      const rOuter = R * 0.72;
      const rInner = hasRecess ? recessR + 2.1 * s : 0;
      const pitch = 4.1 * s;
      const halfSpan = Math.floor((rOuter - 2.5 * s) / pitch);

      const emitFurrow = (u0: number, u1: number, v: number, hFactor: number) => {
        if (u1 - u0 < 4.2 * s) return;
        const x0 = u0 * cosA - v * sinA;
        const y0 = u0 * sinA + v * cosA;
        const x1 = u1 * cosA - v * sinA;
        const y1 = u1 * sinA + v * cosA;
        addRidgePrism(
          mesh,
          x0,
          y0,
          x1,
          y1,
          z0,
          topH * hFactor,
          1.45 * s,
          0.38 * s,
          1.1 * s,
        );
      };

      for (let idx = -halfSpan; idx <= halfSpan; idx++) {
        const v = idx * pitch;
        if (Math.abs(v) >= rOuter - 2.2 * s) continue;
        const uMax = Math.sqrt(Math.max(0, rOuter * rOuter - v * v));
        const hFactor = idx % 2 === 0 ? 0.68 : 0.56;
        if (rInner > 0 && Math.abs(v) < rInner) {
          const uIn = Math.sqrt(Math.max(0, rInner * rInner - v * v));
          emitFurrow(-uMax, -uIn, v, hFactor);
          emitFurrow(uIn, uMax, v, hFactor);
        } else {
          emitFurrow(-uMax, uMax, v, hFactor);
        }
      }
      break;
    }

    case 'Mountains': {
      // 4 dramatic faceted alpine mountain peaks + jagged shoulder foothills.
      const peaks = [
        { a: 0.3, r: 0.97, h: 1.12, rad: 6.4, sides: 5, sx: 0.9, sy: -0.5 },
        { a: 1.85, r: 1.0, h: 0.94, rad: 5.8, sides: 6, sx: -0.7, sy: 0.6 },
        { a: 3.42, r: 0.96, h: 1.06, rad: 6.2, sides: 5, sx: 0.6, sy: 0.8 },
        { a: 4.98, r: 1.01, h: 0.88, rad: 5.5, sides: 6, sx: -0.8, sy: -0.4 },
      ];
      for (const pk of peaks) {
        const dist = rRingMid * pk.r;
        const px = Math.cos(pk.a) * dist;
        const py = Math.sin(pk.a) * dist;
        addFrustum(
          mesh,
          px,
          py,
          z0,
          z0 + topH * pk.h,
          pk.rad * s,
          0,
          pk.sides,
          pk.a,
          pk.sx * s,
          pk.sy * s,
        );
        // Secondary shoulder spur peak.
        const sa = pk.a + 0.42;
        const sDist = dist * 1.04;
        addFrustum(
          mesh,
          Math.cos(sa) * sDist,
          Math.sin(sa) * sDist,
          z0,
          z0 + topH * pk.h * 0.58,
          pk.rad * 0.62 * s,
          0,
          5,
          sa + 0.3,
        );
      }
      break;
    }

    case 'Desert': {
      // Sweeping crescent barchan dune ridges + faceted saguaro cactus prism.
      addCurvedCrest(mesh, -2.5 * s, 2.0 * s, 23.5 * s, 0.35, 2.45, 10, z0, topH * 0.72, 4.2 * s, 0.32);
      addCurvedCrest(mesh, 2.0 * s, -2.0 * s, 22.0 * s, 3.45, 5.65, 10, z0, topH * 0.66, 3.9 * s, 0.3);
      addCurvedCrest(mesh, -3.5 * s, -3.0 * s, 11.5 * s, 2.1, 4.3, 8, z0, topH * 0.5, 3.1 * s, 0.28);

      // Saguaro cactus prism (main trunk + two upward angled arms).
      const cx = 6.5 * s;
      const cy = 3.5 * s;
      addLoftedRings(
        mesh,
        [
          { cx, cy, z: z0, rx: 2.3 * s, ry: 2.3 * s },
          { cx, cy, z: z0 + topH * 0.92, rx: 1.85 * s, ry: 1.85 * s },
        ],
        8,
        0,
        [cx, cy, z0 + topH * 1.06],
      );
      // Left & right cactus arms (45°-friendly upward angled spurs from solid plinths).
      for (const side of [-1, 1]) {
        const armX = cx + side * 3.1 * s;
        const armY = cy + side * 0.5 * s;
        const hArm = side > 0 ? 0.86 : 0.74;
        addRidgePrism(
          mesh,
          cx,
          cy,
          armX,
          armY,
          z0,
          topH * 0.48,
          1.35 * s,
          1.05 * s,
          0.1 * s,
        );
        addLoftedRings(
          mesh,
          [
            { cx: armX, cy: armY, z: z0, rx: 1.45 * s, ry: 1.45 * s },
            { cx: armX, cy: armY, z: z0 + topH * (hArm - 0.1), rx: 1.25 * s, ry: 1.25 * s },
          ],
          6,
          0,
          [armX, armY, z0 + topH * hArm],
        );
      }
      break;
    }

    case 'Gold': {
      // 5 clusters of faceted crystal spires around the outer ring.
      const clusters = [
        { a: 0.2, h: 1.08, rad: 2.9, tx: 1.1, ty: 0.5 },
        { a: 1.46, h: 0.92, rad: 2.6, tx: -0.8, ty: 0.9 },
        { a: 2.72, h: 1.04, rad: 2.8, tx: -1.0, ty: -0.6 },
        { a: 3.98, h: 0.88, rad: 2.5, tx: 0.6, ty: -1.0 },
        { a: 5.24, h: 0.98, rad: 2.7, tx: 1.0, ty: -0.4 },
      ];
      for (const cl of clusters) {
        const cx = Math.cos(cl.a) * rRingMid;
        const cy = Math.sin(cl.a) * rRingMid;
        // Nugget base plinth.
        addFrustum(mesh, cx, cy, z0, z0 + topH * 0.3, 4.8 * s, 3.6 * s, 6, cl.a);
        // Primary crystal spire.
        addCrystalSpire(
          mesh,
          cx,
          cy,
          z0,
          topH * cl.h,
          cl.rad * s,
          cl.tx * s,
          cl.ty * s,
          cl.a,
        );
        // Secondary satellite crystal shard.
        const sa = cl.a + 0.34;
        addCrystalSpire(
          mesh,
          Math.cos(sa) * (rRingMid * 1.04),
          Math.sin(sa) * (rRingMid * 1.04),
          z0,
          topH * cl.h * 0.68,
          cl.rad * 0.72 * s,
          -cl.ty * 0.7 * s,
          cl.tx * 0.7 * s,
          sa + 0.4,
        );
      }
      break;
    }

    case 'Sea': {
      // Stylized asymmetric ocean wave crests sweeping across the sea tile.
      addCurvedCrest(mesh, 0, -4.0 * s, 24.5 * s, 0.55, 2.58, 10, z0, topH * 0.68, 3.4 * s, 0.42);
      addCurvedCrest(mesh, 0, -2.0 * s, 13.0 * s, 0.65, 2.5, 8, z0, topH * 0.62, 3.1 * s, 0.42);
      addCurvedCrest(mesh, 0, 4.0 * s, 14.5 * s, 3.75, 5.68, 8, z0, topH * 0.6, 3.1 * s, -0.42);
      addCurvedCrest(mesh, 0, 4.5 * s, 25.0 * s, 3.7, 5.72, 10, z0, topH * 0.66, 3.4 * s, -0.42);
      break;
    }

    case 'Fog': {
      // Billowy overlapping cloud domes banked around the outer ring.
      const bankAngles = [0.35, 1.6, 2.86, 4.12, 5.38];
      bankAngles.forEach((a, idx) => {
        const cx = Math.cos(a) * rRingMid;
        const cy = Math.sin(a) * rRingMid;
        const hMain = idx % 2 === 0 ? 0.86 : 0.76;
        addDomedMound(mesh, cx, cy, z0, topH * hMain, 4.8 * s, 3.9 * s, 10, a);
        for (const dir of [-1, 1]) {
          const pa = a + dir * 0.22;
          const pDist = rRingMid * (dir > 0 ? 1.03 : 0.96);
          addDomedMound(
            mesh,
            Math.cos(pa) * pDist,
            Math.sin(pa) * pDist,
            z0,
            topH * hMain * 0.72,
            3.6 * s,
            3.1 * s,
            8,
            pa,
          );
        }
      });
      break;
    }
  }
}

function buildTerrainTileMesh(terrain: Terrain, resolved: Required<StlExportOptions>): TriangleMesh {
  const mesh = new TriangleMesh();
  const hasRecess = resolved.tokenRecess && terrain !== 'Desert' && terrain !== 'Sea';
  addHexBase(mesh, resolved.hexSideMm, resolved.baseHeightMm, hasRecess, resolved.magnetSlots);
  addTerrainRelief(
    mesh,
    terrain,
    resolved.hexSideMm,
    resolved.baseHeightMm,
    resolved.reliefHeightMm,
    hasRecess,
  );
  return mesh;
}

function latticeUnit(lx: number, ly: number): [number, number] {
  return [lx * SQRT3_2, ly * 0.5];
}

/**
 * Generates a 1:1 physical-scale Binary STL (`ArrayBuffer`) for a single hexagonal terrain tile
 * with optional side magnet pockets and centered Ø26.0mm number token recess.
 */
export function generateTerrainTileStl(
  terrain: Terrain,
  opts?: Partial<StlExportOptions>,
): ArrayBuffer {
  const resolved = resolveOptions(opts);
  const mesh = buildTerrainTileMesh(terrain, resolved);
  return packBinaryStl(mesh, `${terrain} ${resolved.hexSideMm}mm`);
}

/**
 * Generates a multi-body Binary STL (`ArrayBuffer`) arranging every tile of `draft`
 * in its exact physical board position (`spacing = hexSideMm + clearanceMm`, default `0.6mm` gap,
 * or `0` for a fused mini-diorama).
 */
export function generateFullBoardStl(
  draft: DraftMap,
  topo: Topology,
  opts?: Partial<StlExportOptions>,
): ArrayBuffer {
  const resolved = resolveOptions(opts);
  const numTiles = Math.floor(topo.tilePos.length / 2);
  const numVerts = Math.floor(topo.vertexPos.length / 2);

  const rawTile = Array.from({ length: numTiles }, (_, t) =>
    latticeUnit(topo.tilePos[2 * t], topo.tilePos[2 * t + 1]),
  );
  const rawVert = Array.from({ length: numVerts }, (_, v) =>
    latticeUnit(topo.vertexPos[2 * v], topo.vertexPos[2 * v + 1]),
  );

  const v0Idx = topo.tileVertices[0] ?? 0;
  const unitCircumradius =
    numTiles > 0 && rawVert[v0Idx]
      ? Math.hypot(rawVert[v0Idx][0] - rawTile[0][0], rawVert[v0Idx][1] - rawTile[0][1])
      : 1;

  const spacing = resolved.hexSideMm + resolved.clearanceMm;
  const mmPerUnit = spacing / (unitCircumradius || 1);

  // Map lattice (u, v) to right-handed 3D printer build plate (X = +u, Y = -v, Z = +up)
  // so the printed island viewed from +Z matches the 3D game board and SVG blueprint without mirroring.
  const positions: [number, number][] = rawTile.map(([u, v]) => [u * mmPerUnit, -v * mmPerUnit]);

  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const [px, py] of positions) {
    if (px < minX) minX = px;
    if (px > maxX) maxX = px;
    if (py < minY) minY = py;
    if (py > maxY) maxY = py;
  }
  const centerX = Number.isFinite(minX) ? 0.5 * (minX + maxX) : 0;
  const centerY = Number.isFinite(minY) ? 0.5 * (minY + maxY) : 0;

  const tileCache = new Map<Terrain, TriangleMesh>();
  const boardMesh = new TriangleMesh();

  for (let t = 0; t < numTiles; t++) {
    const terrain = draft.terrain[t] ?? 'Desert';
    let proto = tileCache.get(terrain);
    if (!proto) {
      proto = buildTerrainTileMesh(terrain, resolved);
      tileCache.set(terrain, proto);
    }
    const [px, py] = positions[t];
    boardMesh.appendTranslated(proto, px - centerX, py - centerY, 0);
  }

  return packBinaryStl(boardMesh, `${draft.layout} Board (${numTiles} tiles)`);
}

/** Triggers a browser download of a Binary `.stl` buffer (`application/sla`). */
export function downloadStl(buffer: ArrayBuffer, filename: string): void {
  const safeName = filename.toLowerCase().endsWith('.stl') ? filename : `${filename}.stl`;
  const blob = new Blob([buffer], { type: 'application/sla' });
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
