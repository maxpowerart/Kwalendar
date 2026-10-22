import { batch, createMemo, createRoot, createSignal } from 'solid-js';
import { api, type Calendar, type Instance, type Notice, type Snapshot } from './api';
import { addDays, addMonths, DAY, startOfDay, startOfMonth, startOfWeek } from './dates';

export type View = 'day' | 'week' | 'month' | 'agenda';

export interface Draft {
  mode: 'create' | 'edit';
  source?: Instance;
  accountId: string;
  calendarId: string;
  title: string;
  allDay: boolean;
  start: number;
  end: number;
  location: string;
  description: string;
  /** null = calendar defaults */
  reminders: number[] | null;
  attendees: string;
  addMeet: boolean;
}

export interface Banner {
  id: number;
  text: string;
  tone: 'info' | 'warn' | 'error';
  action?: { label: string; run: () => void };
}

function rangeFor(view: View, anchor: Date): [number, number] {
  switch (view) {
    case 'day': {
      const s = startOfDay(anchor);
      return [s.getTime(), addDays(s, 1).getTime()];
    }
    case 'week': {
      const s = startOfWeek(anchor);
      return [s.getTime(), addDays(s, 7).getTime()];
    }
    case 'month': {
      const s = startOfWeek(startOfMonth(anchor));
      return [s.getTime(), addDays(s, 42).getTime()];
    }
    case 'agenda': {
      const s = startOfDay(anchor);
      return [s.getTime(), addDays(s, 60).getTime()];
    }
  }
}

export function shift(view: View, anchor: Date, dir: number): Date {
  switch (view) {
    case 'day':
      return addDays(anchor, dir);
    case 'week':
      return addDays(anchor, dir * 7);
    case 'month':
      return addMonths(anchor, dir);
    case 'agenda':
      return addDays(anchor, dir * 14);
  }
}

function createStore() {
  const [snap, setSnap] = createSignal<Snapshot | null>(null);
  const [view, setViewSig] = createSignal<View>('week');
  const [anchor, setAnchor] = createSignal(startOfDay(new Date()));
  const [items, setItems] = createSignal<Instance[]>([]);
  const [selected, setSelected] = createSignal<Instance | null>(null);
  const [draft, setDraft] = createSignal<Draft | null>(null);
  const [quickAdd, setQuickAdd] = createSignal(false);
  const [settings, setSettings] = createSignal(false);
  const [sidebar, setSidebarSig] = createSignal(true);
  const [banners, setBanners] = createSignal<Banner[]>([]);
  const [now, setNow] = createSignal(Date.now());

  const range = createMemo(() => rangeFor(view(), anchor()));

  // Range cache makes back/forward navigation instant; invalidated on every data change.
  const cache = new Map<string, Instance[]>();
  let generation = 0;

  async function fetchRange([from, to]: [number, number], gen: number): Promise<Instance[]> {
    const key = `${from}-${to}`;
    const hit = cache.get(key);
    if (hit) return hit;
    const list = await api.instances(from, to);
    if (gen === generation) cache.set(key, list);
    return list;
  }

  async function load() {
    const gen = generation;
    const r = range();
    const list = await fetchRange(r, gen);
    if (gen !== generation || r !== range()) return;
    setItems(list);
    const sel = selected();
    if (sel) {
      const fresh = list.find((i) => i.key === sel.key);
      if (fresh) setSelected(fresh);
    }
    // Prefetch neighbours in idle time.
    const v = view();
    const a = anchor();
    setTimeout(() => {
      for (const d of [-1, 1]) void fetchRange(rangeFor(v, shift(v, a, d)), gen).catch(() => {});
    }, 50);
  }

  async function refresh() {
    generation++;
    cache.clear();
    const [s] = await Promise.all([api.snapshot(), load()]);
    setSnap(s);
  }

  let bannerId = 0;
  function banner(text: string, tone: Banner['tone'] = 'info', action?: Banner['action']) {
    const id = ++bannerId;
    setBanners((b) => [...b.slice(-3), { id, text, tone, action }]);
    setTimeout(() => setBanners((b) => b.filter((x) => x.id !== id)), tone === 'error' ? 9000 : 6000);
  }

  function noticeBanner(n: Notice) {
    switch (n.kind) {
      case 'moved':
        return banner(`«${n.title}» перенесено`, 'info');
      case 'invite':
        return banner(`Новое приглашение: «${n.title}»`, 'info');
      case 'conflict':
        return banner(`«${n.title}» изменили на сервере — показана свежая версия`, 'warn');
      case 'reauth':
        return banner(`${n.email}: нужно войти снова`, 'warn', { label: 'Войти', run: () => setSettings(true) });
      case 'error':
        return banner(n.message, 'error');
    }
  }

  async function run<T>(p: Promise<T>): Promise<T | undefined> {
    try {
      return await p;
    } catch (e) {
      banner(String(e), 'error');
      return undefined;
    }
  }

  const calendars = createMemo(() => snap()?.calendars ?? []);
  const calendarOf = (accountId: string, id: string): Calendar | undefined =>
    calendars().find((c) => c.accountId === accountId && c.id === id);
  const writableCalendars = createMemo(() =>
    calendars().filter((c) => c.accessRole === 'owner' || c.accessRole === 'writer'),
  );
  const defaultCalendar = () => writableCalendars().find((c) => c.primary && c.selected) ?? writableCalendars()[0];

  function newDraft(start: number, end: number, allDay = false, title = ''): Draft | null {
    const cal = defaultCalendar();
    if (!cal) {
      banner('Сначала подключите аккаунт Google', 'warn', { label: 'Настройки', run: () => setSettings(true) });
      return null;
    }
    return {
      mode: 'create',
      accountId: cal.accountId,
      calendarId: cal.id,
      title,
      allDay,
      start,
      end,
      location: '',
      description: '',
      reminders: null,
      attendees: '',
      addMeet: false,
    };
  }

  function editDraft(i: Instance): Draft {
    return {
      mode: 'edit',
      source: i,
      accountId: i.accountId,
      calendarId: i.calendarId,
      title: i.title === '(без названия)' ? '' : i.title,
      allDay: i.allDay,
      start: i.start,
      end: i.end,
      location: i.location ?? '',
      description: i.description ?? '',
      reminders: i.remindersDefault ? null : i.reminders,
      attendees: '',
      addMeet: false,
    };
  }

  function setView(v: View) {
    setViewSig(v);
    const s = snap();
    if (s && s.config.view !== v) void api.setConfig({ ...s.config, view: v }).catch(() => {});
  }

  function setSidebar(v: boolean) {
    setSidebarSig(v);
    const s = snap();
    if (s && s.config.sidebar !== v) void api.setConfig({ ...s.config, sidebar: v }).catch(() => {});
  }

  function goto(date: Date | number, v?: View) {
    batch(() => {
      if (v) setView(v);
      setAnchor(startOfDay(date));
    });
  }

  setInterval(() => setNow(Date.now()), 30_000);

  return {
    snap,
    setSnap,
    view,
    setView,
    anchor,
    setAnchor,
    range,
    items,
    selected,
    setSelected,
    draft,
    setDraft,
    quickAdd,
    setQuickAdd,
    settings,
    setSettings,
    sidebar,
    setSidebar,
    setSidebarSig,
    setViewSig,
    banners,
    setBanners,
    banner,
    noticeBanner,
    now,
    load,
    refresh,
    run,
    calendars,
    calendarOf,
    writableCalendars,
    defaultCalendar,
    newDraft,
    editDraft,
    goto,
    DAY,
  };
}

export const store = createRoot(createStore);
