/* The Stripe webhook.

   This endpoint is what stands between "money arrived" and "the app knows it",
   and it is reachable by anyone on the internet who finds the URL. So the
   signature check gets the attention: a forged POST must not be able to grant
   somebody a plan, and a captured one must not be replayable later.

   Stripe is not called here. The events are built by hand in exactly the shape
   Stripe sends, which is also what lets the replay and tolerance cases be
   tested at all — they depend on controlling the clock. */
import { createHmac } from 'node:crypto';
import { startServer, makeChecker } from './helpers.mjs';
import { verifySignature, planForPrice, rowForEvent, domainAddonsOf, LIVE_STATUSES } from '../lib/stripe.js';

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
