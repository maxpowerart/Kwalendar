import { createMemo, For, Show } from 'solid-js';
import type { Instance } from '../api';
import { store } from '../store';
import { dateLabel, dayMonth, isoDate, sameDay, startOfDay, timeRange, weekdayLong } from '../dates';
import { t } from '../i18n';

export function AgendaView() {
  const groups = createMemo(() => {
    const map = new Map<string, { day: Date; list: Instance[] }>();
    const from = store.range()[0];
    for (const i of store.items()) {
      const d = startOfDay(Math.max(i.start, from));
      const k = isoDate(d);
      if (!map.has(k)) map.set(k, { day: d, list: [] });
      map.get(k)!.list.push(i);
    }
    return [...map.values()].sort((a, b) => a.day.getTime() - b.day.getTime());
  });

  return (
    <div class="agenda">
      <Show when={groups().length} fallback={<div class="empty"><p class="muted">{t().agendaEmpty}</p></div>}>
        <For each={groups()}>
          {(g) => (
            <div class="ag-day">
              <div class="ag-date" classList={{ today: sameDay(g.day, store.now()) }}>
                <b>{dateLabel(g.day, store.now())}</b>
                <span class="muted">{sameDay(g.day, store.now()) || sameDay(g.day, store.now() + 86400_000) ? `${weekdayLong(g.day)}, ${dayMonth(g.day)}` : weekdayLong(g.day)}</span>
              </div>
              <div class="ag-list">
                <For each={g.list}>
                  {(i) => (
                    <button
                      class="ag-item"
                      classList={{ sel: store.selected()?.key === i.key }}
                      style={{ opacity: i.end < store.now() ? 0.55 : 1 }}
                      onClick={() => store.setSelected(i)}
                    >
                      <span class="t">{i.allDay ? t().allDay : timeRange(i.start, i.end)}</span>
                      <span class="bar" style={{ background: i.color }} />
                      <span style={{ display: 'flex', 'flex-direction': 'column', 'min-width': 0 }}>
                        <span class="tt" style={{ 'text-decoration': i.selfResponse === 'declined' ? 'line-through' : undefined }}>
                          {i.title}
                        </span>
                        <Show when={i.location || i.meetLink}>
                          <span class="muted" style={{ 'font-size': '12px' }}>
                            {i.location ?? t().videoCall}
                          </span>
                        </Show>
                      </span>
                    </button>
                  )}
                </For>
              </div>
            </div>
          )}
        </For>
      </Show>
    </div>
  );
}
