// Procedural tabletop sound synthesizer (Web Audio API, zero external assets).
// Safe in sandboxed `dist-embed` iframes (wrapped localStorage) and headless/SSR.

import type { Event } from '../generated/proto';

export type BuildSoundKind = 'road' | 'ship' | 'settlement' | 'city' | 'knight' | 'upgrade';

const MUTE_STORAGE_KEY = 'isle.muted';
const MASTER_LEVEL = 0.28;
const RESOURCE_DEBOUNCE_MS = 160;

function readInitialMute(): boolean {
  try {
    if (typeof localStorage === 'undefined') return false;
    const raw = localStorage.getItem(MUTE_STORAGE_KEY);
    return raw === '1' || raw === 'true';
  } catch {
    return false;
  }
}

function persistMute(value: boolean): void {
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(MUTE_STORAGE_KEY, value ? '1' : '0');
    }
  } catch {
    // Sandboxed iframes without `allow-same-origin` throw SecurityError.
  }
}

const AudioCtx: typeof AudioContext | undefined =
  typeof window !== 'undefined'
    ? window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
    : typeof AudioContext !== 'undefined'
      ? AudioContext
      : undefined;

let muted = readInitialMute();
const muteListeners = new Set<(muted: boolean) => void>();

let ctx: AudioContext | null = null;
let masterBus: DynamicsCompressorNode | null = null;
let masterGain: GainNode | null = null;
let noiseBuf: AudioBuffer | null = null;
let unlockArmed = false;
let lastResourceAt = -1e9;
const scheduledTimers = new Set<number>();

function getCtx(): AudioContext | null {
  return ctx;
}

function armUnlock(): void {
  if (unlockArmed || typeof window === 'undefined' || !AudioCtx) return;
  unlockArmed = true;
  const unlock = () => {
    const current = !muted ? (ensureAudio()?.c ?? getCtx()) : getCtx();
    if (!current) return;
    const cleanup = () => {
      if (current.state === 'running') {
        window.removeEventListener('pointerdown', unlock, true);
        window.removeEventListener('keydown', unlock, true);
        unlockArmed = false;
      }
    };
    if (current.state === 'suspended') {
      void current.resume().then(cleanup, () => {});
    } else {
      cleanup();
    }
  };
  window.addEventListener('pointerdown', unlock, { capture: true, passive: true });
  window.addEventListener('keydown', unlock, { capture: true, passive: true });
}

// Arm once on load so the first user click/key unlocks audio before async Worker/WS events arrive.
armUnlock();

function ensureAudio(): { c: AudioContext; out: AudioNode; noise: AudioBuffer } | null {
  if (muted || !AudioCtx) return null;
  if (!ctx || !masterBus || !masterGain || !noiseBuf) {
    try {
      const c = new AudioCtx();
      const comp = c.createDynamicsCompressor();
      comp.threshold.value = -14;
      comp.knee.value = 18;
      comp.ratio.value = 6;
      comp.attack.value = 0.003;
      comp.release.value = 0.16;

      const gain = c.createGain();
      gain.gain.value = muted ? 0 : MASTER_LEVEL;
      comp.connect(gain);
      gain.connect(c.destination);

      // 1.5s soft pink-ish noise buffer reused across all percussion/splash voices.
      const len = Math.floor(c.sampleRate * 1.5);
      const buf = c.createBuffer(1, len, c.sampleRate);
      const data = buf.getChannelData(0);
      let b0 = 0;
      let b1 = 0;
      let b2 = 0;
      for (let i = 0; i < len; i++) {
        const white = Math.random() * 2 - 1;
        b0 = 0.997 * b0 + white * 0.099;
        b1 = 0.985 * b1 + white * 0.12;
        b2 = 0.95 * b2 + white * 0.18;
        data[i] = Math.max(-1, Math.min(1, (b0 + b1 + b2 + white * 0.22) * 0.7));
      }

      ctx = c;
      masterBus = comp;
      masterGain = gain;
      noiseBuf = buf;
    } catch {
      return null;
    }
  }
  if (ctx.state === 'suspended') {
    armUnlock();
    void ctx.resume().catch(() => {});
  }
  return { c: ctx, out: masterBus, noise: noiseBuf };
}

