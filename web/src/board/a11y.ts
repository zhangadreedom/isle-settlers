// Zero-dependency accessibility & colorblind-mode state shared between
// lightweight SolidJS HUD controls and the lazy-loaded Three.js BoardRenderer.
// Keep free of `three` imports so UI bundles never pull Three.js into the main chunk.

const COLORBLIND_STORAGE_KEY = 'isle.colorblind';

/** Distinct geometric emblems per player seat (0..5) for color-independent identification. */
export const SEAT_SYMBOLS = ['●', '▲', '■', '◆', '★', '✚'] as const;

/** Short terrain badges indexed by numeric terrain code (0..8: Hills..Fog). */
export const TERRAIN_SHORT_BADGE = ['砖', '木', '羊', '麦', '矿', '漠', '海', '金', '雾'] as const;

function readSavedColorblind(): boolean {
  try {
    if (typeof localStorage === 'undefined') return false;
    const raw = localStorage.getItem(COLORBLIND_STORAGE_KEY);
    return raw === '1' || raw === 'true';
  } catch {
    return false;
  }
}

function persistColorblind(on: boolean): void {
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(COLORBLIND_STORAGE_KEY, on ? '1' : '0');
    }
  } catch {
    // Ignore storage access errors (private browsing / sandboxed iframes).
  }
}

let colorblindEnabled = readSavedColorblind();
const listeners = new Set<(on: boolean) => void>();

export function isColorblindMode(): boolean {
  return colorblindEnabled;
}

export function setColorblindMode(on: boolean): void {
  const next = Boolean(on);
  if (next === colorblindEnabled) return;
  colorblindEnabled = next;
  persistColorblind(next);
  for (const fn of [...listeners]) fn(next);
}

export function toggleColorblindMode(): boolean {
  const next = !colorblindEnabled;
  setColorblindMode(next);
  return next;
}

export function onColorblindChange(fn: (on: boolean) => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}
