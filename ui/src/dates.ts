import { locale, t } from './i18n';

export const DAY = 86_400_000;
export const MIN = 60_000;

export function startOfDay(d: Date | number): Date {
  const x = new Date(d);
  x.setHours(0, 0, 0, 0);
  return x;
}

export function addDays(d: Date, n: number): Date {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
}

export function addMonths(d: Date, n: number): Date {
  const x = new Date(d);
  const day = x.getDate();
  x.setDate(1);
  x.setMonth(x.getMonth() + n);
  const last = new Date(x.getFullYear(), x.getMonth() + 1, 0).getDate();
  x.setDate(Math.min(day, last));
  return x;
}

/** Monday-based week start. */
export function startOfWeek(d: Date): Date {
  const x = startOfDay(d);
  const wd = (x.getDay() + 6) % 7;
  return addDays(x, -wd);
}

export function startOfMonth(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), 1);
}

export function sameDay(a: Date | number, b: Date | number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return x.getFullYear() === y.getFullYear() && x.getMonth() === y.getMonth() && x.getDate() === y.getDate();
}

export function isoDate(d: Date | number): string {
  const x = new Date(d);
  const m = String(x.getMonth() + 1).padStart(2, '0');
  const day = String(x.getDate()).padStart(2, '0');
  return `${x.getFullYear()}-${m}-${day}`;
}

export function parseIsoDate(s: string): Date {
  const [y, m, d] = s.split('-').map(Number);
  return new Date(y, m - 1, d);
}

export function isoWeek(d: Date): number {
  const x = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = x.getUTCDay() || 7;
  x.setUTCDate(x.getUTCDate() + 4 - day);
  const y0 = new Date(Date.UTC(x.getUTCFullYear(), 0, 1));
  return Math.ceil(((x.getTime() - y0.getTime()) / DAY + 1) / 7);
}

// Formatters follow the UI language; cached per locale.
const fmtCache = new Map<string, Intl.DateTimeFormat>();
const fmt = (name: string, opts: Intl.DateTimeFormatOptions) => (d: Date | number) => {
  const key = `${locale()}|${name}`;
  let f = fmtCache.get(key);
  if (!f) fmtCache.set(key, (f = new Intl.DateTimeFormat(locale(), opts)));
  return f.format(d);
};
const fmtTime = fmt('time', { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
const fmtMonthYear = fmt('monthYear', { month: 'long', year: 'numeric' });
const fmtDayMonth = fmt('dayMonth', { day: 'numeric', month: 'long' });
const fmtWeekdayShort = fmt('wdShort', { weekday: 'short' });
const fmtWeekdayLong = fmt('wdLong', { weekday: 'long' });
const fmtShortDate = fmt('shortDate', { day: 'numeric', month: 'short' });

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export const time = (ms: number) => fmtTime(ms);
export const monthYear = (d: Date) => cap(fmtMonthYear(d).replace(' г.', ''));
export const dayMonth = (d: Date | number) => fmtDayMonth(d);
export const weekdayShort = (d: Date) => cap(fmtWeekdayShort(d));
export const weekdayLong = (d: Date | number) => cap(fmtWeekdayLong(d));
export const shortDate = (d: Date | number) => fmtShortDate(d).replace('.', '');

/** Monday-first short weekday names for grid headers. */
export const weekdays = () => t().weekdays;

export function timeRange(start: number, end: number): string {
  return `${time(start)}–${time(end)}`;
}

/** "через 1 ч 13 мин" / "идёт сейчас" / "" once it has ended */
export function relative(start: number, end: number, now = Date.now()): string {
  const L = t();
  if (now >= start && now < end) return L.happeningNow;
  if (now >= end) return '';
  const mins = Math.round((start - now) / MIN);
  if (mins < 1) return L.starting;
  if (mins < 60) return L.inMin(mins);
  if (mins < 24 * 60) return L.inHours(Math.floor(mins / 60), mins % 60);
  const days = Math.round((startOfDay(start).getTime() - startOfDay(now).getTime()) / DAY);
  if (days === 1) return L.tomorrowLower;
  if (days < 7) return L.inDays(days);
  return '';
}

export function minutesLabel(m: number): string {
  const L = t();
  if (m === 0) return L.atStart;
  if (m % 1440 === 0) return L.daysBefore(m / 1440);
  if (m % 60 === 0) return L.hoursBefore(m / 60);
  return L.minBefore(m);
}

export function dateLabel(d: Date | number, now = Date.now()): string {
  if (sameDay(d, now)) return t().today;
  if (sameDay(d, now + DAY)) return t().tomorrow;
  if (sameDay(d, now - DAY)) return t().yesterday;
  return `${weekdayShort(new Date(d))}, ${dayMonth(d)}`;
}

/** "Чт, 22 октября · 15:00–16:30" */
export function whenLabel(i: { start: number; end: number; allDay: boolean }): string {
  const s = new Date(i.start);
  if (i.allDay) {
    const lastDay = new Date(i.end - 1);
    return sameDay(s, lastDay)
      ? `${weekdayShort(s)}, ${dayMonth(s)} · ${t().allDay}`
      : `${shortDate(s)} – ${shortDate(lastDay)} · ${t().allDay}`;
  }
  if (sameDay(i.start, i.end - 1)) return `${weekdayShort(s)}, ${dayMonth(s)} · ${timeRange(i.start, i.end)}`;
  return `${shortDate(s)}, ${time(i.start)} – ${shortDate(i.end)}, ${time(i.end)}`;
}

export function timeToMinutes(v: string): number {
  const [h, m] = v.split(':').map(Number);
  return (h || 0) * 60 + (m || 0);
}

export function hhmm(d: Date | number): string {
  const x = new Date(d);
  return `${String(x.getHours()).padStart(2, '0')}:${String(x.getMinutes()).padStart(2, '0')}`;
}
