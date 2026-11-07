import { createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import type { Instance } from '../api';
import { store } from '../store';
import { onSolid, tint } from '../layout';
import { addDays, DAY, isoDate, sameDay, shortDate, startOfDay, time, weekdays } from '../dates';
import { t as L } from '../i18n';

const CHIP = 22;

export function MonthView() {
  let gridEl!: HTMLDivElement;
  const [rowH, setRowH] = createSignal(140);

  const weeks = createMemo(() => {
    const [s] = store.range();
    const first = new Date(s);
    const month = store.anchor().getMonth();
    // 5 or 6 rows, depending on how the month falls.
    const lastOfMonth = new Date(store.anchor().getFullYear(), month + 1, 0);
    const n = Math.ceil((startOfDay(lastOfMonth).getTime() - first.getTime() + DAY) / (7 * DAY));
    return n;
  });

  const byDay = createMemo(() => {
    const map = new Map<string, Instance[]>();
    for (const i of store.items()) {
      for (let d = startOfDay(i.start); d.getTime() < Math.max(i.end, i.start + 1); d = addDays(d, 1)) {
        const k = isoDate(d);
        if (!map.has(k)) map.set(k, []);
        map.get(k)!.push(i);
      }
    }
    for (const list of map.values()) list.sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start - b.start);
    return map;
  });

  const cells = createMemo(() => {
    const [s] = store.range();
    const month = store.anchor().getMonth();
    return Array.from({ length: weeks() * 7 }, (_, i) => {
      const d = addDays(new Date(s), i);
      return {
        d,
        out: d.getMonth() !== month,
        today: sameDay(d, store.now()),
        past: startOfDay(d).getTime() < startOfDay(store.now()).getTime(),
        we: d.getDay() === 0 || d.getDay() === 6,
        list: byDay().get(isoDate(d)) ?? [],
      };
    });
  });

  onMount(() => {
    const measure = () => setRowH(gridEl.clientHeight / weeks());
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(gridEl);
    onCleanup(() => ro.disconnect());
  });

  const capacity = () => Math.max(1, Math.floor((rowH() - 34) / CHIP));

  return (
    <div class="month">
      <div class="month-head">
        <For each={weekdays()}>{(w) => <div class="wd">{w}</div>}</For>
      </div>
      <div class="month-grid" ref={gridEl} style={{ 'grid-template-rows': `repeat(${weeks()}, minmax(0, 1fr))` }}>
        <For each={cells()}>
          {(c) => {
            const shown = () => (c.list.length > capacity() ? c.list.slice(0, capacity() - 1) : c.list);
            const more = () => c.list.length - shown().length;
            return (
              <div
                class="mc"
                classList={{ out: c.out, today: c.today, past: c.past, we: c.we && !c.out }}
                onDblClick={(e) => {
                  if ((e.target as HTMLElement).classList.contains('mc')) {
                    const s = new Date(c.d);
                    s.setHours(10, 0, 0, 0);
                    const dr = store.newDraft(s.getTime(), s.getTime() + 3600_000);
                    if (dr) store.setDraft(dr);
                  }
                }}
              >
                <button class="mn" onClick={() => store.goto(c.d, 'day')}>
                  {c.d.getDate() === 1 ? shortDate(c.d) : c.d.getDate()}
                </button>
                <For each={shown()}>
                  {(i) => {
                    const sel = () => store.selected()?.key === i.key;
                    return (
                      <Show
                        when={i.allDay}
                        fallback={
                          <button
                            class="chip"
                            style={{ background: sel() ? 'var(--active)' : undefined, opacity: i.selfResponse === 'declined' ? 0.5 : undefined }}
                            onClick={() => store.setSelected(i)}
                            title={i.title}
                          >
                            <span class="dot" style={{ width: '7px', height: '7px', background: i.color }} />
                            <span class="ct">{time(i.start)}</span>
                            <span class="tt">{i.title}</span>
                          </button>
                        }
                      >
                        <button
                          class="chip-ad"
                          style={{ background: sel() ? tint(i.color).solid : tint(i.color).bg, color: sel() ? onSolid(i.color) : tint(i.color).fg }}
                          onClick={() => store.setSelected(i)}
                          title={i.title}
                        >
                          {i.title}
                        </button>
                      </Show>
                    );
                  }}
                </For>
                <Show when={more() > 0}>
                  <button class="more" onClick={() => store.goto(c.d, 'day')}>
                    {L().more(more())}
                  </button>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
