/* The Stripe webhook.

   This endpoint is what stands between "money arrived" and "the app knows it",
   and it is reachable by anyone on the internet who finds the URL. So the
   signature check gets the attention: a forged POST must not be able to grant
   somebody a plan, and a captured one must not be replayable later.

   Stripe is not called here. The events are built by hand in exactly the shape
   Stripe sends, which is also what lets the replay and tolerance cases be
   tested at all — they depend on controlling the clock. */
import { createHmac } from 'node:crypto';
import { createServer } from 'node:http';
import { startServer, makeChecker, fakeSupabaseKey } from './helpers.mjs';
import * as stripe from '../lib/stripe.js';
import { verifySignature, planForPrice, rowForEvent, domainAddonsOf, isNewer, LIVE_STATUSES } from '../lib/stripe.js';

const SECRET = 'whsec_test_secret_value';
const ENV = {
  STRIPE_PRICE_BASE: 'price_base_123',
  STRIPE_PRICE_PRO: 'price_pro_456',
  STRIPE_PRICE_ENTERPRISE: 'price_ent_789',
  STRIPE_PRICE_DOMAIN: 'price_dom_000',
};

const sign = (body, secret = SECRET, t = Math.floor(Date.now() / 1000)) =>
  `t=${t},v1=${createHmac('sha256', secret).update(`${t}.${body}`, 'utf8').digest('hex')}`;

const checkoutEvent = (email, price) => ({
  type: 'checkout.session.completed',
  data: { object: {
    customer_details: { email },
    customer: 'cus_ABC123',
    payment_status: 'paid',
    status: 'complete',
    line_items: { data: [{ price: { id: price } }] },
  } },
});

function listen(handler) {
  return new Promise((res) => {
    const srv = createServer(handler);
    srv.listen(0, '127.0.0.1', () => res({ srv, url: `http://127.0.0.1:${srv.address().port}` }));
  });
}
const readBody = (req) => new Promise((res) => { let b = ''; req.on('data', (d) => { b += d; }); req.on('end', () => res(b)); });

/* A stand-in for the subscriptions table, speaking just enough PostgREST for
   the webhook's email branch: a select by address, and an upsert. Every write
   is kept, so a test can say "nothing was written", not only "the row looks
   the same". */
async function fakeSubscriptions() {
  const rows = new Map();
  const writes = [];
  const server = await listen(async (req, res) => {
    const url = new URL(req.url, 'http://x');
    if (!url.pathname.startsWith('/rest/v1/subscriptions')) { res.writeHead(404); res.end(); return; }
    if (req.method === 'GET') {
      const email = (url.searchParams.get('email') || '').replace(/^eq\./, '');
      const row = rows.get(email);
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(row ? [row] : []));
      return;
    }
    if (req.method === 'POST') {
      const body = JSON.parse(await readBody(req));
      writes.push(body);
      rows.set(body.email, { ...(rows.get(body.email) || {}), ...body });
      res.writeHead(201); res.end();
      return;
    }
    res.writeHead(405); res.end();
  });
  return { rows, writes, url: server.url, close: () => new Promise(r => server.srv.close(r)) };
}