function defer(ms: number, fn: () => void): void {
  if (typeof window === 'undefined') return;
  const id = window.setTimeout(() => {
    scheduledTimers.delete(id);
    if (!muted) fn();
  }, ms);
  scheduledTimers.add(id);
}

// ---- Mute state & subscription API ----

export function isMuted(): boolean {
  return muted;
}

export function setMuted(next: boolean): void {
  const value = Boolean(next);
  if (value === muted) return;
  muted = value;
  persistMute(muted);
  if (muted) {
    for (const id of scheduledTimers) window.clearTimeout(id);
    scheduledTimers.clear();
  }
  if (ctx && masterGain) {
    const now = ctx.currentTime;
    masterGain.gain.cancelScheduledValues(now);
    masterGain.gain.setTargetAtTime(muted ? 0 : MASTER_LEVEL, now, 0.015);
  }
  for (const fn of muteListeners) fn(muted);
}

export function toggleMute(): boolean {
  setMuted(!muted);
  if (!muted) playUnmuteClick();
  return muted;
}

export function onMuteChange(fn: (muted: boolean) => void): () => void {
  muteListeners.add(fn);
  return () => {
    muteListeners.delete(fn);
  };
}

// ---- Low-level acoustic synthesis primitives ----

interface ToneOpts {
  type?: OscillatorType;
  freq: number;
  endFreq?: number;
  start: number;
  attack?: number;
  dur: number;
  amp: number;
  lpFreq?: number;
}

function playTone(c: AudioContext, dest: AudioNode, o: ToneOpts): void {
  const t0 = Math.max(c.currentTime, o.start);
  const attack = Math.max(0.002, o.attack ?? 0.005);
  const dur = Math.max(attack + 0.01, o.dur);
  const peak = Math.max(0.0002, o.amp);

  const osc = c.createOscillator();
  osc.type = o.type ?? 'sine';
  osc.frequency.setValueAtTime(Math.max(24, o.freq), t0);
  if (o.endFreq !== undefined && o.endFreq > 20) {
    osc.frequency.exponentialRampToValueAtTime(o.endFreq, t0 + dur);
  }

  const env = c.createGain();
  env.gain.setValueAtTime(0.0001, t0);
  env.gain.linearRampToValueAtTime(peak, t0 + attack);
  env.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);

  if (o.lpFreq !== undefined) {
    const lp = c.createBiquadFilter();
    lp.type = 'lowpass';
    lp.frequency.setValueAtTime(Math.min(c.sampleRate * 0.44, o.lpFreq), t0);
    lp.Q.value = 0.8;
    osc.connect(lp);
    lp.connect(env);
    osc.onended = () => {
      osc.disconnect();
      lp.disconnect();
      env.disconnect();
    };
  } else {
    osc.connect(env);
    osc.onended = () => {
      osc.disconnect();
      env.disconnect();
    };
  }
  env.connect(dest);
  osc.start(t0);
  osc.stop(t0 + dur + 0.01);
}

interface NoiseOpts {
  start: number;
  attack?: number;
  dur: number;
  amp: number;
  filterType?: BiquadFilterType;
  freq: number;
  endFreq?: number;
  q?: number;
}

function playNoise(c: AudioContext, dest: AudioNode, buf: AudioBuffer, o: NoiseOpts): void {
  const t0 = Math.max(c.currentTime, o.start);
  const attack = Math.max(0.002, o.attack ?? 0.004);
  const dur = Math.max(attack + 0.008, Math.min(0.65, o.dur));
  const peak = Math.max(0.0002, o.amp);

  const src = c.createBufferSource();
  src.buffer = buf;

  const filter = c.createBiquadFilter();
  filter.type = o.filterType ?? 'bandpass';
  filter.frequency.setValueAtTime(Math.min(c.sampleRate * 0.44, Math.max(40, o.freq)), t0);
  if (o.endFreq !== undefined) {
    filter.frequency.exponentialRampToValueAtTime(
      Math.min(c.sampleRate * 0.44, Math.max(40, o.endFreq)),
      t0 + dur,
    );
  }
  filter.Q.value = o.q ?? 2.0;

  const env = c.createGain();
  env.gain.setValueAtTime(0.0001, t0);
  env.gain.linearRampToValueAtTime(peak, t0 + attack);
  env.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);

  src.connect(filter);
  filter.connect(env);
  env.connect(dest);
  src.onended = () => {
    src.disconnect();
    filter.disconnect();
    env.disconnect();
  };

  const offset = Math.random() * Math.max(0.05, buf.duration - dur - 0.05);
  src.start(t0, offset, dur + 0.01);
}

