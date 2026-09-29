import { createHmac, timingSafeEqual } from 'node:crypto';
import { LIVE_STATUSES } from './plan.js';

/* ---------------------------------------------------------------------------
   Stripe, without the SDK.

   RATA sells through Payment Links, so the app never creates a charge, never
   renders a card form and never holds a Stripe secret key — Stripe hosts all
   of that. What it needs is the other direction: Stripe telling the server
   that somebody paid, so the plan is recorded somewhere the server trusts.

   That is one signed POST. Verifying it is forty lines of HMAC, which is a
   better trade than a megabyte of SDK for a single endpoint, and it means the
   verification is exercised by the tests rather than taken on faith.

   Nothing here calls the Stripe API. Every field these events need is carried
   in the event itself, which also means a webhook cannot be turned into a
   request loop against Stripe by a malformed payload.

   Which event knows what. A Checkout Session names the buyer's address, the
   customer and whether the money is in, but not what was bought: its
   line_items come only when the session is retrieved with
   expand[]=line_items, and a webhook event is never expanded (and RATA holds
   no secret key to retrieve it with). The subscription events carry the
   prices, on items.data[].price. So the plan and the add-ons come from
   customer.subscription.created/updated, and a checkout event writes them
   only when it does carry line items, which a real delivery never does.
   --------------------------------------------------------------------------- */

/* Stripe signs `${timestamp}.${raw body}` with the endpoint's signing secret
   and sends `t=…,v1=…` — sometimes several v1 values during a secret roll, so
   any one matching is a pass. */
export function verifySignature(rawBody, header, secret, nowSec = Math.floor(Date.now() / 1000)) {
  if (!secret) return { ok: false, error: 'STRIPE_WEBHOOK_SECRET is not set' };
  if (!header) return { ok: false, error: 'missing Stripe-Signature header' };

  const parts = Object.create(null);
  const v1 = [];
  for (const piece of String(header).split(',')) {
    const i = piece.indexOf('=');
    if (i < 0) continue;
    const k = piece.slice(0, i).trim(), v = piece.slice(i + 1).trim();
    if (k === 'v1') v1.push(v); else parts[k] = v;
  }

  const t = Number(parts.t);
  if (!Number.isFinite(t)) return { ok: false, error: 'signature header has no timestamp' };

  /* A signature stays valid forever without this: an attacker who captured one
     delivery could replay it indefinitely. Five minutes is Stripe's own
     recommended tolerance. */
  if (Math.abs(nowSec - t) > 300) return { ok: false, error: 'signature timestamp outside tolerance' };

  const expected = createHmac('sha256', secret).update(`${t}.${rawBody}`, 'utf8').digest();
  const match = v1.some(sig => {
    let given;
    try { given = Buffer.from(sig, 'hex'); } catch { return false; }
    return given.length === expected.length && timingSafeEqual(given, expected);
  });

  return match ? { ok: true } : { ok: false, error: 'signature did not match' };
}

/* Which plan a Stripe price belongs to. The price ids come from the
   environment rather than being guessed from the amount: two plans can share a
   price, prices change, and a mistake here would hand somebody the wrong tier. */
export function planForPrice(priceId, env = process.env) {
  if (!priceId) return null;
  /* Compared one at a time rather than through an object literal: unset
     variables would otherwise collide on a shared placeholder key, and the
     last one written would answer for all of them. */
  if (env.STRIPE_PRICE_BASE && priceId === env.STRIPE_PRICE_BASE) return 'base';
  if (env.STRIPE_PRICE_PRO && priceId === env.STRIPE_PRICE_PRO) return 'pro';
  if (env.STRIPE_PRICE_ENTERPRISE && priceId === env.STRIPE_PRICE_ENTERPRISE) return 'enterprise';
  return null;
}

/* Which Stripe statuses are entitled (active, trialing, past_due): defined in
   lib/plan.js, which the licence routes read, so the webhook and the licence
   can never disagree about who has paid. */
export { LIVE_STATUSES };

/* A checkout's payment_status that means the money is in: "paid", or
   "no_payment_required" (a full discount, a free trial). An absent one is
   treated as unpaid. */
const PAID = ['paid', 'no_payment_required'];

/* What a checkout still waiting for its money is recorded as: Stripe's word
   for a subscription whose first payment has not cleared. Not live. */
export const PENDING = 'incomplete';

