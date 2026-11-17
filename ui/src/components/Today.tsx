import { batch, createEffect, createMemo, createSignal, For, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';
import { api, emitToMain, isTauri, on as listen, type Instance, type SyncStatus } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { addDays, dateLabel, dayMonth, isoWeek, MIN, relative, sameDay, startOfDay, time, timeRange, weekdayLong, weekdayShort, whenLabel } from '../dates';
import { parseQuick } from '../quickparse';
import { tint } from '../layout';
import { applyLang, t } from '../i18n';
import { setTheme } from '../theme';
import { EventPanel } from './EventPanel';
import { Editor } from './Editor';
import mark from '../assets/mark.svg';

/** Gaps at least this long show up as "free 16:00–19:00 · 3 h". */
const FREE_MIN = 60;

type Row = { kind: 'event'; i: Instance } | { kind: 'now' } | { kind: 'free'; from: number; to: number };

function ago(ms: number, now: number): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return t().secAgo(s);
  const m = Math.round(s / 60);
  if (m < 60) return t().minAgo(m);
  return t().hoursAgo(Math.round(m / 60));
}

const dur = (ms: number) => {
  const m = Math.round(ms / MIN);
  return t().tdDur(Math.floor(m / 60), m % 60);
};

const placeOf = (i: Instance) =>
  i.meetLink ? (i.meetLink.includes('zoom') ? 'Zoom' : i.meetLink.includes('meet.google') ? 'Google Meet' : t().videoCall) : i.location ?? '';

/**
 * The tray popup: one day (today by default) with everything the main window can do with it —
 * details, joining, answering, editing, deleting, quick add. Rendered in the "today" window.
 */
