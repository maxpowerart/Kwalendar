import { createMemo, createSignal, onCleanup, Show } from 'solid-js';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { api, isTauri } from '../api';
import { store } from '../store';
import { I, WinClose, WinMax, WinMin } from '../icons';
import { time } from '../dates';
import { t } from '../i18n';
import mark from '../assets/mark.svg';
import { UpdateButton } from './Updates';

function ago(ms: number, now: number): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return t().secAgo(s);
  const m = Math.round(s / 60);
  if (m < 60) return t().minAgo(m);
  const h = Math.round(m / 60);
  return t().hoursAgo(h);
}

export function TitleBar() {
  const win = isTauri ? getCurrentWindow() : null;
  const sync = () => store.snap()?.sync;
  const hasAccounts = () => (store.snap()?.accounts.length ?? 0) > 0;
  const muted = () => (store.snap()?.mutedUntil ?? 0) > store.now();
  // "N s ago" ticks faster than the global 30 s clock.
  const [tick, setTick] = createSignal(Date.now());
  const timer = setInterval(() => setTick(Date.now()), 5000);
  onCleanup(() => clearInterval(timer));

  const status = createMemo(() => {
    const s = sync();
    if (!s || !hasAccounts()) return { color: 'var(--chrome-muted)', text: t().noAccounts };
    const q = s.pending ? t().inQueue(s.pending) : '';
    switch (s.state) {
      case 'syncing':
        return { color: 'var(--icon)', text: `${t().syncing}${q}`, spin: true };
      case 'offline':
        return { color: 'var(--node)', text: `${t().offline}${q}` };
      case 'error':
        return { color: 'var(--err-text)', text: t().syncError };
      default:
        return {
          color: 'var(--ok)',
          text: s.lastSync ? `${t().synced(ago(s.lastSync, tick()))}${q}` : `${t().ready}${q}`,
        };
    }
  });


  return (
    <header class="titlebar" data-tauri-drag-region>
      <div class="side" data-tauri-drag-region>
        <img src={mark} alt="" width="20" height="22" style={{ display: 'block' }} data-tauri-drag-region />
        <span class="wm" style={{ 'font-size': '16px' }} data-tauri-drag-region>
          <span class="k">K</span>walendar
        </span>
      </div>
      <button class="search-pill" onClick={() => store.setQuickAdd(true)}>
        {I.search({ size: 14 })}
        <span class="t">{t().searchPill}</span>
        <span class="kbd">Ctrl K</span>
      </button>
      <div class="side end" data-tauri-drag-region>
        <UpdateButton />
        <Show when={muted()}>
          <button
            class="cb"
            title={t().unmuteHint(time(store.snap()!.mutedUntil))}
            aria-label={t().unmute}
            onClick={async () => {
              await api.mute(0);
              await store.refresh();
            }}
          >
            {I.bellOff({ size: 15 })}
          </button>
        </Show>
        <span class="sync-ind" title={sync()?.message ?? ''} data-tauri-drag-region>
          <span class="dot" style={{ width: '7px', height: '7px', background: status().color }} />
          {status().text}
        </span>
        <button class="cb" aria-label={t().syncNowAria} title={t().syncNowTitle} onClick={() => api.syncNow()}>
          <span class={status().spin ? 'spin' : ''} style={{ display: 'inline-flex' }}>
            {I.sync({ size: 16, node: [20, 20, 2.2] })}
          </span>
        </button>
        <button class="cb" aria-label={t().settings} title={t().settingsTitle} onClick={() => store.setSettings(true)}>
          {I.settings({ size: 16, node: [12, 12, 3.2] })}
        </button>
        <Show when={win}>
          <button class="wc" aria-label={t().minimize} onClick={() => win!.minimize()}>
            <WinMin />
          </button>
          <button class="wc" aria-label={t().maximize} onClick={() => win!.toggleMaximize()}>
            <WinMax />
          </button>
          <button class="wc x" aria-label={t().closeToTray} onClick={() => win!.hide()}>
            <WinClose />
          </button>
        </Show>
      </div>
    </header>
  );
}
