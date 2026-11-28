// Global anti-cheat Daily Challenge speedrun leaderboard client (`/api/daily`).

import { EMBEDDED } from '../embed';
import type { ReplayRecord } from '../generated/proto';
import { account, sessionToken } from './account';
import { savedName } from './connection';

export interface DailyScoreEntry {
  rank: number;
  playerName: string;
  registered: boolean;
  won: boolean;
  turns: number;
  points: number;
  submittedAt: number;
}

export interface DailyLeaderboard {
  dateKey: string;
  entries: DailyScoreEntry[];
}

export interface SubmitDailyScoreParams {
  dateKey: string;
  seat: number;
  playerName: string;
  replay: ReplayRecord;
}

const leaderboardListeners = new Set<(board: DailyLeaderboard) => void>();

function isDailyScoreEntry(value: unknown): value is DailyScoreEntry {
  if (!value || typeof value !== 'object') return false;
  const e = value as Partial<DailyScoreEntry>;
  return (
    typeof e.rank === 'number' &&
    typeof e.playerName === 'string' &&
    typeof e.registered === 'boolean' &&
    typeof e.won === 'boolean' &&
    typeof e.turns === 'number' &&
    typeof e.points === 'number' &&
    typeof e.submittedAt === 'number'
  );
}

function isDailyLeaderboard(value: unknown): value is DailyLeaderboard {
  if (!value || typeof value !== 'object') return false;
  const b = value as Partial<DailyLeaderboard>;
  return (
    typeof b.dateKey === 'string' &&
    Array.isArray(b.entries) &&
    b.entries.every(isDailyScoreEntry)
  );
}

/** Subscribes to live leaderboard updates returned by score submissions. */
export function onDailyLeaderboardUpdate(
  listener: (board: DailyLeaderboard) => void,
): () => void {
  leaderboardListeners.add(listener);
  return () => {
    leaderboardListeners.delete(listener);
  };
}

/** Fetches the global Daily Challenge speedrun leaderboard for `dateKey`. */
export async function fetchDailyLeaderboard(
  dateKey: string,
): Promise<DailyLeaderboard | null> {
  if (EMBEDDED) return null;
  try {
    const res = await fetch(`/api/daily?date=${encodeURIComponent(dateKey)}`);
    if (!res.ok) return null;
    const data: unknown = await res.json().catch(() => null);
    return isDailyLeaderboard(data) ? data : null;
  } catch {
    return null;
  }
}

/** Submits a deterministic replay for anti-cheat verification and ranking on `/api/daily`. */
export async function submitDailyScore(
  params: SubmitDailyScoreParams,
): Promise<DailyLeaderboard | null> {
  if (EMBEDDED) return null;
  try {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };
    const token = sessionToken();
    if (token !== null) {
      headers.Authorization = `Bearer ${token}`;
    }
    const res = await fetch('/api/daily', {
      method: 'POST',
      headers,
      body: JSON.stringify({
        dateKey: params.dateKey,
        seat: params.seat,
        playerName: params.playerName,
        replay: params.replay,
      }),
    });
    if (!res.ok) return null;
    const data: unknown = await res.json().catch(() => null);
    if (!isDailyLeaderboard(data)) return null;
    for (const listener of leaderboardListeners) {
      listener(data);
    }
    return data;
  } catch {
    return null;
  }
}

/** Background helper for submitting a finished Daily Challenge replay automatically. */
export async function autoSubmitDailyReplay(
  dateKey: string,
  seat: number,
  replay: ReplayRecord,
): Promise<void> {
  if (EMBEDDED) return;
  const playerName = account()?.username ?? (savedName().trim() || '玩家');
  await submitDailyScore({ dateKey, seat, playerName, replay });
}
