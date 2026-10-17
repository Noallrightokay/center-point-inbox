import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SignedDataVerifier, VerificationException, VerificationStatus, Environment } from '@apple/app-store-server-library';
import { APPLE_PRODUCTS, ORDER, PLANS } from './plan.js';
import { tableMissing } from './stripe.js';

/* ---------------------------------------------------------------------------
   Subscriptions bought in the iPhone app, through Apple's In-App Purchase.

   App Store guideline 3.1.1 forbids unlocking an iOS app with a licence key,
   so the iPhone app sells Base and Pro itself, and the purchase is what
   mailrata.org issues the licence against: the same Ed25519 licence a
   Stripe subscription gets (lib/licence.js), whose `sub` is
   `apple:<originalTransactionId>` instead of an address. Buying needs no
   mailrata.org account, and Apple tells RATA nothing personal: no name, no
   address, no Apple ID.

   What proves a purchase is Apple's signature. StoreKit hands the app each
   transaction as a JWS that the App Store signed (ES256, its certificate
   chain in the header), and App Store Server Notifications V2 arrive signed
   the same way. Apple's own library checks them: SignedDataVerifier, given
   Apple's root certificates (lib/apple-roots, fingerprints pinned in
   tests/apple.test.mjs). It checks the chain up to one of those roots,
   Apple's marker extensions on the leaf and the intermediate, every
   certificate's dates at the time Apple signed the data, the signature, the
   bundle id and the environment.

   Online checks are off (`enableOnlineChecks` false). On, the library asks
   Apple's OCSP responder whether the leaf or intermediate certificate has
   been revoked, and checks their dates against today rather than the day
   the data was signed. It caches a good answer for 15 minutes per chain but
   not a failure, so while Apple's responder is slow or down every request
   would wait up to 30 seconds and call out again, and a flood of replayed
   genuine purchases would become a flood of requests to Apple. A revoked
   App Store signing certificate would be news Apple announces, not one a
   per-request check is needed for; and a refunded or revoked purchase is a
   different thing, which reaches RATA as a transaction carrying
   `revocationDate` or as a REFUND or REVOKE notification. So RATA checks
   offline, which costs no request to anybody.
   --------------------------------------------------------------------------- */

/* Apple's root certificates, as the library's documentation asks: every one
   in the Apple Root Certificates section of apple.com/certificateauthority.
   The App Store signs under Apple Root CA - G3. */
export const APPLE_ROOT_FILES = ['AppleRootCA-G3.cer', 'AppleRootCA-G2.cer', 'AppleIncRootCertificate.cer'];

/* The bundle id every purchase must carry, unless APPLE_BUNDLE_ID says
   otherwise: RATA's App Store app, which the iPhone and the Mac App Store
   apps share (Universal Purchase), so one subscription covers both and a
   purchase from either carries this id, under one app record and one
   APPLE_APP_ID. The desktop app downloaded from mailrata.org is
   org.mailrata.desktop, sells nothing through Apple, and has nothing to do
   with this path: a transaction signed for it is another app's. */
export const APPLE_DEFAULT_BUNDLE_ID = 'org.mailrata.mail';

/* What /api/licence/apple takes: the app's current entitlements, at most
   ten signed transactions of at most 16 KiB each. A real one is roughly 5
   to 7 KiB: the three certificates in its header's x5c come to about
   3.5 KB of base64, and the header is base64url-encoded again around
   them. */
export const APPLE_MAX_TRANSACTIONS = 10;
export const APPLE_MAX_JWS = 16 * 1024;
const APPLE_MAX_BODY = APPLE_MAX_TRANSACTIONS * (APPLE_MAX_JWS + 16) + 1024;
/* A notification carries a transaction and its renewal info inside it. */
const APPLE_MAX_NOTIFICATION = 64 * 1024;

/* Every sentence a customer can be shown, kept here so the help page can
   quote them (tests/help.test.mjs) and each route says the same thing. */
