import { createEffect, createSignal, For, onCleanup, Show, type JSX } from 'solid-js';
import { Portal } from 'solid-js/web';
import { PLAYER_CSS } from '../board/palette';
import { t } from '../i18n';
import { SeatDot } from './bits';

export type QuickEmoteKey =
  | 'NeedBrick'
  | 'NeedGrainOre'
  | 'OpenToTrade'
  | 'GoodRoll'
  | 'MercyRobber'
  | 'WatchLeader'
  | 'DefendTogether'
  | 'WellPlayed';

export const QUICK_EMOTES: readonly { key: QuickEmoteKey; icon: string; zh: string; en: string }[] = [
  { key: 'NeedBrick', icon: '🧱', zh: '缺砖木求换！', en: 'Need Brick/Wood!' },
  { key: 'NeedGrainOre', icon: '🌾', zh: '求麦矿升城！', en: 'Need Grain/Ore!' },
  { key: 'OpenToTrade', icon: '🤝', zh: '欢迎报价交易！', en: 'Open to trade!' },
  { key: 'GoodRoll', icon: '🎲', zh: '好骰子！', en: 'Nice roll!' },
  { key: 'MercyRobber', icon: '🏳️', zh: '手下留情别抢我！', en: 'Spare me!' },
  { key: 'WatchLeader', icon: '👀', zh: '盯紧榜首！', en: 'Watch the leader!' },
  { key: 'DefendTogether', icon: '🛡️', zh: '快激活铁卫守城！', en: 'Defend the cities!' },
  { key: 'WellPlayed', icon: '👏', zh: '精彩好局！', en: 'Well played!' },
];

const EMOTE_MAP: Record<QuickEmoteKey, (typeof QUICK_EMOTES)[number]> = {
  NeedBrick: QUICK_EMOTES[0],
  NeedGrainOre: QUICK_EMOTES[1],
  OpenToTrade: QUICK_EMOTES[2],
  GoodRoll: QUICK_EMOTES[3],
  MercyRobber: QUICK_EMOTES[4],
  WatchLeader: QUICK_EMOTES[5],
  DefendTogether: QUICK_EMOTES[6],
  WellPlayed: QUICK_EMOTES[7],
};

/** Formats a localized tabletop emote phrase (`${icon} ${t(zh, en)}`). */
export function emoteText(emote: QuickEmoteKey): string {
  const item = EMOTE_MAP[emote];
  return item ? `${item.icon} ${t(item.zh, item.en)}` : String(emote);
}

export interface ActiveBubble {
  id: number;
  seat: number | null;
  name: string;
  emote: QuickEmoteKey;
}

const QUICK_CHAT_STYLE_ATTR = 'data-isle-quick-chat';