/* Whether a checkout, keyed by address, would take over a live subscription
   that belongs to a different Stripe customer.

   Stripe's checkout lets the buyer type any address. Written blindly, a
   second customer's checkout replaces a paying customer's plan and customer
   id; cancelling it then cancels them, and their own subscription events
   match no row. So a live row owned by another customer is never replaced.
   The same customer checking out again updates their own row; a row with no
   customer (minted by hand) is claimed by the first checkout; a row whose
   subscription has ended may be taken by a new one. A checkout with no
   customer id at all counts as another customer: it would strip the live
   one's id.

   A row still waiting for a bank transfer (PENDING) is held too (review
   P3-5): taken over by another customer's card checkout, the transfer
   clearing would be refused as a conflict and the first customer's
   subscription events would match no row, so they would have paid for
   nothing. An abandoned transfer does not lock the address for good: Stripe
   ends the subscription as incomplete_expired, keyed by customer, which is
   not held. */
export function checkoutConflict(held, row) {
  if (!held || !held.stripe_customer) return false;
  const status = String(held.status || '').toLowerCase();
  if (!LIVE_STATUSES.includes(status) && status !== PENDING) return false;
  return held.stripe_customer !== (row && row.stripe_customer);
}

/* What to write for an event, or null for one we do not act on.

   Only the events that change entitlement are handled. Everything else gets a
   200 and is ignored: Stripe retries anything it does not get a 200 for, so
   answering "understood, nothing to do" is the correct reply to the ninety
   other event types an account emits. */
export function rowForEvent(event, env = process.env) {
  const type = event?.type;
  const o = event?.data?.object;
  if (!type || !o) return null;

  /* When Stripe says this happened.

     Deliveries are not ordered and are retried for days, so an upgrade and the
     downgrade that followed it can arrive the wrong way round — and the row
     would end up holding whichever landed last rather than whichever happened
     last. Carrying the event's own timestamp lets the write refuse to go
     backwards. Stripe sends it in seconds. */
  const at = Number.isFinite(event?.created)
    ? new Date(event.created * 1000).toISOString()
    : null;
  /* Whether this object carries any lines at all. Without them an event
     knows nothing about the plan or the add-ons, and says nothing (null)
     rather than "Base" and "none". */
  const lined = linesOf(o).length > 0;

  if (type === 'checkout.session.completed' || type === 'checkout.session.async_payment_succeeded') {
    /* The only events that reliably carry the buyer's email, which is the key
       the app looks them up by. */
    const email = (o.customer_details?.email || o.customer_email || '').trim().toLowerCase();
    if (!email) return null;
    if (o.status && o.status !== 'complete') return null;
    /* A checkout paid by a delayed method (SEPA, ACH, Boleto) completes
       before the money arrives, with payment_status "unpaid". It is recorded,
       so the customer's later subscription events find their row, but as
       Stripe's own "incomplete", which is not entitled (LIVE_STATUSES):
       async_payment_succeeded, or the subscription turning active, makes it
       live. async_payment_failed needs nothing, since it never was. */
    const paid = PAID.includes(o.payment_status);
    return {
      by: 'email',
      email,
      stripe_customer: typeof o.customer === 'string' ? o.customer : (o.customer?.id || null),
      /* A delivery carries no line_items (see the top of this file), so this
         is null and the route leaves the plan to the subscription events.
         Writing 'base' here recorded every Pro checkout as Base, and
         async_payment_succeeded, days later, turned a Pro customer back into
         Base (review part 3). */
      plan: planForPrice(priceOf(o, env), env),
      domain_addons: lined ? domainAddonsOf(o, env) : null,
      status: paid ? 'active' : PENDING,
      event_at: at,
      /* What the row is stamped with: when the buyer opened this checkout
         (the session's own `created`), not when the event was raised. Every
         subscription event of this purchase happens after that, so none is
         refused as stale because its checkout event, or the payment clearing
         days later, happened to be raised after it; and that is what
         brought the plan in. The event's time still decides whether this
         event is stale itself. */
      stamp_at: Number.isFinite(o.created) ? new Date(o.created * 1000).toISOString() : at,
    };
  }

  if (type === 'customer.subscription.created' || type === 'customer.subscription.updated') {
    const customer = typeof o.customer === 'string' ? o.customer : (o.customer?.id || null);
    if (!customer) return null;
    /* Keyed by customer, because a subscription event carries no email: the row
       was created by the checkout event above, and this updates it. */
    return {
      by: 'customer',
      stripe_customer: customer,
      plan: planForPrice(priceOf(o, env), env),
      domain_addons: lined ? domainAddonsOf(o, env) : null,
      status: LIVE_STATUSES.includes(o.status) ? o.status : (o.status || 'canceled'),
      event_at: at,
    };
  }

  if (type === 'customer.subscription.deleted') {
    const customer = typeof o.customer === 'string' ? o.customer : (o.customer?.id || null);
    if (!customer) return null;
    return { by: 'customer', stripe_customer: customer, plan: null, domain_addons: 0, status: 'canceled', event_at: at };
  }

  return null;
}