/** Tactile maple-wood tabletop block knock (fundamental + ~2.45x xylophone bar mode + transient). */
function woodKnock(c: AudioContext, dest: AudioNode, buf: AudioBuffer, start: number, pitch: number, amp: number, dur = 0.075): void {
  playTone(c, dest, { type: 'triangle', freq: pitch * 1.16, endFreq: pitch, start, attack: 0.002, dur, amp, lpFreq: pitch * 4.2 });
  playTone(c, dest, { type: 'sine', freq: pitch * 2.44, start, attack: 0.002, dur: dur * 0.55, amp: amp * 0.28 });
  playNoise(c, dest, buf, { start, attack: 0.0015, dur: 0.018, amp: amp * 0.36, freq: pitch * 3.6, q: 2.8 });
}

function playUnmuteClick(): void {
  const a = ensureAudio();
  if (!a) return;
  woodKnock(a.c, a.out, a.noise, a.c.currentTime, 520, 0.32, 0.055);
}

// ---- Public game sound effects ----

/** Wooden dice tumble & clatter; rolls of 7 follow with a low suspenseful timpani & dark horn. */
export function playDice(d1: number, d2: number): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;
  const sum = (d1 | 0) + (d2 | 0);
  const seedSkew = ((d1 * 7 + d2 * 13) % 11) * 9;

  // Five decelerating wooden dice bounces on cardboard/wood.
  const hits = [0, 0.044, 0.094, 0.152, 0.215];
  hits.forEach((dt, i) => {
    const fade = 1 - i * 0.13;
    const base = 330 + seedSkew + ((i * 67) % 140);
    woodKnock(c, out, noise, now + dt, base, 0.26 * fade, 0.048);
    playNoise(c, out, noise, {
      start: now + dt + 0.006,
      attack: 0.002,
      dur: 0.032,
      amp: 0.2 * fade,
      freq: 1150 + i * 160,
      q: 2.4,
    });
  });

  if (sum === 7) {
    const t7 = now + 0.25;
    // Deep timpani strike (pitch glide 116 Hz -> C2 65.4 Hz) + muffled mallet thump.
    playTone(c, out, { type: 'sine', freq: 116, endFreq: 65.41, start: t7, attack: 0.008, dur: 0.68, amp: 0.52 });
    playTone(c, out, { type: 'triangle', freq: 174, endFreq: 98.0, start: t7, attack: 0.012, dur: 0.54, amp: 0.24, lpFreq: 320 });
    playNoise(c, out, noise, { start: t7, attack: 0.004, dur: 0.09, amp: 0.24, filterType: 'lowpass', freq: 260, q: 1.0 });
    // Suspenseful low muted brass minor swell (C2 + G2 + Eb3).
    for (const [f, amp] of [
      [65.41, 0.2],
      [98.0, 0.16],
      [155.56, 0.14],
    ] as const) {
      playTone(c, out, { type: 'sawtooth', freq: f, start: t7 + 0.03, attack: 0.09, dur: 0.62, amp, lpFreq: 480 });
    }
  }
}

