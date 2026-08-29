/* The IMAP link is the fallback for mail we cannot reach over OAuth: Apple
   publishes none for Mail, and Gmail's read scopes are restricted, so one-click
   Gmail needs Google verification plus an annual CASA assessment. These check
   the routes are wired, guarded, and reachable for both providers. */
import { startServer, makeChecker } from './helpers.mjs';

export default async function run(state) {
  const check = makeChecker(state);

  const s = await startServer({ env: {
    SUPABASE_URL: 'https://demo.supabase.co',
    SUPABASE_SERVICE_ROLE_KEY: 'test-only-not-a-real-key',
  }});

  try {
    const post = (path, body) => fetch(s.url + path, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body || {}),
    }).then(async r => ({ status: r.status, body: await r.json().catch(() => null) }));
    const get = path => fetch(s.url + path)
      .then(async r => ({ status: r.status, body: await r.json().catch(() => null) }));

    console.log('\n— both providers are routed —');
    for (const p of ['gmail', 'apple']) {
      const link = await post(`/api/link/imap/${p}`, { email: 'x@example.com', appPassword: 'abcdefghijklmnop' });
      check(link.body && typeof link.body.error === 'string',
        `/api/link/imap/${p} responds as JSON: ${JSON.stringify(link.body?.error)}`);
      const sync = await get(`/api/sync/imap/${p}`);
      check(sync.body && typeof sync.body.error === 'string',
        `/api/sync/imap/${p} responds as JSON: ${JSON.stringify(sync.body?.error)}`);
    }

    console.log('\n— an unknown provider is rejected, not attempted —');
    const bad = await post('/api/link/imap/hotmail', { email: 'x@example.com', appPassword: 'abcdefghijkl' });
    check(bad.status === 400 && /unknown mail provider/i.test(bad.body?.error || ''),
      `unknown provider -> ${bad.status} ${JSON.stringify(bad.body?.error)}`);

    console.log('\n— credentials are never accepted without a signed-in user —');
    const unauth = await post('/api/link/imap/gmail', { email: 'x@example.com', appPassword: 'abcdefghijklmnop' });
    check(/not signed in|sign in again/i.test(unauth.body?.error || ''),
      `anonymous link refused: ${JSON.stringify(unauth.body?.error)}`);
  } finally {
    await s.stop();
  }
}