/* Every line on the subscription, not just the first.

   Taking items.data[0] was safe while a subscription had exactly one line. The
   domain add-on makes that false: Stripe does not promise an order, so a
   subscription carrying "Pro" and "your own domain" could arrive add-on first,
   and the plan would be read as null and cleared — downgrading a paying
   customer because they bought something extra. */
function linesOf(o) {
  return o?.items?.data || o?.line_items?.data || (o?.plan ? [{ price: { id: o.plan.id }, quantity: 1 }] : []);
}

/* The plan line, wherever it sits. */
function priceOf(o, env) {
  for (const line of linesOf(o)) {
    const id = line?.price?.id;
    if (id && planForPrice(id, env)) return id;
  }
  return null;
}

/* How many custom domains were bought, as a quantity on the add-on line. */
export function domainAddonsOf(o, env = process.env) {
  const want = env.STRIPE_PRICE_DOMAIN;
  if (!want) return 0;
  let n = 0;
  for (const line of linesOf(o)) {
    if (line?.price?.id === want) n += Number(line.quantity) || 1;
  }
  return n;
}

/* Should this event be written over what is already stored?

   Only when it is newer than whatever last wrote the row. A row with no
   timestamp predates this guard and is always overwritten; an event with no
   timestamp is assumed current, since refusing it would be worse than a rare
   out-of-order write. */
export function isNewer(eventAt, storedAt) {
  if (!storedAt) return true;
  if (!eventAt) return true;
  return new Date(eventAt).getTime() >= new Date(storedAt).getTime();
}

/* What a checkout event writes to its row.

   `held` is the row as read (stripe_customer, status, event_at); `row` is
   rowForEvent's answer. The route has already refused a stale event and a
   conflict; it refuses a PENDING write over a live row after this.

   The plan and the add-ons are written only when the event names them (line
   items, which a delivery never has), so nothing without price information
   ever lowers a plan a subscription event set. Otherwise:

   - The plan is known when this customer's row has been written by an event
     raised after the buyer opened this checkout (held.event_at later than
     stamp_at): only a subscription event of this purchase can be, and it
     named the plan. The row keeps plan and add-ons, and the checkout sets
     only whether the money is in.
   - Otherwise (a new row, another customer's ended row, or this customer's
     row from an earlier subscription) the plan is not known yet. The row
     starts from 'base' and no add-ons, what the table defaults to, and is
     PENDING even when paid: not entitled until the subscription event names
     the plan and makes it live. A live row with the default plan would get
     a Base licence from /account, good for 30 days, if the subscription
     event landed a few seconds later (it is raised first, so it often finds
     no row and waits for Stripe's retry). Waiting says "not yet"; the Base
     licence would have been wrong for a month.

   The row's stamp never moves backwards, and moves forward only to when the
   buyer opened the checkout (stamp_at). */
export function checkoutWrite(held, row, now = new Date().toISOString()) {
  const stamp = row.stamp_at === undefined ? row.event_at : row.stamp_at;
  const same = !!(held && held.stripe_customer && held.stripe_customer === row.stripe_customer);
  const named = !!(same && held.event_at && stamp && Date.parse(held.event_at) > Date.parse(stamp));
  const write = {
    email: row.email,
    status: row.status,
    stripe_customer: row.stripe_customer,
    event_at: later(held && held.event_at, stamp),
    updated_at: now,
  };
  if (row.plan) write.plan = row.plan;
  else if (!named) { write.plan = 'base'; write.status = PENDING; }
  if (typeof row.domain_addons === 'number') write.domain_addons = row.domain_addons;
  else if (!named) write.domain_addons = 0;
  return write;
}

function later(a, b) {
  if (!a) return b || null;
  if (!b) return a;
  return new Date(b).getTime() > new Date(a).getTime() ? b : a;
}