/** Tactile piece placement / upgrade sounds. */
export function playBuild(kind: BuildSoundKind): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;

  switch (kind) {
    case 'road':
      woodKnock(c, out, noise, now, 300, 0.38, 0.065);
      woodKnock(c, out, noise, now + 0.068, 378, 0.34, 0.075);
      break;
    case 'settlement':
      woodKnock(c, out, noise, now, 261.63, 0.4, 0.075);
      woodKnock(c, out, noise, now + 0.075, 392.0, 0.42, 0.095);
      playTone(c, out, { type: 'triangle', freq: 523.25, start: now + 0.08, attack: 0.006, dur: 0.2, amp: 0.18, lpFreq: 1600 });
      break;
    case 'ship':
      // Gentle water splash + timber tap.
      playNoise(c, out, noise, {
        start: now,
        attack: 0.018,
        dur: 0.26,
        amp: 0.34,
        filterType: 'bandpass',
        freq: 980,
        endFreq: 390,
        q: 1.35,
      });
      woodKnock(c, out, noise, now + 0.035, 340, 0.34, 0.075);
      playTone(c, out, { type: 'sine', freq: 440, endFreq: 493.88, start: now + 0.06, attack: 0.01, dur: 0.16, amp: 0.12 });
      break;
    case 'city':
    case 'upgrade': {
      // Resonant stone foundation + bright acoustic fifth chord.
      playTone(c, out, { type: 'sine', freq: 138, endFreq: 87.31, start: now, attack: 0.004, dur: 0.18, amp: 0.44 });
      playNoise(c, out, noise, { start: now, attack: 0.003, dur: 0.065, amp: 0.28, filterType: 'bandpass', freq: 580, q: 1.8 });
      playTone(c, out, { type: 'triangle', freq: 523.25, start: now + 0.045, attack: 0.008, dur: 0.38, amp: 0.26, lpFreq: 2200 });
      playTone(c, out, { type: 'triangle', freq: 783.99, start: now + 0.075, attack: 0.008, dur: 0.42, amp: 0.24, lpFreq: 2600 });
      if (kind === 'upgrade') {
        playTone(c, out, { type: 'sine', freq: 1174.66, start: now + 0.115, attack: 0.008, dur: 0.46, amp: 0.2 });
      }
      break;
    }
    case 'knight':
      // Shield thud + crisp tempered metallic chime.
      playTone(c, out, { type: 'triangle', freq: 150, endFreq: 92.5, start: now, attack: 0.003, dur: 0.14, amp: 0.4, lpFreq: 420 });
      playNoise(c, out, noise, { start: now, attack: 0.002, dur: 0.04, amp: 0.24, freq: 760, q: 2.2 });
      playTone(c, out, { type: 'sine', freq: 880, start: now + 0.03, attack: 0.003, dur: 0.32, amp: 0.22 });
      playTone(c, out, { type: 'sine', freq: 1318.51, start: now + 0.036, attack: 0.003, dur: 0.36, amp: 0.18 });
      playTone(c, out, { type: 'sine', freq: 2093.0, start: now + 0.04, attack: 0.002, dur: 0.22, amp: 0.1 });
      break;
  }
}

/** Soft plucked marimba/kalimba arpeggio, debounced against simultaneous batch production. */
export function playResourceGain(): void {
  const stamp = typeof performance !== 'undefined' ? performance.now() : Date.now();
  if (stamp - lastResourceAt < RESOURCE_DEBOUNCE_MS) return;
  const a = ensureAudio();
  if (!a) return;
  lastResourceAt = stamp;
  const { c, out } = a;
  const now = c.currentTime;
  const notes = [523.25, 659.25, 783.99]; // C5 -> E5 -> G5
  notes.forEach((f, i) => {
    const t = now + i * 0.052;
    playTone(c, out, { type: 'sine', freq: f, start: t, attack: 0.004, dur: 0.24, amp: 0.24 });
    playTone(c, out, { type: 'triangle', freq: f * 2.72, start: t, attack: 0.002, dur: 0.065, amp: 0.07, lpFreq: 3200 });
  });
}

