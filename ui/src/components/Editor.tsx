import { createMemo, createSignal, For, onMount, Show } from 'solid-js';
import { api, targetOf, type EventInput } from '../api';
import { store, type Draft } from '../store';
import { I } from '../icons';
import { addDays, DAY, hhmm, isoDate, minutesLabel, parseIsoDate, timeToMinutes } from '../dates';
import { descriptionText } from './EventPanel';
import { t } from '../i18n';

const REMINDER_OPTIONS = (): { label: string; value: number[] | null }[] => [
  { label: t().remCalendar, value: null },
  { label: t().remNone, value: [] },
  { label: t().remAtStart, value: [0] },
  { label: t().remMin(5), value: [5] },
  { label: t().remMin(10), value: [10] },
  { label: t().remMin(15), value: [15] },
  { label: t().remMin(30), value: [30] },
  { label: t().remHour, value: [60] },
  { label: t().remDay, value: [1440] },
  { label: t().rem10and60, value: [10, 60] },
];

const key = (v: number[] | null) => (v === null ? 'default' : v.join(','));

export function Editor(props: { draft: Draft }) {
  const d = props.draft;
  const initialDesc = d.description ? descriptionText(d.description) : '';
  const [title, setTitle] = createSignal(d.title);
  const [allDay, setAllDay] = createSignal(d.allDay);
  const [startDate, setStartDate] = createSignal(isoDate(d.start));
  const [startTime, setStartTime] = createSignal(hhmm(d.start));
  const [endDate, setEndDate] = createSignal(isoDate(d.allDay ? d.end - DAY : d.end - 1));
  const [endTime, setEndTime] = createSignal(hhmm(d.end));
  const [calKey, setCalKey] = createSignal(`${d.accountId}|${d.calendarId}`);
  const [location, setLocation] = createSignal(d.location);
  const [desc, setDesc] = createSignal(initialDesc);
  const [reminders, setReminders] = createSignal<number[] | null>(d.reminders);
  const [attendees, setAttendees] = createSignal(d.attendees);
  const [meet, setMeet] = createSignal(d.addMeet);
  const [busy, setBusy] = createSignal(false);
  const recurring = d.mode === 'edit' && !!d.source?.recurring;
  const [scope, setScope] = createSignal<'this' | 'series'>('this');
  let titleEl!: HTMLInputElement;

  onMount(() => titleEl.focus());

  const cals = () => store.writableCalendars();
  const accounts = () => store.snap()?.accounts ?? [];
  const reminderOptions = createMemo(() => {
    const r = reminders();
    const opts = REMINDER_OPTIONS();
    if (opts.some((o) => key(o.value) === key(r))) return opts;
    return [...opts, { label: r!.map(minutesLabel).join(', '), value: r }];
  });

  const startMs = () => {
    const x = parseIsoDate(startDate());
    const m = timeToMinutes(startTime());
    x.setHours(Math.floor(m / 60), m % 60, 0, 0);
    return x.getTime();
  };
  const endMs = () => {
    const x = parseIsoDate(endDate());
    const m = timeToMinutes(endTime());
    x.setHours(Math.floor(m / 60), m % 60, 0, 0);
    return x.getTime();
  };
  const invalid = () => (allDay() ? endDate() < startDate() : endMs() <= startMs());

  // Moving the start keeps the duration, like every calendar does.
  const onStartDate = (v: string) => {
    if (!v) return;
    const shift = Math.round((parseIsoDate(v).getTime() - parseIsoDate(startDate()).getTime()) / DAY);
    setStartDate(v);
    setEndDate(isoDate(addDays(parseIsoDate(endDate()), shift)));
  };
  const onStartTime = (v: string) => {
    if (!v) return;
    const dur = endMs() - startMs();
    setStartTime(v);
    const ne = startMs() + Math.max(dur, 15 * 60_000);
    setEndDate(isoDate(ne));
    setEndTime(hhmm(ne));
  };

  async function save() {
    if (invalid() || busy()) return;
    const [accountId, calendarId] = calKey().split('|');
    const input: EventInput = {
      accountId,
      calendarId,
      title: title().trim(),
      allDay: allDay(),
      start: allDay() ? null : startMs(),
      end: allDay() ? null : endMs(),
      startDate: allDay() ? startDate() : null,
      endDate: allDay() ? isoDate(addDays(parseIsoDate(endDate()), 1)) : null,
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      location: location().trim() || null,
      description: desc() === initialDesc ? d.description || null : desc().trim() || null,
      reminders: reminders(),
      attendees: attendees()
        .split(/[,;\s]+/)
        .map((s) => s.trim())
        .filter((s) => s.includes('@')),
      addMeet: meet(),
    };
    setBusy(true);
    const ok =
      d.mode === 'create'
        ? await store.run(api.createEvent(input))
        : await store.run(
            (scope() === 'series' ? api.updateSeries(targetOf(d.source!), input) : api.updateEvent(targetOf(d.source!), input)).then(() => true),
          );
    setBusy(false);
    if (ok !== undefined) store.setDraft(null);
  }

  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      store.setDraft(null);
    } else if (e.key === 'Enter' && (e.ctrlKey || (e.target as HTMLElement).tagName === 'INPUT')) {
      e.preventDefault();
      void save();
    }
  };

  return (
    <div class="scrim" onPointerDown={(e) => e.target === e.currentTarget && store.setDraft(null)}>
      <div class="dialog editor" role="dialog" aria-label={d.mode === 'create' ? t().newEvent : t().editEvent} onKeyDown={onKey}>
        <div class="ed-head">
          <span class="lbl" style={{ flex: 1 }}>
            {d.mode === 'create' ? t().newEvent : t().editEvent}
          </span>
          <Show when={recurring}>
            <div class="seg" role="group" aria-label={t().whatToChange} style={{ flex: '0 0 auto' }}>
              <button classList={{ on: scope() === 'this' }} aria-pressed={scope() === 'this'} onClick={() => setScope('this')}>
                {t().onlyThis}
              </button>
              <button classList={{ on: scope() === 'series' }} aria-pressed={scope() === 'series'} onClick={() => setScope('series')}>
                {t().wholeSeriesEdit}
              </button>
            </div>
          </Show>
          <button class="ib" aria-label={t().close} onClick={() => store.setDraft(null)}>
            {I.close({ size: 15 })}
          </button>
        </div>
        <div class="ed-body">
          <label class="field">
            <span class="sq" style={{ background: store.calendarOf(...(calKey().split('|') as [string, string]))?.color ?? 'var(--accent)', width: '12px', height: '12px' }} />
            <input ref={titleEl} class="ed-title" placeholder={t().title} value={title()} onInput={(e) => setTitle(e.currentTarget.value)} aria-label={t().title} />
          </label>
          <div class="field">
            {I.clock()}
            <div class="grow">
              <input type="date" class="inp" value={startDate()} onInput={(e) => onStartDate(e.currentTarget.value)} aria-label={t().startDate} />
              <Show when={!allDay()}>
                <input type="time" class="inp" step="300" value={startTime()} onInput={(e) => onStartTime(e.currentTarget.value)} aria-label={t().startTime} />
              </Show>
              <span class="muted">—</span>
              <Show when={!allDay()}>
                <input type="time" class="inp" step="300" value={endTime()} onInput={(e) => setEndTime(e.currentTarget.value)} aria-label={t().endTime} />
              </Show>
              <input type="date" class="inp" value={endDate()} onInput={(e) => setEndDate(e.currentTarget.value)} aria-label={t().endDate} />
            </div>
          </div>
          <div class="field" style={{ 'padding-left': '28px', gap: '18px' }}>
            <label class="check">
              <input type="checkbox" checked={allDay()} onChange={(e) => setAllDay(e.currentTarget.checked)} />
              {t().allDayCap}
            </label>
            <Show when={d.mode === 'create'}>
              <label class="check">
                <input type="checkbox" checked={meet()} onChange={(e) => setMeet(e.currentTarget.checked)} />
                {t().addMeet}
              </label>
            </Show>
            <Show when={invalid()}>
              <span style={{ color: 'var(--err-text)', 'font-size': '12px' }}>{t().endBeforeStart}</span>
            </Show>
          </div>
          <div class="field">
            {I.calendar()}
            <select class="sel-inp inp wide" value={calKey()} disabled={d.mode === 'edit'} onChange={(e) => setCalKey(e.currentTarget.value)} aria-label={t().calendar}>
              <For each={accounts()}>
                {(a) => (
                  <optgroup label={a.email}>
                    <For each={cals().filter((c) => c.accountId === a.id)}>
                      {(c) => <option value={`${c.accountId}|${c.id}`}>{store.calendarLabel(c)}</option>}
                    </For>
                  </optgroup>
                )}
              </For>
            </select>
          </div>
          <div class="field">
            {I.bell()}
            <select
              class="sel-inp inp wide"
              value={key(reminders())}
              onChange={(e) => setReminders(reminderOptions().find((o) => key(o.value) === e.currentTarget.value)?.value ?? null)}
              aria-label={t().reminder}
            >
              <For each={reminderOptions()}>{(o) => <option value={key(o.value)}>{o.label}</option>}</For>
            </select>
          </div>
          <div class="field">
            {I.pin()}
            <input class="inp wide" placeholder={t().locationPh} value={location()} onInput={(e) => setLocation(e.currentTarget.value)} aria-label={t().location} />
          </div>
          <div class="field">
            {I.users()}
            <input
              class="inp wide"
              placeholder={d.mode === 'create' ? t().invitePh : t().addAttendeesPh}
              value={attendees()}
              onInput={(e) => setAttendees(e.currentTarget.value)}
              aria-label={t().attendees}
            />
          </div>
          <div class="field" style={{ 'align-items': 'flex-start' }}>
            <span style={{ 'margin-top': '8px', display: 'inline-flex', color: 'var(--muted)' }}>{I.text()}</span>
            <textarea class="inp" placeholder={t().description} value={desc()} onInput={(e) => setDesc(e.currentTarget.value)} aria-label={t().description} />
          </div>
        </div>
        <div class="ed-foot">
          <span class="muted" style={{ flex: 1, 'font-size': '12px' }}>
            <span class="kbd">Ctrl ↵</span> {t().saveHint} · <span class="kbd">Esc</span> {t().cancelHint}
          </span>
          <button class="btn ghost" onClick={() => store.setDraft(null)}>{t().cancel}</button>
          <button class="btn primary" disabled={invalid() || busy()} onClick={save}>
            {d.mode === 'create' ? t().create : t().save}
          </button>
        </div>
      </div>
    </div>
  );
}