export function Today() {
  let root!: HTMLDivElement;
  let quickEl!: HTMLInputElement;
  const [text, setText] = createSignal('');
  const [pinned, setPinned] = createSignal(false);
  const [tick, setTick] = createSignal(Date.now());
  const timer = setInterval(() => setTick(Date.now()), 5000);
  onCleanup(() => clearInterval(timer));

  // ---------- data ----------
  let refreshTimer: number | undefined;
  const scheduleRefresh = () => {
    clearTimeout(refreshTimer);
    refreshTimer = window.setTimeout(() => void store.refresh(), 60);
  };

  onMount(async () => {
    const s = await api.snapshot();
    applyLang(s.config.language, s.systemLang);
    batch(() => {
      store.setSnap(s);
      store.setViewSig('day');
      store.setAnchor(startOfDay(new Date()));
    });
    await store.load();
    const offs = await Promise.all([
      listen('kw:changed', scheduleRefresh),
      listen<SyncStatus>('kw:sync', (sync) => {
        const cur = store.snap();
        if (cur) store.setSnap({ ...cur, sync });
      }),
      listen<{ quick: boolean }>('kw:today-open', (a) => reset(!!a?.quick)),
    ]);
    onCleanup(() => offs.forEach((off) => off()));
    root.focus();
  });

  createEffect(on(store.range, () => void store.load(), { defer: true }));
  createEffect(() => setTheme(store.snap()?.config.theme ?? 'system'));
  createEffect(() => {
    const s = store.snap();
    if (s) applyLang(s.config.language, s.systemLang);
  });
  // A banner's "Sign in" asks for Settings: those live in the main window.
  createEffect(() => {
    if (store.settings()) {
      store.setSettings(false);
      void api.openMain().then(() => emitToMain('kw:open-settings'));
    }
  });

  /** Every opening starts on today, in the list. */
  function reset(quick: boolean) {
    batch(() => {
      store.setSelected(null);
      store.setDraft(null);
      store.setAnchor(startOfDay(new Date()));
      setText('');
    });
    void store.refresh();
    requestAnimationFrame(() => (quick ? quickEl : root)?.focus());
  }

  // ---------- the day ----------
  const day = () => store.anchor();
  const dayEnd = () => addDays(day(), 1).getTime();
  const isToday = () => sameDay(day(), store.now());
  const list = createMemo(() => store.items().filter((i) => i.start < dayEnd() && i.end > day().getTime()));
  const allDay = createMemo(() => list().filter((i) => i.allDay));
  const timed = createMemo(() =>
    list()
      .filter((i) => !i.allDay)
      .sort((a, b) => a.start - b.start || a.end - b.end),
  );
  const declined = (i: Instance) => i.selfResponse === 'declined';

  const rows = createMemo<Row[]>(() => {
    const now = store.now();
    const today = isToday();
    const out: Row[] = [];
    let nowPlaced = !today;
    let lastEnd: number | null = null;
    for (const i of timed()) {
      if (lastEnd !== null && i.start - lastEnd >= FREE_MIN * MIN && (!today || i.start > now)) {
        if (!nowPlaced && now < i.start) {
          out.push({ kind: 'now' });
          nowPlaced = true;
        }
        out.push({ kind: 'free', from: Math.max(lastEnd, today ? now : 0), to: i.start });
      }
      if (!nowPlaced && i.start > now) {
        out.push({ kind: 'now' });
        nowPlaced = true;
      }
      out.push({ kind: 'event', i });
      if (!declined(i)) lastEnd = Math.max(lastEnd ?? 0, i.end);
    }
    if (!nowPlaced && timed().length) out.push({ kind: 'now' });
    return out;
  });

  const next = createMemo(() => (isToday() ? timed().find((i) => i.start > store.now() && !declined(i)) : undefined));

  // ---------- actions ----------
  const shiftDay = (n: number) =>
    batch(() => {
      store.setSelected(null);
      store.setAnchor(addDays(day(), n));
    });
  const goToday = () =>
    batch(() => {
      store.setSelected(null);
      store.setAnchor(startOfDay(new Date()));
    });

  const hide = () => {
    if (isTauri) void api.todayHide();
  };

  const togglePin = () => {
    const v = !pinned();
    setPinned(v);
    void api.todayPin(v);
  };

  async function openApp() {
    const sel = store.selected();
    await api.openMain();
    if (sel) await emitToMain('kw:open-event', sel);
    else await emitToMain('kw:goto', day().getTime());
  }

  function newEvent(from?: number, to?: number) {
    let s = from;
    if (s === undefined) {
      const d = new Date(day());
      const now = new Date(store.now());
      d.setHours(isToday() ? now.getHours() + 1 : 10, 0, 0, 0);
      s = d.getTime();
    }
    const e = to ?? s + 3600_000;
    const d = store.newDraft(s, Math.min(e, s + 3600_000));
    if (d) store.setDraft(d);
  }

  // ---------- quick add ----------
  /** Words without a date land on the shown day: "now" there is this moment today, 08:00 on other days. */
  const refDate = () => {
    if (isToday()) return new Date(store.now());
    const d = new Date(day());
    d.setHours(8, 0, 0, 0);
    return d;
  };
  const parsed = createMemo(() => (text().trim() ? parseQuick(text(), store.writableCalendars(), refDate()) : null));
  const quickCal = () => parsed()?.calendar ?? store.defaultCalendar();

  async function addQuick() {
    const p = parsed();
    if (!p?.title) return;
    const c = quickCal();
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
    if (res === undefined) return;
    setText('');
    if (!sameDay(p.start, day())) store.setAnchor(startOfDay(p.start));
    store.banner(t().qaCreated(p.title, whenLabel(p)));
  }

  function quickDetails() {
    const p = parsed();
    if (!p) return;
    const d = store.newDraft(p.start, p.end, p.allDay, p.title);
    if (!d) return;
    const c = quickCal();
    if (c) Object.assign(d, { accountId: c.accountId, calendarId: c.id });
    setText('');
    store.setDraft(d);
  }

  const onQuickKey = (e: KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.ctrlKey) quickDetails();
      else void addQuick();
    } else if (e.key === 'Tab' && parsed()?.title && !e.shiftKey) {
      e.preventDefault();
      quickDetails();
    }
  };

  // ---------- keyboard ----------
  const onKey = (e: KeyboardEvent) => {
    if (store.draft()) return; // the editor handles its own keys
    const tag = (e.target as HTMLElement).tagName;
    const typing = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    const k = e.key.toLowerCase();
    if (e.key === 'Escape') {
      e.preventDefault();
      if (typing && text()) setText('');
      else if (store.selected()) store.setSelected(null);
      else hide();
      return;
    }
    if (e.key === 'F5') {
      e.preventDefault();
      void api.syncNow();
      return;
    }
    if (e.ctrlKey && (k === 'k' || k === 'л')) {
      e.preventDefault();
      quickEl.focus();
      return;
    }
    if (typing || e.ctrlKey || e.altKey || e.metaKey) return;
    const sel = store.selected();
    switch (e.code) {
      case 'ArrowLeft':
      case 'KeyK':
        shiftDay(-1);
        break;
      case 'ArrowRight':
      case 'KeyJ':
        shiftDay(1);
        break;
      case 'KeyT':
        goToday();
        break;
      case 'KeyN':
        newEvent();
        break;
      case 'KeyE':
        if (sel?.canEdit) store.setDraft(store.editDraft(sel));
        break;
      default:
        return;
    }
    e.preventDefault();
  };
  window.addEventListener('keydown', onKey);
  onCleanup(() => window.removeEventListener('keydown', onKey));

  // ---------- status ----------
  const muted = () => (store.snap()?.mutedUntil ?? 0) > store.now();
  const status = createMemo(() => {
    const s = store.snap()?.sync;
    if (!s || !store.snap()?.accounts.length) return { color: 'var(--muted)', text: t().noAccounts };
    const q = s.pending ? t().inQueue(s.pending) : '';
    switch (s.state) {
      case 'syncing':
        return { color: 'var(--icon)', text: `${t().syncing}${q}`, spin: true };
      case 'offline':
        return { color: 'var(--node)', text: `${t().offline}${q}` };
      case 'error':
        return { color: 'var(--err-text)', text: t().syncError };
      default:
        return { color: 'var(--ok)', text: s.lastSync ? `${t().synced(ago(s.lastSync, tick()))}${q}` : `${t().ready}${q}` };
    }
  });

  async function toggleMute() {
    await store.run(api.mute(muted() ? 0 : 60));
    await store.refresh();
  }

  return (
    <div class="td" ref={root} tabindex="-1" role="application" aria-label={t().tdAria}>
      <header class="td-head">
        <img src={mark} alt="" width="20" height="22" />
        <div class="td-title">
          <b>
            {/* Short weekday on other days: the "Today" button takes the room. */}
            <span class="tx">{`${isToday() ? weekdayLong(day()) : weekdayShort(day())}, ${dayMonth(day())}`}</span>
            <Show when={isToday()}>
              <span class="td-badge">{t().today}</span>
            </Show>
          </b>
          <span>{t().tdSub(isoWeek(day()), list().length)}</span>
        </div>
        <Show when={!isToday()}>
          <button class="chrome-btn" title={t().todayTitle} onClick={goToday}>
            {t().today}
          </button>
        </Show>
        <button class="cb" aria-label={t().back} title={t().tdPrevDay} onClick={() => shiftDay(-1)}>
          {I.left()}
        </button>
        <button class="cb" aria-label={t().forward} title={t().tdNextDay} onClick={() => shiftDay(1)}>
          {I.right()}
        </button>
        <button class="cb" classList={{ on: pinned() }} aria-pressed={pinned()} aria-label={pinned() ? t().tdUnpin : t().tdPin} title={pinned() ? t().tdUnpin : t().tdPin} onClick={togglePin}>
          {I.pushpin({ size: 15 })}
        </button>
        <button class="cb" aria-label={t().tdOpenApp} title={t().tdOpenApp} onClick={() => void openApp()}>
          {I.external({ size: 15 })}
        </button>
      </header>

      <Switch>
        <Match when={store.selected()} keyed>
          {(i) => <EventPanel inst={i} />}
        </Match>
        <Match when={!store.selected()}>
          <div class="td-body">
            <Show when={next()} keyed>
              {(n) => (
                <div class="td-next">
                  <div class="h">
                    <span class="lbl">{t().tdNext}</span>
                    <span class="in">{relative(n.start, n.end, store.now())}</span>
                  </div>
                  <div class="tt">
                    <span class="sq" style={{ background: n.color, width: '9px', height: '9px', 'border-radius': '2px' }} />
                    <span class="x">{n.title}</span>
                  </div>
                  <span class="w">{[timeRange(n.start, n.end), placeOf(n)].filter(Boolean).join(' · ')}</span>
                  <div class="acts">
                    <Show when={n.meetLink}>
                      <button class="join" onClick={() => api.openUrl(n.meetLink!)}>
                        {I.video({ size: 15 })}
                        {n.meetLink!.includes('zoom') ? t().joinZoom : n.meetLink!.includes('meet.google') ? t().joinMeet : t().join}
                      </button>
                    </Show>
                    <button class="btn" onClick={() => store.setSelected(n)}>
                      {t().tdDetails}
                    </button>
                  </div>
                </div>
              )}
            </Show>

            <Show when={allDay().length}>
              <div class="td-ad">
                <For each={allDay()}>
                  {(i) => (
                    <button class="ad" style={{ background: tint(i.color).bg, color: tint(i.color).fg }} title={whenLabel(i)} onClick={() => store.setSelected(i)}>
                      {i.title}
                    </button>
                  )}
                </For>
              </div>
            </Show>

            <div class="td-list">
              <For each={rows()}>
                {(r) => (
                  <Switch>
                    <Match when={r.kind === 'event' && r.i} keyed>
                      {(i) => {
                        const live = () => isToday() && i.start <= store.now() && i.end > store.now();
                        return (
                          <button
                            class="td-ev"
                            classList={{ past: i.end <= store.now(), declined: declined(i), pending: i.pending }}
                            onClick={() => store.setSelected(i)}
                          >
                            <span class="t">
                              {time(i.start)}
                              <span>{time(i.end)}</span>
                            </span>
                            <span class="bar" style={{ background: i.color }} />
                            <span class="m">
                              <span class="tt">{i.title}</span>
                              <span class="sub" classList={{ now: live() }}>
                                {live() ? `${t().happeningNow} · ${t().until(time(i.end))}` : placeOf(i) || store.calendarLabel(store.calendarOf(i.accountId, i.calendarId))}
                              </span>
                            </span>
                            <Show when={i.meetLink && i.end > store.now()}>{I.video({ size: 15 })}</Show>
                            <Show when={i.selfResponse === 'tentative'}>
                              <span class="rs">{t().maybe}</span>
                            </Show>
                          </button>
                        );
                      }}
                    </Match>
                    <Match when={r.kind === 'now'}>
                      <div class="td-now" aria-hidden="true">
                        <span class="p">{time(store.now())}</span>
                        <span class="d" />
                        <span class="l" />
                      </div>
                    </Match>
                    <Match when={r.kind === 'free' && r} keyed>
                      {(f) => (
                        <button class="td-free" title={t().tdFreeTitle} onClick={() => newEvent(f.from, f.to)}>
                          {t().tdFree(time(f.from), time(f.to), dur(f.to - f.from))}
                        </button>
                      )}
                    </Match>
                  </Switch>
                )}
              </For>
            </div>

            <Show when={!list().length && store.snap()}>
              <div class="td-empty">
                {I.calendar({ size: 40, stroke: 1.5 })}
                <b>{t().tdNothing}</b>
                <span>{t().tdNothingHint}</span>
              </div>
            </Show>
          </div>

          <div class="td-qa">
            <label class="td-qa-in">
              {I.plus({ size: 15, node: [19, 5, 2.2] })}
              <input
                ref={quickEl}
                placeholder={t().tdQuickPh}
                value={text()}
                onInput={(e) => setText(e.currentTarget.value)}
                onKeyDown={onQuickKey}
                aria-label={t().tdQuickAria}
                spellcheck={false}
              />
              <Show when={parsed()?.title}>
                <button class="ib sm" aria-label={t().qaCreateDetailed} title={t().qaCreateDetailed} onClick={quickDetails}>
                  {I.dots({ size: 15 })}
                </button>
              </Show>
            </label>
            <Show when={parsed()?.title ? parsed() : null} keyed>
              {(p) => (
                <div class="td-qa-pv">
                  <span class="dot" style={{ background: quickCal()?.color ?? 'var(--accent)' }} />
                  <span class="x">
                    <b>{p.title}</b>
                    {' · '}
                    {sameDay(p.start, day()) ? '' : `${dateLabel(p.start, store.now())} · `}
                    {p.allDay ? t().allDay : timeRange(p.start, p.end)}
                    {quickCal() ? ` · ${store.calendarLabel(quickCal())}` : ''}
                  </span>
                  <span class="kbd">↵</span>
                </div>
              )}
            </Show>
          </div>
        </Match>
      </Switch>

      <footer class="td-foot">
        <span class="s" title={store.snap()?.sync.message ?? ''}>
          <span class="dot" style={{ width: '7px', height: '7px', background: status().color }} />
          <span class="x">{status().text}</span>
        </span>
        <button class="ib sm" aria-label={t().syncNowAria} title={t().syncNowTitle} onClick={() => api.syncNow()}>
          <span class={status().spin ? 'spin' : ''} style={{ display: 'inline-flex' }}>
            {I.sync({ size: 14 })}
          </span>
        </button>
        <button
          class="ib sm"
          classList={{ on: muted() }}
          aria-pressed={muted()}
          aria-label={muted() ? t().unmute : t().tdMute}
          title={muted() ? t().unmuteHint(time(store.snap()!.mutedUntil)) : t().tdMute}
          onClick={() => void toggleMute()}
        >
          {muted() ? I.bellOff({ size: 14 }) : I.bell({ size: 14 })}
        </button>
        <button class="ib sm" aria-label={t().newEvent} title={`${t().newEvent} (N)`} onClick={() => newEvent()}>
          {I.plus({ size: 15, node: [19, 5, 2.2] })}
        </button>
      </footer>

      <Show when={store.draft()} keyed>
        {(d) => <Editor draft={d} />}
      </Show>

      <div class="banners" aria-live="polite">
        <For each={store.banners()}>
          {(b) => (
            <div class="banner" classList={{ warn: b.tone === 'warn', error: b.tone === 'error' }}>
              <span class="t">{b.text}</span>
              <Show when={b.action}>
                <button class="btn" style={{ height: '26px' }} onClick={() => b.action!.run()}>
                  {b.action!.label}
                </button>
              </Show>
              <button class="ib sm" aria-label={t().hide} onClick={() => store.setBanners((x) => x.filter((y) => y.id !== b.id))}>
                {I.close({ size: 13 })}
              </button>
            </div>
          )}
        </For>
      </div>
    </div>
  );
}