/** Airy rising mist shimmer; adds bright golden bell notes when a Gold field is discovered. */
export function playFogReveal(isGold: boolean): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;

  playNoise(c, out, noise, {
    start: now,
    attack: 0.04,
    dur: 0.38,
    amp: 0.24,
    filterType: 'bandpass',
    freq: 420,
    endFreq: 1680,
    q: 1.9,
  });
  const shimmer = [587.33, 739.99, 880.0]; // D5 -> F#5 -> A5
  shimmer.forEach((f, i) => {
    playTone(c, out, { type: 'sine', freq: f, start: now + 0.03 + i * 0.06, attack: 0.015, dur: 0.34, amp: 0.16 });
  });

  if (isGold) {
    const bells = [1108.73, 1318.51, 1760.0]; // C#6 -> E6 -> A6
    bells.forEach((f, i) => {
      const t = now + 0.14 + i * 0.065;
      playTone(c, out, { type: 'sine', freq: f, start: t, attack: 0.003, dur: 0.44, amp: 0.2 });
      playTone(c, out, { type: 'sine', freq: f * 2.0, start: t, attack: 0.002, dur: 0.22, amp: 0.08 });
    });
  }
}

/** Gentle merchant desk bell (`E5 -> B5`). */
export function playTradeOffer(): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out } = a;
  const now = c.currentTime;
  for (const [dt, f] of [
    [0, 659.25],
    [0.115, 987.77],
  ] as const) {
    playTone(c, out, { type: 'sine', freq: f, start: now + dt, attack: 0.003, dur: 0.36, amp: 0.24 });
    playTone(c, out, { type: 'sine', freq: f * 2.0, start: now + dt, attack: 0.002, dur: 0.16, amp: 0.07 });
  }
}

/** Crisp double coin clink. */
export function playTradeDone(): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out } = a;
  const now = c.currentTime;
  const clinks = [
    { dt: 0, f1: 2093.0, f2: 3135.96 },
    { dt: 0.082, f1: 2349.32, f2: 3520.0 },
  ];
  for (const cl of clinks) {
    playTone(c, out, { type: 'sine', freq: cl.f1, start: now + cl.dt, attack: 0.002, dur: 0.18, amp: 0.22 });
    playTone(c, out, { type: 'sine', freq: cl.f2, start: now + cl.dt, attack: 0.0015, dur: 0.12, amp: 0.12 });
  }
}

/** Stealthy low pizzicato step for the robber, or low brass swell + splash for the pirate. */
export function playRobber(isPirate = false): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;

  if (isPirate) {
    playNoise(c, out, noise, { start: now, attack: 0.015, dur: 0.24, amp: 0.22, freq: 620, endFreq: 280, q: 1.5 });
    playTone(c, out, { type: 'sawtooth', freq: 73.42, endFreq: 65.41, start: now + 0.02, attack: 0.06, dur: 0.44, amp: 0.26, lpFreq: 540 });
    playTone(c, out, { type: 'sawtooth', freq: 110.0, endFreq: 98.0, start: now + 0.03, attack: 0.07, dur: 0.42, amp: 0.18, lpFreq: 580 });
  } else {
    // Two stealthy plucked contrabass pizzicato footsteps (A2 -> E2).
    for (const [dt, f] of [
      [0, 110.0],
      [0.125, 82.41],
    ] as const) {
      playTone(c, out, { type: 'triangle', freq: f * 1.05, endFreq: f, start: now + dt, attack: 0.004, dur: 0.22, amp: 0.36, lpFreq: 520 });
      playNoise(c, out, noise, { start: now + dt, attack: 0.002, dur: 0.022, amp: 0.14, freq: 480, q: 2.0 });
    }
  }
}

