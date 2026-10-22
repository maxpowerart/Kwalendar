import { createResource, createSignal, For, Match, Show, Switch } from 'solid-js';
import { api, isTauri, type Config } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { time } from '../dates';
import { UpdatesSettings } from './Updates';

type Section = 'accounts' | 'google' | 'sync' | 'notify' | 'system' | 'updates' | 'keys';

const NAV: { id: Section; label: string; icon: () => any }[] = [
  { id: 'accounts', label: 'Аккаунты', icon: () => I.users() },
  { id: 'google', label: 'Подключение Google', icon: () => I.settings() },
  { id: 'sync', label: 'Синхронизация', icon: () => I.sync() },
  { id: 'notify', label: 'Уведомления', icon: () => I.bell() },
  { id: 'system', label: 'Система', icon: () => I.sidebar() },
  { id: 'updates', label: 'Обновления', icon: () => I.down() },
  { id: 'keys', label: 'Горячие клавиши', icon: () => I.text() },
];

function Toggle(props: { on: boolean; label: string; onChange: (v: boolean) => void }) {
  return (
    <button class="sw" classList={{ on: props.on }} role="switch" aria-checked={props.on} aria-label={props.label} onClick={() => props.onChange(!props.on)}>
      <span />
    </button>
  );
}

export function GoogleClientForm(props: { onSaved?: () => void }) {
  const cfg = () => store.snap()?.config;
  const [id, setId] = createSignal(cfg()?.googleClientId ?? '');
  const [secret, setSecret] = createSignal(cfg()?.googleClientSecret ?? '');
  const save = async () => {
    const c = cfg();
    if (!c) return;
    const ok = await store.run(api.setConfig({ ...c, googleClientId: id().trim(), googleClientSecret: secret().trim() }).then(() => true));
    if (ok) {
      await store.refresh();
      store.banner('Клиент Google сохранён');
      props.onSaved?.();
    }
  };
  return (
    <div style={{ display: 'flex', 'flex-direction': 'column', gap: '12px' }}>
      <ol class="steps">
        <li>
          Откройте <a href="#" onClick={(e) => { e.preventDefault(); void api.openUrl('https://console.cloud.google.com/apis/credentials'); }}>Google Cloud Console → Credentials</a> и создайте проект.
        </li>
        <li>
          Включите <a href="#" onClick={(e) => { e.preventDefault(); void api.openUrl('https://console.cloud.google.com/apis/library/calendar-json.googleapis.com'); }}>Google Calendar API</a>.
        </li>
        <li>OAuth consent screen: тип External, добавьте scope <code>…/auth/calendar</code>. Чтобы вход не слетал раз в 7 дней, переведите статус в <b>In production</b>.</li>
        <li>
          Create credentials → OAuth client ID → тип <b>Desktop app</b>. Скопируйте Client ID и Client secret сюда.
        </li>
      </ol>
      <input class="inp" style={{ width: '100%' }} placeholder="Client ID (…apps.googleusercontent.com)" value={id()} onInput={(e) => setId(e.currentTarget.value)} aria-label="Client ID" spellcheck={false} />
      <input class="inp" style={{ width: '100%' }} placeholder="Client secret" type="password" value={secret()} onInput={(e) => setSecret(e.currentTarget.value)} aria-label="Client secret" spellcheck={false} />
      <div>
        <button class="btn primary" disabled={!id().trim()} onClick={save}>Сохранить</button>
      </div>
    </div>
  );
}

