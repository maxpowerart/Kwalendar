import * as chrono from 'chrono-node';
import type { Calendar } from './api';

export interface Parsed {
  title: string;
  start: number;
  end: number;
  allDay: boolean;
  calendar?: Calendar;
  /** something time-like was recognised */
  hasDate: boolean;
}

const DURATION = /(?:^|\s)(?:на|for)\s+(\d+(?:[.,]\d+)?)\s*(минут[а-я]*|мин\.?|м|часа|часов|час|ч\.?|min(?:utes?)?|m|hours?|hrs?|h)(?=\s|$|[,.!])/i;
const HALF_HOUR = /(?:^|\s)на\s+полчаса(?=\s|$)/i;
const AT_HOUR = /(?:^|\s)(?:в|at)\s+(\d{1,2})(?:[:.](\d{2}))?(?:\s*(?:часов|часа|час|ч))?(?=\s|$|[,.!])/i;
const TAG = /(?:^|\s)#([\p{L}\d_-]+)/u;

function cleanup(s: string): string {
  return s
    .replace(/\s{2,}/g, ' ')
    .replace(/^[\s,.;:—-]+|[\s,.;:—-]+$/g, '')
    .replace(/\s+(в|на|at|on|с|до)$/i, '')
    .trim();
}

export function parseQuick(input: string, calendars: Calendar[], now = new Date()): Parsed {
  let text = ` ${input} `;
  let calendar: Calendar | undefined;

  const tag = text.match(TAG);
  if (tag) {
    const t = tag[1].toLowerCase();
    calendar =
      calendars.find((c) => c.summary.toLowerCase() === t) ??
      calendars.find((c) => c.summary.toLowerCase().startsWith(t)) ??
      calendars.find((c) => c.summary.toLowerCase().includes(t));
    text = text.replace(tag[0], ' ');
  }

  let durationMin: number | null = null;
  const dm = text.match(DURATION);
  if (dm) {
    const n = parseFloat(dm[1].replace(',', '.'));
    durationMin = /^(ч|час|hour|hr|h)/i.test(dm[2]) ? Math.round(n * 60) : Math.round(n);
    text = text.replace(dm[0], ' ');
  } else if (HALF_HOUR.test(text)) {
    durationMin = 30;
    text = text.replace(HALF_HOUR, ' ');
  }

  const results = [...chrono.ru.parse(text, now, { forwardDate: true }), ...chrono.parse(text, now, { forwardDate: true })];
  const r = results.sort((a, b) => b.text.length - a.text.length)[0];
  let start: Date | null = null;
  let end: Date | null = null;
  let hasTime = false;
  if (r) {
    start = r.start.date();
    hasTime = r.start.isCertain('hour');
    end = r.end ? r.end.date() : null;
    text = text.replace(r.text, ' ');
  }

  if (!hasTime) {
    const at = text.match(AT_HOUR);
    if (at) {
      const h = Number(at[1]);
      const m = at[2] ? Number(at[2]) : 0;
      if (h < 24 && m < 60) {
        const base = start ?? new Date(now);
        base.setHours(h, m, 0, 0);
        // "в 9" said at 15:00 without a date means tomorrow.
        if (!start && base.getTime() < now.getTime()) base.setDate(base.getDate() + 1);
        start = base;
        hasTime = true;
        text = text.replace(at[0], ' ');
      }
    }
  }

  const title = cleanup(text);
  if (!start) {
    const s = new Date(now);
    s.setMinutes(0, 0, 0);
    s.setHours(s.getHours() + 1);
    return { title, start: s.getTime(), end: s.getTime() + (durationMin ?? 60) * 60_000, allDay: false, calendar, hasDate: false };
  }
  if (!hasTime && durationMin === null) {
    const s = new Date(start);
    s.setHours(0, 0, 0, 0);
    const e = end ? new Date(end) : new Date(s);
    e.setHours(0, 0, 0, 0);
    e.setDate(e.getDate() + 1);
    return { title, start: s.getTime(), end: e.getTime(), allDay: true, calendar, hasDate: true };
  }
  if (!hasTime) start.setHours(10, 0, 0, 0);
  const s = start.getTime();
  const e = end && end.getTime() > s ? end.getTime() : s + (durationMin ?? 60) * 60_000;
  return { title, start: s, end: e, allDay: false, calendar, hasDate: true };
}
