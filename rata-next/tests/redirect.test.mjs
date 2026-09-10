/* REDIRECT_TO retires a domain: mailrata.com should send everything to
   mailrata.org rather than serving a second copy of the app. */
import { startServer, makeChecker } from './helpers.mjs';

export default async function run(state) {
  const check = makeChecker(state);

  /* fetch will not let us forge a Host header, so the request arrives as
     127.0.0.1 — a host that is not the destination, which is exactly the
     condition the retired domain meets. */
  console.log('\n— with REDIRECT_TO set, the whole site moves —');
  {
    const s = await startServer({ env: { REDIRECT_TO: 'https://mailrata.org' } });
    try {
      const hit = path => fetch(s.url + path, { redirect: 'manual' })
        .then(r => ({ status: r.status, loc: r.headers.get('location') || '' }));

      for (const [path, want] of [
        ['/', 'https://mailrata.org/'],
        ['/app', 'https://mailrata.org/app'],
        ['/api/sync/imap/gmail', 'https://mailrata.org/api/sync/imap/gmail'],
      ]) {
        const r = await hit(path);
        check(r.status === 308 && r.loc === want, `${path} -> ${r.status} ${r.loc}`);
      }

      const q = await fetch(s.url + '/app?tab=mail&x=1', { redirect: 'manual' });
      check((q.headers.get('location') || '').includes('tab=mail&x=1'), 'query string is preserved');

    } finally { await s.stop(); }
  }

  /* fetch refuses to set a Host header, so the only way to exercise the
     loop guard is to hand the server its own origin as REDIRECT_TO — the
     same host === target.host condition production hits on the destination. */
  console.log('\n— a request already on the destination must not loop —');
  {
    const s = await startServer({ env: url => ({ REDIRECT_TO: url }) });
    try {
      const r = await fetch(s.url + '/', { redirect: 'manual' });
      check(r.status !== 308, `already on the destination -> ${r.status}, no loop`);
      const api = await fetch(s.url + '/api/sync/imap/gmail', { redirect: 'manual' });
      check(api.status !== 308, `/api on the destination -> ${api.status}, no loop`);
    } finally { await s.stop(); }
  }

  console.log('\n— a malformed REDIRECT_TO is ignored, not fatal —');
  {
    const s = await startServer({ env: { REDIRECT_TO: 'not a url' } });
    try {
      const r = await fetch(s.url + '/', { redirect: 'manual' });
      check(r.status === 200, `/ still serves -> ${r.status}`);
    } finally { await s.stop(); }
  }

  console.log('\n— without REDIRECT_TO, the app serves normally —');
  {
    const s = await startServer();
    try {
      const r = await fetch(s.url + '/', { redirect: 'manual' });
      check(r.status === 200, `/ -> ${r.status}`);
      const api = await fetch(s.url + '/api/sync/imap/gmail');
      const body = await api.json().catch(() => null);
      check(!!body && typeof body.error === 'string', `/api still answers as JSON: ${JSON.stringify(body?.error)}`);
    } finally { await s.stop(); }
  }
}