export const APPLE_SAID = Object.freeze({
  nothingLive: 'There is no active RATA subscription on this Apple account.',
  unverified: 'mailrata.org could not verify this App Store purchase. Try Restore purchases.',
  off: 'App Store purchases are not switched on for RATA yet.',
  busy: 'mailrata.org could not record this App Store purchase just now. The purchase is safe with Apple, so try again in a few minutes.',
  renewBusy: 'mailrata.org could not check this App Store subscription just now, so RATA keeps the licence it has and tries again later.',
  malformed: 'That was not a list of App Store purchases mailrata.org can read.',
  /* /api/licence/renew's refusals of a licence from the App Store, which
     the iPhone app asks there when StoreKit has nothing. A subscription
     bought in the app has no account at mailrata.org, so these never send
     anyone to sign in there, as the words for a licence bought on the
     website do. */
  tooOld: 'This licence expired too long ago to renew itself. Subscribe again, or press Restore purchases.',
  unreadable: 'This licence could not be read. Press Restore purchases to get a new one.',
});

/* The auto-renewal sentence, as the purchase screen in the iPhone app says
   it and the terms repeat it (tests/legal.test.mjs). */
export const APPLE_AUTO_RENEWAL = 'Payment is charged to your Apple Account when you confirm. The subscription renews automatically unless it is cancelled at least 24 hours before the end of the current period. Manage or cancel it in your Apple Account settings.';

/* The notification types that change a row (appleChange). Each states the
   whole of what it knows; only REFUND and REVOKE, and REFUND_REVERSED, also
   say by their type whether the subscription is revoked. Anything else is
   answered 200 and left alone. */
const CURRENT = ['SUBSCRIBED', 'DID_RENEW', 'DID_CHANGE_RENEWAL_PREF', 'OFFER_REDEEMED'];
const DATES = ['DID_FAIL_TO_RENEW', 'EXPIRED', 'GRACE_PERIOD_EXPIRED'];
const REVOKED = ['REFUND', 'REVOKE'];
export const APPLE_ACTED = Object.freeze([...CURRENT, ...DATES, ...REVOKED, 'REFUND_REVERSED']);

const AUTO_RENEWABLE = 'Auto-Renewable Subscription';
const COLS = 'original_transaction_id,product_id,plan,environment,period_from,expires_at,grace_until,grace_signed_at,revoked_at,signed_at';

/* ---- the licence's subject ------------------------------------------------ */

/* Apple's transaction ids are decimal numbers, written as strings. Nothing
   else is accepted as one, so an `apple:` subject can never be an address
   and an address can never be one. */
export function appleIdOk(id) {
  return typeof id === 'string' && /^\d{1,40}$/.test(id);
}

export function appleSub(originalTransactionId) {
  if (!appleIdOk(originalTransactionId)) throw new Error('not an App Store transaction id');
  return `apple:${originalTransactionId}`;
}

/* The original transaction id an `apple:` subject names, or null for any
   other subject (an address). */
export function appleIdOf(sub) {
  const m = /^apple:(\d{1,40})$/.exec(String(sub || ''));
  return m ? m[1] : null;
}

/* Whether a licence's subject is the App Store's kind (`apple:…`, well
   formed or not), as opposed to an address. */
export function appleSubject(sub) {
  return String(sub || '').trim().toLowerCase().startsWith('apple:');
}

/* Whether a licence *says* it is for an App Store subscription, read from
   its payload without its signature being checked. For choosing the words
   of a refusal of a licence that did not verify, and nothing else: what it
   says unlocks nothing and is never looked up. */
export function appleClaimed(token) {
  try {
    const payload = JSON.parse(Buffer.from(String(token || '').split('.')[1] || '', 'base64url').toString('utf8'));
    return !!payload && typeof payload.sub === 'string' && appleSubject(payload.sub);
  } catch {
    return false;
  }
}

/* ---- settings and verifiers ----------------------------------------------- */

