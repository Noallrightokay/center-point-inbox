/* Delivery orders, for tests/apple.test.mjs.

   Apple sends each App Store Server Notification until it is answered 200,
   for days, so the notifications about one subscription can arrive in any
   order and more than once, and the phone's purchases arrive in between. A
   notification answered 500 wrote nothing and comes again later, so a
   retry is the same notification arriving later in the order. Whatever
   the order, the row must end exactly as it would had each arrived once,
   in the order Apple signed them (lib/apple.js, appleWrite).

   This runs the lib's own appleChange, applePurchase and appleWrite, as the
   two routes do once Apple's signature has been checked, over:
   - every order of every set of up to five of eight events about one
     period: a renewal, the phone's purchase, a refund, a billing grace
     period starting, a change of renewal preference during it, the refund
     reversed, the grace period ending and the subscription expiring; each
     order then delivers its first and last events again;
   - every order of three small sets: two that cross from one period to the
     next (one of them an upgrade from a period paid for in advance, which
     ends after the one that replaced it), and one where the phone's
     purchase, signed after a grace notice, arrives before it.
   The answer for each set is that set delivered in signing order, and a
   few of those answers are also checked against what they must say.

   `sb` is anything with supabase-js's `from()`: the in-memory stand-in, or
   a real PostgREST over a real Postgres (APPLE_TEST_POSTGREST in
   tests/apple.test.mjs). Each order writes its own subscription, under an
   id no other run uses. */
import * as apple from '../lib/apple.js';

const DAY = 86400e3;
const PRO = 'org.mailrata.pro.monthly';
const BASE = 'org.mailrata.base.monthly';
const FIELDS = ['product_id', 'plan', 'environment', 'period_from', 'expires_at', 'grace_until', 'grace_signed_at', 'revoked_at', 'signed_at'];
const TIMES = new Set(['period_from', 'expires_at', 'grace_until', 'grace_signed_at', 'revoked_at', 'signed_at']);

let made = 0;
const RUN = String(Date.now());
const freshId = () => `5${RUN}${String(++made).padStart(7, '0')}`;

/* A transaction of the subscription `id`, as Apple signs it at `signed`. */
const txOf = (id, signed, over) => ({
  transactionId: `${id}${String(Math.round(signed / 1000) % 100000)}`, originalTransactionId: id, bundleId: 'org.mailrata.app',
  productId: PRO, type: 'Auto-Renewable Subscription', inAppOwnershipType: 'PURCHASED', environment: 'Production', signedDate: signed, ...over,
});
/* A notification, carrying renewal info when `grace` is given (null for
   renewal info with no grace period in it, undefined for none at all). */
function note(name, type, subtype, signed, tx, grace) {
  const renewal = grace === undefined ? null : { autoRenewStatus: 1, ...(grace === null ? {} : { gracePeriodExpiresDate: grace }) };
  return { name, signed, change: (id) => apple.appleChange({ notificationType: type, ...(subtype ? { subtype } : {}), signedDate: signed }, txOf(id, signed, tx), renewal, 'Production') };
}
const phone = (name, signed, tx) => ({ name, signed, change: (id) => apple.applePurchase(txOf(id, signed, tx), 'Production') });

function* subsets(list, most) {
  const n = list.length;
  for (let mask = 1; mask < 1 << n; mask++) {
    const pick = list.filter((_, i) => mask & (1 << i));
    if (pick.length <= most) yield pick;
  }
}
function* orders(list) {
  if (list.length <= 1) { yield list; return; }
  for (let i = 0; i < list.length; i++) {
    for (const rest of orders([...list.slice(0, i), ...list.slice(i + 1)])) yield [list[i], ...rest];
  }
}

const shown = (s) => (s ? FIELDS.map((f) => `${f}=${s[f] === null ? '-' : TIMES.has(f) ? new Date(s[f]).toISOString().slice(0, 10) : s[f]}`).join(' ') : 'no row');

