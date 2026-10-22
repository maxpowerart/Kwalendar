import { render } from 'solid-js/web';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import './styles.css';
import { App } from './App';
import { store } from './store';

if (import.meta.env.DEV) (window as unknown as { __kw: typeof store }).__kw = store;

render(() => <App />, document.getElementById('root')!);
