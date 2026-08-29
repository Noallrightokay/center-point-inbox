/* /config.js emits the PUBLIC site configuration from environment variables.
   The stakes are asymmetric: publishing a Supabase service_role key by mistake
   would hand every visitor read/write over every user's data, including the
   stored mail credentials in provider_tokens. These tests exist mainly to keep
   that from ever regressing silently. */
import { startServer, makeChecker, fakeSupabaseKey } from './helpers.mjs';
import { writeFile, rm } from 'node:fs/promises';

export default async function run(state) {
  const check = makeChecker(state);
  const load = async url => {
    const body = await (await fetch(url + '/config.js')).text();
    const scope = {};
    new Function('window', body)(scope);
    return { body, cfg: scope.RATA_CONFIG };
  };

  console.log('\n— unset: must still serve, so an unconfigured deploy runs on device accounts —');
  {
    const s = await startServer();
    try {
      const res = await fetch(s.url + '/config.js');
      const { cfg } = await load(s.url);
      check(res.headers.get('content-type')?.includes('application/javascript'), `content-type: ${res.headers.get('content-type')}`);
      check(/no-store/.test(res.headers.get('cache-control') || ''), `cache-control: ${res.headers.get('cache-control')}`);
      check(cfg && Object.values(cfg).every(v => v === ''), 'every value empty — cloud mode stays off');
    } finally { await s.stop(); }
  }

  console.log('\n— configured: values reach the browser, and hostile characters cannot break out —');
  {
    const anon = fakeSupabaseKey('anon');
    const s = await startServer({ env: {
      SUPABASE_URL: 'https://demo.supabase.co',
      SUPABASE_ANON_KEY: anon,
      STRIPE_MONTHLY: 'https://buy.stripe.com/x";alert(1);//',
    }});
    try {
      const { cfg } = await load(s.url);
      check(cfg.supabaseUrl === 'https://demo.supabase.co', `supabaseUrl: ${cfg.supabaseUrl}`);
      check(cfg.supabaseKey === anon, 'anon key published as-is');
      check(cfg.stripeMonthly === 'https://buy.stripe.com/x";alert(1);//', 'quote/semicolon payload survived escaping intact, still parsed as one string');
    } finally { await s.stop(); }
  }

  /* Archive deploys are additive: a public/config.js from an earlier deployment
     keeps being served after it is deleted from source, and Next serves public/
     files for paths it owns. The middleware rewrite is what stops that stale
     file from silently reverting the site to a hand-edited config. */
  console.log('\n— a stale public/config.js must NOT shadow the route —');
  {
    const decoy = 'public/config.js';
    await writeFile(decoy, 'window.RATA_CONFIG = { supabaseUrl: "STALE-LEFTOVER" };\n');
    try {
      const s = await startServer({ env: { SUPABASE_URL: 'https://fresh.supabase.co' } });
      try {
        const { cfg } = await load(s.url);
        check(cfg.supabaseUrl === 'https://fresh.supabase.co',
          `route won over the leftover file (got ${JSON.stringify(cfg.supabaseUrl)})`);
        check(cfg.supabaseUrl !== 'STALE-LEFTOVER', 'the stale file was not served');
      } finally { await s.stop(); }
    } finally { await rm(decoy, { force: true }); }
  }

  console.log('\n— service_role key mispasted into the public variable: MUST be refused —');
  for (const [label, key] of [
    ['legacy JWT service_role', fakeSupabaseKey('service_role')],
    ['current sb_secret_ key', 'sb_secret_abcdefghijklmnop'],
  ]) {
    const s = await startServer({ env: {
      SUPABASE_URL: 'https://demo.supabase.co',
      SUPABASE_ANON_KEY: key,
    }});
    try {
      const { body, cfg } = await load(s.url);
      check(cfg.supabaseKey === '', `${label}: not published`);
      check(!body.includes(key), `${label}: the key appears nowhere in the response body`);
      check(/service role/i.test(body), `${label}: the reason is logged to the console`);
    } finally { await s.stop(); }
  }
}
