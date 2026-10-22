import type { Instance } from './api';

export interface Placed {
  inst: Instance;
  /** minutes from day start */
  top: number;
  bottom: number;
  col: number;
  cols: number;
  /** how many columns this block may span to the right */
  span: number;
}

/**
 * Classic calendar packing: overlapping events form clusters; inside a cluster each event takes
 * the first free column, then stretches right over columns that stay empty for its whole span.
 */
export function layoutDay(events: Instance[], dayStart: number): Placed[] {
  const dayEnd = dayStart + 24 * 3600_000;
  const items = events
    .filter((e) => !e.allDay && e.end > dayStart && e.start < dayEnd)
    .map((inst) => {
      const top = Math.max(0, (inst.start - dayStart) / 60_000);
      const bottom = Math.min(24 * 60, (inst.end - dayStart) / 60_000);
      // Very short events still need room for one line of text.
      return { inst, top, bottom: Math.max(bottom, top + 20), col: 0, cols: 1, span: 1 };
    })
    .sort((a, b) => a.top - b.top || b.bottom - a.bottom);

  const out: Placed[] = [];
  let cluster: Placed[] = [];
  let columns: number[] = []; // bottom of last block per column
  let clusterEnd = -1;

  const flush = () => {
    const n = columns.length;
    for (const p of cluster) {
      p.cols = n;
      let span = 1;
      for (let c = p.col + 1; c < n; c++) {
        const blocked = cluster.some((q) => q.col === c && q.top < p.bottom && q.bottom > p.top);
        if (blocked) break;
        span++;
      }
      p.span = span;
      out.push(p);
    }
    cluster = [];
    columns = [];
  };

  for (const it of items) {
    if (it.top >= clusterEnd && cluster.length) flush();
    let col = columns.findIndex((b) => b <= it.top);
    if (col === -1) {
      col = columns.length;
      columns.push(it.bottom);
    } else {
      columns[col] = it.bottom;
    }
    it.col = col;
    cluster.push(it);
    clusterEnd = Math.max(clusterEnd, it.bottom);
  }
  if (cluster.length) flush();
  return out;
}

export interface AllDayPlaced {
  inst: Instance;
  startCol: number;
  span: number;
  row: number;
}

/** Packs all-day / multi-day items into rows across a strip of `days` columns starting at `firstDay`. */
export function layoutAllDay(events: Instance[], firstDay: number, days: number): AllDayPlaced[] {
  const dayIndex = (ms: number) => Math.round((startOfDayMs(ms) - firstDay) / 86_400_000);
  const items = events
    .filter((e) => e.allDay || e.end - e.start >= 24 * 3600_000)
    .map((inst) => {
      const s = Math.max(0, dayIndex(inst.start));
      const e = Math.min(days - 1, dayIndex(inst.end - 1));
      return { inst, startCol: s, span: e - s + 1, row: 0 };
    })
    .filter((p) => p.span > 0 && p.startCol < days)
    .sort((a, b) => a.startCol - b.startCol || b.span - a.span);
  const rows: number[] = [];
  for (const p of items) {
    let r = rows.findIndex((end) => end < p.startCol);
    if (r === -1) {
      r = rows.length;
      rows.push(p.startCol + p.span - 1);
    } else rows[r] = p.startCol + p.span - 1;
    p.row = r;
  }
  return items;
}

function startOfDayMs(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Tinted fill + readable text for an event colour. Pure CSS (color-mix), so it follows the theme. */
export function tint(hex: string): { bg: string; fg: string; solid: string } {
  return {
    bg: `color-mix(in srgb, ${hex} var(--ev-bg-pct), transparent)`,
    fg: `color-mix(in srgb, ${hex} var(--ev-fg-pct), var(--ev-base))`,
    solid: hex,
  };
}

/** Dark or light text for a solid colour fill. */
export function onSolid(hex: string): string {
  const h = hex.replace('#', '');
  const n = parseInt(h, 16);
  const r = ((n >> 16) & 255) / 255;
  const g = ((n >> 8) & 255) / 255;
  const b = (n & 255) / 255;
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return lum > 0.38 ? '#0F2240' : '#FFFFFF';
}