export function Settings() {
  const [section, setSection] = createSignal<Section>(store.snap()?.builtinClient || store.snap()?.config.googleClientId ? 'accounts' : 'google');
  const [adding, setAdding] = createSignal(false);
  const [autostart, { mutate: setAutostart }] = createResource(() => api.autostartGet().catch(() => false));
  const snap = () => store.snap();
  const cfg = () => snap()!.config;

  const patch = async (p: Partial<Config>) => {
    await store.run(api.setConfig({ ...cfg(), ...p }));
    await store.refresh();
  };

  const add = async (hint?: string) => {
    if (!snap()!.builtinClient && !cfg().googleClientId) {
      setSection('google');
      return store.banner('Сначала укажите OAuth-клиент Google', 'warn');
    }
    setAdding(true);
    store.banner('Откроется браузер — войдите в аккаунт Google и разрешите доступ к календарю');
    const acc = await store.run(api.addAccount(hint));
    setAdding(false);
    if (acc) {
      store.banner(`Подключён ${acc.email} — загружаем календари`);
      await store.refresh();
    }
  };

  const [confirmId, setConfirmId] = createSignal<string | null>(null);
  const remove = async (id: string) => {
    setConfirmId(null);
    await store.run(api.removeAccount(id));
    await store.refresh();
  };

  return (
    <div class="scrim" onPointerDown={(e) => e.target === e.currentTarget && store.setSettings(false)} onKeyDown={(e) => e.key === 'Escape' && store.setSettings(false)}>
      <div class="dialog settings" role="dialog" aria-label="Настройки">
        <nav class="set-nav" aria-label="Разделы настроек">
          <For each={NAV}>
            {(n) => (
              <button class="nav" classList={{ on: section() === n.id }} aria-current={section() === n.id ? 'page' : undefined} onClick={() => setSection(n.id)}>
                {n.icon()}
                {n.label}
              </button>
            )}
          </For>
          <div style={{ flex: 1 }} />
          <p class="muted" style={{ padding: '0 10px', 'font-size': '12px' }}>Kwalendar 0.1.0{isTauri ? '' : ' · превью'}</p>
        </nav>
        <main class="set-main">
          <div style={{ display: 'flex', 'align-items': 'center' }}>
            <h1 style={{ flex: 1 }}>{NAV.find((n) => n.id === section())!.label}</h1>
            <button class="ib" aria-label="Закрыть настройки" onClick={() => store.setSettings(false)}>{I.close()}</button>
          </div>
          <Switch>
            <Match when={section() === 'accounts'}>
              <p class="muted">Каждый аккаунт синхронизируется независимо. Refresh-токены хранятся в Диспетчере учётных данных Windows.</p>
              <div style={{ display: 'flex', 'flex-direction': 'column', gap: '8px' }}>
                <For each={snap()!.accounts} fallback={<p class="muted">Аккаунтов пока нет.</p>}>
                  {(a) => {
                    const cals = () => snap()!.calendars.filter((c) => c.accountId === a.id).length;
                    return (
                      <div class="card" classList={{ warn: a.status === 'reauth' || a.status === 'error' }}>
                        <span class="avb" style={{ background: a.color }}>{(a.name || a.email).charAt(0).toUpperCase()}</span>
                        <div style={{ flex: 1, 'min-width': 0 }}>
                          <p style={{ 'font-weight': 600 }}>
                            {a.email} <Show when={a.name}><span class="muted" style={{ 'font-weight': 400 }}>· {a.name}</span></Show>
                          </p>
                          <p class="muted" style={{ 'font-size': '12px', color: a.status === 'reauth' || a.status === 'error' ? 'var(--warn-text)' : undefined }}>
                            {a.status === 'reauth'
                              ? 'Доступ отозван — показываем кэш, изменения не отправляются'
                              : a.status === 'error'
                                ? a.error
                                : `${cals()} календ. · ${a.lastSync ? `проверено в ${time(a.lastSync)}` : 'первая загрузка…'}`}
                          </p>
                        </div>
                        <Show when={a.status === 'reauth'}>
                          <button class="btn" disabled={adding()} onClick={() => add(a.email)}>Войти снова</button>
                        </Show>
                        <Show
                          when={confirmId() === a.id}
                          fallback={
                            <button class="ib" aria-label={`Отключить ${a.email}`} title="Отключить аккаунт" onClick={() => setConfirmId(a.id)}>{I.trash({ size: 15 })}</button>
                          }
                        >
                          <button class="btn danger" onClick={() => remove(a.id)}>Отключить и удалить кэш</button>
                          <button class="btn ghost" onClick={() => setConfirmId(null)}>Отмена</button>
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
              <div>
                <button class="btn primary" disabled={adding()} onClick={() => add()}>
                  {I.plus({ size: 14, stroke: 2.4 })}
                  {adding() ? 'Ждём вход в браузере…' : 'Добавить аккаунт Google'}
                </button>
              </div>
            </Match>
            <Match when={section() === 'google'}>
              <Show
                when={snap()!.builtinClient}
                fallback={
                  <p class="muted">Kwalendar обращается к Google напрямую, без своих серверов. Для этого нужен собственный OAuth-клиент — это бесплатно и занимает пару минут.</p>
                }
              >
                <p class="muted">
                  В приложение уже встроен OAuth-клиент Kwalendar — для входа ничего настраивать не нужно. Свой клиент из Google Cloud Console нужен, только если хотите работать через собственный проект (свои квоты, своя страница согласия). Пустые поля — встроенный клиент.
                </p>
              </Show>
              <GoogleClientForm onSaved={() => setSection('accounts')} />
            </Match>
            <Match when={section() === 'sync'}>
              <div>
                <div class="set">
                  <div class="l"><p>Проверка изменений при открытом окне</p><p>Инкрементально по syncToken — приходят только изменения</p></div>
                  <select class="sel-inp" value={cfg().pollFocusedSecs} onChange={(e) => patch({ pollFocusedSecs: Number(e.currentTarget.value) })}>
                    <For each={[15, 30, 60, 120]}>{(s) => <option value={s}>каждые {s} с</option>}</For>
                  </select>
                </div>
                <div class="set">
                  <div class="l"><p>Проверка в фоне (окно свёрнуто в трей)</p><p>Плюс сразу после выхода из сна и при возврате в окно</p></div>
                  <select class="sel-inp" value={cfg().pollBackgroundSecs} onChange={(e) => patch({ pollBackgroundSecs: Number(e.currentTarget.value) })}>
                    <For each={[60, 180, 300, 600, 900]}>{(s) => <option value={s}>каждые {s / 60} мин</option>}</For>
                  </select>
                </div>
                <div class="set" style={{ 'flex-wrap': 'wrap' }}>
                  <div class="l">
                    <p>Push-relay {snap()!.pushActive ? <span style={{ color: 'var(--ok)' }}>· активен</span> : cfg().relayUrl ? <span style={{ color: 'var(--warn)' }}>· нет связи</span> : null}</p>
                    <p>Google сам сообщает об изменениях — за секунды, опрос становится редким. Развёртывание: папка <code class="code">relay/</code>, бесплатный Cloudflare Worker</p>
                  </div>
                  <input
                    class="inp"
                    style={{ width: '320px' }}
                    placeholder="https://kwalendar-relay.<имя>.workers.dev"
                    value={cfg().relayUrl}
                    onChange={(e) => patch({ relayUrl: e.currentTarget.value.trim() })}
                    aria-label="Адрес push-relay"
                    spellcheck={false}
                  />
                </div>
                <div class="set">
                  <div class="l"><p>Офлайн-изменения</p><p>Правки сохраняются локально и уходят при появлении сети</p></div>
                  <span class="muted">{snap()!.sync.pending ? `в очереди: ${snap()!.sync.pending}` : 'очередь пуста'}</span>
                </div>
                <div class="set">
                  <div class="l"><p>Синхронизировать сейчас</p><p>{snap()!.sync.lastSync ? `Последняя: ${time(snap()!.sync.lastSync!)}` : 'Ещё не выполнялась'}</p></div>
                  <button class="btn" onClick={() => api.syncNow()}>{I.sync({ size: 14 })}Синхронизировать</button>
                </div>
              </div>
            </Match>
            <Match when={section() === 'notify'}>
              <div>
                <div class="set">
                  <div class="l"><p>Напоминания о событиях</p><p>Уведомления Windows с кнопками «Присоединиться» и «Отложить»</p></div>
                  <Toggle on={cfg().remindersEnabled} label="Напоминания" onChange={(v) => patch({ remindersEnabled: v })} />
                </div>
                <div class="set">
                  <div class="l"><p>Изменения от других</p><p>Перенос события, новое приглашение</p></div>
                  <Toggle on={cfg().notifyChanges} label="Уведомлять об изменениях" onChange={(v) => patch({ notifyChanges: v })} />
                </div>
                <div class="set">
                  <div class="l"><p>Тишина</p><p>{snap()!.mutedUntil > store.now() ? `Уведомления выключены до ${time(snap()!.mutedUntil)}` : 'Временно выключить все напоминания'}</p></div>
                  <Show
                    when={snap()!.mutedUntil > store.now()}
                    fallback={<button class="btn" onClick={async () => { await api.mute(60); await store.refresh(); }}>{I.bellOff({ size: 14 })}На 1 час</button>}
                  >
                    <button class="btn" onClick={async () => { await api.mute(0); await store.refresh(); }}>Включить</button>
                  </Show>
                </div>
                <div class="set">
                  <div class="l"><p>Проверить уведомление</p><p>Покажет тестовое напоминание</p></div>
                  <button class="btn" onClick={() => api.testNotification()}>{I.bell({ size: 14 })}Показать</button>
                </div>
              </div>
            </Match>
            <Match when={section() === 'system'}>
              <div>
                <div class="set">
                  <div class="l"><p>Тема</p><p>«Как в Windows» следует за системной настройкой</p></div>
                  <div class="seg" role="group" aria-label="Тема">
                    <For each={[['system', 'Как в Windows'], ['dark', 'Тёмная'], ['light', 'Светлая']] as const}>
                      {([v, label]) => (
                        <button classList={{ on: cfg().theme === v }} aria-pressed={cfg().theme === v} onClick={() => patch({ theme: v })}>
                          {label}
                        </button>
                      )}
                    </For>
                  </div>
                </div>
                <div class="set">
                  <div class="l"><p>Запускать вместе с Windows</p><p>Свёрнуто в трей — напоминания работают, окно не мешает</p></div>
                  <Toggle
                    on={!!autostart()}
                    label="Автозапуск"
                    onChange={async (v) => {
                      const ok = await store.run(api.autostartSet(v).then(() => true));
                      if (ok) setAutostart(v);
                    }}
                  />
                </div>
                <div class="set">
                  <div class="l"><p>Начало рабочего дня</p><p>С этого часа открывается сетка недели</p></div>
                  <select class="sel-inp" value={cfg().dayStartHour} onChange={(e) => patch({ dayStartHour: Number(e.currentTarget.value) })}>
                    <For each={[6, 7, 8, 9, 10]}>{(h) => <option value={h}>{h}:00</option>}</For>
                  </select>
                </div>
                <div class="set">
                  <div class="l"><p>Закрытие окна</p><p>Крестик сворачивает в трей. Выход — через меню значка в трее</p></div>
                </div>
              </div>
            </Match>
            <Match when={section() === 'updates'}>
              <UpdatesSettings />
            </Match>
            <Match when={section() === 'keys'}>
              <div style={{ display: 'grid', 'grid-template-columns': 'max-content 1fr', gap: '10px 24px', 'align-items': 'center' }}>
                <For
                  each={[
                    ['Ctrl K', 'Быстрый ввод и поиск'],
                    ['Ctrl Alt K', 'Быстрый ввод из любого окна Windows'],
                    ['N', 'Новое событие'],
                    ['T', 'Сегодня'],
                    ['J / →', 'Вперёд'],
                    ['K / ←', 'Назад'],
                    ['1 2 3 4', 'День / неделя / месяц / повестка'],
                    ['E', 'Изменить выбранное'],
                    ['Del', 'Удалить выбранное'],
                    ['Ctrl B', 'Сайдбар'],
                    ['F5', 'Синхронизировать'],
                    ['Ctrl ,', 'Настройки'],
                    ['Esc', 'Закрыть'],
                  ]}
                >
                  {([k, t]) => (
                    <>
                      <span class="kbd" style={{ 'justify-self': 'start' }}>{k}</span>
                      <span>{t}</span>
                    </>
                  )}
                </For>
              </div>
            </Match>
          </Switch>
        </main>
      </div>
    </div>
  );
}
