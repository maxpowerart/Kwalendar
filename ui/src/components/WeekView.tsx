import { createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { api, targetOf, type Instance } from '../api';
import { store } from '../store';
import { layoutAllDay, layoutDay, onSolid, tint } from '../layout';
import { addDays, DAY, hhmm, sameDay, startOfDay, time, timeRange, weekdayShort } from '../dates';

const HOUR = 52;
const SNAP = 15;
const px = (min: number) => (min * HOUR) / 60;

type Drag =
  | { kind: 'create'; day: number; from: number; to: number; moved: boolean }
  | { kind: 'move'; inst: Instance; day: number; grabMin: number; startMin: number; dayDelta: number; minDelta: number; moved: boolean; x0: number; y0: number }
  | { kind: 'resize'; inst: Instance; day: number; endMin: number; moved: boolean };

export function WeekView() {
  let scrollEl!: HTMLDivElement;
  const cols: HTMLDivElement[] = [];

  const days = createMemo(() => {
    const [s, e] = store.range();
    const n = Math.round((e - s) / DAY);
    return Array.from({ length: n }, (_, i) => addDays(new Date(s), i));
  });
  const timed = createMemo(() => store.items().filter((i) => !i.allDay && i.end - i.start < DAY));
  const allDay = createMemo(() => layoutAllDay(store.items(), days()[0].getTime(), days().length));
  const allDayRows = createMemo(() => Math.max(1, ...allDay().map((a) => a.row + 1)));
  const perDay = createMemo(() => days().map((d) => layoutDay(timed(), d.getTime())));
  const todayIdx = createMemo(() => days().findIndex((d) => sameDay(d, store.now())));
  const nowMin = createMemo(() => {
    const n = new Date(store.now());
    return n.getHours() * 60 + n.getMinutes();
  });

  const [drag, setDrag] = createSignal<Drag | null>(null);
  const [sbw, setSbw] = createSignal(0);

  onMount(() => {
    const cfgStart = store.snap()?.config.dayStartHour ?? 8;
    const h = todayIdx() >= 0 ? Math.max(0, Math.min(cfgStart, Math.floor(nowMin() / 60) - 2)) : cfgStart;
    scrollEl.scrollTop = px(h * 60) - 4;
    const measure = () => setSbw(scrollEl.offsetWidth - scrollEl.clientWidth);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(scrollEl);
    onCleanup(() => ro.disconnect());
  });

  const minuteAt = (dayIdx: number, clientY: number) => {
    const r = cols[dayIdx].getBoundingClientRect();
    return Math.max(0, Math.min(24 * 60, ((clientY - r.top) / HOUR) * 60));
  };
  const dayAt = (clientX: number) => {
    for (let i = 0; i < cols.length; i++) {
      const r = cols[i]?.getBoundingClientRect();
      if (r && clientX >= r.left && clientX < r.right) return i;
    }
    return clientX < (cols[0]?.getBoundingClientRect().left ?? 0) ? 0 : cols.length - 1;
  };
  const snap = (m: number) => Math.round(m / SNAP) * SNAP;

  function listen(move: (e: PointerEvent) => void, up: (e: PointerEvent) => void) {
    const mv = (e: PointerEvent) => move(e);
    const u = (e: PointerEvent) => {
      window.removeEventListener('pointermove', mv);
      window.removeEventListener('pointerup', u);
      up(e);
    };
    window.addEventListener('pointermove', mv);
    window.addEventListener('pointerup', u);
  }

  function onColDown(e: PointerEvent, dayIdx: number) {
    if (e.button !== 0 || !(e.target as HTMLElement).classList.contains('col')) return;
    const m0 = Math.floor(minuteAt(dayIdx, e.clientY) / SNAP) * SNAP;
    setDrag({ kind: 'create', day: dayIdx, from: m0, to: m0 + SNAP, moved: false });
    listen(
      (ev) => {
        const m = snap(minuteAt(dayIdx, ev.clientY));
        const d = drag();
        if (d?.kind !== 'create') return;
        setDrag({ ...d, from: Math.min(m0, m), to: Math.max(m0 + SNAP, m), moved: true });
      },
      () => {
        const d = drag();
        setDrag(null);
        if (d?.kind !== 'create') return;
        const base = days()[dayIdx].getTime();
        let from = d.from;
        let to = d.to;
        if (!d.moved || to - from < 30) {
          from = Math.floor(m0 / 30) * 30;
          to = from + 60;
        }
        const draft = store.newDraft(base + from * 60_000, base + to * 60_000);
        if (draft) store.setDraft(draft);
      },
    );
  }

  function onEvDown(e: PointerEvent, inst: Instance, dayIdx: number, resize: boolean) {
    if (e.button !== 0) return;
    e.stopPropagation();
    const dayBase = days()[dayIdx].getTime();
    const startMin = (inst.start - dayBase) / 60_000;
    if (resize) {
      if (!inst.canEdit) return;
      setDrag({ kind: 'resize', inst, day: dayIdx, endMin: (inst.end - dayBase) / 60_000, moved: false });
      listen(
        (ev) => {
          const d = drag();
          if (d?.kind !== 'resize') return;
          const m = Math.max(startMin + SNAP, snap(minuteAt(dayIdx, ev.clientY)));
          setDrag({ ...d, endMin: m, moved: true });
        },
        () => {
          const d = drag();
          setDrag(null);
          if (d?.kind === 'resize' && d.moved) {
            void store.run(api.moveEvent(targetOf(inst), inst.start, dayBase + d.endMin * 60_000));
          }
        },
      );
      return;
    }
    const grabMin = minuteAt(dayIdx, e.clientY) - startMin;
    setDrag({ kind: 'move', inst, day: dayIdx, grabMin, startMin, dayDelta: 0, minDelta: 0, moved: false, x0: e.clientX, y0: e.clientY });
    listen(
      (ev) => {
        const d = drag();
        if (d?.kind !== 'move') return;
        const far = Math.abs(ev.clientX - d.x0) + Math.abs(ev.clientY - d.y0) > 4;
        if (!d.moved && (!far || !inst.canEdit)) return;
        const target = dayAt(ev.clientX);
        const m = snap(minuteAt(target, ev.clientY) - grabMin);
        setDrag({ ...d, moved: true, dayDelta: target - dayIdx, minDelta: m - startMin });
      },
      () => {
        const d = drag();
        setDrag(null);
        if (d?.kind !== 'move') return;
        if (!d.moved) {
          store.setSelected(store.selected()?.key === inst.key ? null : inst);
          return;
        }
        const delta = d.dayDelta * DAY + d.minDelta * 60_000;
        if (delta === 0) return;
        // Day shifts go through the calendar so DST changes keep wall-clock time.
        const s = new Date(inst.start);
        s.setDate(s.getDate() + d.dayDelta);
        const ns = s.getTime() + d.minDelta * 60_000;
        void store.run(api.moveEvent(targetOf(inst), ns, ns + (inst.end - inst.start)));
      },
    );
  }

  const evClass = (inst: Instance, h: number) => ({
    ev: true,
    short: h < 34,
    past: inst.end < store.now(),
    declined: inst.selfResponse === 'declined',
    tentative: inst.selfResponse === 'tentative' || inst.selfResponse === 'needsAction',
    pending: inst.pending,
    sel: store.selected()?.key === inst.key,
    drag: (() => {
      const d = drag();
      return !!d && d.kind !== 'create' && d.moved && d.inst.key === inst.key;
    })(),
  });

  const evStyle = (inst: Instance, top: number, height: number, left: string, width: string) => {
    const t = tint(inst.color);
    const sel = store.selected()?.key === inst.key;
    return {
      top: `${px(top)}px`,
      height: `${Math.max(px(height) - 2, 16)}px`,
      left,
      width,
      background: sel ? t.solid : t.bg,
      color: sel ? onSolid(inst.color) : t.fg,
    };
  };

  return (
    <div class="week" style={{ '--days': days().length, '--sbw': `${sbw()}px` }}>
      <div class="week-head">
        <div />
        <For each={days()}>
          {(d) => (
            <div
              class="dh"
              classList={{ today: sameDay(d, store.now()), we: d.getDay() === 0 || d.getDay() === 6 }}
              onClick={() => store.goto(d, 'day')}
              title="Открыть день"
            >
              <span class="dh-n">{weekdayShort(d)}</span>
              <span class="dh-d">{d.getDate()}</span>
            </div>
          )}
        </For>
      </div>

      <div class="allday" style={{ 'grid-template-rows': `repeat(${allDayRows()}, 24px)` }}>
        <div class="lab">весь день</div>
        <For each={allDay()}>
          {(a) => {
            const t = tint(a.inst.color);
            return (
              <button
                class="ad"
                classList={{ past: a.inst.end < store.now() }}
                style={{
                  'grid-column': `${a.startCol + 2} / span ${a.span}`,
                  'grid-row': `${a.row + 1}`,
                  background: store.selected()?.key === a.inst.key ? t.solid : t.bg,
                  color: store.selected()?.key === a.inst.key ? onSolid(a.inst.color) : t.fg,
                  opacity: a.inst.end < store.now() ? 0.6 : 1,
                }}
                title={a.inst.title}
                onClick={() => store.setSelected(a.inst)}
              >
                {a.inst.allDay ? a.inst.title : `${time(a.inst.start)} ${a.inst.title}`}
              </button>
            );
          }}
        </For>
      </div>

      <div class="scroll" ref={scrollEl}>
        <div class="grid">
          <div class="tcol">
            <For each={Array.from({ length: 23 }, (_, i) => i + 1)}>
              {(h) => (
                <Show when={todayIdx() < 0 || Math.abs(nowMin() - h * 60) > 14}>
                  <span class="tl" style={{ top: `${px(h * 60)}px` }}>
                    {String(h).padStart(2, '0')}:00
                  </span>
                </Show>
              )}
            </For>
            <Show when={todayIdx() >= 0}>
              <span class="nowpill" style={{ top: `${px(nowMin())}px` }}>
                {hhmm(store.now())}
              </span>
            </Show>
          </div>

          <For each={days()}>
            {(d, i) => (
              <div
                class="col"
                ref={(el) => (cols[i()] = el)}
                classList={{ today: sameDay(d, store.now()), we: d.getDay() === 0 || d.getDay() === 6 }}
                onPointerDown={(e) => onColDown(e, i())}
              >
                <For each={perDay()[i()]}>
                  {(p) => {
                    const h = () => p.bottom - p.top;
                    const left = `calc(${(p.col / p.cols) * 100}% + 2px)`;
                    const width = `calc(${(p.span / p.cols) * 100}% - 5px)`;
                    return (
                      <button
                        classList={evClass(p.inst, h())}
                        style={{
                          ...evStyle(p.inst, p.top, h(), left, width),
                          visibility: (() => {
                            const dr = drag();
                            return dr && dr.kind === 'move' && dr.moved && dr.inst.key === p.inst.key ? 'hidden' : 'visible';
                          })(),
                        }}
                        title={`${p.inst.title}\n${timeRange(p.inst.start, p.inst.end)}${p.inst.location ? '\n' + p.inst.location : ''}`}
                        onPointerDown={(e) => onEvDown(e, p.inst, i(), false)}
                        onDblClick={() => p.inst.canEdit && store.setDraft(store.editDraft(p.inst))}
                      >
                        <Show
                          when={h() >= 34}
                          fallback={
                            <b>
                              {p.inst.title} <span style={{ display: 'inline', opacity: 0.8, 'font-weight': 400 }}>· {time(p.inst.start)}</span>
                            </b>
                          }
                        >
                          <b>{p.inst.title}</b>
                          <span>
                            {timeRange(p.inst.start, p.inst.end)}
                            {p.inst.location ? ` · ${p.inst.location}` : p.inst.meetLink ? ' · видео' : ''}
                          </span>
                        </Show>
                        <Show when={p.inst.canEdit}>
                          <span class="rz" onPointerDown={(e) => onEvDown(e, p.inst, i(), true)} />
                        </Show>
                      </button>
                    );
                  }}
                </For>

                <Show when={sameDay(d, store.now())}>
                  <div class="nowline" style={{ top: `${px(nowMin())}px` }} />
                </Show>

                {/* drag previews */}
                <Show when={(() => { const x = drag(); return x?.kind === 'create' && x.day === i() && x.moved ? x : null; })()}>
                  {(x) => {
                    const c = x() as Extract<Drag, { kind: 'create' }>;
                    return (
                      <div class="ghost" style={{ top: `${px(c.from)}px`, height: `${px(c.to - c.from)}px` }}>
                        {hhmm(startOfDay(d).getTime() + c.from * 60_000)}–{hhmm(startOfDay(d).getTime() + c.to * 60_000)}
                      </div>
                    );
                  }}
                </Show>
                <Show when={(() => { const x = drag(); return x?.kind === 'move' && x.moved && x.day + x.dayDelta === i() ? x : null; })()}>
                  {(x) => {
                    const m = x() as Extract<Drag, { kind: 'move' }>;
                    const dur = () => (m.inst.end - m.inst.start) / 60_000;
                    const st = () => m.startMin + m.minDelta;
                    const base = startOfDay(d).getTime();
                    const t = tint(m.inst.color);
                    return (
                      <div class="ev drag" style={{ top: `${px(st())}px`, height: `${px(dur()) - 2}px`, left: '3px', right: '5px', background: t.solid, color: onSolid(m.inst.color), 'pointer-events': 'none' }}>
                        <b>{m.inst.title}</b>
                        <span>
                          {hhmm(base + st() * 60_000)}–{hhmm(base + (st() + dur()) * 60_000)}
                        </span>
                      </div>
                    );
                  }}
                </Show>
                <Show when={(() => { const x = drag(); return x?.kind === 'resize' && x.moved && x.day === i() ? x : null; })()}>
                  {(x) => {
                    const r = x() as Extract<Drag, { kind: 'resize' }>;
                    const base = startOfDay(d).getTime();
                    const st = (r.inst.start - base) / 60_000;
                    return (
                      <div class="ghost" style={{ top: `${px(st)}px`, height: `${px(r.endMin - st)}px` }}>
                        {r.inst.title} · до {hhmm(base + r.endMin * 60_000)}
                      </div>
                    );
                  }}
                </Show>
              </div>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
