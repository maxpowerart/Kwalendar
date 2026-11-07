import { createEffect, createMemo, createSignal, For, on, Show } from 'solid-js';
import { api, type Calendar } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { addDays, addMonths, isoDate, monthYear, sameDay, startOfMonth, startOfWeek, weekdays } from '../dates';
import { t } from '../i18n';

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
          <button class="ib sm" aria-label={t().prevMonth} onClick={() => setMonth(addMonths(month(), -1))}>
            {I.left({ size: 14 })}
          </button>
          <button class="ib sm" aria-label={t().nextMonth} onClick={() => setMonth(addMonths(month(), 1))}>
            {I.right({ size: 14 })}
          </button>
        </div>
      </div>
      <div class="mini-grid">
        <For each={weekdays()}>{(w) => <div class="md head">{w}</div>}</For>
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
      title={props.cal.accessRole === 'reader' ? `${store.calendarLabel(props.cal)} · ${t().readOnly}` : store.calendarLabel(props.cal)}
      onClick={() => store.run(api.setCalendarSelected(props.cal.accountId, props.cal.id, !props.cal.selected))}
    >
      <span class="sq" classList={{ off: !props.cal.selected }} style={{ background: props.cal.color }}>
        <Show when={props.cal.selected}>{I.check({ size: 10, stroke: 4 })}</Show>
      </span>
      <span class="nm" style={{ color: props.cal.selected ? undefined : 'var(--text-3)' }}>
        {store.calendarLabel(props.cal)}
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

  // Collapsed account sections survive restarts (per-machine UI state).
  const KEY = 'kw.collapsed';
  const [collapsed, setCollapsed] = createSignal<Set<string>>(
    new Set((() => {
      try {
        return JSON.parse(localStorage.getItem(KEY) ?? '[]') as string[];
      } catch {
        return [];
      }
    })()),
  );
  const toggle = (id: string) => {
    const next = new Set(collapsed());
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setCollapsed(next);
    try {
      localStorage.setItem(KEY, JSON.stringify([...next]));
    } catch {
      /* storage unavailable */
    }
  };

  const create = () => {
    const start = new Date(store.anchor());
    const now = new Date();
    start.setHours(sameDay(start, now) ? now.getHours() + 1 : 10, 0, 0, 0);
    const d = store.newDraft(start.getTime(), start.getTime() + 3600_000);
    if (d) store.setDraft(d);
  };

  return (
    <aside class="sidebar">
      <button class="btn create-btn" onClick={create}>
        {I.plus({ size: 16, stroke: 2.2, node: [19, 5, 2.4] })}{t().createEvent}
        <span class="kbd" style={{ 'margin-left': '4px' }}>N</span>
      </button>
      <MiniMonth />
      <div style={{ display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
        <For each={groups()}>
          {(g) => (
            <div class="acc-group">
              <button
                class="sec"
                classList={{ closed: collapsed().has(g.account.id) }}
                aria-expanded={!collapsed().has(g.account.id)}
                onClick={() => toggle(g.account.id)}
              >
                <svg class="chev" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                  <path d="M1 3l4 4 4-4z" fill="currentColor" />
                </svg>
                <span class="nm" title={g.account.name || g.account.email}>{g.account.name || g.account.email.split('@')[0]}</span>
                <span class="n">{g.account.email}</span>
              </button>
              <Show when={!collapsed().has(g.account.id)}>
                <Show when={g.account.status === 'reauth'}>
                  <button class="cal warn-chip" onClick={() => store.setSettings(true)}>
                    {t().reauthChip}
                  </button>
                </Show>
                <For each={g.cals}>{(c) => <CalendarRow cal={c} />}</For>
              </Show>
            </div>
          )}
        </For>
      </div>
      <div class="side-foot">
        <span style={{ flex: 1 }}>
          {t().sideFoot(groups().length, total())}
        </span>
        <button class="ib sm" aria-label={t().collapseSidebar} title={t().collapseTitle} onClick={() => store.setSidebar(false)}>
          {I.sidebar({ size: 15 })}
        </button>
      </div>
    </aside>
  );
}
