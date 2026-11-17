import { createResource, createSignal, For, Match, Show, Switch } from 'solid-js';
import { api, isTauri, type Config } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { time } from '../dates';
import { UpdatesSettings } from './Updates';
import { t } from '../i18n';

type Section = 'accounts' | 'google' | 'sync' | 'notify' | 'system' | 'updates' | 'keys';

const NAV = (): { id: Section; label: string; icon: () => any }[] => [
  { id: 'accounts', label: t().navAccounts, icon: () => I.users() },
  { id: 'google', label: t().navGoogle, icon: () => I.globe() },
  { id: 'sync', label: t().navSync, icon: () => I.sync() },
  { id: 'notify', label: t().navNotify, icon: () => I.bell() },
  { id: 'system', label: t().navSystem, icon: () => I.settings() },
  { id: 'updates', label: t().navUpdates, icon: () => I.down() },
  { id: 'keys', label: t().navKeys, icon: () => I.text() },
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
      store.banner(t().clientSaved);
      props.onSaved?.();
    }
  };
  return (
    <div style={{ display: 'flex', 'flex-direction': 'column', gap: '12px' }}>
      <ol class="steps">
        <li>
          {t().step1a}<a href="#" onClick={(e) => { e.preventDefault(); void api.openUrl('https://console.cloud.google.com/apis/credentials'); }}>Google Cloud Console → Credentials</a>{t().step1b}
        </li>
        <li>
          {t().step2a}<a href="#" onClick={(e) => { e.preventDefault(); void api.openUrl('https://console.cloud.google.com/apis/library/calendar-json.googleapis.com'); }}>Google Calendar API</a>.
        </li>
        <li>{t().step3} <code>…/auth/calendar</code>{t().step3b} <b>In production</b>.</li>
        <li>
          {t().step4a} <b>Desktop app</b>{t().step4b}
        </li>
      </ol>
      <input class="inp" style={{ width: '100%' }} placeholder="Client ID (…apps.googleusercontent.com)" value={id()} onInput={(e) => setId(e.currentTarget.value)} aria-label="Client ID" spellcheck={false} />
      <input class="inp" style={{ width: '100%' }} placeholder="Client secret" type="password" value={secret()} onInput={(e) => setSecret(e.currentTarget.value)} aria-label="Client secret" spellcheck={false} />
      <div>
        <button class="btn primary" disabled={!id().trim()} onClick={save}>{t().save}</button>
      </div>
    </div>
  );
}

function IcalForm(props: { onDone: () => void }) {
  const [url, setUrl] = createSignal('');
  const [name, setName] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const add = async () => {
    setBusy(true);
    const acc = await store.run(api.addFeed(url().trim(), name()));
    setBusy(false);
    if (acc) {
      store.banner(t().icalAdded(acc.name || acc.email));
      await store.refresh();
      props.onDone();
    }
  };
  return (
    <div class="ical-form">
      <p style={{ 'font-weight': 600 }}>{t().icalTitle}</p>
      <p class="muted" style={{ 'font-size': '12px' }}>{t().icalHint}</p>
      <input
        class="inp"
        style={{ width: '100%' }}
        type="password"
        placeholder={t().icalUrl}
        aria-label={t().icalUrl}
        value={url()}
        onInput={(e) => setUrl(e.currentTarget.value)}
        onKeyDown={(e) => e.key === 'Enter' && url().trim() && !busy() && add()}
        spellcheck={false}
        autocomplete="off"
      />
      <input
        class="inp"
        style={{ width: '100%' }}
        placeholder={t().icalName}
        aria-label={t().icalName}
        value={name()}
        onInput={(e) => setName(e.currentTarget.value)}
        onKeyDown={(e) => e.key === 'Enter' && url().trim() && !busy() && add()}
      />
      <div style={{ display: 'flex', gap: '8px' }}>
        <button class="btn primary" disabled={!url().trim() || busy()} onClick={add}>{busy() ? t().icalAdding : t().icalAdd}</button>
        <button class="btn ghost" disabled={busy()} onClick={props.onDone}>{t().cancel}</button>
      </div>
    </div>
  );
}

