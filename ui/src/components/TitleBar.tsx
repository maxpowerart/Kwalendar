import { createMemo, createSignal, onCleanup, Show } from 'solid-js';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { api, isTauri } from '../api';
import { store } from '../store';
import { I, WinClose, WinMax, WinMin } from '../icons';
import { plural, time } from '../dates';

function ago(ms: number, now: number): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `${s} с назад`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} мин назад`;
  const h = Math.round(m / 60);
  return `${h} ${plural(h, 'час', 'часа', 'часов')} назад`;
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
    if (!s || !hasAccounts()) return { color: '#5A5C66', text: 'Нет аккаунтов' };
    const q = s.pending ? ` · ${s.pending} в очереди` : '';
    switch (s.state) {
      case 'syncing':
        return { color: 'var(--accent)', text: `Синхронизация…${q}`, spin: true };
      case 'offline':
        return { color: 'var(--warn)', text: `Офлайн${q}` };
      case 'error':
        return { color: 'var(--danger)', text: 'Ошибка синхронизации' };
      default:
        return {
          color: 'var(--ok)',
          text: s.lastSync ? `Синхронизировано · ${ago(s.lastSync, tick())}${q}` : `Готово${q}`,
        };
    }
  });


  return (
    <header class="titlebar" data-tauri-drag-region>
      <div class="side" data-tauri-drag-region>
        <div class="logo" data-tauri-drag-region>K</div>
        <span style={{ 'font-weight': 600, 'letter-spacing': '-0.01em' }} data-tauri-drag-region>
          Kwalendar
        </span>
      </div>
      <button class="search-pill" onClick={() => store.setQuickAdd(true)}>
        {I.search({ size: 14 })}
        <span class="t">Поиск или быстрое создание…</span>
        <span class="kbd">Ctrl K</span>
      </button>
      <div class="side end" data-tauri-drag-region>
        <Show when={muted()}>
          <button
            class="ib"
            title={`Уведомления выключены до ${time(store.snap()!.mutedUntil)} — включить`}
            aria-label="Включить уведомления"
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
        <button class="ib" aria-label="Синхронизировать сейчас" title="Синхронизировать (F5)" onClick={() => api.syncNow()}>
          <span class={status().spin ? 'spin' : ''} style={{ display: 'inline-flex' }}>
            {I.sync({ size: 15 })}
          </span>
        </button>
        <button class="ib" aria-label="Настройки" title="Настройки (Ctrl+,)" onClick={() => store.setSettings(true)}>
          {I.settings({ size: 15 })}
        </button>
        <Show when={win}>
          <button class="wc" aria-label="Свернуть" onClick={() => win!.minimize()}>
            <WinMin />
          </button>
          <button class="wc" aria-label="Развернуть" onClick={() => win!.toggleMaximize()}>
            <WinMax />
          </button>
          <button class="wc x" aria-label="Закрыть в трей" onClick={() => win!.hide()}>
            <WinClose />
          </button>
        </Show>
      </div>
    </header>
  );
}