function ensureQuickChatStyles(): void {
  if (typeof document === 'undefined' || !document.head) return;
  if (document.head.querySelector(`style[${QUICK_CHAT_STYLE_ATTR}]`)) return;
  const style = document.createElement('style');
  style.setAttribute(QUICK_CHAT_STYLE_ATTR, '');
  style.textContent = `
.isle-qc-popover {
  position: fixed;
  z-index: 60;
  width: min(352px, calc(100vw - 16px));
  padding: 8px;
  background: rgba(11, 34, 48, 0.96);
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: 12px;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.52);
  backdrop-filter: blur(10px);
  animation: isle-qc-pop 0.14s cubic-bezier(0.22, 1.2, 0.36, 1);
}
.isle-qc-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
}
.isle-qc-item {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 7px 9px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  color: var(--text, #eef4f6);
  font-size: 12.5px;
  line-height: 1.25;
  text-align: left;
  cursor: pointer;
  transition: background 0.14s, border-color 0.14s, transform 0.06s;
}
.isle-qc-item:hover {
  background: rgba(240, 180, 41, 0.18);
  border-color: rgba(240, 180, 41, 0.55);
}
.isle-qc-item:active {
  transform: translateY(1px);
}
.isle-qc-icon {
  flex: none;
  font-size: 16px;
  line-height: 1;
}
.isle-qc-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}
.isle-speech-layer {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: 35;
}
.isle-speech-bubble {
  position: fixed;
  pointer-events: none;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  max-width: min(280px, calc(100vw - 24px));
  padding: 6px 11px;
  border-radius: 999px;
  background: rgba(12, 30, 42, 0.95);
  border: 1.5px solid var(--seat-accent, rgba(240, 180, 41, 0.65));
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.46);
  color: var(--text, #eef4f6);
  font-size: 13px;
  line-height: 1.3;
  white-space: nowrap;
  animation: isle-bubble-in 0.22s cubic-bezier(0.22, 1.35, 0.36, 1);
}
.isle-speech-bubble::after {
  content: '';
  position: absolute;
  width: 8px;
  height: 8px;
  background: rgba(12, 30, 42, 0.95);
  border-color: var(--seat-accent, rgba(240, 180, 41, 0.65));
  border-style: solid;
  border-width: 0;
}
.isle-speech-bubble.tail-right::after {
  right: -5px;
  top: 50%;
  margin-top: -4px;
  border-top-width: 1.5px;
  border-right-width: 1.5px;
  transform: rotate(45deg);
}
.isle-speech-bubble.tail-left::after {
  left: -5px;
  top: 50%;
  margin-top: -4px;
  border-bottom-width: 1.5px;
  border-left-width: 1.5px;
  transform: rotate(45deg);
}
.isle-speech-bubble.tail-top::after {
  top: -5px;
  left: 50%;
  margin-left: -4px;
  border-top-width: 1.5px;
  border-left-width: 1.5px;
  transform: rotate(45deg);
}
.isle-speech-bubble.tail-top-left::after {
  top: -5px;
  left: 18px;
  border-top-width: 1.5px;
  border-left-width: 1.5px;
  transform: rotate(45deg);
}
.isle-speech-speaker {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--muted, #9fb7c2);
  max-width: 84px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.isle-speech-text {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
}
@keyframes isle-qc-pop {
  from {
    opacity: 0;
    transform: scale(0.95) translateY(-3px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}
@keyframes isle-bubble-in {
  from {
    opacity: 0;
    transform: var(--bubble-shift, translate(0, 0)) scale(0.84);
  }
  to {
    opacity: 1;
    transform: var(--bubble-shift, translate(0, 0)) scale(1);
  }
}
@media (prefers-reduced-motion: reduce) {
  .isle-qc-popover,
  .isle-speech-bubble {
    animation: none;
  }
}
`;
  document.head.appendChild(style);
}

