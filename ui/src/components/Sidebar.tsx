import { createEffect, createMemo, createSignal, For, on, Show } from 'solid-js';
import { api, type Calendar } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { addDays, addMonths, isoDate, monthYear, sameDay, startOfMonth, startOfWeek, WEEKDAYS } from '../dates';

function MiniMonth() {
  const [month, setMonth] = createSignal(startOfMonth(store.anchor()));
  // Follow navigation in the main view.
  createEffect(on(store.anchor, (a) => setMonth(startOfMonth(a)), { defer: true }));

  const cells = createMemo(() => {
    const first = startOfWeek(month());
    const [rs, re] = store.range();
    const highlight = store.view() === 'week' || store.view() === 'day';
    return Array.from({ length: 42 }, (_, i) => {
      const d = addDays(first, i);
      const t = d.getTime();
      return {
        d,
        out: d.getMonth() !== month().getMonth(),
        today: sameDay(d, store.now()),
        sel: sameDay(d, store.anchor()),
        range: highlight && t >= rs && t < re,
      };
    });
  });

  return (
    <div class="mini">
      <div class="mini-head">
        <span>{monthYear(month())}</span>
        <div style={{ display: 'flex' }}>
          <button class="ib sm" aria-label="Предыдущий месяц" onClick={() => setMonth(addMonths(month(), -1))}>
            {I.left({ size: 14 })}
          </button>
          <button class="ib sm" aria-label="Следующий месяц" onClick={() => setMonth(addMonths(month(), 1))}>
            {I.right({ size: 14 })}
          </button>
        </div>
      </div>
      <div class="mini-grid">
        <For each={WEEKDAYS}>{(w) => <div class="md head">{w}</div>}</For>
        <For each={cells()}>
          {(c) => (
            <button
              class="md"
              classList={{ out: c.out, today: c.today, sel: c.sel, range: c.range }}
              aria-label={isoDate(c.d)}
              onClick={() => store.goto(c.d)}
            >
              {c.d.getDate()}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

function CalendarRow(props: { cal: Calendar }) {
  return (
    <button
      class="cal"
      aria-pressed={props.cal.selected}
      title={props.cal.accessRole === 'reader' ? `${props.cal.summary} · только чтение` : props.cal.summary}
      onClick={() => store.run(api.setCalendarSelected(props.cal.accountId, props.cal.id, !props.cal.selected))}
    >
      <span class="sq" classList={{ off: !props.cal.selected }} style={{ background: props.cal.color }}>
        <Show when={props.cal.selected}>{I.check({ size: 11, stroke: 3.5 })}</Show>
      </span>
      <span class="nm" style={{ color: props.cal.selected ? undefined : 'var(--text-3)' }}>
        {props.cal.summary}
      </span>
    </button>
  );
}

export function Sidebar() {
  const groups = createMemo(() => {
    const s = store.snap();
    if (!s) return [];
    return s.accounts.map((a) => ({ account: a, cals: s.calendars.filter((c) => c.accountId === a.id) }));
  });
  const total = () => store.snap()?.calendars.length ?? 0;

  const create = () => {
    const start = new Date(store.anchor());
    const now = new Date();
    start.setHours(sameDay(start, now) ? now.getHours() + 1 : 10, 0, 0, 0);
    const d = store.newDraft(start.getTime(), start.getTime() + 3600_000);
    if (d) store.setDraft(d);
  };

  return (
    <aside class="sidebar">
      <button class="create-btn" onClick={create}>
        {I.plus({ size: 16, stroke: 2.2 })}Создать событие
        <span style={{ 'font-family': 'var(--mono)', 'font-size': '11px', opacity: 0.7 }}>N</span>
      </button>
      <MiniMonth />
      <div style={{ display: 'flex', 'flex-direction': 'column', gap: '14px' }}>
        <For each={groups()}>
          {(g) => (
            <div class="acc-group">
              <div class="acc-head">
                <span class="lbl" style={{ color: g.account.status === 'reauth' ? '#F0B877' : undefined }}>
                  {g.account.name || g.account.email.split('@')[0]}
                </span>
                <span class="em">{g.account.email}</span>
              </div>
              <Show when={g.account.status === 'reauth'}>
                <button class="cal warn-chip" onClick={() => store.setSettings(true)}>
                  Нужно войти снова →
                </button>
              </Show>
              <For each={g.cals}>{(c) => <CalendarRow cal={c} />}</For>
            </div>
          )}
        </For>
      </div>
      <div class="side-foot">
        {I.check({ size: 14 })}
        <span style={{ flex: 1 }}>
          {groups().length} акк. · {total()} календ.
        </span>
        <button class="ib sm" aria-label="Свернуть сайдбар (Ctrl+B)" title="Свернуть (Ctrl+B)" onClick={() => store.setSidebar(false)}>
          {I.sidebar({ size: 15 })}
        </button>
      </div>
    </aside>
  );
}