/* APPLE_APP_ID is the app's numeric App Store id, which the library needs to
   check Production notifications (and refuses to build a Production
   verifier without). Unset, or not a number, there is no Production
   verifier: a Production purchase is answered "not switched on" rather than
   checked with less than it needs. APPLE_ALLOW_LOCAL and APPLE_TEST_ROOT are
   for tests only (see appleVerifiers). */
export function appleSettings(env = process.env) {
  const bundleId = String(env.APPLE_BUNDLE_ID || '').trim() || APPLE_DEFAULT_BUNDLE_ID;
  const rawId = String(env.APPLE_APP_ID || '').trim();
  const appId = /^[1-9]\d{0,15}$/.test(rawId) && Number.isSafeInteger(Number(rawId)) ? Number(rawId) : null;
  return {
    bundleId,
    appId,
    appIdUnreadable: !!rawId && appId === null,
    allowLocal: String(env.APPLE_ALLOW_LOCAL || '').trim() === '1',
    testRoot: String(env.APPLE_TEST_ROOT || '').trim(),
  };
}

/* Apple's roots, read from lib/apple-roots. APPLE_TEST_ROOT (base64 DER)
   replaces them, never adds to them: set anywhere but a test, no real
   purchase verifies, which fails closed and is noticed at once. The site
   runs from rata-next (`npm start`), so that is where the files are; the
   path is written out in full so the build traces only that folder. */
export function appleRoots(settings) {
  if (settings.testRoot) return [Buffer.from(settings.testRoot, 'base64')];
  return APPLE_ROOT_FILES.map((f) => readFileSync(join(process.cwd(), 'lib', 'apple-roots', f)));
}

let built = null;
const warned = new Set();
function warnOnce(what) {
  if (warned.has(what)) return;
  warned.add(what);
  console.warn(what);
}

/* The verifiers, one per environment the settings allow, built once per
   set of settings. `off` names what is missing instead, for the server log
   only. Sandbox is always there: App Review and TestFlight buy in the
   sandbox, so a production server must take sandbox purchases. Xcode and
   LocalTesting purchases are signed by nobody (the library checks no
   signature for them), so they are refused unless APPLE_ALLOW_LOCAL=1,
   which only tests set. */
export function appleVerifiers(env = process.env) {
  const settings = appleSettings(env);
  const key = JSON.stringify(settings);
  if (built && built.key === key) return built.value;
  if (settings.testRoot) warnOnce('apple: APPLE_TEST_ROOT is set, so Apple\'s own roots are not trusted and no real purchase verifies; it is for tests only');
  if (settings.allowLocal) warnOnce('apple: APPLE_ALLOW_LOCAL=1, so Xcode and LocalTesting purchases, which nobody signs, are accepted; it is for tests only');
  if (settings.appIdUnreadable) warnOnce('apple: APPLE_APP_ID is not a number, so Production purchases are not accepted; see rata-next/LAUNCH.md section 11');
  let verifiers;
  try {
    const roots = appleRoots(settings);
    const one = (environment, appId) => new SignedDataVerifier(roots, false, environment, settings.bundleId, appId);
    verifiers = { [Environment.SANDBOX]: one(Environment.SANDBOX) };
    if (settings.appId !== null) verifiers[Environment.PRODUCTION] = one(Environment.PRODUCTION, settings.appId);
    if (settings.allowLocal) {
      verifiers[Environment.XCODE] = one(Environment.XCODE);
      verifiers[Environment.LOCAL_TESTING] = one(Environment.LOCAL_TESTING);
    }
  } catch (e) {
    /* Not cached: a deploy that is fixed should not need a restart. */
    return { settings, off: `the App Store root certificates could not be read (${(e && (e.code || e.name)) || 'unknown'})` };
  }
  built = { key, value: { settings, verifiers } };
  return built.value;
}

/* Whether the verifiers can take what the App Store itself signs: Apple's
   roots were read, and APPLE_APP_ID gave a Production verifier. */