/** Compact `💬` / `快捷语` button opening a floating 2×4 tabletop emote picker popover. */
export function QuickChatTrigger(props: { onSend: (emote: QuickEmoteKey) => void; class?: string }) {
  ensureQuickChatStyles();
  const [open, setOpen] = createSignal(false);
  const [coords, setCoords] = createSignal<{ left: number; top: number }>({ left: 12, top: 48 });
  let triggerRef: HTMLButtonElement | undefined;
  let popoverRef: HTMLDivElement | undefined;

  const syncPopoverPos = () => {
    if (typeof window === 'undefined' || !triggerRef) return;
    const btn = triggerRef.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const popW = popoverRef?.offsetWidth || Math.min(352, vw - 16);
    const popH = popoverRef?.offsetHeight || 196;

    const preferRightAlign = btn.left > vw * 0.55;
    const rawLeft = preferRightAlign ? btn.right - popW : btn.left;
    const left = Math.min(Math.max(8, rawLeft), Math.max(8, vw - popW - 8));

    const spaceBelow = vh - btn.bottom;
    const openAbove = spaceBelow < popH + 14 && btn.top > spaceBelow;
    const rawTop = openAbove ? btn.top - popH - 6 : btn.bottom + 6;
    const top = Math.min(Math.max(8, rawTop), Math.max(8, vh - popH - 8));

    setCoords({ left: Math.round(left), top: Math.round(top) });
  };

  createEffect(() => {
    if (!open() || typeof window === 'undefined') return;
    syncPopoverPos();
    const raf = window.requestAnimationFrame(syncPopoverPos);

    const onPointerDown = (ev: PointerEvent) => {
      const target = ev.target as Node | null;
      if (target && (triggerRef?.contains(target) || popoverRef?.contains(target))) return;
      setOpen(false);
    };
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') {
        ev.preventDefault();
        ev.stopPropagation();
        setOpen(false);
        triggerRef?.focus();
      }
    };

    document.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('resize', syncPopoverPos);
    window.addEventListener('scroll', syncPopoverPos, true);

    onCleanup(() => {
      window.cancelAnimationFrame(raf);
      document.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('resize', syncPopoverPos);
      window.removeEventListener('scroll', syncPopoverPos, true);
    });
  });

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        class={props.class ?? 'ghost'}
        classList={{ on: open() }}
        aria-haspopup="dialog"
        aria-expanded={open()}
        aria-label={t('快捷语', 'Quick Emotes')}
        title={t('发送桌游快捷语', 'Send tabletop quick emote')}
        data-testid="quick-chat-trigger"
        onClick={() => setOpen((prev) => !prev)}
      >
        💬 {t('快捷语', 'Emotes')}
      </button>
      <Show when={open()}>
        <Portal>
          <div
            ref={popoverRef}
            class="isle-qc-popover"
            role="dialog"
            aria-label={t('桌游快捷语', 'Tabletop Quick Emotes')}
            data-testid="quick-chat-popover"
            style={{
              left: `${coords().left}px`,
              top: `${coords().top}px`,
            }}
          >
            <div class="isle-qc-grid">
              <For each={QUICK_EMOTES}>
                {(item) => (
                  <button
                    type="button"
                    class="isle-qc-item"
                    data-emote={item.key}
                    data-testid={`quick-emote-${item.key}`}
                    title={`${item.icon} ${t(item.zh, item.en)}`}
                    onClick={() => {
                      props.onSend(item.key);
                      setOpen(false);
                    }}
                  >
                    <span class="isle-qc-icon" aria-hidden="true">
                      {item.icon}
                    </span>
                    <span class="isle-qc-label">{t(item.zh, item.en)}</span>
                  </button>
                )}
              </For>
            </div>
          </div>
        </Portal>
      </Show>
    </>
  );
}

type TailSide = 'right' | 'left' | 'top' | 'top-left';

interface BubblePlacement {
  style: JSX.CSSProperties;
  tail: TailSide;
}

function clamp(val: number, lo: number, hi: number): number {
  return Math.min(Math.max(val, lo), Math.max(lo, hi));
}

