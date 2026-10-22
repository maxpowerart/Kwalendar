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

const fmtTime = new Intl.DateTimeFormat('ru-RU', { hour: '2-digit', minute: '2-digit' });
const fmtMonthYear = new Intl.DateTimeFormat('ru-RU', { month: 'long', year: 'numeric' });
const fmtDayMonth = new Intl.DateTimeFormat('ru-RU', { day: 'numeric', month: 'long' });
const fmtWeekdayShort = new Intl.DateTimeFormat('ru-RU', { weekday: 'short' });
const fmtWeekdayLong = new Intl.DateTimeFormat('ru-RU', { weekday: 'long' });
const fmtShortDate = new Intl.DateTimeFormat('ru-RU', { day: 'numeric', month: 'short' });

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export const time = (ms: number) => fmtTime.format(ms);
export const monthYear = (d: Date) => cap(fmtMonthYear.format(d).replace(' г.', ''));
export const dayMonth = (d: Date | number) => fmtDayMonth.format(d);
export const weekdayShort = (d: Date) => cap(fmtWeekdayShort.format(d));
export const weekdayLong = (d: Date | number) => cap(fmtWeekdayLong.format(d));
export const shortDate = (d: Date | number) => fmtShortDate.format(d).replace('.', '');

export const WEEKDAYS = ['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'];

export function timeRange(start: number, end: number): string {
  return `${time(start)}–${time(end)}`;
}

/** "через 1 ч 13 мин" / "идёт сейчас" / "закончилось" */
export function relative(start: number, end: number, now = Date.now()): string {
  if (now >= start && now < end) return 'идёт сейчас';
  if (now >= end) return '';
  const mins = Math.round((start - now) / MIN);
  if (mins < 1) return 'начинается';
  if (mins < 60) return `через ${mins} мин`;
  if (mins < 24 * 60) {
    const h = Math.floor(mins / 60);
    const m = mins % 60;
    return m ? `через ${h} ч ${m} мин` : `через ${h} ч`;
  }
  const days = Math.round((startOfDay(start).getTime() - startOfDay(now).getTime()) / DAY);
  if (days === 1) return 'завтра';
  if (days < 7) return `через ${days} ${plural(days, 'день', 'дня', 'дней')}`;
  return '';
}

export function plural(n: number, one: string, few: string, many: string): string {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few;
  return many;
}

export function minutesLabel(m: number): string {
  if (m === 0) return 'в момент начала';
  if (m % 1440 === 0) return `за ${m / 1440} ${plural(m / 1440, 'день', 'дня', 'дней')}`;
  if (m % 60 === 0) return `за ${m / 60} ч`;
  return `за ${m} мин`;
}

export function dateLabel(d: Date | number, now = Date.now()): string {
  if (sameDay(d, now)) return 'Сегодня';
  if (sameDay(d, now + DAY)) return 'Завтра';
  if (sameDay(d, now - DAY)) return 'Вчера';
  return `${weekdayShort(new Date(d))}, ${dayMonth(d)}`;
}

/** "Чт, 22 октября · 15:00–16:30" */
export function whenLabel(i: { start: number; end: number; allDay: boolean }): string {
  const s = new Date(i.start);
  if (i.allDay) {
    const lastDay = new Date(i.end - 1);
    return sameDay(s, lastDay)
      ? `${weekdayShort(s)}, ${dayMonth(s)} · весь день`
      : `${shortDate(s)} – ${shortDate(lastDay)} · весь день`;
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
