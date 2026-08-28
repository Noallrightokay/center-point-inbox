/* The provider-link flow binds its one-time state to the browser with an
   HttpOnly cookie. Without that binding, anyone who obtained a state value
   could finish consent with THEIR OWN Microsoft/Slack account and have it
   attached to the victim's workspace. These tests pin that behaviour. */
import { startServer, makeChecker } from './helpers.mjs';

const STATE = '11111111-1111-1111-1111-111111111111';

export default async function run(state) {
  const check = makeChecker(state);

  /* APP_URL must match the server's own origin, so derive it from the
     OS-assigned port rather than guessing. */
  const s = await startServer({ env: url => ({
    APP_URL: url,
    SUPABASE_URL: 'https://demo.supabase.co',
    SUPABASE_SERVICE_ROLE_KEY: 'test-only-not-a-real-key',
    MS_CLIENT_ID: 'test', MS_CLIENT_SECRET: 'test',
  })});

  try {
    const hit = (path, cookie) =>
      fetch(s.url + path, { redirect: 'manual', headers: cookie ? { cookie } : {} })
        .then(r => ({ status: r.status, location: r.headers.get('location') || '' }));

    console.log('\n— API routes must be server-rendered, not statically served —');
    const sync = await fetch(s.url + '/api/sync/ms');
    const syncBody = await sync.text();
    check(syncBody.trim().startsWith('{'), `/api/sync/ms returns JSON, not HTML: ${syncBody.slice(0, 40)}`);

    console.log('\n— a state value without its cookie is rejected —');
    const noCookie = await hit(`/api/link/ms/start?state=${STATE}`);
    check(/did not match this browser/.test(decodeURIComponent(noCookie.location)),
      `/start rejects: ${decodeURIComponent(noCookie.location).split('linkerr=')[1] || noCookie.location}`);

    const cbNoCookie = await hit(`/api/link/ms/callback?state=${STATE}&code=abc`);
    check(/did not match this browser/.test(decodeURIComponent(cbNoCookie.location)),
      '/callback rejects before touching the state row');

    console.log('\n— a mismatched cookie is rejected —');
    const wrong = await hit(`/api/link/ms/start?state=${STATE}`, 'rata_link_state=some-other-value');
    check(/did not match this browser/.test(decodeURIComponent(wrong.location)), '/start rejects a non-matching cookie');

    console.log('\n— the matching cookie passes the binding check —');
    const right = await hit(`/api/link/ms/start?state=${STATE}`, `rata_link_state=${STATE}`);
    const msg = decodeURIComponent(right.location);
    check(!/did not match this browser/.test(msg),
      `binding passed, flow proceeded to the state lookup: ${msg.split('linkerr=')[1] || msg}`);
  } finally {
    await s.stop();
  }
}