function computeBubblePlacement(seat: number | null, stackIdx: number): BubblePlacement {
  const accent =
    seat !== null && seat >= 0 ? PLAYER_CSS[seat % PLAYER_CSS.length] : 'rgba(240, 180, 41, 0.72)';

  if (typeof document === 'undefined' || typeof window === 'undefined') {
    return {
      tail: 'top-left',
      style: {
        left: '16px',
        top: `${52 + stackIdx * 40}px`,
        '--seat-accent': accent,
      },
    };
  }

  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const seatEl = seat !== null ? document.getElementById(`seat-${seat}`) : null;
  const rect = seatEl?.getBoundingClientRect();

  if (rect && rect.width > 0 && rect.height > 0) {
    const spaceLeft = rect.left;
    const spaceRight = vw - rect.right;

    // Desktop right-side player panel: place bubble immediately to the left of `#seat-${seat}`.
    if (spaceLeft >= 190 && spaceLeft >= spaceRight) {
      const top = clamp(Math.round(rect.top + rect.height / 2 - 18 + stackIdx * 40), 10, vh - 48);
      return {
        tail: 'right',
        style: {
          right: `${Math.round(vw - rect.left + 10)}px`,
          top: `${top}px`,
          '--seat-accent': accent,
        },
      };
    }

    // Left-aligned seat card: place bubble immediately to the right of `#seat-${seat}`.
    if (spaceRight >= 190) {
      const top = clamp(Math.round(rect.top + rect.height / 2 - 18 + stackIdx * 40), 10, vh - 48);
      return {
        tail: 'left',
        style: {
          left: `${Math.round(rect.right + 10)}px`,
          top: `${top}px`,
          '--seat-accent': accent,
        },
      };
    }

    // Compact / mobile horizontal seat strip: place bubble right underneath `#seat-${seat}`.
    const cx = clamp(Math.round(rect.left + rect.width / 2), 116, vw - 116);
    const top = clamp(Math.round(rect.bottom + 8 + stackIdx * 40), 10, vh - 48);
    return {
      tail: 'top',
      style: {
        left: `${cx}px`,
        top: `${top}px`,
        transform: 'translateX(-50%)',
        '--bubble-shift': 'translateX(-50%)',
        '--seat-accent': accent,
      },
    };
  }

  // Spectator (`seat === null`) or missing seat card: anchor top-left directly under `.topbar`.
  const topbar = document.querySelector('.topbar');
  const tr = topbar?.getBoundingClientRect();
  const left = tr && tr.width > 0 ? Math.max(12, Math.round(tr.left)) : 16;
  const baseTop = tr && tr.height > 0 ? Math.round(tr.bottom + 8) : 52;
  return {
    tail: 'top-left',
    style: {
      left: `${left}px`,
      top: `${clamp(baseTop + stackIdx * 40, 10, vh - 48)}px`,
      '--seat-accent': accent,
    },
  };
}

/** Non-blocking (`pointer-events: none`) animated tabletop speech bubbles anchored beside `#seat-${seat}`. */
export function SeatSpeechBubbles(props: { bubbles: ActiveBubble[] }) {
  ensureQuickChatStyles();
  const [layoutTick, setLayoutTick] = createSignal(0);

  createEffect(() => {
    if (!props.bubbles.length || typeof window === 'undefined') return;
    const bump = () => setLayoutTick((n) => n + 1);
    const raf = window.requestAnimationFrame(bump);
    window.addEventListener('resize', bump);
    onCleanup(() => {
      window.cancelAnimationFrame(raf);
      window.removeEventListener('resize', bump);
    });
  });

  const placedBubbles = () => {
    layoutTick();
    const counts = new Map<string, number>();
    return props.bubbles.map((b) => {
      const bucket = b.seat !== null && document.getElementById(`seat-${b.seat}`) ? `seat-${b.seat}` : 'topbar';
      const stackIdx = counts.get(bucket) ?? 0;
      counts.set(bucket, stackIdx + 1);
      return {
        bubble: b,
        placement: computeBubblePlacement(b.seat, stackIdx),
      };
    });
  };

  return (
    <Show when={props.bubbles.length > 0}>
      <Portal>
        <div class="isle-speech-layer" role="log" aria-live="polite" data-testid="seat-speech-bubbles">
          <For each={placedBubbles()}>
            {({ bubble, placement }) => (
              <div
                class={`isle-speech-bubble tail-${placement.tail}`}
                data-bubble-id={bubble.id}
                data-seat={bubble.seat ?? 'spectator'}
                style={placement.style}
              >
                <Show when={bubble.seat !== null}>
                  <SeatDot seat={bubble.seat!} />
                </Show>
                <span class="isle-speech-speaker">{bubble.name}</span>
                <span class="isle-speech-text">{emoteText(bubble.emote)}</span>
              </div>
            )}
          </For>
        </div>
      </Portal>
    </Show>
  );
}