export async function deliveryOrders({ sb, check, label, width = 1 }) {
  let deliveries = 0;
  const deliver = async (id, e) => {
    deliveries += 1;
    const w = await apple.appleWrite(sb, id, e.change(id));
    if (w.error) throw new Error(`${e.name}: ${w.error.code || ''} ${w.error.message || ''}`);
  };
  const state = async (id) => {
    const { data, error } = await sb.from('apple_subscriptions').select(FIELDS.join(',')).eq('original_transaction_id', id).maybeSingle();
    if (error) throw new Error(`reading the row: ${error.code || ''} ${error.message || ''}`);
    if (!data) return null;
    return Object.fromEntries(FIELDS.map((f) => [f, data[f] === null || data[f] === undefined ? null : TIMES.has(f) ? Date.parse(data[f]) : String(data[f])]));
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  /* Each set: its answer in signing order, then every order of it, with
     its first and last events delivered again at the end, `width` orders
     at a time. */
  const settle = async (sets) => {
    const wrong = [];
    let count = 0;
    const answers = new Map();
    for (const set of sets) {
      const id = freshId();
      for (const e of [...set].sort((a, b) => a.signed - b.signed)) await deliver(id, e);
      answers.set(set, await state(id));
    }
    const jobs = [];
    for (const set of sets) for (const order of orders(set)) jobs.push([set, order]);
    let next = 0;
    const worker = async () => {
      while (next < jobs.length) {
        const [set, order] = jobs[next++];
        const id = freshId();
        for (const e of [...order, order[0], order[order.length - 1]]) await deliver(id, e);
        count += 1;
        const got = await state(id);
        if (!same(got, answers.get(set)) && wrong.length < 3) {
          wrong.push(`${order.map((e) => e.name).join(' > ')}: ${shown(got)}; in signing order: ${shown(answers.get(set))}`);
        }
      }
    };
    await Promise.all(Array.from({ length: width }, worker));
    return { wrong, count, answers };
  };

  /* One period, P, and what Apple signs about it, in this order. The
     refund is in force from day 10 until it is reversed on day 40, so
     what Apple signs in between carries its revocationDate; the grace
     period starts when P runs out, on day 30, and ends on day 46. */
  const T0 = Date.now() - 60 * DAY;
  const P = { purchaseDate: T0, expiresDate: T0 + 30 * DAY };
  const refundAt = T0 + 10 * DAY;
  const refunded = { ...P, revocationDate: refundAt, revocationReason: 0 };
  const G = T0 + 46 * DAY;
  const period = [
    note('DID_RENEW', 'DID_RENEW', null, T0, P, null),
    phone('phone', T0 + 5 * DAY, P),
    note('REFUND', 'REFUND', null, refundAt, refunded, null),
    note('GRACE', 'DID_FAIL_TO_RENEW', 'GRACE_PERIOD', T0 + 30 * DAY, refunded, G),
    note('PREF', 'DID_CHANGE_RENEWAL_PREF', 'DOWNGRADE', T0 + 32 * DAY, refunded, G),
    note('REVERSED', 'REFUND_REVERSED', null, T0 + 40 * DAY, P, G),
    note('GRACE_EXPIRED', 'GRACE_PERIOD_EXPIRED', null, T0 + 46 * DAY, P, G),
    note('EXPIRED', 'EXPIRED', 'BILLING_RETRY', T0 + 48 * DAY, P, null),
  ];
  const sets = [...subsets(period, 5)];
  const one = await settle(sets);
  check(one.wrong.length === 0,
    `${label}one period: all ${one.count} orders of the ${sets.length} sets of up to five of its eight events, each with two deliveries again, end as signing order does${one.wrong.length ? `; not: ${one.wrong.join(' | ')}` : ''}`);
  const answer = (...names) => one.answers.get(sets.find((s) => s.length === names.length && names.every((n) => s.some((e) => e.name === n))));
  const a1 = answer('REFUND', 'GRACE');
  check(!!a1 && a1.revoked_at === refundAt && a1.grace_until === G,
    `${label}a refund and a later grace notice: revoked, and in grace (${shown(a1)})`);
  const a2 = answer('GRACE', 'PREF');
  check(!!a2 && a2.grace_until === G && a2.signed_at === T0 + 32 * DAY && a2.grace_signed_at === T0 + 32 * DAY,
    `${label}a grace notice and a later change of renewal preference: still in grace (${shown(a2)})`);
  const a3 = answer('REVERSED', 'GRACE_EXPIRED');
  check(!!a3 && a3.revoked_at === null && a3.grace_until === G, `${label}a reversal and a later notice: not revoked (${shown(a3)})`);
  const a4 = answer('DID_RENEW', 'phone');
  check(!!a4 && a4.grace_until === null && a4.grace_signed_at === T0 && a4.signed_at === T0 + 5 * DAY,
    `${label}the phone's purchase stamps the subscription, never the grace period (${shown(a4)})`);

  /* Across periods. An upgrade from Base paid three months in advance
     (P1, to day 90) to Pro monthly (P2, from day 10 to day 40), the phone
     recording P2, a refund of P1 signed after the upgrade, and P2's grace
     period. And a monthly period in grace, recovered by a renewal (P2)
     that carries no renewal info, with P1's refund and the end of P1's
     grace period signed after it. Then a period whose grace notice is
     followed by the phone's purchase, StoreKit having signed P's
     transaction again after it. */
  const P1 = { productId: BASE, purchaseDate: T0, expiresDate: T0 + 90 * DAY, offerType: 1, offerDiscountType: 'PAY_UP_FRONT' };
  const P2 = { productId: PRO, purchaseDate: T0 + 10 * DAY, expiresDate: T0 + 40 * DAY };
  const upgrade = [
    note('P1 SUBSCRIBED', 'SUBSCRIBED', 'INITIAL_BUY', T0, P1, null),
    note('P2 UPGRADE', 'DID_CHANGE_RENEWAL_PREF', 'UPGRADE', T0 + 10 * DAY, P2, null),
    phone('P2 phone', T0 + 15 * DAY, P2),
    note('P1 REFUND', 'REFUND', null, T0 + 20 * DAY, { ...P1, revocationDate: T0 + 20 * DAY, revocationReason: 0 }, null),
    note('P2 GRACE', 'DID_FAIL_TO_RENEW', 'GRACE_PERIOD', T0 + 40 * DAY, P2, T0 + 56 * DAY),
  ];
  const M1 = { purchaseDate: T0, expiresDate: T0 + 30 * DAY };
  const M2 = { purchaseDate: T0 + 35 * DAY, expiresDate: T0 + 65 * DAY };
  const recovered = [
    note('P1 GRACE', 'DID_FAIL_TO_RENEW', 'GRACE_PERIOD', T0 + 30 * DAY, M1, T0 + 46 * DAY),
    note('P2 RECOVERY', 'DID_RENEW', 'BILLING_RECOVERY', T0 + 35 * DAY, M2, undefined),
    note('P1 REFUND', 'REFUND', null, T0 + 38 * DAY, { ...M1, revocationDate: T0 + 38 * DAY, revocationReason: 0 }, null),
    note('P1 GRACE_EXPIRED', 'GRACE_PERIOD_EXPIRED', null, T0 + 46 * DAY, M1, T0 + 46 * DAY),
  ];
  const resigned = [
    note('DID_RENEW', 'DID_RENEW', null, T0, M1, null),
    note('GRACE', 'DID_FAIL_TO_RENEW', 'GRACE_PERIOD', T0 + 30 * DAY, M1, T0 + 46 * DAY),
    phone('phone', T0 + 31 * DAY, M1),
  ];
  const cross = await settle([upgrade, recovered, resigned]);
  check(cross.wrong.length === 0,
    `${label}across periods: all ${cross.count} orders of an upgrade, a recovery and a re-signed purchase, each with two deliveries again, end as signing order does${cross.wrong.length ? `; not: ${cross.wrong.join(' | ')}` : ''}`);
  const u = cross.answers.get(upgrade);
  check(!!u && u.period_from === P2.purchaseDate && u.plan === 'pro' && u.expires_at === P2.expiresDate && u.revoked_at === null && u.grace_until === T0 + 56 * DAY,
    `${label}the upgrade: Pro, P2, not revoked by P1's refund, in P2's grace period (${shown(u)})`);
  const r = cross.answers.get(recovered);
  check(!!r && r.period_from === M2.purchaseDate && r.revoked_at === null && r.grace_until === null && r.grace_signed_at === 0,
    `${label}the recovery: P2, whose grace fields started afresh, untouched by P1's refund and grace (${shown(r)})`);
  const s = cross.answers.get(resigned);
  check(!!s && s.grace_until === T0 + 46 * DAY && s.signed_at === T0 + 31 * DAY,
    `${label}the purchase StoreKit signed after the grace notice does not hide it (${shown(s)})`);
  return { orders: one.count + cross.count, deliveries };
}
