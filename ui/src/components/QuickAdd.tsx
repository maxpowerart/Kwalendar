import { createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { api, type Instance } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { parseQuick } from '../quickparse';
import { dateLabel, time, timeRange, whenLabel } from '../dates';
import { t } from '../i18n';

type Item =
  | { kind: 'create' }
  | { kind: 'edit' }
  | { kind: 'event'; inst: Instance }
  | { kind: 'goto'; label: string; run: () => void };

export function QuickAdd() {
  const [text, setText] = createSignal('');
  const [found, setFound] = createSignal<Instance[]>([]);
  const [sel, setSel] = createSignal(0);
  let input!: HTMLInputElement;
  let timer: number | undefined;

  onMount(() => input.focus());
  onCleanup(() => clearTimeout(timer));

  const parsed = createMemo(() => (text().trim() ? parseQuick(text(), store.writableCalendars(), new Date(store.now())) : null));
  const cal = () => parsed()?.calendar ?? store.defaultCalendar();
  const conflicts = createMemo(() => {
    const p = parsed();
    if (!p || p.allDay) return [];
    return store.items().filter((i) => !i.allDay && i.start < p.end && i.end > p.start && i.selfResponse !== 'declined');
  });

  const items = createMemo<Item[]>(() => {
    const list: Item[] = [];
    const p = parsed();
    if (p && p.title) list.push({ kind: 'create' }, { kind: 'edit' });
    for (const inst of found()) list.push({ kind: 'event', inst });
    if (!text().trim()) {
      list.push(
        { kind: 'goto', label: t().today, run: () => store.goto(new Date()) },
        { kind: 'goto', label: t().qaViewWeek, run: () => store.setView('week') },
        { kind: 'goto', label: t().qaViewMonth, run: () => store.setView('month') },
        { kind: 'goto', label: t().viewAgenda, run: () => store.setView('agenda') },
        { kind: 'goto', label: t().settings, run: () => store.setSettings(true) },
      );
    }
    return list;
  });

  const onInput = (v: string) => {
    setText(v);
    setSel(0);
    clearTimeout(timer);
    // Search on the words left after the date/time is stripped, plus the raw text.
    timer = window.setTimeout(async () => {
      const q = (parsed()?.title || v).trim();
      setFound(q.length >= 2 ? (await api.search(q).catch(() => [])).slice(0, 6) : []);
    }, 90);
  };

  const close = () => store.setQuickAdd(false);

  async function activate(item: Item | undefined) {
    const p = parsed();
    if (!item) return;
    if (item.kind === 'create' && p) {
      const c = cal();
      if (!c) return store.banner(t().qaNoCalendar, 'warn');
      const res = await store.run(
        api.createEvent({
          accountId: c.accountId,
          calendarId: c.id,
          title: p.title,
          allDay: p.allDay,
          start: p.allDay ? null : p.start,
          end: p.allDay ? null : p.end,
          startDate: p.allDay ? new Date(p.start).toLocaleDateString('sv-SE') : null,
          endDate: p.allDay ? new Date(p.end).toLocaleDateString('sv-SE') : null,
          timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
          reminders: null,
        }),
      );
      if (res !== undefined) {
        close();
        store.goto(p.start);
        store.banner(t().qaCreated(p.title, whenLabel(p)));
      }
    } else if (item.kind === 'edit' && p) {
      const d = store.newDraft(p.start, p.end, p.allDay, p.title);
      if (d) {
        const c = cal();
        if (c) Object.assign(d, { accountId: c.accountId, calendarId: c.id });
        close();
        store.setDraft(d);
      }
    } else if (item.kind === 'event') {
      close();
      store.goto(item.inst.start);
      store.setSelected(item.inst);
    } else if (item.kind === 'goto') {
      close();
      item.run();
    }
  }

  const onKey = (e: KeyboardEvent) => {
    const n = items().length;
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSel((s) => (n ? (s + 1) % n : 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSel((s) => (n ? (s - 1 + n) % n : 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (e.ctrlKey) void activate({ kind: 'edit' });
      else void activate(items()[sel()]);
    }
  };

  const idx = (it: Item) => items().indexOf(it);

  return (
    <div class="scrim" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog qa" role="dialog" aria-label={t().qaDialog} onKeyDown={onKey}>
        <div class="qa-input">
          <span style={{ color: 'var(--accent)', display: 'inline-flex' }}>{parsed()?.title ? I.plus({ size: 18 }) : I.search({ size: 18 })}</span>
          <input
            ref={input}
            placeholder={t().qaPlaceholder}
            value={text()}
            onInput={(e) => onInput(e.currentTarget.value)}
            aria-label={t().qaAria}
            spellcheck={false}
          />
          <span class="kbd">Esc</span>
        </div>

        <Show when={parsed()?.title}>
          <div class="qa-preview">
            <div style={{ display: 'flex', 'align-items': 'center', gap: '8px' }}>
              <span style={{ width: '10px', height: '10px', 'border-radius': '3px', background: cal()?.color ?? 'var(--accent)' }} />
              <span style={{ 'font-weight': 600, 'font-size': '14px' }}>{parsed()!.title}</span>
            </div>
            <div style={{ display: 'flex', 'flex-wrap': 'wrap', gap: '6px' }}>
              <span class="pill">{I.calendar({ size: 13 })}{dateLabel(parsed()!.start, store.now())}</span>
              <span class="pill">
                {I.clock({ size: 13 })}
                {parsed()!.allDay ? t().allDay : timeRange(parsed()!.start, parsed()!.end)}
              </span>
              <Show when={cal()}>
                <span class="pill">
                  <span class="dot" style={{ background: cal()!.color }} />
                  {store.calendarLabel(cal())}
                </span>
              </Show>
              <span class="pill">{I.bell({ size: 13 })}{t().qaCalDefault}</span>
            </div>
            <Show
              when={!parsed()!.allDay}
              fallback={null}
            >
              <p style={{ display: 'flex', 'align-items': 'center', gap: '6px', 'font-size': '12px', color: 'var(--muted)' }}>
                <span class="dot" style={{ width: '7px', height: '7px', background: conflicts().length ? 'var(--warn)' : 'var(--ok)' }} />
                {conflicts().length
                  ? t().qaOverlaps(conflicts().map((c) => `«${c.title}» ${time(c.start)}`).join(', '))
                  : t().qaFree}
              </p>
            </Show>
          </div>
        </Show>

        <div style={{ padding: '4px 6px 8px', 'overflow-y': 'auto' }}>
          <Show when={parsed()?.title}>
            <div class="qa-sec">{t().qaActions}</div>
          </Show>
          <For each={items()}>
            {(it) => {
              const on = () => idx(it) === sel();
              const props = { class: 'qa-item', classList: { on: on() }, onMouseEnter: () => setSel(idx(it)), onClick: () => activate(it) };
              if (it.kind === 'create')
                return (
                  <button {...props} classList={{ on: on() }}>
                    {I.plus()}<span class="nm">{t().createEvent}</span><span class="kbd">↵</span>
                  </button>
                );
              if (it.kind === 'edit')
                return (
                  <button {...props} classList={{ on: on() }}>
                    {I.edit()}<span class="nm">{t().qaCreateDetailed}</span><span class="kbd">Ctrl ↵</span>
                  </button>
                );
              if (it.kind === 'event')
                return (
                  <>
                    <Show when={found()[0] === it.inst}>
                      <div class="qa-sec">{t().qaFound}</div>
                    </Show>
                    <button {...props} classList={{ on: on() }}>
                      <span class="dot" style={{ background: it.inst.color }} />
                      <span class="nm">{it.inst.title}</span>
                      <span class="sub">
                        {dateLabel(it.inst.start, store.now())}
                        {it.inst.allDay ? '' : ` · ${time(it.inst.start)}`}
                      </span>
                    </button>
                  </>
                );
              return (
                <button {...props} classList={{ on: on() }}>
                  {I.right()}<span class="nm">{it.label}</span>
                </button>
              );
            }}
          </For>
        </div>

        <div class="qa-foot">
          <span><span class="kbd">↑↓</span>{t().qaSelect}</span>
          <span><span class="kbd">#</span>{t().qaCalendar}</span>
          <span><span class="kbd">Ctrl ↵</span>{t().qaDetails}</span>
          <span style={{ flex: 1 }} />
          <span>{t().qaLangs}</span>
        </div>
      </div>
    </div>
  );
}