export default async function run(state) {
  const check = makeChecker(state);

  console.log('\n— a forged webhook cannot grant anybody a plan —');
  {
    const body = JSON.stringify(checkoutEvent('buyer@example.com', ENV.STRIPE_PRICE_PRO));

    check(verifySignature(body, sign(body), SECRET).ok, 'a correctly signed delivery is accepted');

    check(!verifySignature(body, sign(body, 'whsec_someone_elses_secret'), SECRET).ok,
      'signed with the wrong secret — refused');
    check(!verifySignature(body + ' ', sign(body), SECRET).ok,
      'body altered after signing — refused');
    check(!verifySignature(body, 'v1=deadbeef', SECRET).ok,
      'no timestamp in the header — refused');
    check(!verifySignature(body, '', SECRET).ok, 'no signature at all — refused');
    check(!verifySignature(body, sign(body), '').ok,
      'and with no secret configured it refuses rather than accepting everything');

    /* The one that is easy to miss: without a tolerance check a captured
       delivery stays valid forever. */
    const old = Math.floor(Date.now() / 1000) - 3600;
    check(!verifySignature(body, sign(body, SECRET, old), SECRET).ok,
      'an hour-old delivery replayed — refused');
    const soon = Math.floor(Date.now() / 1000) - 120;
    check(verifySignature(body, sign(body, SECRET, soon), SECRET).ok,
      'two minutes old, which is ordinary network delay — still accepted');
  }

  console.log('\n— which plan was bought —');
  {
    check(planForPrice(ENV.STRIPE_PRICE_BASE, ENV) === 'base', 'the Base price maps to Base');
    check(planForPrice(ENV.STRIPE_PRICE_PRO, ENV) === 'pro', 'Pro to Pro');
    check(planForPrice(ENV.STRIPE_PRICE_ENTERPRISE, ENV) === 'enterprise', 'Enterprise to Enterprise');
    check(planForPrice('price_unknown', ENV) === null, 'an unknown price maps to nothing, not a guess');
    check(planForPrice(undefined, {}) === null, 'and no price at all is not silently the free-est thing');
    /* With the price variables unset, nothing may match — including the empty
       string a missing id would arrive as. */
    check(planForPrice('', {}) === null && planForPrice('price_pro_456', {}) === null,
      'unconfigured prices match nothing rather than collapsing onto one plan');
  }

  console.log('\n— what each event does to the record —');
  {
    const paid = rowForEvent(checkoutEvent('Buyer@Example.com', ENV.STRIPE_PRICE_PRO), ENV);
    check(paid.by === 'email' && paid.email === 'buyer@example.com',
      `checkout is keyed by the buyer's address, lowercased: ${paid.email}`);
    check(paid.plan === 'pro' && paid.status === 'active', `and records: ${paid.plan}/${paid.status}`);
    check(paid.stripe_customer === 'cus_ABC123', 'keeping the customer id, which later events arrive with');

    const upgraded = rowForEvent({
      type: 'customer.subscription.updated',
      data: { object: { customer: 'cus_ABC123', status: 'active', items: { data: [{ price: { id: ENV.STRIPE_PRICE_ENTERPRISE } }] } } },
    }, ENV);
    check(upgraded.by === 'customer' && upgraded.plan === 'enterprise',
      'an upgrade arrives with no email and is matched on the customer instead');

    const gone = rowForEvent({
      type: 'customer.subscription.deleted',
      data: { object: { customer: 'cus_ABC123', status: 'canceled' } },
    }, ENV);
    check(gone.status === 'canceled' && gone.plan === null, 'cancelling clears the plan');

    const failed = rowForEvent({
      type: 'customer.subscription.updated',
      data: { object: { customer: 'cus_ABC123', status: 'past_due', items: { data: [{ price: { id: ENV.STRIPE_PRICE_PRO } }] } } },
    }, ENV);
    check(failed.status === 'past_due' && LIVE_STATUSES.includes(failed.status),
      'a card that failed this morning stays entitled while Stripe retries it');

    /* The add-on makes a subscription carry two lines, and Stripe promises
       nothing about their order. Reading items.data[0] was safe with one line
       and silently wrong with two: an add-on listed first would read as no
       plan at all and clear it — downgrading a paying customer for buying
       something extra. */
    const bothWaysRound = [
      [{ price: { id: ENV.STRIPE_PRICE_DOMAIN }, quantity: 2 }, { price: { id: ENV.STRIPE_PRICE_PRO }, quantity: 1 }],
      [{ price: { id: ENV.STRIPE_PRICE_PRO }, quantity: 1 }, { price: { id: ENV.STRIPE_PRICE_DOMAIN }, quantity: 2 }],
    ];
    for (const [i, items] of bothWaysRound.entries()) {
      const r = rowForEvent({ type: 'customer.subscription.updated',
        data: { object: { customer: 'cus_ABC123', status: 'active', items: { data: items } } } }, ENV);
      check(r.plan === 'pro' && r.domain_addons === 2,
        `${i ? 'plan first' : 'add-on first'}: plan=${r.plan}, domains=${r.domain_addons}`);
    }

    check(domainAddonsOf({ items: { data: [{ price: { id: ENV.STRIPE_PRICE_PRO }, quantity: 1 }] } }, ENV) === 0,
      'a subscription with no add-on line buys no domains');
    check(domainAddonsOf({ items: { data: [{ price: { id: ENV.STRIPE_PRICE_DOMAIN }, quantity: 3 }] } }, {}) === 0,
      'and with the add-on price unconfigured, nothing is granted by accident');

    /* Removing the extra domain sends a quantity of nothing, which has to be
       written, not skipped — otherwise it stays entitled after the money stops. */
    const dropped = rowForEvent({ type: 'customer.subscription.updated',
      data: { object: { customer: 'cus_ABC123', status: 'active',
        items: { data: [{ price: { id: ENV.STRIPE_PRICE_PRO }, quantity: 1 }] } } } }, ENV);
    check(dropped.domain_addons === 0, 'dropping the add-on records zero rather than leaving the old count');

    check(rowForEvent({ type: 'invoice.created', data: { object: {} } }, ENV) === null,
      'events that change nothing are ignored rather than acted on');
    check(rowForEvent({ type: 'checkout.session.completed', data: { object: { customer_details: {} } } }, ENV) === null,
      'and a checkout with no email is not turned into a row keyed by nothing');
  }

  console.log('\n— a late delivery cannot undo a newer one —');
  {
    /* Stripe does not order deliveries and retries for days, so an upgrade and
       the downgrade after it can arrive the wrong way round. Without a guard
       the row holds whichever landed last, not whichever happened last — and
       the customer is on the wrong plan with nothing in the logs to say why. */
    const t = 1780000000;
    const ev = (created, status) => ({
      type: 'customer.subscription.updated', created,
      data: { object: { customer: 'cus_X', status, items: { data: [{ price: { id: ENV.STRIPE_PRICE_PRO } }] } } },
    });

    const older = rowForEvent(ev(t, 'active'), ENV);
    const newer = rowForEvent(ev(t + 3600, 'canceled'), ENV);
    check(!!older.event_at && !!newer.event_at, `each event carries when Stripe says it happened: ${newer.event_at}`);
    check(new Date(newer.event_at) > new Date(older.event_at), 'and the later one is later');

    check(isNewer(newer.event_at, older.event_at), 'a newer delivery is applied');
    check(!isNewer(older.event_at, newer.event_at), 'an older one arriving late is refused');
    check(isNewer(older.event_at, older.event_at), 'a redelivery of the same event still applies, so a retry is not lost');
    check(isNewer(newer.event_at, null), 'a row written before this guard existed is always overwritten');
    check(isNewer(null, newer.event_at), 'and an event with no timestamp is treated as current rather than dropped');

    const checkout = rowForEvent({ ...checkoutEvent('a@b.com', ENV.STRIPE_PRICE_PRO), created: t }, ENV);
    check(!!checkout.event_at, 'checkout events are stamped too — they race the subscription ones');
  }

  console.log('\n— a checkout never takes over another customer\'s live subscription —');
  {
    /* Stripe's checkout lets the buyer type any address. Keyed by address, a
       checkout from a second Stripe customer would replace a paying
       customer's plan and customer id; cancelling it would then cancel them,
       and their own later events would match no row. */
    const conflict = stripe.checkoutConflict;
    check(typeof conflict === 'function', 'lib/stripe.js decides it in one place (checkoutConflict)');
    if (typeof conflict === 'function') {
      const row = { stripe_customer: 'cus_B' };
      for (const status of ['active', 'trialing', 'past_due', 'ACTIVE']) {
        check(conflict({ stripe_customer: 'cus_A', status }, row) === true,
          `held ${status} under another customer: a conflict`);
      }
      check(conflict({ stripe_customer: 'cus_A', status: 'active' }, { stripe_customer: null }) === true,
        'a checkout with no customer id cannot strip the live one either');
      check(conflict({ stripe_customer: 'cus_B', status: 'active' }, row) === false,
        'the same customer updating their own row: no conflict');
      check(conflict({ stripe_customer: null, status: 'active' }, row) === false,
        'a row minted by hand (no customer) may be claimed by the first checkout');
      for (const status of ['canceled', 'unpaid', 'incomplete_expired', null]) {
        check(conflict({ stripe_customer: 'cus_A', status }, row) === false,
          `held ${status} under another customer: no longer live, so a new checkout may take it`);
      }
      check(conflict(null, row) === false, 'no row at all: nothing to conflict with');
    }
  }

  console.log('\n— the same, through the endpoint and a stand-in database —');
  {
    const db = await fakeSubscriptions();
    const s = await startServer({ env: {
      ...ENV,
      STRIPE_WEBHOOK_SECRET: SECRET,
      SUPABASE_URL: db.url,
      SUPABASE_SERVICE_ROLE_KEY: fakeSupabaseKey('service_role'),
    } });
    const t = Math.floor(Date.now() / 1000);
    const checkout = (email, customer, price, created) => ({
      type: 'checkout.session.completed', created,
      data: { object: {
        customer_details: { email }, customer, payment_status: 'paid', status: 'complete',
        line_items: { data: [{ price: { id: price } }] },
      } },
    });
    const deliver = async (event) => {
      const body = JSON.stringify(event);
      const r = await fetch(s.url + '/api/stripe/webhook', {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'stripe-signature': sign(body) }, body,
      });
      return { status: r.status, d: await r.json().catch(() => ({})) };
    };
    try {
      const victim = 'victim@example.com';
      const held = { email: victim, plan: 'pro', status: 'active', stripe_customer: 'cus_A', domain_addons: 0,
        event_at: new Date((t - 86400) * 1000).toISOString(), updated_at: 'then' };
      db.rows.set(victim, { ...held });

      const r = await deliver(checkout(victim, 'cus_B', ENV.STRIPE_PRICE_BASE, t));
      check(r.status === 200, `a second customer's checkout for a live address is answered 200, so Stripe stops: ${r.status}`);
      check(r.d.conflict === true && r.d.acted === false, `with a conflict outcome: ${JSON.stringify(r.d)}`);
      check(db.writes.length === 0, `and nothing is written: ${db.writes.length} write(s)`);
      check(JSON.stringify(db.rows.get(victim)) === JSON.stringify(held),
        `the paying customer keeps their row: ${JSON.stringify(db.rows.get(victim))}`);
      await new Promise(res => setTimeout(res, 200));
      const log = s.log();
      check(/conflict/i.test(log), 'a status line is logged for the owner to reconcile');
      check(!log.includes(victim) && !log.includes('cus_A') && !log.includes('cus_B'),
        'and it names neither the address nor either customer');

      const own = await deliver(checkout(victim, 'cus_A', ENV.STRIPE_PRICE_PRO, t + 10));
      check(own.status === 200 && own.d.acted === true && db.rows.get(victim).stripe_customer === 'cus_A',
        `the same customer checking out again still updates their own row: ${JSON.stringify(own.d)}`);

      const minted = 'minted@example.com';
      db.rows.set(minted, { email: minted, plan: 'pro', status: 'active', stripe_customer: null, event_at: null });
      const claim = await deliver(checkout(minted, 'cus_C', ENV.STRIPE_PRICE_PRO, t));
      check(claim.d.acted === true && db.rows.get(minted).stripe_customer === 'cus_C',
        `a row minted by hand is claimed by the first checkout: ${JSON.stringify(claim.d)}`);

      const lapsed = 'lapsed@example.com';
      db.rows.set(lapsed, { email: lapsed, plan: null, status: 'canceled', stripe_customer: 'cus_OLD',
        event_at: new Date((t - 86400) * 1000).toISOString() });
      const back = await deliver(checkout(lapsed, 'cus_NEW', ENV.STRIPE_PRICE_BASE, t));
      check(back.d.acted === true && db.rows.get(lapsed).stripe_customer === 'cus_NEW' && db.rows.get(lapsed).status === 'active',
        `someone whose subscription ended may subscribe again as a new customer: ${JSON.stringify(back.d)}`);

      const before = db.writes.length;
      const late = await deliver(checkout(victim, 'cus_A', ENV.STRIPE_PRICE_BASE, t - 3600));
      check(late.d.stale === true && db.writes.length === before,
        `and a late delivery is still refused as stale: ${JSON.stringify(late.d)}`);
    } finally { await s.stop(); await db.close(); }
  }

  console.log('\n— the live endpoint —');
  {
    const s = await startServer({ env: { STRIPE_WEBHOOK_SECRET: SECRET } });
    try {
      const body = JSON.stringify(checkoutEvent('buyer@example.com', ENV.STRIPE_PRICE_PRO));

      const forged = await fetch(s.url + '/api/stripe/webhook', {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'stripe-signature': 't=1,v1=00' }, body,
      });
      check(forged.status === 400, `an unsigned POST is refused: ${forged.status}`);
      check(/signature/i.test((await forged.json()).error || ''), 'and says why');

      /* Signed correctly, but this deployment has no database — it must fail
         loudly with a 500 so Stripe retries, never a quiet 200 that would drop
         a real payment on the floor. */
      const real = await fetch(s.url + '/api/stripe/webhook', {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'stripe-signature': sign(body) }, body,
      });
      check(real.status === 500, `a signed event with no database returns ${real.status}, so Stripe retries`);

      /* An event we do not act on is a 200: Stripe retries anything else for
         days, and "understood, nothing to do" is the honest answer. */
      const noise = JSON.stringify({ type: 'invoice.created', data: { object: {} } });
      const ignored = await fetch(s.url + '/api/stripe/webhook', {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'stripe-signature': sign(noise) }, body: noise,
      });
      const ib = await ignored.json();
      check(ignored.status === 200 && ib.acted === false,
        `an event it does not act on is accepted, not retried forever: ${ignored.status} acted=${ib.acted}`);

      const info = await (await fetch(s.url + '/api/stripe/webhook')).json();
      check(info.configured === true, 'and a browser visiting the URL gets an explanation, not a stack trace');
    } finally { await s.stop(); }
  }
}
