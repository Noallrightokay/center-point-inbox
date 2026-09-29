/* Deleting an account.

   The parts that can be checked without a database are the ones that decide
   whether deletion happens at all: the confirmation, and the refusal to delete
   while Stripe is still charging. The deletes themselves are four statements
   against tables that cascade anyway; what is worth testing is that nothing
   gets past the gate — and, against a stand-in Supabase, that every table
   keyed to the person is emptied, ai_usage included, even on a deploy that
   has not created it yet. */
import { createServer } from 'node:http';
import { startServer, makeChecker, fakeSupabaseKey } from './helpers.mjs';
import { blocksDeletion, DELETION_REMOVES, DELETION_KEEPS } from '../lib/account.js';

export default async function run(state) {
  const check = makeChecker(state);

  console.log('\n— a live subscription stops the deletion —');
  {
    check(blocksDeletion(null) === null, 'no subscription: nothing in the way');
    check(blocksDeletion({ status: 'canceled', plan: 'pro' }) === null, 'a cancelled one: nothing in the way');
    check(blocksDeletion({ status: 'incomplete_expired' }) === null, 'an expired attempt: nothing in the way');

    for (const status of ['active', 'trialing', 'past_due']) {
      const why = blocksDeletion({ status, plan: 'pro' });
      check(!!why && /billing portal/i.test(why),
        `${status}: refused, and points at the portal`);
    }

    /* The reason this refuses rather than deleting and warning: RATA sells
       through Payment Links and holds no Stripe secret key, so it cannot
       cancel on somebody's behalf. Deleting anyway would leave them paying for
       an account that no longer exists. */
    const why = blocksDeletion({ status: 'active', plan: 'pro' });
    check(/keeps charging/i.test(why), `and says what would otherwise happen: "${why}"`);
  }

  console.log('\n— a payment still clearing stops it too (review part 3) —');
  {
    /* A bank transfer still clearing: the row is `incomplete`. Deleted now,
       the transfer clearing would write the row back as active (checkout
       events are keyed by address) and Stripe would bill an address with no
       login. */
    const why = blocksDeletion({ status: 'incomplete', plan: 'pro' });
    check(!!why && /bank transfer clears/i.test(why) && /keeps charging/i.test(why),
      `incomplete: refused, and says why: "${why}"`);
    check(!!blocksDeletion({ status: 'INCOMPLETE' }), 'in any case');
  }

  console.log('\n— nor can deletion reset the AI allowance while a licence still works (P3-6) —');
  {
    /* A licence is issued only while the subscription is live and lasts 30
       days, and the AI relay's only credential is that licence. Deleting the
       account removes the month's AI usage, so deleting straight after the
       subscription ended let the licence spend the month's cap a second
       time. So an ended subscription blocks deletion until every licence it
       could have issued has run out: 30 days after the row last changed. */
    const now = Date.parse('2026-09-29T12:00:00Z');
    const daysAgo = (n) => new Date(now - n * 86400000).toISOString();
    const recent = blocksDeletion({ status: 'canceled', plan: 'pro', event_at: daysAgo(5), updated_at: daysAgo(5) }, now);
    check(!!recent && /2026-10-24/.test(recent) && /licen[cs]e/i.test(recent),
      `ended 5 days ago: refused until its last licence runs out, with the date: "${recent}"`);
    check(!!blocksDeletion({ status: 'unpaid', updated_at: daysAgo(29) }, now), 'unpaid 29 days ago: still refused');
    check(!!blocksDeletion({ status: 'canceled', event_at: daysAgo(40), updated_at: daysAgo(2) }, now),
      'the later of when Stripe said it and when it was written counts (a late delivery)');
    check(blocksDeletion({ status: 'canceled', updated_at: daysAgo(31) }, now) === null,
      'ended 31 days ago: every licence it issued has run out, nothing in the way');
    check(blocksDeletion({ status: 'incomplete_expired', updated_at: daysAgo(1) }, now) === null,
      'a first payment that never cleared issued no licence: nothing in the way');
    check(blocksDeletion({ status: 'canceled', updated_at: 'not a date' }, now) === null,
      'a row with no readable time is left to the rules above, as before');
  }

  console.log('\n— the warning is generated, not written twice —');
  {
    check(DELETION_REMOVES.some(r => /login/i.test(r)) && DELETION_REMOVES.some(r => /subscription/i.test(r)),
      `removes: ${DELETION_REMOVES.length} things, including the subscription and the login`);
    /* The route deletes the person's ai_usage rows too, so the notice has to
       say so: a deletion that takes more than it lists is not honest either. */
    check(DELETION_REMOVES.some(r => /\bAI usage\b/i.test(r)),
      'and the AI usage record, which the route deletes as well');
    /* And no longer claims to remove a mailbox password, because there is not
       one here to remove — that is the whole point of the move to the device,
       and a deletion notice that overstates itself is worse than none. */
    check(!DELETION_REMOVES.some(r => /mailbox|password|credential/i.test(r)),
      'and claims nothing about mailbox passwords, which RATA no longer holds');
    check(DELETION_KEEPS.some(k => /keychain/i.test(k)),
      'while saying where they actually are');
    check(DELETION_KEEPS.some(k => /your provider/i.test(k)),
      'and is honest that the mail is not RATA\'s to delete');
    check(DELETION_KEEPS.some(k => /Stripe/.test(k)),
      "nor Stripe's billing records");
  }

  console.log('\n— the endpoint itself —');
  {
    const s = await startServer();
    try {
      const anon = await fetch(s.url + '/api/account', { method: 'DELETE' });
      const body = await anon.json();
      check(!!body.error, `no session, no deletion: ${JSON.stringify(body.error)}`);

      const look = await (await fetch(s.url + '/api/account')).json();
      check(!!look.error, 'and the summary of what would go is behind the same check');

      /* Every other verb is absent by design: there is no way to delete an
         account with a link somebody could be tricked into clicking. */
      const get = await fetch(s.url + '/api/account', { method: 'POST' });
      check(get.status === 405, `POST to the account endpoint: ${get.status}`);
    } finally { await s.stop(); }
  }

  await everythingGoes(check);
}

