import { createEffect, For, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';
import { api, on as listen, targetOf, type Instance, type Notice, type SyncStatus } from './api';
import { shift, store, type View } from './store';
import { TitleBar } from './components/TitleBar';
import { Sidebar } from './components/Sidebar';
import { Toolbar } from './components/Toolbar';
import { WeekView } from './components/WeekView';
import { MonthView } from './components/MonthView';
import { AgendaView } from './components/AgendaView';
import { EventPanel } from './components/EventPanel';
import { Editor } from './components/Editor';
import { QuickAdd } from './components/QuickAdd';
import { GoogleClientForm, Settings } from './components/Settings';
import { initUpdates, UpdateDialog } from './components/Updates';
import { I } from './icons';
import { startOfDay } from './dates';
import { setTheme } from './theme';
import { applyLang, t } from './i18n';
import mark from './assets/mark.svg';

function Onboarding() {
  const hasClient = () => !!store.snap()?.builtinClient || !!store.snap()?.config.googleClientId;
  return (
    <div class="empty">
      <div class="box">
        <img src={mark} alt="Kwalendar" width="64" height="70" />
        <h1>{t().obTitle}</h1>
        <Show
          when={hasClient()}
          fallback={
            <>
              <p class="muted">{t().obOwnClient}</p>
              <GoogleClientForm />
            </>
          }
        >
          <p class="muted">{t().obAccounts}</p>
          <div>
            <button class="btn primary" style={{ height: '38px' }} onClick={() => store.setSettings(true)}>
              {I.plus({ size: 14, stroke: 2.4 })}{t().addGoogleAccount}
            </button>
          </div>
        </Show>
      </div>
    </div>
  );
}

function Banners() {
  return (
    <div class="banners" aria-live="polite">
      <For each={store.banners()}>
        {(b) => (
          <div class="banner" classList={{ warn: b.tone === 'warn', error: b.tone === 'error' }}>
            <span class="t">{b.text}</span>
            <Show when={b.action}>
              <button class="btn" style={{ height: '28px' }} onClick={() => b.action!.run()}>
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
  );
}

const VIEW_KEYS: Record<string, View> = { '1': 'day', '2': 'week', '3': 'month', '4': 'agenda' };

export function App() {
  let refreshTimer: number | undefined;
  const scheduleRefresh = () => {
    clearTimeout(refreshTimer);
    refreshTimer = window.setTimeout(() => void store.refresh(), 60);
  };

  onMount(async () => {
    try {
      const s = await api.snapshot();
      applyLang(s.config.language, s.systemLang);
      store.setSnap(s);
      store.setViewSig((['day', 'week', 'month', 'agenda'].includes(s.config.view) ? s.config.view : 'week') as View);
      store.setSidebarSig(s.config.sidebar);
      await store.load();
    } finally {
      // Show the window only after the first frame with data is painted.
      requestAnimationFrame(() => requestAnimationFrame(() => void api.appReady()));
    }

    void initUpdates();
    const offs = await Promise.all([
      listen('kw:changed', scheduleRefresh),
      listen<SyncStatus>('kw:sync', (sync) => {
        const s = store.snap();
        if (s) store.setSnap({ ...s, sync });
      }),
      listen<Notice>('kw:notice', (n) => store.noticeBanner(n)),
      listen('kw:quickadd', () => store.setQuickAdd(true)),
      listen<Instance>('kw:open-event', (i) => {
        store.goto(i.start);
        store.setSelected(i);
      }),
    ]);
    onCleanup(() => offs.forEach((off) => off()));
  });

  createEffect(on(store.range, () => void store.load(), { defer: true }));
  createEffect(() => setTheme(store.snap()?.config.theme ?? 'system'));
  createEffect(() => {
    const s = store.snap();
    if (s) applyLang(s.config.language, s.systemLang);
  });

  const onKey = (e: KeyboardEvent) => {
    const tag = (e.target as HTMLElement).tagName;
    const typing = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    const overlay = store.quickAdd() || !!store.draft() || store.settings();
    const k = e.key.toLowerCase();

    if (e.ctrlKey && (k === 'k' || k === 'л')) {
      e.preventDefault();
      store.setQuickAdd(!store.quickAdd());
      return;
    }
    if (e.ctrlKey && e.key === ',') {
      e.preventDefault();
      store.setSettings(true);
      return;
    }
    if (e.key === 'F5') {
      e.preventDefault();
      void api.syncNow();
      return;
    }
    if (overlay) {
      if (e.key === 'Escape') {
        store.setQuickAdd(false);
        store.setDraft(null);
        store.setSettings(false);
      }
      return;
    }
    if (typing) return;
    if (e.ctrlKey && (k === 'b' || k === 'и')) {
      e.preventDefault();
      store.setSidebar(!store.sidebar());
      return;
    }
    if (e.ctrlKey || e.altKey || e.metaKey) return;

    const sel = store.selected();
    // Layout-independent: e.code works on both QWERTY and ЙЦУКЕН.
    switch (e.code) {
      case 'KeyT':
        store.goto(startOfDay(new Date()));
        break;
      case 'KeyJ':
      case 'ArrowRight':
        store.setAnchor(shift(store.view(), store.anchor(), 1));
        break;
      case 'KeyK':
      case 'ArrowLeft':
        store.setAnchor(shift(store.view(), store.anchor(), -1));
        break;
      case 'KeyN': {
        const s = new Date(store.anchor());
        const now = new Date();
        s.setHours(s.toDateString() === now.toDateString() ? now.getHours() + 1 : 10, 0, 0, 0);
        const d = store.newDraft(s.getTime(), s.getTime() + 3600_000);
        if (d) store.setDraft(d);
        break;
      }
      case 'KeyE':
        if (sel?.canEdit) store.setDraft(store.editDraft(sel));
        break;
      case 'Delete':
        if (sel?.canEdit) {
          void store.run(api.deleteEvent(targetOf(sel), 'this'));
          store.setSelected(null);
        }
        break;
      case 'Escape':
        store.setSelected(null);
        break;
      default:
        if (VIEW_KEYS[e.key]) store.setView(VIEW_KEYS[e.key]);
        return;
    }
    e.preventDefault();
  };
  window.addEventListener('keydown', onKey);
  onCleanup(() => window.removeEventListener('keydown', onKey));

  const hasAccounts = () => (store.snap()?.accounts.length ?? 0) > 0;

  return (
    <div class="app">
      <TitleBar />
      <div class="body">
        <Show when={store.sidebar() && hasAccounts()}>
          <Sidebar />
        </Show>
        <div class="main">
          <Show when={store.snap()}>
            <Show when={hasAccounts()} fallback={<Onboarding />}>
              <Toolbar />
              <Switch>
                <Match when={store.view() === 'day' || store.view() === 'week'}>
                  {/* keyed by day count so the grid re-measures when switching day ↔ week */}
                  <Show when={store.view()} keyed>
                    {(_v) => <WeekView />}
                  </Show>
                </Match>
                <Match when={store.view() === 'month'}>
                  <MonthView />
                </Match>
                <Match when={store.view() === 'agenda'}>
                  <AgendaView />
                </Match>
              </Switch>
            </Show>
          </Show>
        </div>
        <Show when={store.selected()} keyed>
          {(i) => <EventPanel inst={i} />}
        </Show>
      </div>
      <Show when={store.draft()} keyed>
        {(d) => <Editor draft={d} />}
      </Show>
      <Show when={store.quickAdd()}>
        <QuickAdd />
      </Show>
      <Show when={store.settings() && store.snap()}>
        <Settings />
      </Show>
      <UpdateDialog />
      <Banners />
    </div>
  );
}