export function appleTakesProduction(made) {
  return !!made && !made.off && !!made.verifiers[Environment.PRODUCTION];
}

/* ---- verifying ------------------------------------------------------------ */

const DECODE = {
  transaction: 'verifyAndDecodeTransaction',
  renewal: 'verifyAndDecodeRenewalInfo',
  notification: 'verifyAndDecodeNotification',
};

/* The library's status name for a refusal, which is all the log is given. */
function statusOf(e) {
  return (e instanceof VerificationException && VerificationStatus[e.status]) || 'FAILURE';
}

/* The environment signed data says it is from, read without checking the
   signature: it only chooses which verifier checks it. It is Apple's
   environment (Production, Sandbox, Xcode, LocalTesting), never the
   platform: with Universal Purchase a purchase made in the Mac App Store
   app carries the same bundle id, app id and environments as one made on
   the iPhone, and nothing here asks which it was. */
export function claimedEnvironment(kind, jws) {
  try {
    const p = JSON.parse(Buffer.from(String(jws).split('.')[1] || '', 'base64url').toString('utf8'));
    const env = kind === 'notification' ? (p.data || p.summary || p.appData || {}).environment : p.environment;
    return typeof env === 'string' ? env : null;
  } catch {
    return null;
  }
}

async function attempt(made, environment, kind, jws) {
  const verifier = made.verifiers[environment];
  if (!verifier) return { ok: false, status: 'INVALID_ENVIRONMENT' };
  try {
    return { ok: true, payload: await verifier[DECODE[kind]](jws), environment };
  } catch (e) {
    return { ok: false, status: statusOf(e) };
  }
}

/* Verify a signed transaction, renewal info or notification. As
   Production, unless the data says it is from the sandbox, when it is
   verified as Sandbox instead; Xcode and LocalTesting only where allowed.
   Each verifier refuses data from any other environment once the signature
   has verified (the library's INVALID_ENVIRONMENT), so what the data says
   only chooses the verifier and cannot get anything past one. It has to
   choose: a sandbox notification carries no appAppleId, so Production's
   verifier refuses it as another app's (INVALID_APP_IDENTIFIER) before it
   looks at the environment. Data that says nothing readable goes to
   Production, and to Sandbox if Production finds it is from there.
   Answers { ok, payload, environment } or { ok: false, status }, with
   `noAppId` when it is a genuine Production purchase and APPLE_APP_ID is
   not set. */
export async function verifyApple(kind, jws, made) {
  const claimed = claimedEnvironment(kind, jws);
  if (claimed === Environment.SANDBOX || claimed === Environment.XCODE || claimed === Environment.LOCAL_TESTING) {
    return attempt(made, claimed, kind, jws);
  }
  if (!made.verifiers[Environment.PRODUCTION]) {
    /* Checked as Sandbox, so a forgery is still reported as one. */
    const sandbox = await attempt(made, Environment.SANDBOX, kind, jws);
    if (!sandbox.ok && sandbox.status === 'INVALID_ENVIRONMENT' && claimed === Environment.PRODUCTION) return { ...sandbox, noAppId: true };
    return sandbox;
  }
  const first = await attempt(made, Environment.PRODUCTION, kind, jws);
  if (!first.ok && first.status === 'INVALID_ENVIRONMENT' && claimed !== Environment.PRODUCTION) {
    return attempt(made, Environment.SANDBOX, kind, jws);
  }
  return first;
}

/* What a verified notification carries inside it is checked by the verifier
   of the notification's own environment. */
export function verifyAppleIn(environment, kind, jws, made) {
  return attempt(made, environment, kind, jws);
}

/* ---- reading a request ---------------------------------------------------- */

