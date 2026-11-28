import { createSignal, For, onCleanup, type JSX } from 'solid-js';
import type { ResourceSet } from '../generated/proto';
import { isColorblindMode, onColorblindChange, SEAT_SYMBOLS } from '../board/a11y';
import { PLAYER_CSS } from '../board/palette';
import { COMMODITIES, COMMODITY_CSS, COMMODITY_NAME, COMMODITY_SHORT, RESOURCES, RESOURCE_CSS, RESOURCE_NAME, RESOURCE_SHORT } from '../game/text';

/** A small resource tile, e.g. for costs and trade pickers. */
export function ResIcon(props: { r: number; size?: 'sm' | 'md' | 'lg' }) {
  return (
    <span class={`res res-${props.size ?? 'sm'}`} style={{ '--c': RESOURCE_CSS[props.r] }} title={RESOURCE_NAME[RESOURCES[props.r]]}>
      {RESOURCE_SHORT[RESOURCES[props.r]]}
    </span>
  );
}

/** A small commodity tile (Paper, Cloth, Coin). */
export function CommIcon(props: { c: number; size?: 'sm' | 'md' | 'lg' }) {
  return (
    <span class={`res res-${props.size ?? 'sm'}`} style={{ '--c': COMMODITY_CSS[props.c] }} title={COMMODITY_NAME[COMMODITIES[props.c]]}>
      {COMMODITY_SHORT[COMMODITIES[props.c]]}
    </span>
  );
}

/** Inline list of resources with counts ("2×麦 1×矿"). */
export function SetView(props: { set: ResourceSet }) {
  return (
    <span class="setview">
      <For each={[...props.set.keys()].filter((i) => props.set[i] > 0)}>
        {(i) => (
          <span class="setview-item">
            {props.set[i] > 1 ? <b>{props.set[i]}×</b> : null}
            <ResIcon r={i} />
          </span>
        )}
      </For>
    </span>
  );
}

export function SeatDot(props: { seat: number }) {
  const [cb, setCb] = createSignal(isColorblindMode());
  onCleanup(onColorblindChange(setCb));
  return (
    <span class="dot" classList={{ cb: cb() }} style={{ background: PLAYER_CSS[props.seat] }}>
      {cb() ? SEAT_SYMBOLS[props.seat] ?? '' : ''}
    </span>
  );
}

/** − n + control. */
export function Stepper(props: { value: number; min?: number; max: number; onChange: (v: number) => void; label: JSX.Element }) {
  return (
    <div class="stepper">
      {props.label}
      <button class="icon" disabled={props.value <= (props.min ?? 0)} onClick={() => props.onChange(props.value - 1)} aria-label="减少">
        −
      </button>
      <span class="stepper-value">{props.value}</span>
      <button class="icon" disabled={props.value >= props.max} onClick={() => props.onChange(props.value + 1)} aria-label="增加">
        +
      </button>
    </div>
  );
}

export function Modal(props: { title: string; children: JSX.Element; onClose?: () => void; wide?: boolean }) {
  return (
    <div class="modal-backdrop" onClick={(e) => e.target === e.currentTarget && props.onClose?.()}>
      <div class={`modal${props.wide ? ' wide' : ''}`} role="dialog" aria-label={props.title}>
        <header>
          <h2>{props.title}</h2>
          {props.onClose ? (
            <button class="icon close" onClick={() => props.onClose?.()} aria-label="关闭">
              ×
            </button>
          ) : null}
        </header>
        {props.children}
      </div>
    </div>
  );
}

const DIE_PIPS: Record<number, number[]> = {
  1: [4],
  2: [0, 8],
  3: [0, 4, 8],
  4: [0, 2, 6, 8],
  5: [0, 2, 4, 6, 8],
  6: [0, 2, 3, 5, 6, 8],
};

export function Die(props: { value: number }) {
  return (
    <div class="die">
      <For each={[0, 1, 2, 3, 4, 5, 6, 7, 8]}>{(i) => <span class={DIE_PIPS[props.value]?.includes(i) ? 'pip on' : 'pip'} />}</For>
    </div>
  );
}
