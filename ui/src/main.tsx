import { render } from 'solid-js/web';
import '@fontsource/fredoka/600.css';
import './styles.css';
import { App } from './App';
import { store } from './store';
import { initTheme } from './theme';

initTheme();

if (import.meta.env.DEV) (window as unknown as { __kw: typeof store }).__kw = store;

render(() => <App />, document.getElementById('root')!);
