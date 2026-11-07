import { createRoot, createSignal, For, Show } from 'solid-js';
import { cmpVersion, on, updates, type Listed, type Release, type UpdateInfo } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { t } from '../i18n';

const u = createRoot(() => {
  const [info, setInfo] = createSignal<UpdateInfo | null>(null);
  /** Release shown in the dialog (an offered update, a rollback target or a picked version). */
  const [dialog, setDialog] = createSignal<Release | null>(null);
  const [progress, setProgress] = createSignal<{ done: number; total: number } | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  return { info, setInfo, dialog, setDialog, progress, setProgress, error, setError };
});

let warnedWithdrawn = false;

function onInfo(i: UpdateInfo) {
  u.setInfo(i);
  const w = i.withdrawn;
  if (w && !warnedWithdrawn) {
    warnedWithdrawn = true;
    const target = w.target;
    store.banner(
      t().versionWithdrawn(i.current, w.reason),
      'warn',
      target ? { label: t().goTo(target.version), run: () => openListed(target) } : undefined,
    );
  }
}

export async function initUpdates() {
  try {
    onInfo(await updates.info());
  } catch {
    /* no updater in this build */
  }
  await on<UpdateInfo>('kw:update', onInfo);
  await on<{ done: number; total: number }>('kw:update-progress', (p) => u.setProgress(p));
}

async function openListed(l: Listed) {
  u.setError(null);
  try {
    u.setDialog(await updates.manifest(l.manifestUrl));
  } catch (e) {
    store.banner(String(e), 'error');
  }
}

async function install(r: Release) {
  u.setError(null);
  u.setProgress({ done: 0, total: 0 });
  try {
    await updates.install(r); // the app exits and restarts on success
  } catch (e) {
    u.setError(String(e));
    u.setProgress(null);
  }
}

/** Title-bar button: shown when an update is available or the running version was withdrawn. */
export function UpdateButton() {
  const label = () => {
    const i = u.info();
    if (i?.withdrawn) return { text: t().withdrawnBtn, warn: true };
    if (i?.available) return { text: t().updateBtn(i.available.version), warn: false };
    return null;
  };
  return (
    <Show when={label()}>
      <button
        class="chrome-btn"
        classList={{ warn: label()!.warn }}
        onClick={() => {
          const i = u.info()!;
          if (i.available) u.setDialog(i.available);
          else if (i.withdrawn?.target) void openListed(i.withdrawn.target);
          else store.setSettings(true);
        }}
      >
        {I.down({ size: 13 })}
        {label()!.text}
      </button>
    </Show>
  );
}

function bytes(n: number) {
  return n > 1048576 ? t().mb((n / 1048576).toFixed(1)) : t().kb(Math.round(n / 1024));
}