/* /api/licence/apple's body: { transactions: [<JWS>, ...] }. */
export function readTransactions(raw) {
  if (typeof raw !== 'string' || raw.length > APPLE_MAX_BODY) return { ok: false };
  let body;
  try { body = JSON.parse(raw); } catch { return { ok: false }; }
  const list = body && Array.isArray(body.transactions) ? body.transactions : null;
  if (!list || list.length > APPLE_MAX_TRANSACTIONS) return { ok: false };
  if (!list.every((t) => typeof t === 'string' && t.length > 0 && t.length <= APPLE_MAX_JWS)) return { ok: false };
  return { ok: true, list };
}

/* A notification's body: { signedPayload: <JWS> }. */
export function readNotification(raw) {
  if (typeof raw !== 'string' || raw.length > APPLE_MAX_NOTIFICATION) return null;
  try {
    const body = JSON.parse(raw);
    return body && typeof body.signedPayload === 'string' && body.signedPayload ? body.signedPayload : null;
  } catch {
    return null;
  }
}

/* ---- judging a purchase --------------------------------------------------- */

/* Why a verified transaction unlocks nothing by itself, or null when it is
   live: not RATA's app, not a RATA product, not an auto-renewable
   subscription, refunded or revoked, upgraded from, or past its expiry.
   Family Sharing is the owner's choice in App Store Connect, so PURCHASED
   and FAMILY_SHARED count alike. An upgraded transaction (`isUpgraded`) is
   one the customer moved up from: the subscription it was upgraded to
   took its place, and StoreKit no longer lists it as an entitlement.

   'expired' is the one a row can still overrule. In a billing grace
   period StoreKit's current entitlements still list the subscription, by
   its transaction for the period that has ended, and what says the grace
   period has not is Apple's DID_FAIL_TO_RENEW notification, recorded in
   the row as grace_until. So /api/licence/apple asks the row about an
   expired transaction (appleLive) and writes nothing from it. */
export function appleIgnored(t, settings, now = Date.now()) {
  if (!t || !appleIdOk(t.originalTransactionId)) return 'id';
  if (t.bundleId !== settings.bundleId) return 'bundle';
  if (!Object.hasOwn(APPLE_PRODUCTS, String(t.productId))) return 'product';
  if (t.type !== AUTO_RENEWABLE) return 'type';
  if (t.revocationDate !== undefined && t.revocationDate !== null) return 'revoked';
  if (t.isUpgraded === true) return 'upgraded';
  if (typeof t.expiresDate !== 'number' || !(t.expiresDate > now)) return 'expired';
  return null;
}

/* The live transactions in the order they are tried: the higher plan
   first (Pro over Base), then the later expiry. */
export function applePreferred(list) {
  const rank = (t) => ORDER.indexOf(APPLE_PRODUCTS[t.productId]);
  return [...list].sort((a, b) => rank(b) - rank(a) || b.expiresDate - a.expiresDate);
}

const iso = (ms) => new Date(ms).toISOString();
/* When Apple signed it, which orders what Apple says about one period of a
   subscription (appleWrite). Apple always says; something that does not is
   treated as the oldest there is, so it never overwrites anything. */
const signedAt = (o) => iso(o && typeof o.signedDate === 'number' && Number.isFinite(o.signedDate) ? o.signedDate : 0);
/* When the period a transaction pays for began, its purchaseDate, which
   orders the periods of a subscription (appleWrite). Treated as the
   earliest there is when missing, as signedAt is. */
const periodFrom = (t) => iso(t && typeof t.purchaseDate === 'number' && Number.isFinite(t.purchaseDate) ? t.purchaseDate : 0);
/* The grace fields of a row that has heard of no grace period for its
   period: what a new row, and a later period replacing a row, starts
   with. */
const EPOCH = iso(0);
const NO_GRACE = Object.freeze({ grace_until: null, grace_signed_at: EPOCH });

/* A row's subscription fields as a transaction describes them: the
   product, the plan, the period and its expiry, whether the transaction is
   revoked, and when Apple signed it. Everything but the grace period. */
