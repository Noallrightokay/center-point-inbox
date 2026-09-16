/* The health endpoint.

   What is worth testing here is not that it answers — it is what it answers
   with, and to whom. A monitor must be able to tell "the app is up" from "the
   app is up but cannot do its job", and a stranger must not be able to read
   off which piece of the configuration is missing. */
import { startServer, makeChecker } from './helpers.mjs';

export default async function run(state) {
  const check = makeChecker(state);

  console.log('\n— an unconfigured deployment reports itself, and does not hide it —');
  {
    const s = await startServer();
    try {
      const r = await fetch(s.url + '/api/health');
      const b = await r.json();

      /* No Supabase configured, so the app cannot serve: 503 is the honest
         answer and it is what a monitor needs to see. */
      check(r.status === 503, `no database -> ${r.status}`);
      check(b.ok === false && b.ready === false, `ok:${b.ok} ready:${b.ready}`);
      check(b.service === 'rata' && !!b.time, 'it says what it is and when it answered');
      check(r.headers.get('cache-control')?.includes('no-store'),
        'and is never cached — a cached health check is a lie waiting to be told');
    } finally { await s.stop(); }
  }

  console.log('\n— up and ready are different questions —');
  {
    /* The failure this endpoint exists for: everything serves, the site loads,
       and nobody can be issued a licence because a variable went missing — so
       every copy of the app stops working within thirty days. */
    const s = await startServer({ env: { SUPABASE_URL: 'https://db.example.com', SUPABASE_SERVICE_ROLE_KEY: 'x' } });
    try {
      const b = await (await fetch(s.url + '/api/health')).json();
      check(b.ready === false, 'with no signing key it is not ready');
      check(Array.isArray(b.failing) && b.failing.includes('licensing'),
        `and names which piece: ${JSON.stringify(b.failing)}`);
    } finally { await s.stop(); }
  }

  console.log('\n— a stranger learns that something is wrong, not what —');
  {
    const s = await startServer({ env: { HEALTH_TOKEN: 'correct-horse-battery-staple' } });
    try {
      const open = await (await fetch(s.url + '/api/health')).json();
      check(open.checks === undefined, 'no detail without the token');
      check(!JSON.stringify(open).includes('LICENCE_PRIVATE_KEY'),
        'and no environment variable is named in the public body');
      check(Array.isArray(open.failing) && open.failing.length > 0,
        `only the names of what is failing: ${JSON.stringify(open.failing)}`);

      const wrong = await (await fetch(s.url + '/api/health?token=nearly-right')).json();
      check(wrong.checks === undefined, 'a wrong token is no token');

      const ok = await (await fetch(s.url + '/api/health?token=correct-horse-battery-staple')).json();
      check(!!ok.checks && !!ok.checks.database, 'the right one gets the detail');
      check(/LICENCE_PRIVATE_KEY/.test(ok.checks.licensing.detail || ''),
        `including the variable to go and set: "${ok.checks.licensing.detail}"`);
      check(typeof ok.tookMs === 'number', 'and how long the checks took');

      const hdr = await (await fetch(s.url + '/api/health', {
        headers: { authorization: 'Bearer correct-horse-battery-staple' } })).json();
      check(!!hdr.checks, 'the token also travels as a bearer header, for monitors that prefer it');
    } finally { await s.stop(); }
  }

  console.log('\n— with no token configured, nobody gets the detail —');
  {
    /* The dangerous default would be treating "no token set" as "no token
       needed". */
    const s = await startServer();
    try {
      const b = await (await fetch(s.url + '/api/health?token=')).json();
      check(b.checks === undefined, 'an unset HEALTH_TOKEN does not open the endpoint up');
    } finally { await s.stop(); }
  }
}