/** Barbarian raid war-horn followed by triumphant major chord (defended) or minor impact (pillaged). */
export function playBarbarianRaid(defended: boolean): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;

  // Deep war-horn call (A2 + E3).
  playTone(c, out, { type: 'sawtooth', freq: 110.0, endFreq: 112.0, start: now, attack: 0.07, dur: 0.36, amp: 0.26, lpFreq: 760 });
  playTone(c, out, { type: 'sawtooth', freq: 164.81, start: now + 0.02, attack: 0.07, dur: 0.34, amp: 0.2, lpFreq: 860 });

  const resolveAt = now + 0.32;
  if (defended) {
    // Triumphant A-major brass/chime flourish (A4 - C#5 - E5 - A5).
    const chord = [440.0, 554.37, 659.25, 880.0];
    chord.forEach((f, i) => {
      playTone(c, out, {
        type: 'triangle',
        freq: f,
        start: resolveAt + i * 0.035,
        attack: 0.008,
        dur: 0.56,
        amp: 0.2,
        lpFreq: 2800,
      });
    });
  } else {
    // Pillaged city: heavy drum/stone impact + dark A-minor/diminished chord.
    playTone(c, out, { type: 'sine', freq: 108, endFreq: 55.0, start: resolveAt, attack: 0.005, dur: 0.58, amp: 0.48 });
    playNoise(c, out, noise, { start: resolveAt, attack: 0.004, dur: 0.18, amp: 0.3, filterType: 'lowpass', freq: 380, q: 1.2 });
    for (const f of [110.0, 130.81, 164.81, 220.0]) {
      playTone(c, out, { type: 'sawtooth', freq: f, start: resolveAt + 0.02, attack: 0.02, dur: 0.55, amp: 0.16, lpFreq: 520 });
    }
  }
}

/** Subtle two-note wooden chime (`C5 -> G5`) alerting the player it is their turn. */
export function playYourTurn(): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;
  for (const [dt, f] of [
    [0, 523.25],
    [0.11, 783.99],
  ] as const) {
    playTone(c, out, { type: 'sine', freq: f, start: now + dt, attack: 0.004, dur: 0.3, amp: 0.26 });
    woodKnock(c, out, noise, now + dt, f, 0.16, 0.05);
  }
}

/** Subtle wooden escapement clock tick (`urgent` raises pitch for `<= 5s`). */
export function playTimerTick(urgent: boolean): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out, noise } = a;
  const now = c.currentTime;
  const pitch = urgent ? 940 : 640;
  woodKnock(c, out, noise, now, pitch, urgent ? 0.24 : 0.14, 0.036);
  if (urgent) {
    playTone(c, out, { type: 'sine', freq: 1410, start: now + 0.045, attack: 0.002, dur: 0.045, amp: 0.12 });
  }
}

/** Warm 4-note victory fanfare (`C5-E5-G5-C6`) when won, or gentle resolving cadence otherwise. */
export function playGameOver(won: boolean): void {
  const a = ensureAudio();
  if (!a) return;
  const { c, out } = a;
  const now = c.currentTime;

  if (won) {
    const fanfare = [523.25, 659.25, 783.99, 1046.5]; // C5 -> E5 -> G5 -> C6
    fanfare.forEach((f, i) => {
      const last = i === fanfare.length - 1;
      const t = now + i * 0.115;
      playTone(c, out, {
        type: 'triangle',
        freq: f,
        start: t,
        attack: 0.008,
        dur: last ? 0.85 : 0.32,
        amp: last ? 0.28 : 0.22,
        lpFreq: 3000,
      });
    });
    // Warm C-major root harmony under the final C6.
    for (const f of [261.63, 392.0, 659.25]) {
      playTone(c, out, { type: 'sine', freq: f, start: now + 0.345, attack: 0.02, dur: 0.82, amp: 0.14 });
    }
  } else {
    // Gentle warm acoustic resolution (Dm7 -> Cmaj).
    for (const f of [293.66, 440.0, 523.25]) {
      playTone(c, out, { type: 'triangle', freq: f, start: now, attack: 0.015, dur: 0.34, amp: 0.16, lpFreq: 1600 });
    }
    for (const f of [261.63, 392.0, 493.88, 523.25]) {
      playTone(c, out, { type: 'sine', freq: f, start: now + 0.26, attack: 0.025, dur: 0.72, amp: 0.15 });
    }
  }
}

const hasAny = (counts: readonly number[]) => counts.some((n) => n > 0);

/**
 * Inspects a batch of game `Event`s from one step and plays the most
 * appropriate non-overlapping sound(s) for viewer seat `me` (`null` for spectators).
 */
