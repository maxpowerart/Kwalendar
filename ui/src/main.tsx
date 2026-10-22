import { render } from 'solid-js/web';
import '@fontsource/fredoka/600.css';
import './styles.css';
import { App } from './App';
import { store } from './store';
import { initTheme } from './theme';

initTheme();

if (import.meta.env.DEV) {
  (window as unknown as { __kw: typeof store }).__kw = store;
  // ?select=<title part> opens that event's panel once the week has loaded (README screenshots).
  const pick = new URLSearchParams(location.search).get('select');
  if (pick) {
    const timer = setInterval(() => {
      const hit = store.items().find((i) => i.title.includes(pick) && i.start > Date.now() - 86_400_000);
      if (hit) {
        store.setSelected(hit);
        clearInterval(timer);
      }
    }, 100);
  }
}

render(() => <App />, document.getElementById('root')!);
