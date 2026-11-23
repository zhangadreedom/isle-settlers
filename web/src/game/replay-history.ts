import type { ReplayRecord } from '../generated/proto';

const key = 'isle:replay-history:v1';
const limit = 20;
export type HistoryEntry = { savedAt: number; record: ReplayRecord };

export function localReplayHistory(): HistoryEntry[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(key) ?? '[]');
    if (!Array.isArray(saved)) return [];
    return saved.filter((e): e is HistoryEntry => {
      const r = e?.record;
      return Number.isFinite(e?.savedAt) && Math.abs(e.savedAt) <= 8.64e15 &&
        typeof r?.seed === 'string' && Array.isArray(r.players) &&
        r.players.length >= 2 && r.players.length <= 6 && r.players.every((p: unknown) =>
          typeof (p as { name?: unknown } | null)?.name === 'string') &&
        Number.isInteger(r.winner) && r.winner >= 0 && r.winner < r.players.length && Array.isArray(r.actions);
    }).slice(0, limit);
  } catch {
    return [];
  }
}

export function saveLocalReplay(record: ReplayRecord): void {
  try {
    const history = localReplayHistory();
    const entry = { savedAt: Date.now(), record };
    localStorage.setItem(key, JSON.stringify([entry, ...history].slice(0, limit)));
  } catch {
    // Storage can be disabled (e.g. sandboxed embeds) or full. Viewing the
    // just-finished replay must still work without persistent storage.
  }
}
