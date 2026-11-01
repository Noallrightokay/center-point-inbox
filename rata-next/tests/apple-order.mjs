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
   - every order of seventeen small sets: two that cross from one period
     to the next (one of them an upgrade from a period paid for in
     advance, which ends after the one that replaced it); two where the
     period's renewal date is extended, with the phone's copy of it from
     before the extension and, in one, from after; one where the
     phone's purchase, signed after a grace notice, arrives before it;
     three where the phone sends its renewal info with its purchase,
     recording a grace period with no notification, then ended by one,
     then recovered by the next period; three where the phone's
     transaction and renewal info are of different moments (a lapsed
     transaction beside a renewal info signed after billing recovered,
     with and without the recovery arriving, and the next period's
     transaction beside the renewal info of the grace period before it);
     and six with a renewal Apple charged ahead of time, before its
     period began (an upgrade made after the charge, that period's grace
     period from Apple and from the phone alone, the phone's grace period
     after another device's copy, or Apple's notice, signed later placed
     the period later, and a refund of the period before it).
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
  transactionId: `${id}${String(Math.round(signed / 1000) % 100000)}`, originalTransactionId: id, bundleId: 'org.mailrata.mail',
  productId: PRO, type: 'Auto-Renewable Subscription', inAppOwnershipType: 'PURCHASED', environment: 'Production', signedDate: signed, ...over,
});
/* A notification, carrying renewal info when `grace` is given (null for
   renewal info with no grace period in it, undefined for none at all). */
function note(name, type, subtype, signed, tx, grace) {
  const renewal = grace === undefined ? null : { autoRenewStatus: 1, ...(grace === null ? {} : { gracePeriodExpiresDate: grace }) };
  return { name, signed, change: (id) => apple.appleChange({ notificationType: type, ...(subtype ? { subtype } : {}), signedDate: signed }, txOf(id, signed, tx), renewal, 'Production') };
}
const phone = (name, signed, tx) => ({ name, signed, change: (id) => apple.applePurchase(txOf(id, signed, tx), 'Production') });
/* The phone's purchase sent with its renewal info (`renewalInfos`), which
   StoreKit signs on its own, at `renewed`: with a grace period ending at
   `grace`, or null for none. */
