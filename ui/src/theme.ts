export type ThemePref = 'system' | 'dark' | 'light';

const KEY = 'kw.theme';
const media = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
let pref: ThemePref = 'system';

function apply() {
  const dark = pref === 'dark' || (pref === 'system' && (media?.matches ?? true));
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
}

/** Applies a theme preference; remembered locally so the next start paints right away. */
export function setTheme(p: ThemePref) {
  pref = p;
  try {
    localStorage.setItem(KEY, p);
  } catch {
    /* storage unavailable */
  }
  apply();
}

/** Called before the first render, from the last known preference. */
export function initTheme() {
  try {
    const saved = localStorage.getItem(KEY) as ThemePref | null;
    if (saved === 'dark' || saved === 'light' || saved === 'system') pref = saved;
  } catch {
    /* storage unavailable */
  }
  apply();
  media?.addEventListener('change', apply);
}
