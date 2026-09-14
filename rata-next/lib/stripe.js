import { createHmac, timingSafeEqual } from 'node:crypto';

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

/* Stripe's statuses, reduced to the question the app actually asks: is this
   person entitled right now? past_due is deliberately still entitled — a card
   that failed this morning should not lock someone out of their mail while
   Stripe retries it. */
export const LIVE_STATUSES = ['active', 'trialing', 'past_due'];

/* What to write for an event, or null for one we do not act on.

   Only the events that change entitlement are handled. Everything else gets a
   200 and is ignored: Stripe retries anything it does not get a 200 for, so
   answering "understood, nothing to do" is the correct reply to the ninety
   other event types an account emits. */
export function rowForEvent(event, env = process.env) {
  const type = event?.type;
  const o = event?.data?.object;
  if (!type || !o) return null;

  if (type === 'checkout.session.completed') {
    /* The only event that reliably carries the buyer's email, which is the key
       the app looks them up by. */
    const email = (o.customer_details?.email || o.customer_email || '').trim().toLowerCase();
    if (!email) return null;
    if (o.payment_status && o.payment_status !== 'paid' && o.status !== 'complete') return null;
    return {
      by: 'email',
      email,
      stripe_customer: typeof o.customer === 'string' ? o.customer : (o.customer?.id || null),
      plan: planForPrice(priceOf(o), env) || 'base',
      status: 'active',
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
      plan: planForPrice(priceOf(o), env),
      status: LIVE_STATUSES.includes(o.status) ? o.status : (o.status || 'canceled'),
    };
  }

  if (type === 'customer.subscription.deleted') {
    const customer = typeof o.customer === 'string' ? o.customer : (o.customer?.id || null);
    if (!customer) return null;
    return { by: 'customer', stripe_customer: customer, plan: null, status: 'canceled' };
  }

  return null;
}

function priceOf(o) {
  return o?.items?.data?.[0]?.price?.id
    || o?.line_items?.data?.[0]?.price?.id
    || o?.plan?.id
    || null;
}