export function UpdateDialog() {
  const r = () => u.dialog()!;
  const current = () => u.info()?.current ?? '';
  const older = () => cmpVersion(r().version, current()) < 0;
  const same = () => cmpVersion(r().version, current()) === 0;
  const close = () => {
    if (!u.progress()) u.setDialog(null);
  };
  return (
    <Show when={u.dialog()}>
      <div class="scrim" onPointerDown={(e) => e.target === e.currentTarget && close()} onKeyDown={(e) => e.key === 'Escape' && close()}>
        <div class="dialog editor" role="dialog" aria-label={t().updateDialog}>
          <div class="ed-head">
            <span class="lbl" style={{ flex: 1 }}>
              {older() ? t().rollback : same() ? t().reinstallTitle : t().updateAvailable}
            </span>
            <button class="ib" aria-label={t().close} onClick={close}>{I.close({ size: 15 })}</button>
          </div>
          <div class="ed-body">
            <div style={{ display: 'flex', 'align-items': 'baseline', gap: '10px' }}>
              <h2 style={{ 'font-size': '20px', 'font-weight': 600 }}>Kwalendar {r().version}</h2>
              <span class="muted">{t().nowVersion(current())}{r().published ? t().released(r().published!) : ''}</span>
            </div>
            <Show when={older()}>
              <p style={{ color: 'var(--warn-text)' }}>{t().olderWarn}</p>
            </Show>
            <Show when={r().notes} fallback={<p class="muted">{t().noNotes}</p>}>
              <div class="desc" style={{ 'max-height': '300px', 'overflow-y': 'auto', padding: '10px 12px', background: 'var(--raised)', 'border-radius': '8px', border: '1px solid var(--line-3)' }}>
                {r().notes}
              </div>
            </Show>
            <Show when={u.progress()}>
              {(p) => (
                <div style={{ display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
                  <div style={{ height: '6px', background: 'var(--raised)', 'border-radius': '3px', overflow: 'hidden' }}>
                    <div style={{ height: '100%', width: p().total ? `${(p().done / p().total) * 100}%` : '30%', background: 'var(--accent)', transition: 'width .15s' }} />
                  </div>
                  <span class="muted" style={{ 'font-size': '12px' }}>
                    {p().total && p().done >= p().total
                      ? t().verified
                      : t().downloading(bytes(p().done), p().total ? bytes(p().total) : '')}
                  </span>
                </div>
              )}
            </Show>
            <Show when={u.error()}>
              <p style={{ color: 'var(--err-text)' }}>{u.error()}</p>
            </Show>
          </div>
          <div class="ed-foot">
            <Show when={!older() && u.info()?.available?.version === r().version}>
              <button
                class="btn ghost"
                disabled={!!u.progress()}
                onClick={async () => {
                  await store.run(updates.skip(r().version));
                  u.setInfo({ ...u.info()!, available: null });
                  u.setDialog(null);
                }}
              >
                {t().skipVersion}
              </button>
            </Show>
            <span style={{ flex: 1 }} />
            <button class="btn ghost" disabled={!!u.progress()} onClick={close}>{t().later}</button>
            <button class="btn primary" disabled={!!u.progress()} onClick={() => install(r())}>
              {older() ? t().rollbackTo(r().version) : same() ? t().reinstall : t().updateRestart}
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
}

/** Settings → Updates. */
export function UpdatesSettings() {
  const [list, setList] = createSignal<Listed[] | null>(null);
  const [listErr, setListErr] = createSignal<string | null>(null);
  const [checking, setChecking] = createSignal(false);
  const cfg = () => store.snap()!.config;

  const check = async () => {
    setChecking(true);
    try {
      const i = await updates.check();
      onInfo(i);
      if (i.error) store.banner(t().checkFailed(i.error), 'error');
      else if (!i.available) store.banner(t().latestInstalled(i.current));
    } finally {
      setChecking(false);
    }
  };
  const loadList = async () => {
    setListErr(null);
    try {
      setList(await updates.list());
    } catch (e) {
      setListErr(String(e));
    }
  };

  return (
    <div>
      <div class="set">
        <div class="l">
          <p>Kwalendar {u.info()?.current ?? ''}</p>
          <p>
            {u.info()?.available
              ? t().versionAvailable(u.info()!.available!.version)
              : u.info()?.checkedAt
                ? t().isLatest
                : t().fromGithub}
          </p>
        </div>
        <Show when={u.info()?.available}>
          <button class="btn primary" onClick={() => u.setDialog(u.info()!.available!)}>{t().update}</button>
        </Show>
        <button class="btn" disabled={checking()} onClick={check}>
          <span class={checking() ? 'spin' : ''} style={{ display: 'inline-flex' }}>{I.sync({ size: 14 })}</span>
          {t().check}
        </button>
      </div>
      <div class="set">
        <div class="l"><p>{t().autoCheck}</p><p>{t().autoCheckHint}</p></div>
        <button
          class="sw"
          classList={{ on: cfg().checkUpdates }}
          role="switch"
          aria-checked={cfg().checkUpdates}
          aria-label={t().autoCheckAria}
          onClick={async () => {
            await store.run(store.snapConfigPatch({ checkUpdates: !cfg().checkUpdates }));
          }}
        >
          <span />
        </button>
      </div>
      <Show when={cfg().skippedVersion}>
        <div class="set">
          <div class="l"><p>{t().skipped(cfg().skippedVersion)}</p><p>{t().skippedHint}</p></div>
          <button class="btn" onClick={() => store.run(store.snapConfigPatch({ skippedVersion: '' }))}>{t().offerAgain}</button>
        </div>
      </Show>
      <div class="set" style={{ 'flex-direction': 'column', 'align-items': 'stretch' }}>
        <div style={{ display: 'flex', 'align-items': 'center', gap: '16px' }}>
          <div class="l"><p>{t().otherVersion}</p><p>{t().otherVersionHint}</p></div>
          <button class="btn" onClick={loadList}>{t().showVersions}</button>
        </div>
        <Show when={listErr()}>
          <p style={{ color: 'var(--err-text)', 'font-size': '12px' }}>{listErr()}</p>
        </Show>
        <Show when={list()}>
          <div style={{ display: 'flex', 'flex-direction': 'column', gap: '2px', 'margin-top': '6px' }}>
            <For each={list()!} fallback={<p class="muted">{t().noReleases}</p>}>
              {(l) => {
                const isCurrent = () => cmpVersion(l.version, u.info()?.current ?? '0') === 0;
                return (
                  <button class="qa-item" style={{ height: '38px' }} disabled={l.withdrawn} onClick={() => openListed(l)} title={l.withdrawn ? l.reason : ''}>
                    <span class="nm" style={{ 'font-weight': 500, opacity: l.withdrawn ? 0.5 : 1 }}>
                      {l.version}
                      {isCurrent() ? t().installedTag : ''}
                      {l.prerelease && !l.withdrawn ? t().prereleaseTag : ''}
                    </span>
                    <Show when={l.withdrawn}>
                      <span class="sub" style={{ color: 'var(--warn-text)' }}>{t().withdrawnTag(l.reason)}</span>
                    </Show>
                    <span class="sub">{l.published}</span>
                  </button>
                );
              }}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
}