const phoneWith = (name, signed, tx, renewed, grace) => ({
  name,
  signed: renewed,
  change: (id) => apple.applePurchase(txOf(id, signed, tx), 'Production',
    { originalTransactionId: id, autoRenewStatus: 1, signedDate: renewed, environment: 'Production', ...(grace === null ? {} : { gracePeriodExpiresDate: grace }) }),
});

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
    /* What changes nothing RATA keeps is answered and left, as the
       notifications route leaves it. */
    const change = e.change(id);
    if (!change) return;
    const w = await apple.appleWrite(sb, id, change);
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
  /* The phone alone, with no notification but where it says: its purchase
     of M1 while live, then M1 again past its expiry with the renewal info
     StoreKit signed on day 29 (no grace period yet) and on day 31 (a grace
     period to day 46); the same with Apple's EXPIRED notice of day 48,
     which ends the grace period; and the grace period recovered by M2,
     whose purchase the phone sends too. */
  const alone = [
    phone('M1 phone', T0 + 5 * DAY, M1),
    phoneWith('M1 phone, day 29', T0 + 5 * DAY, M1, T0 + 29 * DAY, null),
    phoneWith('M1 phone in grace', T0 + 5 * DAY, M1, T0 + 31 * DAY, T0 + 46 * DAY),
  ];
  const aloneEnded = [...alone, note('M1 EXPIRED', 'EXPIRED', 'BILLING_RETRY', T0 + 48 * DAY, M1, null)];
  const aloneRecovered = [
    phoneWith('M1 phone in grace', T0 + 5 * DAY, M1, T0 + 31 * DAY, T0 + 46 * DAY),
    note('M2 RECOVERY', 'DID_RENEW', 'BILLING_RECOVERY', T0 + 35 * DAY, M2, undefined),
    phone('M2 phone', T0 + 36 * DAY, M2),
  ];
  /* The phone's transaction and renewal info need not be of one moment:
     StoreKit's transactions are its local copy, the renewal info is
     fetched from Apple, and neither names a period (lib/apple.js,
     appleGraceFor). M1 in grace by Apple's notice, and the phone still
     sending M1 beside a renewal info Apple signed on day 36, after billing
     recovered: no grace period, since M2 runs. Then the same with M2's
     recovery arriving. And M2's transaction beside M1's renewal info of
     day 31, whose grace period ends on day 46, before M2 does. Neither
     renewal info is the period's beside it, so neither is used. */
  const lagging = [
    note('M1 GRACE', 'DID_FAIL_TO_RENEW', 'GRACE_PERIOD', T0 + 30 * DAY, M1, T0 + 46 * DAY),
    phoneWith('M1 phone, renewal info after recovery', T0 + 5 * DAY, M1, T0 + 36 * DAY, null),
  ];
  const laggingRecovered = [...lagging, note('M2 RECOVERY', 'DID_RENEW', 'BILLING_RECOVERY', T0 + 35 * DAY, M2, undefined)];
  const oldInfo = [
    phoneWith('M1 phone in grace', T0 + 5 * DAY, M1, T0 + 31 * DAY, T0 + 46 * DAY),
    phoneWith('M2 phone, M1\'s renewal info', T0 + 36 * DAY, M2, T0 + 31 * DAY, T0 + 46 * DAY),
  ];
  /* A renewal Apple charged ahead of time. Base runs to day 30 (B1), and
     Apple charges its renewal (B2, days 30 to 60) eight hours before
     that, reporting it then with B2's purchaseDate still to come; the
     phone gets B2 an hour later. Two hours after the charge the customer
     upgrades to Pro (U), which begins at once and replaces B2. By
     purchaseDate alone B2 would outrank U, and the customer would have
     Base until U's renewal; B2 is placed when Apple reported it instead
     (lib/apple.js, periodFrom), so U is the later. Then B2 renewed as it
     was and in grace at its end, from Apple and from the phone alone;
     the phone's grace period after a copy of B2 signed later than the
     phone's placed the row later: the Mac's (Universal Purchase), signed
     once B2 had begun, or Apple's DID_RENEW, signed after the phone's but
     before B2 began, with Apple's own grace notice not yet delivered;
     and a refund of B1, signed after B2 was charged, which is about the
     period before and changes nothing. */
  const H = 3600e3;
  const B1 = { productId: BASE, purchaseDate: T0, expiresDate: T0 + 30 * DAY };
  const B2 = { productId: BASE, purchaseDate: T0 + 30 * DAY, expiresDate: T0 + 60 * DAY, transactionReason: 'RENEWAL' };
  const U = { productId: PRO, purchaseDate: T0 + 30 * DAY - 6 * H, expiresDate: T0 + 60 * DAY - 6 * H };
  const charged = note('B2 DID_RENEW, charged ahead', 'DID_RENEW', null, T0 + 30 * DAY - 8 * H, B2, null);
  const chargedPhone = phone('B2 phone, charged ahead', T0 + 30 * DAY - 7 * H, B2);
  const aheadUpgrade = [
    note('B1 SUBSCRIBED', 'SUBSCRIBED', 'INITIAL_BUY', T0, B1, null),
    charged,
    chargedPhone,
    note('U UPGRADE', 'DID_CHANGE_RENEWAL_PREF', 'UPGRADE', T0 + 30 * DAY - 6 * H, U, null),
    phone('U phone', T0 + 30 * DAY - 5 * H, U),
  ];
  const BG = T0 + 76 * DAY;
  const chargedInGrace = phoneWith('B2 phone in grace', T0 + 30 * DAY - 7 * H, B2, T0 + 61 * DAY, BG);
  const aheadGrace = [charged, chargedPhone, note('B2 GRACE', 'DID_FAIL_TO_RENEW', 'GRACE_PERIOD', T0 + 60 * DAY, B2, BG), chargedInGrace];
  const aheadAlone = [chargedPhone, chargedInGrace];
  const aheadTwoDevices = [chargedPhone, phone('B2 Mac, once B2 began', T0 + 31 * DAY, B2), chargedInGrace];
  const aheadRenewedLater = [chargedPhone, note('B2 DID_RENEW, after the phone', 'DID_RENEW', null, T0 + 30 * DAY - 6 * H, B2, null), chargedInGrace];
  const aheadRefund = [
    note('B1 SUBSCRIBED', 'SUBSCRIBED', 'INITIAL_BUY', T0, B1, null),
    charged,
    note('B1 REFUND', 'REFUND', null, T0 + 30 * DAY - 2 * H, { ...B1, revocationDate: T0 + 30 * DAY - 2 * H, revocationReason: 0 }, null),
  ];
  /* A renewal date extended (RENEWAL_EXTENDED): on day 20 M1's end moves
     from day 30 to day 40, and Apple's transaction is M1 with the later
     expiry and the same purchaseDate. The phone's copy of M1 from before
     the extension, signed on day 5, and in the second set its copy of
     the extended M1, signed on day 21. */
  const M1x = { ...M1, expiresDate: T0 + 40 * DAY };
  const extended = [
    note('M1 SUBSCRIBED', 'SUBSCRIBED', 'INITIAL_BUY', T0, M1, null),
    phone('M1 phone', T0 + 5 * DAY, M1),
    note('M1 RENEWAL_EXTENDED', 'RENEWAL_EXTENDED', null, T0 + 20 * DAY, M1x, null),
  ];
  const extendedPhone = [...extended, phone('M1 phone, extended', T0 + 21 * DAY, M1x)];
  const cross = await settle([upgrade, recovered, extended, extendedPhone, resigned, alone, aloneEnded, aloneRecovered, lagging, laggingRecovered, oldInfo, aheadUpgrade, aheadGrace, aheadAlone, aheadTwoDevices, aheadRenewedLater, aheadRefund]);
  check(cross.wrong.length === 0,
    `${label}across periods: all ${cross.count} orders of an upgrade, a recovery, a renewal date extended, a re-signed purchase, the phone alone in a grace period, the phone's two halves of different moments and a renewal charged ahead of time (with two devices, and with a notice signed after the phone's copy), each with two deliveries again, end as signing order does${cross.wrong.length ? `; not: ${cross.wrong.join(' | ')}` : ''}`);
  const ex = cross.answers.get(extended);
  check(!!ex && ex.period_from === M1.purchaseDate && ex.expires_at === M1x.expiresDate && ex.signed_at === T0 + 20 * DAY && ex.revoked_at === null,
    `${label}a renewal date extended: the later expiry, which the phone's copy from before the extension does not undo (${shown(ex)})`);
  const ep = cross.answers.get(extendedPhone);
  check(!!ep && ep.period_from === M1.purchaseDate && ep.expires_at === M1x.expiresDate && ep.signed_at === T0 + 21 * DAY,
    `${label}and the phone's copy of the extended period, signed after it, stamps it (${shown(ep)})`);
  const au = cross.answers.get(aheadUpgrade);
  check(!!au && au.plan === 'pro' && au.product_id === PRO && au.period_from === U.purchaseDate && au.expires_at === U.expiresDate && au.revoked_at === null
    && au.signed_at === T0 + 30 * DAY - 5 * H,
    `${label}an upgrade made after Apple charged the Base renewal ahead of time: Pro, from the upgrade (${shown(au)})`);
  const ag = cross.answers.get(aheadGrace);
  check(!!ag && ag.plan === 'base' && ag.period_from === B2.purchaseDate && ag.expires_at === B2.expiresDate && ag.grace_until === BG && ag.grace_signed_at === T0 + 61 * DAY,
    `${label}a renewal charged ahead, then in grace at its end: the grace period stands, from Apple's notice and then from the phone's later renewal info of the same period, though its transaction was signed before the period began (${shown(ag)})`);
  const aa = cross.answers.get(aheadAlone);
  check(!!aa && aa.period_from === T0 + 30 * DAY - 7 * H && aa.expires_at === B2.expiresDate && aa.grace_until === BG && aa.grace_signed_at === T0 + 61 * DAY,
    `${label}and from the phone alone, its transaction signed before its period began: the grace period is recorded (${shown(aa)})`);
  const td = cross.answers.get(aheadTwoDevices);
  check(!!td && td.period_from === B2.purchaseDate && td.signed_at === T0 + 31 * DAY && td.expires_at === B2.expiresDate && td.grace_until === BG && td.grace_signed_at === T0 + 61 * DAY,
    `${label}and from the phone alone after the Mac's copy, signed once B2 began, placed the row later: the grace period is recorded on B2 (${shown(td)})`);
  const rl = cross.answers.get(aheadRenewedLater);
  check(!!rl && rl.period_from === T0 + 30 * DAY - 6 * H && rl.signed_at === T0 + 30 * DAY - 6 * H && rl.grace_until === BG && rl.grace_signed_at === T0 + 61 * DAY,
    `${label}and after Apple's DID_RENEW, signed after the phone's copy, with no grace notice yet: the grace period is recorded from the phone (${shown(rl)})`);
  const ar = cross.answers.get(aheadRefund);
  check(!!ar && ar.period_from === T0 + 30 * DAY - 8 * H && ar.expires_at === B2.expiresDate && ar.revoked_at === null,
    `${label}a refund of the period before, signed after the renewal was charged ahead: B2, not revoked (${shown(ar)})`);
  const lg = cross.answers.get(lagging);
  check(!!lg && lg.period_from === M1.purchaseDate && lg.grace_until === T0 + 46 * DAY && lg.grace_signed_at === T0 + 30 * DAY,
    `${label}a renewal info signed after billing recovered, beside the lapsed transaction, leaves the grace period Apple reported (${shown(lg)})`);
  const lr = cross.answers.get(laggingRecovered);
  check(!!lr && lr.period_from === M2.purchaseDate && lr.grace_until === null,
    `${label}and the recovery, when it arrives, records M2 (${shown(lr)})`);
  const oi = cross.answers.get(oldInfo);
  check(!!oi && oi.period_from === M2.purchaseDate && oi.grace_until === null && oi.grace_signed_at === 0,
    `${label}the next period's transaction beside the renewal info of the grace period before it: no grace period (${shown(oi)})`);
  const g = cross.answers.get(alone);
  check(!!g && g.period_from === M1.purchaseDate && g.grace_until === T0 + 46 * DAY && g.grace_signed_at === T0 + 31 * DAY && g.signed_at === T0 + 5 * DAY,
    `${label}the phone alone records the grace period from its renewal info, stamped when Apple signed that (${shown(g)})`);
  const ge = cross.answers.get(aloneEnded);
  check(!!ge && ge.grace_until === null && ge.grace_signed_at === T0 + 48 * DAY,
    `${label}and a later notice ending it is not undone by the phone's older renewal info (${shown(ge)})`);
  const gr = cross.answers.get(aloneRecovered);
  check(!!gr && gr.period_from === M2.purchaseDate && gr.grace_until === null,
    `${label}and once M2 is recorded, M1's grace period changes nothing (${shown(gr)})`);
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