export function playForEvents(events: Event[], me: number | null): void {
  if (muted || !events.length) return;

  let dice: [number, number] | null = null;
  let raid: 'defended' | 'pillaged' | null = null;
  let fogRevealed = false;
  let fogGold = false;
  let build: BuildSoundKind | null = null;
  let robber: 'robber' | 'pirate' | null = null;
  let trade: 'offer' | 'done' | null = null;
  let gained = false;
  let wonSeat: number | null = null;

  for (const e of events) {
    switch (e.type) {
      case 'DiceRolled':
        dice = e.value.dice;
        break;
      case 'BarbariansRepelled':
        raid = 'defended';
        break;
      case 'CityPillaged':
        raid = 'pillaged';
        break;
      case 'TileRevealed':
        fogRevealed = true;
        if (e.value.terrain === 'Gold') fogGold = true;
        break;
      case 'CityUpgraded':
        build = 'upgrade';
        break;
      case 'CityBuilt':
        if (build !== 'upgrade') build = 'city';
        break;
      case 'SettlementBuilt':
        build ??= 'settlement';
        break;
      case 'KnightRecruited':
      case 'KnightActivated':
      case 'KnightPromoted':
      case 'KnightMoved':
        build ??= 'knight';
        break;
      case 'ShipBuilt':
      case 'ShipMoved':
        build ??= 'ship';
        break;
      case 'RoadBuilt':
        build ??= 'road';
        break;
      case 'DevCardBought':
        build ??= 'upgrade';
        break;
      case 'DevCardPlayed':
        if (e.value.card === 'Knight') build ??= 'knight';
        else if (e.value.card === 'RoadBuilding') build ??= 'road';
        break;
      case 'PirateMoved':
        robber = 'pirate';
        break;
      case 'RobberMoved':
        robber ??= 'robber';
        break;
      case 'TradeExecuted':
      case 'MaritimeTraded':
      case 'GoodTraded':
        trade = 'done';
        break;
      case 'TradeProposed':
        trade ??= 'offer';
        break;
      case 'ResourcesProduced':
      case 'StartingResources':
      case 'YearOfPlentyTaken':
      case 'GoldChosen':
        if ((me === null || e.value.player === me) && hasAny(e.value.resources)) gained = true;
        break;
      case 'CommoditiesProduced':
        if ((me === null || e.value.player === me) && hasAny(e.value.commodities)) gained = true;
        break;
      case 'MonopolyTaken':
        if ((me === null || e.value.player === me) && hasAny(e.value.taken)) gained = true;
        break;
      case 'FishCaught':
      case 'FishResourceTaken':
        if (me === null || e.value.player === me) gained = true;
        break;
      case 'FishSpent':
        trade = 'done';
        break;
      case 'OldBootDrawn':
      case 'OldBootPassed':
        robber ??= 'robber';
        break;
      case 'GameWon':
        wonSeat = e.value.player;
        break;
      default:
        break;
    }
  }

  let delayMs = 0;

  if (dice) {
    playDice(dice[0], dice[1]);
    delayMs = 200;
    if (raid !== null) {
      const defended = raid === 'defended';
      defer(220, () => playBarbarianRaid(defended));
      delayMs = 560;
    } else if (gained && dice[0] + dice[1] !== 7) {
      defer(180, () => playResourceGain());
      delayMs = 340;
    }
  } else if (raid !== null) {
    playBarbarianRaid(raid === 'defended');
    delayMs = 360;
  } else if (build !== null) {
    playBuild(build);
    delayMs = 150;
  } else if (robber !== null) {
    playRobber(robber === 'pirate');
    delayMs = 180;
  } else if (trade === 'done') {
    playTradeDone();
    delayMs = 150;
  } else if (trade === 'offer') {
    playTradeOffer();
    delayMs = 160;
  } else if (gained) {
    playResourceGain();
    delayMs = 160;
  }

  if (fogRevealed) {
    const isGold = fogGold;
    if (delayMs > 0) defer(delayMs, () => playFogReveal(isGold));
    else playFogReveal(isGold);
    delayMs += 200;
  }

  if (wonSeat !== null) {
    const won = me === null || wonSeat === me;
    if (delayMs > 0) defer(delayMs, () => playGameOver(won));
    else playGameOver(won);
  }
}