export function appleRowOf(t, environment) {
  return {
    product_id: t.productId,
    plan: APPLE_PRODUCTS[t.productId],
    environment,
    period_from: periodFrom(t),
    expires_at: iso(t.expiresDate),
    revoked_at: typeof t.revocationDate === 'number' ? iso(t.revocationDate) : null,
    signed_at: signedAt(t),
  };
}

/* What a notification says, as { core, grace, insert }, or null when it
   changes nothing RATA keeps: another type, another product, no
   transaction to read, or a transaction the customer upgraded from
   (`isUpgraded`). The subscription it was upgraded to has a transaction of
   its own, which writes the row, and Apple refunding what was left of the
   old one is no refund of the new one.

   Every notification states the whole of what it knows at the moment
   Apple signed it, whatever its type, so that the newest one wins
   (appleWrite) and one delivered late, or twice, changes nothing:

   - `core`: the product, the plan, the period, its expiry and the
     environment, from its transaction, which is the subscription's latest;
     whether that transaction is revoked (its revocationDate, or null), and
     when Apple signed the notification. A REFUND or REVOKE whose
     transaction gives no revocationDate is revoked when Apple signed it;
     a REFUND_REVERSED is not revoked, whatever its transaction still says.
   - `grace`, only when the notification carries renewal info: the end of
     a billing grace period (its gracePeriodExpiresDate, or null when it
     gives none), and when Apple signed the notification. Without renewal
     info a notification says nothing about the grace period, and leaves it
     as it is. DID_FAIL_TO_RENEW with the subtype GRACE_PERIOD is the one
     that starts one; the rest carry it on, or end it.
   - `insert`: the whole row, for a subscription not seen yet. */
export function appleChange(n, t, r, environment) {
  const type = n && n.notificationType;
  if (!APPLE_ACTED.includes(type)) return null;
  if (!t || !appleIdOk(t.originalTransactionId) || !Object.hasOwn(APPLE_PRODUCTS, String(t.productId))) return null;
  if (typeof t.expiresDate !== 'number') return null;
  if (t.isUpgraded === true) return null;
  const at = signedAt(n);
  const own = typeof t.revocationDate === 'number' ? t.revocationDate : null;
  let revoked = own;
  if (REVOKED.includes(type) && own === null) revoked = typeof n.signedDate === 'number' && Number.isFinite(n.signedDate) ? n.signedDate : Date.now();
  if (type === 'REFUND_REVERSED') revoked = null;
  const core = { ...appleRowOf(t, environment), revoked_at: revoked === null ? null : iso(revoked), signed_at: at };
  const grace = r && typeof r === 'object'
    ? { grace_until: typeof r.gracePeriodExpiresDate === 'number' && Number.isFinite(r.gracePeriodExpiresDate) ? iso(r.gracePeriodExpiresDate) : null, grace_signed_at: at }
    : null;
  return { core, grace, insert: { ...core, ...(grace || NO_GRACE) } };
}

/* What a live transaction the app sent says (/api/licence/apple): its
   subscription fields, and nothing about a grace period, which StoreKit's
   transaction does not carry. An upgraded transaction never gets here,
   since appleIgnored refuses it. */
export function applePurchase(t, environment) {
  const core = appleRowOf(t, environment);
  return { core, grace: null, insert: { ...core, ...NO_GRACE } };
}

/* ---- the row -------------------------------------------------------------- */