function listen(handler) {
  return new Promise((res) => {
    const srv = createServer(handler);
    srv.listen(0, '127.0.0.1', () => res({ srv, url: `http://127.0.0.1:${srv.address().port}` }));
  });
}

/* A stand-in for the three Supabase services the route talks to — GoTrue for
   the session and the login, PostgREST for the tables — which records every
   request so the test can see what was deleted. `aiUsage` decides how the
   ai_usage table answers: 'rows' (it exists and holds two months), 'missing'
   (database.sql §5 never ran: PostgREST's PGRST205, a 404), 'old-missing'
   (Postgres's 42P01 from an older PostgREST) or 'broken' (any other failure). */
async function everythingGoes(check) {
  const USER = { id: '0b5c1f7e-8a8b-4b8e-9c55-3f1d2a6b7c80', email: 'Leaver@Example.com', aud: 'authenticated' };
  const fake = { aiUsage: 'rows', sub: null, calls: [] };
  const supabase = await listen((req, res) => {
    const url = new URL(req.url, 'http://x');
    fake.calls.push(`${req.method} ${url.pathname}${url.search}`);
    const json = (status, body) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(body)); };
    req.resume();
    req.on('end', () => {
      if (req.method === 'GET' && url.pathname === '/auth/v1/user') return json(200, USER);
      if (req.method === 'DELETE' && url.pathname === `/auth/v1/admin/users/${USER.id}`) return json(200, USER);
      if (req.method === 'GET' && url.pathname === '/rest/v1/subscriptions') return json(200, fake.sub ? [fake.sub] : []);
      if (req.method === 'DELETE' && url.pathname === '/rest/v1/workspaces') return json(200, [{ id: USER.id }]);
      if (req.method === 'DELETE' && url.pathname === '/rest/v1/subscriptions') return json(200, []);
      if (req.method === 'DELETE' && url.pathname === '/rest/v1/ai_usage') {
        if (fake.aiUsage === 'missing') return json(404, { code: 'PGRST205', details: null, hint: null, message: "Could not find the table 'public.ai_usage' in the schema cache" });
        if (fake.aiUsage === 'old-missing') return json(404, { code: '42P01', details: null, hint: null, message: 'relation "public.ai_usage" does not exist' });
        if (fake.aiUsage === 'broken') return json(500, { code: 'XX000', details: null, hint: null, message: 'disk on fire' });
        return json(200, [{ email: 'leaver@example.com' }, { email: 'leaver@example.com' }]);
      }
      json(404, { message: 'not in the stand-in' });
    });
  });

  const s = await startServer({ env: {
    SUPABASE_URL: supabase.url,
    SUPABASE_SERVICE_ROLE_KEY: fakeSupabaseKey('service_role'),
  } });
  const del = async () => {
    fake.calls = [];
    const r = await fetch(s.url + '/api/account', {
      method: 'DELETE',
      headers: { Authorization: 'Bearer a-session', 'Content-Type': 'application/json' },
      body: JSON.stringify({ confirm: 'leaver@example.com' }),
    });
    return { status: r.status, d: await r.json(), calls: fake.calls };
  };
  const loginGone = calls => calls.includes(`DELETE /auth/v1/admin/users/${USER.id}`);

  try {
    console.log('\n— every table keyed to the person is emptied, AI usage included —');
    {
      fake.aiUsage = 'rows';
      const { status, d, calls } = await del();
      check(status === 200 && d.ok === true, `deleted: ${status} ${JSON.stringify(d.deleted)}`);
      check(calls.some(c => c.startsWith('DELETE /rest/v1/ai_usage?') && c.includes('email=eq.leaver%40example.com')),
        'ai_usage rows are deleted, by the lower-cased address the relay charges under');
      check(d.deleted?.ai_usage === 2 && d.deleted?.workspaces === 1 && d.deleted?.subscriptions === 0 && d.deleted?.login === 1,
        'and the reply counts them with the rest');
      check(calls.indexOf(calls.find(c => c.startsWith('DELETE /rest/v1/ai_usage'))) < calls.findIndex(c => c.startsWith('DELETE /auth/v1/admin/users/')),
        'before the login goes, so a failure there leaves an account that can try again');
    }

    for (const [mode, what] of [['missing', 'PGRST205'], ['old-missing', '42P01']]) {
      console.log(`\n— no ai_usage table yet (${what}): the deletion still finishes —`);
      fake.aiUsage = mode;
      const { status, d, calls } = await del();
      check(status === 200 && d.ok === true, `a deploy without database.sql §5 still deletes: ${status}`);
      check(d.deleted?.ai_usage === 0, `and says nothing was there to delete: ai_usage ${d.deleted?.ai_usage}`);
      check(loginGone(calls), 'the login is deleted too');
      check(!/schema cache|does not exist|PGRST|42P01/.test(JSON.stringify(d)),
        "and the database's own words about the missing table do not reach the reply");
    }

    console.log('\n— the endpoint refuses while a payment clears or a licence still works —');
    {
      fake.aiUsage = 'rows';
      for (const [what, sub] of [
        ['a payment still clearing', { plan: 'pro', status: 'incomplete', event_at: null, updated_at: new Date().toISOString() }],
        ['a subscription ended yesterday', { plan: 'pro', status: 'canceled', event_at: new Date(Date.now() - 86400000).toISOString(),
          updated_at: new Date(Date.now() - 86400000).toISOString() }],
      ]) {
        fake.sub = sub;
        const { status, d, calls } = await del();
        check(status === 409 && typeof d.error === 'string' && d.error.length > 0, `${what}: refused, ${status} "${d.error}"`);
        check(!calls.some(c => c.startsWith('DELETE ')), `${what}: and nothing is deleted, ai_usage included`);
        check(calls.some(c => c.startsWith('GET /rest/v1/subscriptions') && /event_at/.test(c) && /updated_at/.test(c)),
          `${what}: the route reads when the row last changed`);
      }
      fake.sub = { plan: 'pro', status: 'canceled', event_at: new Date(Date.now() - 40 * 86400000).toISOString(),
        updated_at: new Date(Date.now() - 40 * 86400000).toISOString() };
      const { status, d } = await del();
      check(status === 200 && d.ok === true, `ended 40 days ago: deleted, ${status}`);
      fake.sub = null;
    }

    console.log('\n— any other failure on ai_usage stops before the login, as for the other tables —');
    {
      fake.aiUsage = 'broken';
      const { status, d, calls } = await del();
      check(status === 500 && /^Could not finish deleting the account/.test(d.error || ''), `refused: ${status} ${JSON.stringify(d.error)}`);
      check(!loginGone(calls), 'and the login is kept, so the customer can try again');
      check(d.deletedSoFar?.workspaces === 1 && !('ai_usage' in (d.deletedSoFar || {})), `and it says what went: ${JSON.stringify(d.deletedSoFar)}`);
    }
  } finally {
    await s.stop();
    supabase.srv.close();
  }
}
