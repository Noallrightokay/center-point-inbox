/* The website's security headers.

   mailrata.org sells licences and signs people in; nothing on it should be
   framed by another site (a clickjacked "Delete account" is the classic), a
   browser should never be let back onto plain http once it has seen https,
   nothing should be sniffed into a type it was not sent as, and a full URL
   should not leak to other sites in a Referer. These are set once in
   next.config.js and checked here on a page and on an API route, and the
   app's cross-origin calls must still get their CORS answer alongside them. */
import { startServer, makeChecker } from './helpers.mjs';

const WANT = {
  'strict-transport-security': v => /max-age=31536000/.test(v) && /includeSubDomains/i.test(v) && !/preload/i.test(v),
  'x-frame-options': v => v === 'DENY',
  'content-security-policy': v => /frame-ancestors 'none'/.test(v),
  'x-content-type-options': v => v === 'nosniff',
  'referrer-policy': v => v === 'strict-origin-when-cross-origin',
};

export default async function run(state) {
  const check = makeChecker(state);
  const s = await startServer();
  try {
    for (const [path, init] of [
      ['/', {}],
      ['/app', {}],
      ['/api/config', {}],
      ['/config.js', {}],
      ['/api/stripe/webhook', {}],
    ]) {
      console.log(`\n— ${path} —`);
      const r = await fetch(s.url + path, { redirect: 'manual', ...init });
      for (const [name, ok] of Object.entries(WANT)) {
        const v = r.headers.get(name) || '';
        check(ok(v), `${path}: ${name}: ${v || '(missing)'}`);
      }
    }

    console.log('\n— the app still gets its CORS answer —');
    for (const path of ['/api/ai', '/api/licence/renew']) {
      const r = await fetch(s.url + path, { method: 'OPTIONS', headers: { Origin: 'tauri://localhost', 'Access-Control-Request-Method': 'POST', 'Access-Control-Request-Headers': 'content-type' } });
      check(r.status === 204 && r.headers.get('access-control-allow-origin') === 'tauri://localhost',
        `${path} preflight from tauri://localhost: ${r.status}, allowed ${r.headers.get('access-control-allow-origin')}`);
      check(WANT['x-content-type-options'](r.headers.get('x-content-type-options') || ''), `${path}: and carries the headers too`);
    }
  } finally { await s.stop(); }
}
