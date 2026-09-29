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
   the webhook: a select by address or by customer, an upsert (which, like
   PostgREST's merge-duplicates, leaves a column the body does not name as it
   was, and like the table gives a new row plan 'base' and no add-ons), and
   the subscription branch's update by customer with its `or=(event_at.is.null,
   event_at.lte.…)` guard (or `lt`, for customer.subscription.created). Every
   write is kept, so a test can say "nothing was written", not only "the row
   looks the same".

   And pending_subscriptions (database.sql section 6), keyed by customer: an
   insert that ignores a duplicate (Prefer: resolution=ignore-duplicates) and
   answers with the rows it inserted, the same guarded update, a select and a
   delete. `state.pendingMissing` makes it answer as a database where
   section 6 never ran (PostgREST's PGRST205). */
async function fakeSubscriptions() {
  const rows = new Map();
  const pending = new Map();
  const writes = [];
  const pendingWrites = [];
  const state = { pendingMissing: false };
  const json = (res, status, body) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(body)); };
  const guarded = (url, r) => {
    const g = /event_at\.(lte|lt)\.([^,)]+)/.exec(url.searchParams.get('or') || '');
    if (!g || !r.event_at) return true;
    const a = new Date(r.event_at).getTime(), b = new Date(g[2]).getTime();
    return g[1] === 'lt' ? a < b : a <= b;
  };
  const server = await listen(async (req, res) => {
    const url = new URL(req.url, 'http://x');
    const eq = (k) => (url.searchParams.get(k) || '').replace(/^eq\./, '');
    if (url.pathname === '/rest/v1/pending_subscriptions') {
      if (state.pendingMissing) {
        req.resume();
        return json(res, 404, { code: 'PGRST205', details: null, hint: null, message: "Could not find the table 'public.pending_subscriptions' in the schema cache" });
      }
      const c = eq('stripe_customer');
      if (req.method === 'GET') { req.resume(); return json(res, 200, pending.has(c) ? [pending.get(c)] : []); }
      if (req.method === 'POST') {
        const body = JSON.parse(await readBody(req));
        const ignore = /ignore-duplicates/.test(req.headers.prefer || '');
        if (pending.has(body.stripe_customer) && ignore) return json(res, 201, []);
        pendingWrites.push(body);
        pending.set(body.stripe_customer, { plan: null, domain_addons: null, ...(pending.get(body.stripe_customer) || {}), ...body });
        return json(res, 201, [{ stripe_customer: body.stripe_customer }]);
      }
      if (req.method === 'PATCH') {
        const patch = JSON.parse(await readBody(req));
        const r = pending.get(c);
        if (!r || !guarded(url, r)) return json(res, 200, []);
        pendingWrites.push({ stripe_customer: c, ...patch });
        Object.assign(r, patch);
        return json(res, 200, [{ stripe_customer: c }]);
      }
      if (req.method === 'DELETE') { req.resume(); const had = pending.delete(c); return json(res, 200, had ? [{ stripe_customer: c }] : []); }
    }
    if (!url.pathname.startsWith('/rest/v1/subscriptions')) { req.resume(); res.writeHead(404); res.end(); return; }
    if (req.method === 'GET') {
      req.resume();
      if (url.searchParams.has('stripe_customer')) {
        const c = eq('stripe_customer');
        return json(res, 200, [...rows.values()].filter(r => r.stripe_customer === c));
      }
      const row = rows.get(eq('email'));
      return json(res, 200, row ? [row] : []);
    }
    if (req.method === 'POST') {
      const body = JSON.parse(await readBody(req));
      writes.push(body);
      const was = rows.get(body.email) || { plan: 'base', domain_addons: 0 };
      rows.set(body.email, { ...was, ...body });
      res.writeHead(201); res.end();
      return;
    }
    if (req.method === 'PATCH') {
      const patch = JSON.parse(await readBody(req));
      const c = eq('stripe_customer');
      const hit = [...rows.values()].filter(r => r.stripe_customer === c && guarded(url, r));
      for (const r of hit) { writes.push({ email: r.email, ...patch }); Object.assign(r, patch); }
      return json(res, 200, hit.map(r => ({ email: r.email, stripe_customer: r.stripe_customer })));
    }
    res.writeHead(405); res.end();
  });
  return { rows, pending, writes, pendingWrites, state, url: server.url, close: () => new Promise(r => server.srv.close(r)) };
}

/* Events in the shape Stripe really sends them. A Checkout Session carries
   `line_items` only when it is retrieved with expand[]=line_items, and a
   webhook event is never expanded, so these have none: the field list is the
   Checkout Session object's (API reference, "The Checkout Session object")
   with line_items left out, as a delivery leaves it out. `created` on the
   session is when the buyer opened the checkout page; `created` on the event
   is when Stripe raised the event. */
function realCheckout({ type = 'checkout.session.completed', email, customer, paid = true, sessionAt, eventAt }) {
  return {
    id: 'evt_' + type.length + '_' + eventAt, object: 'event', api_version: '2025-03-31.basil',
    created: eventAt, livemode: false, pending_webhooks: 1,
    request: { id: null, idempotency_key: null }, type,
    data: { object: {
      id: 'cs_test_' + customer, object: 'checkout.session',
      adaptive_pricing: { enabled: false }, after_expiration: null, allow_promotion_codes: false,
      amount_subtotal: 2399, amount_total: 2399,
      automatic_tax: { enabled: false, liability: null, status: null },
      billing_address_collection: 'auto', cancel_url: null, client_reference_id: null,
      collected_information: null, consent: null, consent_collection: null,
      created: sessionAt, currency: 'usd', currency_conversion: null,
      custom_fields: [], custom_text: { after_submit: null, shipping_address: null, submit: null, terms_of_service_acceptance: null },
      customer, customer_creation: 'always',
      customer_details: { address: { city: null, country: 'DE', line1: null, line2: null, postal_code: null, state: null },
        email, name: 'A Buyer', phone: null, tax_exempt: 'none', tax_ids: [] },
      customer_email: null, discounts: [], expires_at: sessionAt + 86400,
      invoice: 'in_test_1', invoice_creation: null, livemode: false, locale: 'auto', metadata: {},
      mode: 'subscription', payment_intent: null, payment_link: 'plink_test_pro',
      payment_method_collection: 'always', payment_method_configuration_details: null,
      payment_method_options: {}, payment_method_types: paid ? ['card'] : ['sepa_debit'],
      payment_status: paid ? 'paid' : 'unpaid', phone_number_collection: { enabled: false },
      recovered_from: null, saved_payment_method_options: null, setup_intent: null,
      shipping_address_collection: null, shipping_cost: null, shipping_details: null, shipping_options: [],
      status: 'complete', submit_type: null, subscription: 'sub_test_' + customer,
      success_url: 'https://mailrata.org/welcome', total_details: { amount_discount: 0, amount_shipping: 0, amount_tax: 0 },
      ui_mode: 'hosted', url: null,
    } },
  };
}
/* customer.subscription.created / .updated: these do carry the price, on
   items.data[].price. */
function realSubscription({ type = 'customer.subscription.created', customer, price, status, eventAt }) {
  return {
    id: 'evt_sub_' + eventAt, object: 'event', api_version: '2025-03-31.basil',
    created: eventAt, livemode: false, pending_webhooks: 1,
    request: { id: null, idempotency_key: null }, type,
    data: { object: {
      id: 'sub_test_' + customer, object: 'subscription', cancel_at: null, cancel_at_period_end: false,
      canceled_at: null, collection_method: 'charge_automatically', created: eventAt, currency: 'usd',
      customer, default_payment_method: null, ended_at: null, latest_invoice: 'in_test_1', livemode: false,
      metadata: {}, start_date: eventAt, status,
      items: { object: 'list', has_more: false, url: '/v1/subscription_items?subscription=sub_test_' + customer,
        data: [{ id: 'si_test_1', object: 'subscription_item', created: eventAt, quantity: 1,
          subscription: 'sub_test_' + customer,
          price: { id: price, object: 'price', active: true, currency: 'usd', product: 'prod_test',
            recurring: { interval: 'month', interval_count: 1 }, type: 'recurring', unit_amount: 2399 } }] },
    } },
  };
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

  console.log('\n— a checkout event as Stripe sends it names no plan —');
  {
    /* Found in review part 3: Stripe leaves line_items out of every webhook,
       so reading the plan from them recorded every checkout as Base, and the
       payment clearing days later turned a Pro customer back into Base. */
    const t = 1780000000;
    for (const type of ['checkout.session.completed', 'checkout.session.async_payment_succeeded']) {
      const ev = realCheckout({ type, email: 'Pro@Example.com', customer: 'cus_PRO', sessionAt: t - 120, eventAt: t });
      check(!('line_items' in ev.data.object), `${type}: the fixture has no line_items, as a delivery has none`);
      const r = rowForEvent(ev, ENV);
      check(!!r && r.by === 'email' && r.email === 'pro@example.com' && r.stripe_customer === 'cus_PRO' && r.status === 'active',
        `${type}: still records who paid and whether the money is in: ${JSON.stringify(r)}`);
      check(!!r && r.plan === null, `${type}: and claims no plan it was not told (was 'base'): ${r && r.plan}`);
      check(!!r && r.domain_addons === null, `${type}: nor a number of domains (was 0): ${r && r.domain_addons}`);
    }
    const sub = rowForEvent(realSubscription({ customer: 'cus_PRO', price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t }), ENV);
    check(sub.plan === 'pro' && sub.domain_addons === 0,
      `the subscription event, which carries items, is what names the plan: ${sub.plan}`);
    const bare = rowForEvent({ type: 'customer.subscription.updated', created: t,
      data: { object: { customer: 'cus_PRO', status: 'active' } } }, ENV);
    check(bare.plan === null && bare.domain_addons === null,
      'and a subscription event with no items claims neither a plan nor zero domains');
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

    /* BUG-S: created is strictly later than what it replaces; updated and
       deleted may equal it, so a redelivery still applies. */
    const at = new Date(t * 1000).toISOString();
    check(stripe.orderGuard('customer.subscription.created', at) === `event_at.is.null,event_at.lt.${at}`,
      'customer.subscription.created writes only over a row stamped strictly earlier');
    for (const type of ['customer.subscription.updated', 'customer.subscription.deleted']) {
      check(stripe.orderGuard(type, at) === `event_at.is.null,event_at.lte.${at}`, `${type} keeps lte`);
    }
    check(stripe.orderGuard('customer.subscription.created', null) === null, 'and an event with no time is not guarded, as isNewer treats it');
    check(stripe.pendingBelongs({ event_at: at }, { stamp_at: new Date((t - 60) * 1000).toISOString() }) === true
      && stripe.pendingBelongs({ event_at: at }, { stamp_at: new Date((t + 60) * 1000).toISOString() }) === false,
      'a kept subscription event belongs to a checkout only when raised after the buyer opened it');
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
      /* Review P3-5: a row waiting for a bank transfer is held too. Taken
         over by a card checkout, the transfer clearing would then be refused
         as a conflict and its subscription events would match no row. */
      check(conflict({ stripe_customer: 'cus_V', status: 'incomplete' }, { stripe_customer: 'cus_A' }) === true,
        'held incomplete (a bank transfer still clearing) under another customer: a conflict');
      check(conflict({ stripe_customer: 'cus_V', status: 'incomplete' }, { stripe_customer: 'cus_V' }) === false,
        'while the same customer\'s payment clearing still lands on it');
      for (const status of ['canceled', 'unpaid', 'incomplete_expired', null]) {
        check(conflict({ stripe_customer: 'cus_A', status }, row) === false,
          `held ${status} under another customer: no longer live, so a new checkout may take it`);
      }
      check(conflict(null, row) === false, 'no row at all: nothing to conflict with');
    }
  }

  console.log('\n— a checkout that has not been paid for yet grants nothing —');
  {
    /* Delayed payment methods (SEPA, ACH, Boleto) complete the checkout
       before the money arrives: status "complete", payment_status "unpaid".
       Stripe says later, with checkout.session.async_payment_succeeded or
       _failed. Recording the first as active gave a licence to a payment that
       could still fail. */
    const withPayment = (payment_status, type = 'checkout.session.completed') => {
      const e = checkoutEvent('Later@Example.com', ENV.STRIPE_PRICE_PRO);
      return { ...e, type, data: { object: { ...e.data.object, payment_status } } };
    };
    const unpaid = rowForEvent(withPayment('unpaid'), ENV);
    check(!!unpaid && unpaid.by === 'email' && unpaid.email === 'later@example.com',
      `an unpaid checkout is still recorded, so the customer's later events find their row: ${JSON.stringify(unpaid)}`);
    check(!!unpaid && unpaid.status === 'incomplete' && !LIVE_STATUSES.includes(unpaid.status),
      `as incomplete, which is not entitled: ${unpaid && unpaid.status}`);
    check(!!unpaid && unpaid.stripe_customer === 'cus_ABC123' && unpaid.plan === 'pro',
      'keeping the customer and the plan it is for');

    const free = rowForEvent(withPayment('no_payment_required'), ENV);
    check(free.status === 'active', `a checkout that needs no payment (a 100% coupon, a free trial) is active: ${free.status}`);
    check(rowForEvent(withPayment('paid'), ENV).status === 'active', 'a paid one is active, as before');

    const cleared = rowForEvent(withPayment('paid', 'checkout.session.async_payment_succeeded'), ENV);
    check(!!cleared && cleared.by === 'email' && cleared.status === 'active' && cleared.plan === 'pro',
      `the payment clearing later makes it active: ${JSON.stringify(cleared)}`);
    check(rowForEvent(withPayment('unpaid', 'checkout.session.async_payment_failed'), ENV) === null,
      'and failing later changes nothing: the row was never live');
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

      /* Paid by bank transfer: recorded, not live, until the money arrives. */
      const slow = 'slow@example.com';
      const unpaid = checkout(slow, 'cus_SLOW', ENV.STRIPE_PRICE_PRO, t);
      unpaid.data.object.payment_status = 'unpaid';
      const pending = await deliver(unpaid);
      check(pending.status === 200 && db.rows.get(slow)?.status === 'incomplete',
        `an unpaid checkout is recorded as incomplete: ${JSON.stringify(pending.d)} ${JSON.stringify(db.rows.get(slow))}`);
      const paidLater = { ...checkout(slow, 'cus_SLOW', ENV.STRIPE_PRICE_PRO, t + 60),
        type: 'checkout.session.async_payment_succeeded' };
      const settled = await deliver(paidLater);
      check(settled.d.acted === true && db.rows.get(slow).status === 'active',
        `and made active when the payment clears: ${JSON.stringify(settled.d)} ${JSON.stringify(db.rows.get(slow))}`);

      /* The same customer checking out again by bank transfer must not take
         away what they already pay for while the new payment is pending. */
      const again = checkout(victim, 'cus_A', ENV.STRIPE_PRICE_BASE, t + 20);
      again.data.object.payment_status = 'unpaid';
      const before2 = db.writes.length;
      const kept = await deliver(again);
      check(kept.status === 200 && kept.d.acted === false && db.writes.length === before2
        && db.rows.get(victim).status === 'active',
        `a pending checkout never downgrades a live row: ${JSON.stringify(kept.d)} ${JSON.stringify(db.rows.get(victim))}`);

      /* Review P3-5, through the endpoint: a card checkout under another
         customer cannot take over a row whose bank transfer is still
         clearing. */
      const waiting = 'waiting@example.com';
      const heldWaiting = { email: waiting, plan: 'pro', status: 'incomplete', stripe_customer: 'cus_V', domain_addons: 0,
        event_at: new Date((t - 600) * 1000).toISOString() };
      db.rows.set(waiting, { ...heldWaiting });
      const before3 = db.writes.length;
      const grab = await deliver(checkout(waiting, 'cus_ATTACKER', ENV.STRIPE_PRICE_BASE, t));
      check(grab.status === 200 && grab.d.conflict === true && db.writes.length === before3
        && JSON.stringify(db.rows.get(waiting)) === JSON.stringify(heldWaiting),
        `a paid checkout by another customer over an incomplete row writes nothing: ${JSON.stringify(grab.d)}`);
      const cleared2 = await deliver({ ...checkout(waiting, 'cus_V', ENV.STRIPE_PRICE_PRO, t + 60),
        type: 'checkout.session.async_payment_succeeded' });
      check(cleared2.d.acted === true && db.rows.get(waiting).status === 'active' && db.rows.get(waiting).stripe_customer === 'cus_V',
        `and the owner's transfer clearing still makes it theirs and active: ${JSON.stringify(db.rows.get(waiting))}`);

      /* The found-on-the-way bug, as Stripe really delivers it: no line_items
         on either checkout event. Bank transfer: the checkout completes
         unpaid, the subscription (Pro) is created a moment before the
         checkout event is raised, and the money clears three days later. */
      const pro = 'pro-buyer@example.com';
      const opened = t - 300;
      const c1 = await deliver(realCheckout({ email: pro, customer: 'cus_PRO', paid: false, sessionAt: opened, eventAt: t }));
      check(c1.d.acted === true && db.rows.get(pro)?.status === 'incomplete',
        `a Pro checkout paid by transfer is recorded, not live: ${JSON.stringify(db.rows.get(pro))}`);
      const s1 = await deliver(realSubscription({ customer: 'cus_PRO', price: ENV.STRIPE_PRICE_PRO, status: 'incomplete', eventAt: t - 1 }));
      check(s1.d.acted === true && db.rows.get(pro).plan === 'pro',
        `its subscription event, raised a second before the checkout event, still lands and names the plan: ${JSON.stringify(s1.d)} ${db.rows.get(pro).plan}`);
      const d3 = t + 3 * 86400;
      const writesBefore = db.writes.length;
      const a1 = await deliver(realCheckout({ type: 'checkout.session.async_payment_succeeded', email: pro, customer: 'cus_PRO',
        paid: true, sessionAt: opened, eventAt: d3 }));
      check(a1.status === 200 && db.rows.get(pro).status === 'active',
        `the payment clearing makes it active: ${JSON.stringify(a1.d)} ${JSON.stringify(db.rows.get(pro))}`);
      check(db.rows.get(pro).plan === 'pro',
        `and the row ends Pro, not turned back into Base: ${db.rows.get(pro).plan}`);
      check(db.writes.slice(writesBefore).every(w => !('plan' in w) && !('domain_addons' in w)),
        'the checkout event wrote neither plan nor domains, since it knew neither');
      const s2 = await deliver(realSubscription({ type: 'customer.subscription.updated', customer: 'cus_PRO',
        price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: d3 - 2 }));
      check(s2.d.acted === true && db.rows.get(pro).plan === 'pro' && db.rows.get(pro).status === 'active',
        `and the subscription turning active, raised just before it, is not refused as stale: ${JSON.stringify(s2.d)}`);

      /* Card, with the subscription event delivered first, which is the
         usual order (BUG-S): there is no row yet, so the event is kept
         against its customer, and the checkout event that makes the row
         applies it in the same request. No 409, no waiting for Stripe's
         retry (hours in test mode). */
      const card = 'card-buyer@example.com';
      const early = realSubscription({ customer: 'cus_CARD', price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t - 2 });
      const e1 = await deliver(early);
      check(e1.status === 200 && e1.d.pending === true,
        `a subscription event before its checkout is answered 200 and kept, not refused with 409: ${e1.status} ${JSON.stringify(e1.d)}`);
      check(db.pending.get('cus_CARD')?.plan === 'pro' && db.pending.get('cus_CARD')?.status === 'active' && !db.rows.has(card),
        `kept against its customer, with no row made up for an address it does not carry: ${JSON.stringify(db.pending.get('cus_CARD'))}`);
      check(db.pendingWrites.every(w => !('email' in w)), 'and the kept event holds no address');
      const c2 = await deliver(realCheckout({ email: card, customer: 'cus_CARD', paid: true, sessionAt: t - 90, eventAt: t }));
      check(c2.status === 200 && db.rows.get(card)?.plan === 'pro' && db.rows.get(card)?.status === 'active',
        `the checkout event lands it: the row is Pro and live after the first delivery of each: ${JSON.stringify(c2.d)} ${JSON.stringify(db.rows.get(card))}`);
      check(!db.pending.has('cus_CARD'), 'and the kept event is cleared once applied');
      const e2 = await deliver(early);
      check(e2.status === 200 && db.rows.get(card).plan === 'pro' && db.rows.get(card).status === 'active' && !db.pending.has('cus_CARD'),
        `a redelivery of the subscription event changes nothing and keeps nothing: ${JSON.stringify(e2.d)}`);
      const again2 = await deliver(realCheckout({ email: card, customer: 'cus_CARD', paid: true, sessionAt: t - 90, eventAt: t + 5 }));
      check(again2.d.acted === true && db.rows.get(card).plan === 'pro' && db.rows.get(card).status === 'active',
        `and a redelivered checkout, which names no plan, leaves Pro, and live, alone: ${JSON.stringify(db.rows.get(card))}`);

      /* The ordering guard still holds: a subscription event from before the
         buyer opened this checkout (an old subscription of the same
         customer) cannot land over it. */
      const stale = await deliver(realSubscription({ type: 'customer.subscription.updated', customer: 'cus_CARD',
        price: ENV.STRIPE_PRICE_BASE, status: 'canceled', eventAt: t - 3600 }));
      check(stale.d.stale === true && db.rows.get(card).plan === 'pro' && db.rows.get(card).status === 'active',
        `an older subscription event is still refused as stale: ${JSON.stringify(stale.d)}`);

      /* A new customer taking over an ended row does not inherit its plan. */
      const reused = 'reused@example.com';
      db.rows.set(reused, { email: reused, plan: 'pro', status: 'canceled', stripe_customer: 'cus_GONE', domain_addons: 2,
        event_at: new Date((t - 86400) * 1000).toISOString() });
      await deliver(realCheckout({ email: reused, customer: 'cus_FRESH', paid: true, sessionAt: t - 60, eventAt: t }));
      check(db.rows.get(reused).stripe_customer === 'cus_FRESH' && db.rows.get(reused).plan === 'base'
        && db.rows.get(reused).domain_addons === 0 && db.rows.get(reused).status === 'incomplete',
        `a new customer on an ended row inherits neither its plan nor its domains, and waits for its subscription: ${JSON.stringify(db.rows.get(reused))}`);
      await deliver(realSubscription({ customer: 'cus_FRESH', price: ENV.STRIPE_PRICE_BASE, status: 'active', eventAt: t + 1 }));
      check(db.rows.get(reused).plan === 'base' && db.rows.get(reused).status === 'active',
        `which then makes it live on the plan bought: ${JSON.stringify(db.rows.get(reused))}`);

      /* A live customer checking out again (a second subscription) is not
         taken off their live plan while the new one's plan is unknown. */
      const twice = 'twice@example.com';
      const heldTwice = { email: twice, plan: 'pro', status: 'active', stripe_customer: 'cus_TWICE', domain_addons: 0,
        event_at: new Date((t - 86400) * 1000).toISOString() };
      db.rows.set(twice, { ...heldTwice });
      const before4 = db.writes.length;
      const t2 = await deliver(realCheckout({ email: twice, customer: 'cus_TWICE', paid: true, sessionAt: t - 60, eventAt: t }));
      check(t2.d.pending === true && db.writes.length === before4 && JSON.stringify(db.rows.get(twice)) === JSON.stringify(heldTwice),
        `a second checkout naming no plan leaves a live row as it is: ${JSON.stringify(t2.d)}`);

      /* BUG-S: customer.subscription.created at the same second as an
         updated. Nothing happens to a subscription before it is created, so
         the updated is the later one; with an `lte` guard, a created
         delivered after it landed on the tie and lowered a live row to
         incomplete. */
      const tie = 'tie@example.com';
      await deliver(realCheckout({ email: tie, customer: 'cus_TIE', paid: true, sessionAt: t - 60, eventAt: t - 30 }));
      const u1 = await deliver(realSubscription({ type: 'customer.subscription.updated', customer: 'cus_TIE',
        price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t }));
      check(u1.d.acted === true && db.rows.get(tie).status === 'active', `the updated (active) lands: ${JSON.stringify(db.rows.get(tie))}`);
      const cr = await deliver(realSubscription({ customer: 'cus_TIE', price: ENV.STRIPE_PRICE_PRO, status: 'incomplete', eventAt: t }));
      check(cr.status === 200 && cr.d.stale === true && db.rows.get(tie).status === 'active',
        `a created of the same second, delivered after it, is stale and does not lower the row: ${JSON.stringify(cr.d)} ${db.rows.get(tie).status}`);
      const u2 = await deliver(realSubscription({ type: 'customer.subscription.updated', customer: 'cus_TIE',
        price: ENV.STRIPE_PRICE_PRO, status: 'past_due', eventAt: t }));
      check(u2.d.acted === true && db.rows.get(tie).status === 'past_due',
        `while an updated of the same second still applies (lte, so a redelivery is never lost): ${db.rows.get(tie).status}`);

      /* The same rule between kept events, before any row exists. */
      const tie2 = 'tie-first@example.com';
      await deliver(realSubscription({ type: 'customer.subscription.updated', customer: 'cus_TIE2',
        price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t - 10 }));
      const cr2 = await deliver(realSubscription({ customer: 'cus_TIE2', price: ENV.STRIPE_PRICE_PRO, status: 'incomplete', eventAt: t - 10 }));
      check(cr2.status === 200 && db.pending.get('cus_TIE2')?.status === 'active',
        `a kept updated is not replaced by a created of the same second: ${JSON.stringify(cr2.d)} ${JSON.stringify(db.pending.get('cus_TIE2'))}`);
      await deliver(realCheckout({ email: tie2, customer: 'cus_TIE2', paid: true, sessionAt: t - 90, eventAt: t - 5 }));
      check(db.rows.get(tie2)?.status === 'active' && db.rows.get(tie2)?.plan === 'pro',
        `and the checkout applies the later of the two: ${JSON.stringify(db.rows.get(tie2))}`);

      /* Bank transfer, subscription first: kept, applied to the unpaid
         checkout's row, and made live when the money clears. */
      const sepa = 'sepa@example.com';
      await deliver(realSubscription({ customer: 'cus_SEPA', price: ENV.STRIPE_PRICE_PRO, status: 'incomplete', eventAt: t - 3 }));
      await deliver(realCheckout({ email: sepa, customer: 'cus_SEPA', paid: false, sessionAt: t - 200, eventAt: t }));
      check(db.rows.get(sepa)?.plan === 'pro' && db.rows.get(sepa)?.status === 'incomplete' && db.rows.get(sepa)?.stripe_customer === 'cus_SEPA',
        `a transfer still clearing: the row names the plan and the customer, and is not live: ${JSON.stringify(db.rows.get(sepa))}`);
      await deliver(realCheckout({ type: 'checkout.session.async_payment_succeeded', email: sepa, customer: 'cus_SEPA',
        paid: true, sessionAt: t - 200, eventAt: t + 86400 }));
      check(db.rows.get(sepa).plan === 'pro' && db.rows.get(sepa).status === 'active',
        `and the money clearing makes it live, still Pro: ${JSON.stringify(db.rows.get(sepa))}`);

      /* A kept event from before this checkout was opened belongs to an
         earlier subscription of the same customer: not applied, and dropped. */
      const oldp = 'old-pending@example.com';
      await deliver(realSubscription({ type: 'customer.subscription.updated', customer: 'cus_OLDP',
        price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t - 7200 }));
      check(db.pending.has('cus_OLDP'), 'an event for a customer with no row is kept');
      await deliver(realCheckout({ email: oldp, customer: 'cus_OLDP', paid: true, sessionAt: t - 60, eventAt: t }));
      check(db.rows.get(oldp)?.status === 'incomplete' && db.rows.get(oldp)?.plan === 'base' && !db.pending.has('cus_OLDP'),
        `one raised before the buyer opened this checkout is not applied to it, and is dropped: ${JSON.stringify(db.rows.get(oldp))}`);

      /* Cancelled before the checkout event arrived: the row is written as
         ended, never live. */
      const quit = 'quit@example.com';
      await deliver(realSubscription({ customer: 'cus_QUIT', price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t - 50 }));
      await deliver({ ...realSubscription({ type: 'customer.subscription.deleted', customer: 'cus_QUIT',
        price: ENV.STRIPE_PRICE_PRO, status: 'canceled', eventAt: t - 10 }) });
      await deliver(realCheckout({ email: quit, customer: 'cus_QUIT', paid: true, sessionAt: t - 90, eventAt: t - 40 }));
      check(db.rows.get(quit)?.status === 'canceled' && !LIVE_STATUSES.includes(db.rows.get(quit).status),
        `a subscription deleted before its checkout event landed is not made live by it: ${JSON.stringify(db.rows.get(quit))}`);

      /* The one case still left to Stripe's retry: a database where
         database.sql section 6 never ran has nowhere to keep the event. */
      db.state.pendingMissing = true;
      const noTable = await deliver(realSubscription({ customer: 'cus_NOTABLE', price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t - 2 }));
      check(noTable.status === 409, `without pending_subscriptions, a subscription event before its row is 409, so Stripe retries: ${noTable.status}`);
      await new Promise(res => setTimeout(res, 200));
      check(/database\.sql section 6/.test(s.log()) && !s.log().includes('cus_NOTABLE'),
        'the server log says which section to run, and names no customer');
      const nt = 'no-table@example.com';
      const ntc = await deliver(realCheckout({ email: nt, customer: 'cus_NOTABLE', paid: true, sessionAt: t - 90, eventAt: t }));
      check(ntc.status === 200 && db.rows.get(nt)?.status === 'incomplete',
        `and a checkout still records its row there, as before: ${ntc.status} ${JSON.stringify(db.rows.get(nt))}`);
      const ntRetry = await deliver(realSubscription({ customer: 'cus_NOTABLE', price: ENV.STRIPE_PRICE_PRO, status: 'active', eventAt: t - 2 }));
      check(ntRetry.d.acted === true && db.rows.get(nt).plan === 'pro' && db.rows.get(nt).status === 'active',
        `which Stripe's retry then completes: ${JSON.stringify(db.rows.get(nt))}`);
      db.state.pendingMissing = false;
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
