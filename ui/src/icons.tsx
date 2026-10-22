import type { JSX } from 'solid-js';

/** `node`: orange dot (cx, cy, r) — the "today / now" accent from the logo, as on GitKwaken icons. */
type P = { size?: number; stroke?: number; node?: [number, number, number] };

const svg = (paths: JSX.Element, { size = 16, stroke = 2, node }: P = {}) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width={stroke}
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    {paths}
    {node && <circle cx={node[0]} cy={node[1]} r={node[2]} fill="var(--node)" stroke="none" />}
  </svg>
);

export const I = {
  search: (p?: P) => svg(<><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></>, p),
  plus: (p?: P) => svg(<path d="M12 5v14M5 12h14" />, p),
  left: (p?: P) => svg(<path d="m15 18-6-6 6-6" />, p),
  right: (p?: P) => svg(<path d="m9 18 6-6-6-6" />, p),
  down: (p?: P) => svg(<path d="m6 9 6 6 6-6" />, p),
  close: (p?: P) => svg(<path d="M6 6l12 12M18 6 6 18" />, p),
  check: (p?: P) => svg(<path d="M5 12l5 5 9-10" />, p),
  sync: (p?: P) =>
    svg(<><path d="M21 12a9 9 0 0 1-15.5 6.2L3 16" /><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8" /><path d="M21 3v5h-5" /><path d="M3 21v-5h5" /></>, p),
  calendar: (p?: P) => svg(<><rect x="3" y="4" width="18" height="17" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" /></>, p),
  clock: (p?: P) => svg(<><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>, p),
  bell: (p?: P) => svg(<><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a1.9 1.9 0 0 0 3.4 0" /></>, p),
  bellOff: (p?: P) => svg(<><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="m2 2 20 20" /></>, p),
  users: (p?: P) =>
    svg(<><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" /><circle cx="9" cy="7" r="4" /><path d="M22 21v-2a4 4 0 0 0-3-3.9M16 3.1a4 4 0 0 1 0 7.8" /></>, p),
  video: (p?: P) => svg(<><path d="m22 8-6 4 6 4V8z" /><rect x="2" y="6" width="14" height="12" rx="2" /></>, p),
  pin: (p?: P) => svg(<><path d="M12 21s-7-6.2-7-12a7 7 0 0 1 14 0c0 5.8-7 12-7 12z" /><circle cx="12" cy="9" r="2.5" /></>, p),
  text: (p?: P) => svg(<path d="M4 6h16M4 12h16M4 18h10" />, p),
  edit: (p?: P) => svg(<><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></>, p),
  trash: (p?: P) => svg(<><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" /></>, p),
  external: (p?: P) => svg(<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />, p),
  settings: (p?: P) =>
    svg(<><circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" /></>, p),
  sidebar: (p?: P) => svg(<><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></>, p),
  repeat: (p?: P) => svg(<><path d="m17 2 4 4-4 4" /><path d="M3 11v-1a4 4 0 0 1 4-4h14" /><path d="m7 22-4-4 4-4" /><path d="M21 13v1a4 4 0 0 1-4 4H3" /></>, p),
  send: (p?: P) => svg(<path d="m22 2-7 20-4-9-9-4z" />, p),
  dots: (p?: P) => svg(<><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></>, { stroke: 3, ...p }),
};

export function WinMin() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0 5h10" stroke="currentColor" stroke-width="1" />
    </svg>
  );
}
export function WinMax() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
      <rect x="0.5" y="0.5" width="9" height="9" rx="1" stroke="currentColor" stroke-width="1" />
    </svg>
  );
}
export function WinClose() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" stroke="currentColor" stroke-width="1" />
    </svg>
  );
}
