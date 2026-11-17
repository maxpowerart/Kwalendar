import { render } from 'solid-js/web';
import '@fontsource/fredoka/600.css';
import './styles.css';
import { App } from './App';
import { Today } from './components/Today';
import { store } from './store';
import { initTheme } from './theme';

initTheme();

if (import.meta.env.DEV) {
  (window as unknown as { __kw: typeof store }).__kw = store;
  // ?select=<title part> opens that event's panel once the week has loaded (README screenshots).
  const pick = new URLSearchParams(location.search).get('select');
  if (pick) {
    const timer = setInterval(() => {
      // The occurrence nearest to now, past or future.
      const hit = store
        .items()
        .filter((i) => i.title.includes(pick))
        .sort((a, b) => Math.abs(a.start - Date.now()) - Math.abs(b.start - Date.now()))[0];
      if (hit) {
        store.setSelected(hit);
        clearInterval(timer);
      }
    }, 100);
  }
}

// The tray popup is the same bundle in a second window: index.html?view=today.
const isToday = new URLSearchParams(location.search).get('view') === 'today';

render(() => (isToday ? <Today /> : <App />), document.getElementById('root')!);