/* Write what a transaction or notification says, so that whatever order
   Apple's notifications and the phone's purchases arrive in, and however
   often, the row ends as it would had each arrived once, in the order
   Apple signed them.

   A row records one period, the one that began at its period_from: the
   transaction's purchaseDate. Renewals, upgrades, downgrades,
   resubscriptions and offers each begin a period later than the one
   before, however long each lasts; the expiry would not do, since a
   period paid for in advance (an introductory offer of several months, an
   offer code) can end after the one that replaced it. Within one period,
   what Apple signed later wins, in two groups of fields, each with its
   own stamp:

   - the subscription fields (`core`), stamped signed_at. Written over a
     row recording an earlier period, which this one replaces, its grace
     fields reset with it (to this notification's grace, or to none); or
     over a row recording the same period whose signed_at is no later. A
     row recording a later period is never written.
   - the grace fields, grace_until and grace_signed_at, written only by a
     notification that carries renewal info: over a row recording this
     same period whose grace_signed_at is no later. The core write has
     already moved a row of an earlier period on to this one, and only a
     notification knows the grace period: the phone's purchase never
     touches it, so a transaction StoreKit signed later cannot hide a
     grace period Apple reported earlier.

   So each group's (period_from, stamp) never goes down. Every guard is in
   the database, so two deliveries racing each other cannot both pass a
   check and then both write. A row not there yet is inserted whole; if
   another request inserted it in between, the guarded updates are tried
   once more. Answers { row, wrote } with the row as it stands after, or
   { error }. */
export async function appleWrite(sb, id, { core, grace = null, insert }) {
  const table = () => sb.from('apple_subscriptions');
  const update = async (values, guard) => {
    const { data, error } = await guard(table().update(values).eq('original_transaction_id', id)).select(COLS);
    if (error) throw error;
    return data && data.length ? data[0] : null;
  };
  const from = core.period_from;
  const coreWrite = async () => (await update({ ...core, ...(grace || NO_GRACE) }, (q) => q.lt('period_from', from)))
    || update(core, (q) => q.eq('period_from', from).lte('signed_at', core.signed_at));
  try {
    let row = await coreWrite();
    if (!row) {
      const { data, error } = await table()
        .upsert({ original_transaction_id: id, ...insert }, { onConflict: 'original_transaction_id', ignoreDuplicates: true })
        .select(COLS);
      if (error) throw error;
      if (data && data.length) return { row: data[0], wrote: true };
      row = await coreWrite();
    }
    let wrote = !!row;
    if (grace) {
      const kept = await update(grace, (q) => q.eq('period_from', from).lte('grace_signed_at', grace.grace_signed_at));
      if (kept) { row = kept; wrote = true; }
    }
    if (row) return { row, wrote };
    const held = await table().select(COLS).eq('original_transaction_id', id).maybeSingle();
    if (held.error) throw held.error;
    return { row: held.data || null, wrote: false };
  } catch (error) {
    return { error: error || { message: 'unknown' } };
  }
}

/* Whether a database error means the database is behind section 7 of
   database.sql: no apple_subscriptions table (tableMissing), or one made
   by an earlier copy of section 7 without a column this code reads or
   writes (Postgres's undefined column, 42703, or PostgREST's PGRST204,
   which names the column and the table). Either way the log says to run
   section 7, and the answer is "not switched on". */
export function appleTableBehind(error) {
  if (tableMissing(error)) return true;
  const code = error && error.code;
  return code === '42703' || (code === 'PGRST204' && /apple_subscriptions/.test(String(error.message || '')));
}

/* The row for an original transaction id, or { error }. */
export async function appleRow(sb, id) {
  try {
    const { data, error } = await sb.from('apple_subscriptions').select(COLS).eq('original_transaction_id', id).maybeSingle();
    return error ? { error } : { row: data || null };
  } catch (error) {
    return { error: error || { message: 'unknown' } };
  }
}

/* Whether a row is live: not revoked, and either not expired or inside a
   billing grace period Apple reported. And for a plan RATA sells here. */
export function appleLive(row, now = Date.now()) {
  if (!row || row.revoked_at) return false;
  if (!Object.values(APPLE_PRODUCTS).includes(row.plan) || !PLANS[row.plan]) return false;
  const until = appleUntil(row);
  return !!until && Date.parse(until) > now;
}

/* When a row stops being live, if nothing else happens: the later of its
   expiry and its grace period. */
export function appleUntil(row) {
  const times = [row && row.expires_at, row && row.grace_until].map((t) => (t ? Date.parse(t) : NaN)).filter(Number.isFinite);
  return times.length ? iso(Math.max(...times)) : null;
}

export { tableMissing };
