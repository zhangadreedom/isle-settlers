// Deterministic post-game telemetry analyzer powered by the WASM Replay engine.
import { Replay } from '../engine';
import type { Phase, PlayerView, ReplayRecord } from '../generated/proto';
import { setTotal } from './text';

export const PIPS_BY_SUM = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

export interface PlayerEconomy {
  seat: number;
  produced: number;
  commodities: number;
  gold: number;
  stolenGain: number;
  stolenLoss: number;
  discarded: number;
  maritimeTrades: number;
  playerTrades: number;
  barbarianPoints: number;
}

export interface MatchStats {
  totalTurns: number;
  totalActions: number;
  totalRolls: number;
  diceCounts: number[]; // index 0..12 (2..12 used)
  expectedCounts: number[]; // index 0..12: totalRolls * PIPS_BY_SUM[n] / 36
  sevenCount: number;
  hottestRoll: number | null;
  vpTimeline: { turn: number; points: number[] }[];
  economy: PlayerEconomy[];
}

const MAX_TIMELINE_POINTS = 80;

function isSetupPhase(phase: Phase): boolean {
  return (
    phase.type === 'SetupSettlement' ||
    phase.type === 'SetupRoad' ||
    (phase.type === 'Gold' && phase.value.vertex !== null)
  );
}

function downsampleTimeline(
  pts: { turn: number; points: number[] }[],
): { turn: number; points: number[] }[] {
  if (pts.length <= MAX_TIMELINE_POINTS) return pts;
  const out: { turn: number; points: number[] }[] = [pts[0]];
  const interior = MAX_TIMELINE_POINTS - 2;
  for (let i = 1; i <= interior; i++) {
    const idx = Math.round((i * (pts.length - 1)) / (MAX_TIMELINE_POINTS - 1));
    if (out[out.length - 1].turn !== pts[idx].turn) {
      out.push(pts[idx]);
    }
  }
  const last = pts[pts.length - 1];
  if (out[out.length - 1].turn !== last.turn) {
    out.push(last);
  } else {
    out[out.length - 1] = last;
  }
  return out;
}

export function analyzeReplayRecord(record: ReplayRecord): MatchStats {
  const replay = new Replay(JSON.stringify(record));
  try {
    const numPlayers = record.config.numPlayers;
    const diceCounts = new Array<number>(13).fill(0);
    const economy: PlayerEconomy[] = Array.from({ length: numPlayers }, (_, seat) => ({
      seat,
      produced: 0,
      commodities: 0,
      gold: 0,
      stolenGain: 0,
      stolenLoss: 0,
      discarded: 0,
      maritimeTrades: 0,
      playerTrades: 0,
      barbarianPoints: 0,
    }));

    const readTruePoints = (specView?: PlayerView): number[] => {
      const pub = specView ?? replay.view(null);
      const pts: number[] = [];
      for (let p = 0; p < numPlayers; p++) {
        pts.push(replay.view(p).me?.victoryPoints ?? pub.players[p]?.publicVictoryPoints ?? 0);
      }
      return pts;
    };

    const startView = replay.view(null);
    let setupDone = !isSetupPhase(startView.phase);
    let lastTurn = startView.turn;
    const rawTimeline: { turn: number; points: number[] }[] = [];
    if (setupDone) {
      rawTimeline.push({ turn: startView.turn, points: readTruePoints(startView) });
    }

    let totalActions = 0;
    let totalRolls = 0;

    for (;;) {
      const step = replay.step(null);
      if (!step) break;
      totalActions++;

      let turnBoundary = false;
      for (const e of step.events) {
        switch (e.type) {
          case 'DiceRolled': {
            const sum = e.value.dice[0] + e.value.dice[1];
            totalRolls++;
            if (sum >= 2 && sum <= 12) diceCounts[sum]++;
            break;
          }
          case 'ResourcesProduced': {
            const econ = economy[e.value.player];
            if (econ) econ.produced += setTotal(e.value.resources);
            break;
          }
          case 'CommoditiesProduced': {
            const econ = economy[e.value.player];
            if (econ) econ.commodities += setTotal(e.value.commodities);
            break;
          }
          case 'GoldChosen': {
            const econ = economy[e.value.player];
            if (econ) econ.gold += setTotal(e.value.resources);
            break;
          }
          case 'Stolen':
          case 'CommodityStolen': {
            const thief = economy[e.value.thief];
            const victim = economy[e.value.victim];
            if (thief) thief.stolenGain += 1;
            if (victim) victim.stolenLoss += 1;
            break;
          }
          case 'Discarded': {
            const econ = economy[e.value.player];
            if (econ) econ.discarded += setTotal(e.value.resources);
            break;
          }
          case 'CommoditiesDiscarded': {
            const econ = economy[e.value.player];
            if (econ) econ.discarded += setTotal(e.value.commodities);
            break;
          }
          case 'MaritimeTraded':
          case 'GoodTraded': {
            const econ = economy[e.value.player];
            if (econ) econ.maritimeTrades += 1;
            break;
          }
          case 'TradeExecuted': {
            const proposer = economy[e.value.player];
            const partner = economy[e.value.partner];
            if (proposer) proposer.playerTrades += 1;
            if (partner) partner.playerTrades += 1;
            break;
          }
          case 'TurnEnded':
          case 'SpecialBuildPassed':
          case 'PairedTurnEnded':
            turnBoundary = true;
            break;
          default:
            break;
        }
      }

      if (!setupDone) {
        const v = replay.view(null);
        if (!isSetupPhase(v.phase)) {
          setupDone = true;
          lastTurn = v.turn;
          rawTimeline.push({ turn: v.turn, points: readTruePoints(v) });
        }
      } else if (turnBoundary) {
        const v = replay.view(null);
        if (v.turn > lastTurn) {
          lastTurn = v.turn;
          rawTimeline.push({ turn: v.turn, points: readTruePoints(v) });
        } else if (rawTimeline.length > 0 && rawTimeline[rawTimeline.length - 1].turn === v.turn) {
          rawTimeline[rawTimeline.length - 1] = { turn: v.turn, points: readTruePoints(v) };
        }
      }
    }

    const finalView = replay.view(null);
    const finalPoints = finalView.finalPoints
      ? finalView.finalPoints.slice(0, numPlayers)
      : readTruePoints(finalView);

    if (rawTimeline.length === 0) {
      rawTimeline.push({ turn: 0, points: finalPoints });
    } else if (rawTimeline[rawTimeline.length - 1].turn === finalView.turn) {
      rawTimeline[rawTimeline.length - 1] = { turn: finalView.turn, points: finalPoints };
    } else {
      rawTimeline.push({ turn: finalView.turn, points: finalPoints });
    }

    for (let p = 0; p < numPlayers; p++) {
      economy[p].barbarianPoints = finalView.players[p]?.defenderPoints ?? 0;
    }

    const expectedCounts = PIPS_BY_SUM.map((pips) => (totalRolls * pips) / 36);
    const sevenCount = diceCounts[7];
    let hottestRoll: number | null = null;
    let hottestCount = 0;
    for (let n = 2; n <= 12; n++) {
      if (diceCounts[n] > hottestCount) {
        hottestCount = diceCounts[n];
        hottestRoll = n;
      }
    }

    return {
      totalTurns: finalView.turn,
      totalActions,
      totalRolls,
      diceCounts,
      expectedCounts,
      sevenCount,
      hottestRoll,
      vpTimeline: downsampleTimeline(rawTimeline),
      economy,
    };
  } finally {
    replay.free();
  }
}
