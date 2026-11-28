import { createMemo, createSignal, For, Show } from 'solid-js';
import { localReplayHistory } from '../game/replay-history';

export function ReplayHistory(props: { ready: boolean; onOpen: (json: string) => void }) {
  const [query, setQuery] = createSignal('');
  const entries = localReplayHistory();
  const rows = entries.map(({ savedAt, record }) => {
    const winner = record.players[record.winner!];
    const sea =
      record.config.scenario === 'Archipelago'
        ? '航海 · '
        : record.config.scenario === 'FogIslands'
          ? '迷雾 · '
          : record.config.scenario === 'Citadels'
            ? '城邦 · '
            : record.config.scenario === 'Fishermen'
              ? '渔场 · '
              : '';
    const description = `${sea}${record.players.length} 人 · ${winner.name || `AI ${record.winner! + 1}`}获胜 · #${record.seed}`;
    const date = new Date(savedAt).toLocaleString();
    return { record, description, date };
  });
  const visible = createMemo(() => rows.filter(({ description, date }) =>
    `${description} ${date}`.toLowerCase().includes(query().trim().toLowerCase())));
  return (
    <details>
      <summary>本机回放</summary>
      <Show when={entries.length > 0} fallback={<p class="muted small">还没有已结束的本机对局。</p>}>
        <input type="search" aria-label="搜索回放" placeholder="人数、赢家、种子或日期" value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)} />
        <ul>
          <For each={visible()} fallback={<li>没有匹配的回放</li>}>
            {({ record, description, date }) => <li>
              <span>{description} · {date} </span>
              <button disabled={!props.ready} onClick={() => props.onOpen(JSON.stringify(record))}>打开本机回放</button>
            </li>}
          </For>
        </ul>
      </Show>
    </details>
  );
}
