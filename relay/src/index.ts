// Kwalendar push relay (Cloudflare Worker + Durable Object).
//
// Google Calendar push needs a public HTTPS webhook; a desktop app has none. This relay receives
// Google's notifications and wakes the app, which is long-polling it. It stores nothing but a
// counter per channel key — no event data ever passes through it.
//
//   POST /google/<key>   ← Google (events.watch address); verified by X-Goog-Channel-Token == key
//   GET  /wait/<key>?since=N  ← Kwalendar; answers when the counter differs from N (or after ~50 s)

export interface Env {
  CHANNELS: DurableObjectNamespace;
}

const KEY = /^\/(google|wait)\/([a-f0-9]{32})$/;

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    const m = url.pathname.match(KEY);
    if (!m) return new Response('not found', { status: 404 });
    const [, kind, key] = m;
    const stub = env.CHANNELS.get(env.CHANNELS.idFromName(key));

    if (kind === 'google') {
      if (req.method !== 'POST') return new Response('method not allowed', { status: 405 });
      if (req.headers.get('X-Goog-Channel-Token') !== key) return new Response('forbidden', { status: 403 });
      // "sync" is Google's handshake right after watch(); bumping on it is harmless.
      await stub.fetch('https://channel/bump', { method: 'POST' });
      return new Response(null, { status: 200 });
    }
    if (req.method !== 'GET') return new Response('method not allowed', { status: 405 });
    return stub.fetch(`https://channel/wait${url.search}`);
  },
};

export class Channel {
  private seq = 0;
  private waiters: Array<(n: number) => void> = [];

  constructor(private state: DurableObjectState) {
    state.blockConcurrencyWhile(async () => {
      this.seq = (await state.storage.get<number>('seq')) ?? 0;
    });
  }

  async fetch(req: Request): Promise<Response> {
    const url = new URL(req.url);
    if (url.pathname === '/bump') {
      this.seq++;
      await this.state.storage.put('seq', this.seq);
      for (const w of this.waiters.splice(0)) w(this.seq);
      return new Response('ok');
    }
    const since = Number(url.searchParams.get('since') ?? '0');
    if (since !== this.seq) return json(this.seq);
    const seq = await new Promise<number>((resolve) => {
      this.waiters.push(resolve);
      setTimeout(() => resolve(this.seq), 50_000);
    });
    return json(seq);
  }
}

function json(seq: number): Response {
  return new Response(JSON.stringify({ seq }), {
    headers: { 'content-type': 'application/json', 'cache-control': 'no-store' },
  });
}