export function Settings() {
  const [section, setSection] = createSignal<Section>(store.snap()?.builtinClient || store.snap()?.config.googleClientId ? 'accounts' : 'google');
  const [adding, setAdding] = createSignal(false);
  const [icalOpen, setIcalOpen] = createSignal(false);
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
      return store.banner(t().needClientFirst, 'warn');
    }
    setAdding(true);
    store.banner(t().browserWillOpen);
    const acc = await store.run(api.addAccount(hint));
    setAdding(false);
    if (acc) {
      store.banner(t().connected(acc.email));
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
      <div class="dialog settings" role="dialog" aria-label={t().settings}>
        <nav class="set-nav" aria-label={t().settingsSections}>
          <For each={NAV()}>
            {(n) => (
              <button class="nav" classList={{ on: section() === n.id }} aria-current={section() === n.id ? 'page' : undefined} onClick={() => setSection(n.id)}>
                {n.icon()}
                {n.label}
              </button>
            )}
          </For>
          <div style={{ flex: 1 }} />
          <p class="muted" style={{ padding: '0 10px', 'font-size': '12px' }}>Kwalendar 0.3.0{isTauri ? '' : t().preview}</p>
        </nav>
        <main class="set-main">
          <div style={{ display: 'flex', 'align-items': 'center' }}>
            <h1 style={{ flex: 1 }}>{NAV().find((n) => n.id === section())!.label}</h1>
            <button class="ib" aria-label={t().closeSettings} onClick={() => store.setSettings(false)}>{I.close()}</button>
          </div>
          <Switch>
            <Match when={section() === 'accounts'}>
              <p class="muted">{t().accountsHint}</p>
              <div style={{ display: 'flex', 'flex-direction': 'column', gap: '8px' }}>
                <For each={snap()!.accounts} fallback={<p class="muted">{t().noAccountsYet}</p>}>
                  {(a) => {
                    const cals = () => snap()!.calendars.filter((c) => c.accountId === a.id).length;
                    const ics = a.kind === 'ics';
                    const checked = () => (a.lastSync ? t().checkedAt(time(a.lastSync)) : t().firstLoad);
                    return (
                      <div class="card" classList={{ warn: a.status === 'reauth' || a.status === 'error' }}>
                        <span class="avb" style={{ background: a.color }}>{(a.name || a.email).charAt(0).toUpperCase()}</span>
                        <div style={{ flex: 1, 'min-width': 0 }}>
                          <p style={{ 'font-weight': 600 }}>
                            <Show
                              when={!ics}
                              fallback={<>{a.name} <span class="muted" style={{ 'font-weight': 400 }}>· {a.email}</span></>}
                            >
                              {a.email} <Show when={a.name}><span class="muted" style={{ 'font-weight': 400 }}>· {a.name}</span></Show>
                            </Show>
                          </p>
                          <p class="muted" style={{ 'font-size': '12px', color: a.status === 'reauth' || a.status === 'error' ? 'var(--warn-text)' : undefined }}>
                            {a.status === 'reauth'
                              ? t().revoked
                              : a.status === 'error'
                                ? a.error
                                : ics
                                  ? `${t().icalBadge} · ${checked()}`
                                  : `${t().calCount(cals())} · ${checked()}`}
                          </p>
                        </div>
                        <Show when={a.status === 'reauth'}>
                          <button class="btn" disabled={adding()} onClick={() => add(a.email)}>{t().signInAgain}</button>
                        </Show>
                        <Show
                          when={confirmId() === a.id}
                          fallback={
                            <button class="ib" aria-label={t().disconnect(a.email)} title={t().disconnectTitle} onClick={() => setConfirmId(a.id)}>{I.trash({ size: 15 })}</button>
                          }
                        >
                          <button class="btn danger" onClick={() => remove(a.id)}>{t().disconnectConfirm}</button>
                          <button class="btn ghost" onClick={() => setConfirmId(null)}>{t().cancel}</button>
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
              <Show
                when={icalOpen()}
                fallback={
                  <div style={{ display: 'flex', gap: '8px', 'flex-wrap': 'wrap' }}>
                    <button class="btn primary" disabled={adding()} onClick={() => add()}>
                      {I.plus({ size: 14, stroke: 2.4 })}
                      {adding() ? t().waitingBrowser : t().addGoogleAccount}
                    </button>
                    <button class="btn" disabled={adding()} onClick={() => setIcalOpen(true)}>
                      {I.plus({ size: 14, stroke: 2.4 })}
                      {t().addIcal}
                    </button>
                  </div>
                }
              >
                <IcalForm onDone={() => setIcalOpen(false)} />
              </Show>
            </Match>
            <Match when={section() === 'google'}>
              <Show
                when={snap()!.builtinClient}
                fallback={
                  <p class="muted">{t().googleOwn}</p>
                }
              >
                <p class="muted">
                  {t().googleBuiltin}
                </p>
              </Show>
              <GoogleClientForm onSaved={() => setSection('accounts')} />
            </Match>
            <Match when={section() === 'sync'}>
              <div>
                <div class="set">
                  <div class="l"><p>{t().pollFocused}</p><p>{t().pollFocusedHint}</p></div>
                  <select class="sel-inp" value={cfg().pollFocusedSecs} onChange={(e) => patch({ pollFocusedSecs: Number(e.currentTarget.value) })}>
                    <For each={[15, 30, 60, 120]}>{(s) => <option value={s}>{t().everySec(s)}</option>}</For>
                  </select>
                </div>
                <div class="set">
                  <div class="l"><p>{t().pollBg}</p><p>{t().pollBgHint}</p></div>
                  <select class="sel-inp" value={cfg().pollBackgroundSecs} onChange={(e) => patch({ pollBackgroundSecs: Number(e.currentTarget.value) })}>
                    <For each={[60, 180, 300, 600, 900]}>{(s) => <option value={s}>{t().everyMin(s / 60)}</option>}</For>
                  </select>
                </div>
                <div class="set" style={{ 'flex-wrap': 'wrap' }}>
                  <div class="l">
                    <p>Push-relay {snap()!.pushActive ? <span style={{ color: 'var(--ok)' }}>{t().relayActive}</span> : cfg().relayUrl ? <span style={{ color: 'var(--warn)' }}>{t().relayDown}</span> : null}</p>
                    <p>{t().relayHintA} <code class="code">relay/</code>{t().relayHintB}</p>
                  </div>
                  <input
                    class="inp"
                    style={{ width: '320px' }}
                    placeholder={t().relayPh}
                    value={cfg().relayUrl}
                    onChange={(e) => patch({ relayUrl: e.currentTarget.value.trim() })}
                    aria-label={t().relayAria}
                    spellcheck={false}
                  />
                </div>
                <div class="set">
                  <div class="l"><p>{t().offlineChanges}</p><p>{t().offlineChangesHint}</p></div>
                  <span class="muted">{snap()!.sync.pending ? t().queued(snap()!.sync.pending) : t().queueEmpty}</span>
                </div>
                <div class="set">
                  <div class="l"><p>{t().syncNow}</p><p>{snap()!.sync.lastSync ? t().lastSync(time(snap()!.sync.lastSync!)) : t().neverSynced}</p></div>
                  <button class="btn" onClick={() => api.syncNow()}>{I.sync({ size: 14 })}{t().syncBtn}</button>
                </div>
              </div>
            </Match>
            <Match when={section() === 'notify'}>
              <div>
                <div class="set">
                  <div class="l"><p>{t().remindersSet}</p><p>{t().remindersHint}</p></div>
                  <Toggle on={cfg().remindersEnabled} label={t().reminders} onChange={(v) => patch({ remindersEnabled: v })} />
                </div>
                <div class="set">
                  <div class="l"><p>{t().changesSet}</p><p>{t().changesHint}</p></div>
                  <Toggle on={cfg().notifyChanges} label={t().changesAria} onChange={(v) => patch({ notifyChanges: v })} />
                </div>
                <div class="set">
                  <div class="l"><p>{t().quiet}</p><p>{snap()!.mutedUntil > store.now() ? t().mutedUntil(time(snap()!.mutedUntil)) : t().quietHint}</p></div>
                  <Show
                    when={snap()!.mutedUntil > store.now()}
                    fallback={<button class="btn" onClick={async () => { await api.mute(60); await store.refresh(); }}>{I.bellOff({ size: 14 })}{t().forHour}</button>}
                  >
                    <button class="btn" onClick={async () => { await api.mute(0); await store.refresh(); }}>{t().turnOn}</button>
                  </Show>
                </div>
                <div class="set">
                  <div class="l"><p>{t().testNotify}</p><p>{t().testNotifyHint}</p></div>
                  <button class="btn" onClick={() => api.testNotification()}>{I.bell({ size: 14 })}{t().show}</button>
                </div>
              </div>
            </Match>
            <Match when={section() === 'system'}>
              <div>
                <div class="set">
                  <div class="l"><p>{t().language}</p><p>{t().languageHint}</p></div>
                  <div class="seg" role="group" aria-label={t().language}>
                    <For each={[['system', t().asWindows], ['en', 'English'], ['ru', 'Русский']] as const}>
                      {([v, label]) => (
                        <button classList={{ on: (cfg().language || 'system') === v }} aria-pressed={(cfg().language || 'system') === v} onClick={() => patch({ language: v })}>
                          {label}
                        </button>
                      )}
                    </For>
                  </div>
                </div>
                <div class="set">
                  <div class="l"><p>{t().theme}</p><p>{t().themeHint}</p></div>
                  <div class="seg" role="group" aria-label={t().theme}>
                    <For each={[['system', t().asWindows], ['dark', t().dark], ['light', t().light]] as const}>
                      {([v, label]) => (
                        <button classList={{ on: cfg().theme === v }} aria-pressed={cfg().theme === v} onClick={() => patch({ theme: v })}>
                          {label}
                        </button>
                      )}
                    </For>
                  </div>
                </div>
                <div class="set">
                  <div class="l"><p>{t().autostartSet}</p><p>{t().autostartHint}</p></div>
                  <Toggle
                    on={!!autostart()}
                    label={t().autostart}
                    onChange={async (v) => {
                      const ok = await store.run(api.autostartSet(v).then(() => true));
                      if (ok) setAutostart(v);
                    }}
                  />
                </div>
                <div class="set">
                  <div class="l"><p>{t().dayStart}</p><p>{t().dayStartHint}</p></div>
                  <select class="sel-inp" value={cfg().dayStartHour} onChange={(e) => patch({ dayStartHour: Number(e.currentTarget.value) })}>
                    <For each={[6, 7, 8, 9, 10]}>{(h) => <option value={h}>{h}:00</option>}</For>
                  </select>
                </div>
                <div class="set">
                  <div class="l"><p>{t().closeWindow}</p><p>{t().closeWindowHint}</p></div>
                </div>
              </div>
            </Match>
            <Match when={section() === 'updates'}>
              <UpdatesSettings />
            </Match>
            <Match when={section() === 'keys'}>
              <div style={{ display: 'grid', 'grid-template-columns': 'max-content 1fr', gap: '10px 24px', 'align-items': 'center' }}>
                <For each={t().keys}>
                  {([k, text]) => (
                    <>
                      <span class="kbd" style={{ 'justify-self': 'start' }}>{k}</span>
                      <span>{text}</span>
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
