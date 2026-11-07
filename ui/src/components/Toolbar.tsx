import { createMemo, For, Show } from 'solid-js';
import { shift, store, type View } from '../store';
import { I } from '../icons';
import { addDays, dayMonth, isoWeek, monthYear, startOfDay, startOfWeek, weekdayLong } from '../dates';
import { t } from '../i18n';

const VIEWS = (): { v: View; label: string; key: string }[] => [
  { v: 'day', label: t().viewDay, key: '1' },
  { v: 'week', label: t().viewWeek, key: '2' },
  { v: 'month', label: t().viewMonth, key: '3' },
  { v: 'agenda', label: t().viewAgenda, key: '4' },
];

export function Toolbar() {
  const title = createMemo(() => {
    const a = store.anchor();
    switch (store.view()) {
      case 'day':
        return { h: dayMonth(a), sub: weekdayLong(a) };
      case 'week': {
        const s = startOfWeek(a);
        const e = addDays(s, 6);
        const sub =
          s.getMonth() === e.getMonth()
            ? `${s.getDate()}–${e.getDate()} · ${t().weekN(isoWeek(s))}`
            : `${dayMonth(s)} – ${dayMonth(e)} · ${t().weekN(isoWeek(s))}`;
        return { h: monthYear(a), sub };
      }
      case 'month':
        return { h: monthYear(a), sub: '' };
      case 'agenda':
        return { h: t().viewAgenda, sub: t().agendaFrom(dayMonth(a)) };
    }
  });

  return (
    <div class="toolbar">
      <Show when={!store.sidebar()}>
        <button class="ib" aria-label={t().showSidebar} title={t().sidebarTitle} onClick={() => store.setSidebar(true)}>
          {I.sidebar({ size: 17 })}
        </button>
      </Show>
      <button class="btn" title={t().todayTitle} onClick={() => store.goto(startOfDay(new Date()))}>
        {t().today}
      </button>
      <div style={{ display: 'flex' }}>
        <button class="ib" aria-label={t().back} title={t().backTitle} onClick={() => store.setAnchor(shift(store.view(), store.anchor(), -1))}>
          {I.left()}
        </button>
        <button class="ib" aria-label={t().forward} title={t().forwardTitle} onClick={() => store.setAnchor(shift(store.view(), store.anchor(), 1))}>
          {I.right()}
        </button>
      </div>
      <h1>{title().h}</h1>
      <span class="muted">{title().sub}</span>
      <div style={{ flex: 1 }} />
      <div class="seg" role="group" aria-label={t().view}>
        <For each={VIEWS()}>
          {(x) => (
            <button classList={{ on: store.view() === x.v }} aria-pressed={store.view() === x.v} title={`${x.label} (${x.key})`} onClick={() => store.setView(x.v)}>
              {x.label}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}
