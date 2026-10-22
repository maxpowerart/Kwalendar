import { createMemo, createSignal, For, Show, type JSX } from 'solid-js';
import { api, targetOf, type Attendee, type Instance } from '../api';
import { store } from '../store';
import { I } from '../icons';
import { minutesLabel, relative, whenLabel } from '../dates';

const RESP: Record<string, { t: string; c: string }> = {
  accepted: { t: 'да', c: 'var(--ok)' },
  tentative: { t: 'может быть', c: 'var(--warn)' },
  declined: { t: 'нет', c: 'var(--err-text)' },
  needsAction: { t: 'нет ответа', c: 'var(--muted)' },
};

/** Google descriptions are HTML; show them as plain text with clickable links. */
export function descriptionText(html: string): string {
  const withBreaks = html.replace(/<br\s*\/?>/gi, '\n').replace(/<\/(p|div|li)>/gi, '\n');
  const doc = new DOMParser().parseFromString(withBreaks, 'text/html');
  return (doc.body.textContent ?? '').replace(/\n{3,}/g, '\n\n').trim();
}

export function Linkified(props: { text: string }): JSX.Element {
  const parts = createMemo(() => props.text.split(/(https?:\/\/[^\s<>"')]+)/g));
  return (
    <For each={parts()}>
      {(p) =>
        /^https?:\/\//.test(p) ? (
          <a href={p} onClick={(e) => { e.preventDefault(); void api.openUrl(p); }}>
            {p.length > 60 ? p.slice(0, 57) + '…' : p}
          </a>
        ) : (
          p
        )
      }
    </For>
  );
}

function initials(a: Attendee) {
  const s = (a.name || a.email).trim();
  return s.charAt(0).toUpperCase();
}

const AV_COLORS = ['#3D8BD8', '#2EC4D6', '#E8C13D', '#C37BE8', '#3DC28B', '#E85D8A', '#E8665A'];

export function EventPanel(props: { inst: Instance }) {
  const i = () => props.inst;
  const cal = () => store.calendarOf(i().accountId, i().calendarId);
  const account = () => store.snap()?.accounts.find((a) => a.id === i().accountId);
  const [confirm, setConfirm] = createSignal(false);
  const [showAll, setShowAll] = createSignal(false);

  const counts = createMemo(() => {
    const c = { accepted: 0, tentative: 0, declined: 0, needsAction: 0 } as Record<string, number>;
    for (const a of i().attendees) c[a.response] = (c[a.response] ?? 0) + 1;
    return c;
  });
  const isAttendee = () => i().attendees.some((a) => a.isSelf) && !i().organizerSelf;
  const rel = () => relative(i().start, i().end, store.now());

  const respond = (r: string) => store.run(api.respond(targetOf(i()), r));
  const del = async (scope: 'this' | 'series') => {
    setConfirm(false);
    await store.run(api.deleteEvent(targetOf(i()), scope));
    store.setSelected(null);
  };

  return (
    <section class="panel" aria-label="Событие">
      <div style={{ display: 'flex', 'align-items': 'center', 'justify-content': 'space-between' }}>
        <span class="lbl">Событие{i().pending ? ' · сохраняется…' : ''}</span>
        <div style={{ display: 'flex' }}>
          <Show when={i().canEdit}>
            <button class="ib" aria-label="Редактировать" title="Редактировать (E)" onClick={() => store.setDraft(store.editDraft(i()))}>
              {I.edit({ size: 15 })}
            </button>
            <button class="ib" aria-label="Удалить" title="Удалить (Del)" onClick={() => (i().recurring ? setConfirm(!confirm()) : del('this'))}>
              {I.trash({ size: 15 })}
            </button>
          </Show>
          <Show when={i().htmlLink}>
            <button class="ib" aria-label="Открыть в Google Календаре" title="Открыть в Google Календаре" onClick={() => api.openUrl(i().htmlLink!)}>
              {I.external({ size: 15 })}
            </button>
          </Show>
          <button class="ib" aria-label="Закрыть панель" title="Закрыть (Esc)" onClick={() => store.setSelected(null)}>
            {I.close({ size: 15 })}
          </button>
        </div>
      </div>

      <Show when={confirm()}>
        <div class="card" style={{ 'flex-direction': 'column', 'align-items': 'stretch', gap: '8px' }}>
          <span>Удалить повторяющееся событие</span>
          <div style={{ display: 'flex', gap: '6px' }}>
            <button class="btn danger" style={{ flex: 1 }} onClick={() => del('this')}>Только это</button>
            <button class="btn danger" style={{ flex: 1 }} onClick={() => del('series')}>Всю серию</button>
          </div>
        </div>
      </Show>

      <div style={{ display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
        <div style={{ display: 'flex', 'align-items': 'flex-start', gap: '10px' }}>
          <span style={{ width: '12px', height: '12px', 'border-radius': '4px', background: i().color, 'margin-top': '6px', flex: '0 0 auto' }} />
          <h2 style={{ 'text-decoration': i().selfResponse === 'declined' ? 'line-through' : undefined }}>{i().title}</h2>
        </div>
        <p style={{ color: 'var(--text)', 'padding-left': '22px' }}>{whenLabel(i())}</p>
        <Show when={rel()}>
          <p style={{ color: 'var(--node)', 'padding-left': '22px', 'font-size': '12px', 'font-weight': 600 }}>{rel()}</p>
        </Show>
        <Show when={i().recurring}>
          <p class="muted" style={{ 'padding-left': '22px', 'font-size': '12px', display: 'flex', gap: '6px', 'align-items': 'center' }}>
            {I.repeat({ size: 12 })} повторяющееся
          </p>
        </Show>
      </div>

      <Show when={i().meetLink}>
        <div style={{ display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
          <button class="join" onClick={() => api.openUrl(i().meetLink!)}>
            {I.video()}
            {i().meetLink!.includes('zoom') ? 'Подключиться к Zoom' : i().meetLink!.includes('meet.google') ? 'Присоединиться к Google Meet' : 'Присоединиться'}
          </button>
          <span style={{ 'font-family': 'var(--mono)', 'font-size': '11px', color: 'var(--muted)', 'text-align': 'center', overflow: 'hidden', 'text-overflow': 'ellipsis', 'white-space': 'nowrap' }}>
            {i().meetLink!.replace(/^https?:\/\//, '')}
          </span>
        </div>
      </Show>

      <div style={{ display: 'flex', 'flex-direction': 'column', gap: '14px' }}>
        <Show when={i().location}>
          <div class="row">
            {I.pin()}
            <div style={{ 'word-break': 'break-word' }}>
              <Linkified text={i().location!} />
            </div>
          </div>
        </Show>
        <div class="row">
          {I.calendar()}
          <div>
            <div>
              {account()?.name ?? account()?.email} · {cal()?.summary}
            </div>
            <div class="sub">{account()?.email}</div>
          </div>
        </div>
        <div class="row">
          {I.bell()}
          <div>
            <Show when={i().reminders.length} fallback={<span class="muted">без напоминаний</span>}>
              <For each={[...i().reminders].sort((a, b) => a - b)}>{(m) => <div>{minutesLabel(m)}</div>}</For>
            </Show>
          </div>
        </div>
        <Show when={i().attendees.length}>
          <div class="row">
            {I.users()}
            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '8px', flex: 1, 'min-width': 0 }}>
              <div>
                {i().attendees.length} участн.{' '}
                <span class="muted">
                  · {[counts().accepted && `${counts().accepted} да`, counts().tentative && `${counts().tentative} может быть`, counts().declined && `${counts().declined} нет`]
                    .filter(Boolean)
                    .join(', ')}
                </span>
              </div>
              <For each={showAll() ? i().attendees : i().attendees.slice(0, 6)}>
                {(a, n) => (
                  <div style={{ display: 'flex', 'align-items': 'center', gap: '8px', 'min-width': 0 }}>
                    <span class="av" style={{ background: AV_COLORS[n() % AV_COLORS.length] }}>{a.isSelf ? 'Я' : initials(a)}</span>
                    <span style={{ flex: 1, 'min-width': 0, overflow: 'hidden', 'text-overflow': 'ellipsis', 'white-space': 'nowrap' }} title={a.email}>
                      {a.isSelf ? 'Вы' : a.name || a.email}
                    </span>
                    <span style={{ 'font-size': '11px', color: a.organizer ? 'var(--muted)' : RESP[a.response]?.c }}>
                      {a.organizer ? 'организатор' : RESP[a.response]?.t}
                    </span>
                  </div>
                )}
              </For>
              <Show when={i().attendees.length > 6 && !showAll()}>
                <button class="btn ghost" style={{ 'align-self': 'flex-start', height: '26px', padding: '0 6px' }} onClick={() => setShowAll(true)}>
                  ещё {i().attendees.length - 6}
                </button>
              </Show>
            </div>
          </div>
        </Show>
        <Show when={i().description}>
          <div class="row">
            {I.text()}
            <p class="desc">
              <Linkified text={descriptionText(i().description!)} />
            </p>
          </div>
        </Show>
      </div>

      <Show when={isAttendee()}>
        <div style={{ 'margin-top': 'auto', display: 'flex', 'flex-direction': 'column', gap: '8px' }}>
          <span style={{ 'font-size': '12px', color: 'var(--muted)' }}>Пойдёте?</span>
          <div class="seg">
            <button classList={{ on: i().selfResponse === 'accepted' }} onClick={() => respond('accepted')}>Да</button>
            <button classList={{ on: i().selfResponse === 'tentative' }} onClick={() => respond('tentative')}>Может быть</button>
            <button classList={{ on: i().selfResponse === 'declined' }} onClick={() => respond('declined')}>Нет</button>
          </div>
        </div>
      </Show>
    </section>
  );
}
