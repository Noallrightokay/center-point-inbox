/* Subscriptions bought in the iPhone app (the App Store design's server
   half): verifying what Apple signed, issuing and renewing the licence, and
   App Store Server Notifications.

   What is worth proving is that only Apple's signature unlocks anything,
   and only for a live RATA subscription; that nothing about an earlier
   period of a subscription, nor older news of the same one, ever undoes
   something newer, in whatever order it arrives; that a fault at this end
   issues nothing it cannot record; and that an `apple:` subject is never
   taken for an address. Apple is played by a certificate chain made in the test
   (apple-chain.mjs), with Apple's marker extensions, which the servers here
   are told to trust instead of Apple's roots (APPLE_TEST_ROOT); Supabase by
   a stand-in that keeps apple_subscriptions in memory and applies
   PostgREST's filters as Postgres would. */
import { readFileSync, readdirSync } from 'node:fs';
import { X509Certificate, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startServer, makeChecker, fakeSupabaseKey } from './helpers.mjs';
import { makeChain, signJws, unsignedJws } from './apple-chain.mjs';
import { deliveryOrders } from './apple-order.mjs';
import { generateKeys, issue, check as checkLicence } from '../lib/licence.js';
import { APPLE_PRODUCTS, PLANS, ORDER } from '../lib/plan.js';
import * as apple from '../lib/apple.js';
import { createClient } from '@supabase/supabase-js';

const HERE = dirname(fileURLToPath(import.meta.url));
const SITE = join(HERE, '..');
const REPO = join(SITE, '..');

/* Apple's roots, by the SHA-256 fingerprint Apple publishes for each. The
   PKI page (apple.com/certificateauthority) links the files; Apple's lists
   of the roots its systems trust (support.apple.com/en-us/121672, and
   /105116 before it) give the fingerprints. Checked by hand against both on
   2026-10-15; a file changed by anyone since fails here. */
const PINNED = {
  'AppleRootCA-G3.cer': { cn: 'Apple Root CA - G3', sha256: '63:34:3A:BF:B8:9A:6A:03:EB:B5:7E:9B:3F:5F:A7:BE:7C:4F:5C:75:6F:30:17:B3:A8:C4:88:C3:65:3E:91:79' },
  'AppleRootCA-G2.cer': { cn: 'Apple Root CA - G2', sha256: 'C2:B9:B0:42:DD:57:83:0E:7D:11:7D:AC:55:AC:8A:E1:94:07:D3:8E:41:D8:8F:32:15:BC:3A:89:04:44:A0:50' },
  'AppleIncRootCertificate.cer': { cn: 'Apple Root CA', sha256: 'B0:B1:73:0E:CB:C7:FF:45:05:14:2C:49:F1:29:5E:6E:DA:6B:CA:ED:7E:2C:68:C5:BE:91:B5:A1:10:01:F0:24' },
};

const DAY = 86400e3;
const BUNDLE = 'org.mailrata.mail';
const APP_ID = 1234567890;
const PRO = 'org.mailrata.pro.monthly';
const BASE = 'org.mailrata.base.monthly';
const NOTHING_LIVE = 'There is no active RATA subscription on this Apple account.';
const OFF = 'App Store purchases are not switched on for RATA yet.';

let serial = 0;
const nextId = () => String(2000000000001000 + (++serial));

/* A transaction as StoreKit hands one over: a Pro subscription bought a day
   ago, live for 29 more days, signed now, in Production. */
function txn(over = {}) {
  const now = Date.now();
  const id = nextId();
  return {
    transactionId: id,
    originalTransactionId: id,
    webOrderLineItemId: String(Number(id) + 500000),
    bundleId: BUNDLE,
    productId: PRO,
    subscriptionGroupIdentifier: '21000001',
    purchaseDate: now - DAY,
    originalPurchaseDate: now - DAY,
    expiresDate: now + 29 * DAY,
    quantity: 1,
    type: 'Auto-Renewable Subscription',
    inAppOwnershipType: 'PURCHASED',
    signedDate: now,
    environment: 'Production',
    transactionReason: 'PURCHASE',
    storefront: 'USA',
    storefrontId: '143441',
    price: 23990,
    currency: 'USD',
    ...over,
  };
}

function renewalInfo(t, over = {}) {
  return {
    originalTransactionId: t.originalTransactionId,
    autoRenewProductId: t.productId,
    productId: t.productId,
    autoRenewStatus: 1,
    signedDate: Date.now(),
    environment: t.environment,
    recentSubscriptionStartDate: t.purchaseDate,
    renewalDate: t.expiresDate,
    ...over,
  };
}

/* A notification as Apple sends one: its transaction and renewal info
   signed inside it, the whole signed again. */
function notification(chain, type, t, { subtype, renewal, signedDate = Date.now(), environment, appAppleId, signer, bundleId = BUNDLE } = {}) {
  const env = environment || (t ? t.environment : 'Production');
  const data = { bundleId, bundleVersion: '1', environment: env, status: 1 };
  /* Apple sends appAppleId only for an app downloaded from the App Store:
     never in the sandbox. */
  if (env === 'Production') data.appAppleId = appAppleId ?? APP_ID;
  if (t) data.signedTransactionInfo = signJws(t, chain);
  if (renewal) data.signedRenewalInfo = signJws(renewal, chain);
  return signJws({
    notificationType: type, ...(subtype ? { subtype } : {}), notificationUUID: randomUUID(), data, version: '2.0', signedDate,
  }, signer || chain);
}

/* PostgREST's filters, read and applied as PostgREST and Postgres do, for
   the stand-in below. A query parameter is `col=op.value`, or `or=(...)`
   and `and=(...)` holding a comma-separated list of `col.op.value`,
   `and(...)` and `or(...)`, nested to any depth; the top-level parameters
   all hold at once. A comparison with NULL is unknown, so no match, and
   `is.null` asks for one. Times compare as times. Values here are ids and
   ISO times, which hold no comma or parenthesis, so a quoted value is not
   read; anything this does not know (an operator, `not.`, a quote) throws,
   and the stand-in answers 400, so a test can never pass on a filter it
   ignored. */
const PG_OPS = ['eq', 'lt', 'lte', 'gt', 'gte', 'is'];
function pgLeaf(col, rest) {
  const i = rest.indexOf('.');
  const op = i > 0 ? rest.slice(0, i) : '';
  const val = rest.slice(i + 1);
  if (!/^[a-z_]+$/.test(col) || !PG_OPS.includes(op) || /["()]/.test(val) || (op === 'is' && val !== 'null')) {
    throw new Error(`the stand-in does not read ${col}=${rest}`);
  }
  return { col, op, val };
}
function pgList(text) {
  const parts = [];
  let depth = 0;
  let cur = '';
  for (const ch of text) {
    if (ch === '(') depth += 1;
    if (ch === ')') depth -= 1;
    if (depth < 0) throw new Error(`unbalanced: ${text}`);
    if (ch === ',' && depth === 0) { parts.push(cur); cur = ''; } else cur += ch;
  }
  if (depth !== 0) throw new Error(`unbalanced: ${text}`);
  parts.push(cur);
  return parts.map((f) => {
    const m = /^(and|or)\((.*)\)$/.exec(f);
    if (m) return { op: m[1], all: pgList(m[2]) };
    const i = f.indexOf('.');
    if (i < 1) throw new Error(`the stand-in does not read ${f}`);
    return pgLeaf(f.slice(0, i), f.slice(i + 1));
  });
}
function pgFilters(params) {
  return [...params].filter(([k]) => !['select', 'on_conflict', 'columns'].includes(k)).map(([k, v]) => {
    if (k === 'or' || k === 'and') {
      const m = /^\((.*)\)$/.exec(v);
      if (!m) throw new Error(`the stand-in does not read ${k}=${v}`);
      return { op: k, all: pgList(m[1]) };
    }
    return pgLeaf(k, v);
  });
}
const pgTime = (v) => (typeof v === 'string' && /^\d{4}-\d\d-\d\dT/.test(v) ? Date.parse(v) : NaN);
function pgHolds(row, f) {
  if (f.op === 'or') return f.all.some((g) => pgHolds(row, g));
  if (f.op === 'and') return f.all.every((g) => pgHolds(row, g));
  const have = row[f.col];
  if (f.op === 'is') return have === null || have === undefined;
  if (have === null || have === undefined) return false;
  const a = pgTime(String(have));
  const b = pgTime(f.val);
  const [x, y] = Number.isNaN(a) || Number.isNaN(b) ? [String(have), f.val] : [a, b];
  return { eq: x === y, lt: x < y, lte: x <= y, gt: x > y, gte: x >= y }[f.op];
}

/* Supabase, for apple_subscriptions (and subscriptions, which only a
   Stripe licence's renewal may ask). `mode`: 'ok', 'missing' (database.sql
   section 7 never ran: PostgREST's PGRST205), 'behind' (a table made by an
   earlier copy of section 7, without grace_signed_at: PostgREST's
   PGRST204 for a write naming it, Postgres's 42703 for a read) or
   'broken'. `queries` keeps each request's method and query string, and
   `bodies` its body. It
   answers over HTTP, for the site, and through `fetch`, for supabase-js in
   this process, which the delivery orders use since they are many. */
const BEHIND = 'grace_signed_at';
async function fakeSupabase() {
  const fake = { rows: new Map(), mode: 'ok', calls: [], queries: [], bodies: [] };
  const matches = (row, fs) => fs.every((f) => pgHolds(row, f));
  /* The rows a request can match: only the one its primary key names, when
     it names one, as Postgres would find it; the delivery orders make
     thousands. */
  const candidates = (fs) => {
    const key = fs.find((f) => f.col === 'original_transaction_id' && f.op === 'eq');
    if (!key) return [...fake.rows.values()];
    const row = fake.rows.get(key.val);
    return row ? [row] : [];
  };
  const answer = (method, url, prefer, raw) => {
    fake.calls.push(`${method} ${url.pathname}`);
    fake.queries.push(`${method} ${url.pathname}?${decodeURIComponent(url.search.slice(1))}`);
    fake.bodies.push(raw || '');
    if (url.pathname === '/rest/v1/subscriptions') return [200, []];
    if (url.pathname !== '/rest/v1/apple_subscriptions') return [404, { message: 'not in the stand-in' }];
    if (fake.mode === 'missing') return [404, { code: 'PGRST205', details: null, hint: null, message: "Could not find the table 'public.apple_subscriptions' in the schema cache" }];
    if (fake.mode === 'broken') return [500, { code: 'XX000', details: null, hint: null, message: 'disk on fire' }];
    if (fake.mode === 'behind') {
      if (raw && raw.includes(`"${BEHIND}"`)) return [400, { code: 'PGRST204', details: null, hint: null, message: `Could not find the '${BEHIND}' column of 'apple_subscriptions' in the schema cache` }];
      if ((url.searchParams.get('select') || '').split(',').includes(BEHIND)) return [400, { code: '42703', details: null, hint: null, message: `column apple_subscriptions.${BEHIND} does not exist` }];
    }
    let fs;
    try { fs = pgFilters(url.searchParams); } catch (e) { return [400, { code: 'PGRST100', message: e.message }]; }
    if (method === 'GET') return [200, candidates(fs).filter((r) => matches(r, fs))];
    if (method === 'PATCH') {
      const patch = JSON.parse(raw);
      const hit = candidates(fs).filter((r) => matches(r, fs));
      for (const r of hit) Object.assign(r, patch, { updated_at: new Date().toISOString() });
      return [200, hit.map((r) => ({ ...r }))];
    }
    if (method === 'POST') {
      const body = JSON.parse(raw);
      const ignore = /resolution=ignore-duplicates/.test(prefer || '');
      const out = [];
      for (const row of Array.isArray(body) ? body : [body]) {
        if (fake.rows.has(row.original_transaction_id)) {
          if (!ignore) return [409, { code: '23505', message: 'duplicate key value violates unique constraint' }];
          continue;
        }
        const stored = { grace_until: null, revoked_at: null, app_account_token: null, email: null, ...row, updated_at: new Date().toISOString() };
        fake.rows.set(row.original_transaction_id, stored);
        out.push({ ...stored });
      }
      return [201, out];
    }
    return [405, { message: 'not in the stand-in' }];
  };
  const srv = createServer((req, res) => {
    let raw = '';
    req.on('data', (d) => { raw += d; });
    req.on('end', () => {
      const [status, body] = answer(req.method, new URL(req.url, 'http://x'), req.headers.prefer, raw);
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(JSON.stringify(body));
    });
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  const url = `http://127.0.0.1:${srv.address().port}`;
  fake.fetch = async (input, init = {}) => {
    const headers = new Headers(init.headers);
    const [status, body] = answer(init.method || 'GET', new URL(String(input)), headers.get('prefer'), typeof init.body === 'string' ? init.body : '');
    return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
  };
  return { fake, url, close: () => new Promise((r) => { srv.close(() => r()); srv.closeAllConnections(); }) };
}

/* supabase-js for a real PostgREST at `base` (no /rest/v1 in front of it,
   and no JWT secret, so the stand-in's key is not sent). */
function viaPostgrest(base) {
  return createClient('http://postgrest.invalid', 'none', {
    auth: { persistSession: false },
    global: {
      fetch: (input, init = {}) => {
        const headers = new Headers(init.headers);
        headers.delete('authorization');
        headers.delete('apikey');
        return fetch(base + String(input).replace(/^http:\/\/postgrest\.invalid\/rest\/v1/, ''), { ...init, headers });
      },
    },
  });
}

/* Every .rs file under a folder, for the product ids the iOS app's
   StoreKit commands name. */
function rustFiles(dir) {
  let out = [];
  let entries = [];
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name === 'target' || e.name === 'node_modules' || e.name.startsWith('.')) continue;
    const p = join(dir, e.name);
    if (e.isDirectory()) out = out.concat(rustFiles(p));
    else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

export default async function run(state) {
  const check = makeChecker(state);
  const chain = makeChain();
  const stranger = makeChain({ tag: 'Not Apple' });
  const keys = generateKeys();

  console.log('\n— Apple\'s root certificates, pinned —');
  {
    const dir = join(SITE, 'lib', 'apple-roots');
    const there = readdirSync(dir).filter((f) => f.endsWith('.cer')).sort();
    check(JSON.stringify(there) === JSON.stringify(Object.keys(PINNED).sort()) && JSON.stringify([...apple.APPLE_ROOT_FILES].sort()) === JSON.stringify(there),
      `lib/apple-roots holds the three roots Apple's PKI page lists, and the verifier is given exactly those: ${there.join(', ')}`);
    check(apple.APPLE_ROOT_FILES.includes('AppleRootCA-G3.cer'), 'Apple Root CA - G3, which the App Store signs under, is one of them');
    for (const [file, want] of Object.entries(PINNED)) {
      let cert = null;
      try { cert = new X509Certificate(readFileSync(join(dir, file))); } catch { /* reported below */ }
      check(!!cert && cert.fingerprint256 === want.sha256, `${file}: SHA-256 ${cert ? cert.fingerprint256 : 'unreadable'}, as Apple publishes it`);
      check(!!cert && cert.subject.split('\n').includes(`CN=${want.cn}`) && cert.subject === cert.issuer && cert.ca && cert.verify(cert.publicKey),
        `${file}: ${want.cn}, a self-signed certificate authority`);
      check(!!cert && Date.parse(cert.validTo) > Date.now(), `${file}: valid until ${cert && cert.validTo}`);
    }
  }

  console.log('\n— verifying what Apple signs, against a chain made here —');
  {
    const made = apple.appleVerifiers({ APPLE_TEST_ROOT: chain.rootBase64, APPLE_APP_ID: String(APP_ID) });
    check(!made.off && !!made.verifiers.Production && !!made.verifiers.Sandbox && !made.verifiers.Xcode,
      'with APPLE_APP_ID there is a Production verifier and a Sandbox one, and none for Xcode');
    const t = txn();
    const good = await apple.verifyApple('transaction', signJws(t, chain), made);
    check(good.ok && good.environment === 'Production' && good.payload.originalTransactionId === t.originalTransactionId,
      `a Production transaction signed by the chain verifies: ${good.ok ? good.environment : good.status}`);
    const sandbox = await apple.verifyApple('transaction', signJws(txn({ environment: 'Sandbox' }), chain), made);
    check(sandbox.ok && sandbox.environment === 'Sandbox', `a sandbox one verifies as Sandbox, as App Review and TestFlight need: ${sandbox.ok ? sandbox.environment : sandbox.status}`);

    const real = apple.appleVerifiers({ APPLE_APP_ID: String(APP_ID) });
    const refused = await apple.verifyApple('transaction', signJws(t, chain), real);
    check(!real.off && !refused.ok && refused.status === 'VERIFICATION_FAILURE',
      `a verifier with Apple's real roots refuses the test chain: ${refused.status}`);

    for (const [what, c] of [['no leaf marker', makeChain({ leafMarker: false })], ['no intermediate marker', makeChain({ intermediateMarker: false })]]) {
      const own = apple.appleVerifiers({ APPLE_TEST_ROOT: c.rootBase64, APPLE_APP_ID: String(APP_ID) });
      const r = await apple.verifyApple('transaction', signJws(t, c), own);
      check(!r.ok && r.status === 'VERIFICATION_FAILURE', `a chain with ${what}, even from a trusted root, is refused: ${r.status}`);
    }
    const other = await apple.verifyApple('transaction', signJws(t, stranger), made);
    check(!other.ok && other.status === 'VERIFICATION_FAILURE', `a chain from another root is refused: ${other.status}`);
    const [h, , s] = signJws(t, chain).split('.');
    const edited = `${h}.${Buffer.from(JSON.stringify({ ...t, productId: PRO, expiresDate: t.expiresDate + 365 * DAY })).toString('base64url')}.${s}`;
    const ed = await apple.verifyApple('transaction', edited, made);
    check(!ed.ok && ed.status === 'VERIFICATION_FAILURE', `a genuine transaction with its expiry edited is refused: ${ed.status}`);
    for (const junk of ['', 'abc', 'a.b.c', 'x'.repeat(200)]) {
      const r = await apple.verifyApple('transaction', junk, made);
      check(!r.ok, `${JSON.stringify(junk.slice(0, 12))} is refused: ${r.status}`);
    }
    const bundle = await apple.verifyApple('transaction', signJws(txn({ bundleId: 'com.example.other' }), chain), made);
    check(!bundle.ok && bundle.status === 'INVALID_APP_IDENTIFIER', `another app's purchase is refused as another app's: ${bundle.status}`);
    const desktop = await apple.verifyApple('transaction', signJws(txn({ bundleId: 'org.mailrata.desktop' }), chain), made);
    check(!desktop.ok && desktop.status === 'INVALID_APP_IDENTIFIER',
      `so is one signed for org.mailrata.desktop, the downloaded desktop app's id, which sells nothing through Apple: ${desktop.status}`);
    const shared = await apple.verifyApple('transaction', signJws(txn(), chain), made);
    check(shared.ok && shared.payload.bundleId === 'org.mailrata.mail',
      'and one carrying org.mailrata.mail verifies, whether bought on the iPhone or the Mac (Universal Purchase: one bundle id, and a transaction names no platform)');

    const xcode = txn({ environment: 'Xcode' });
    const noFlag = await apple.verifyApple('transaction', unsignedJws(xcode), made);
    check(!noFlag.ok && noFlag.status === 'INVALID_ENVIRONMENT', `an Xcode purchase, which nobody signs, is refused without APPLE_ALLOW_LOCAL: ${noFlag.status}`);
    const local = apple.appleVerifiers({ APPLE_TEST_ROOT: chain.rootBase64, APPLE_APP_ID: String(APP_ID), APPLE_ALLOW_LOCAL: '1' });
    const flagged = await apple.verifyApple('transaction', unsignedJws(xcode), local);
    check(flagged.ok && flagged.environment === 'Xcode', `and accepted with APPLE_ALLOW_LOCAL=1, which only tests set: ${flagged.ok ? flagged.environment : flagged.status}`);

    const noId = apple.appleVerifiers({ APPLE_TEST_ROOT: chain.rootBase64 });
    const prod = await apple.verifyApple('transaction', signJws(t, chain), noId);
    check(!noId.verifiers.Production && !prod.ok && prod.noAppId === true, 'without APPLE_APP_ID a genuine Production purchase is "not switched on", not checked with less');
    const forged = await apple.verifyApple('transaction', signJws(t, stranger), noId);
    check(!forged.ok && !forged.noAppId && forged.status === 'VERIFICATION_FAILURE', 'and a forged one is still reported as forged');
    check(apple.appleSettings({ APPLE_APP_ID: '12ab' }).appId === null && apple.appleSettings({ APPLE_APP_ID: '12ab' }).appIdUnreadable,
      'an APPLE_APP_ID that is not a number is not used');
    check(apple.APPLE_DEFAULT_BUNDLE_ID === 'org.mailrata.mail' && apple.appleSettings({}).bundleId === 'org.mailrata.mail' && apple.appleSettings({ APPLE_BUNDLE_ID: ' org.example.rata ' }).bundleId === 'org.example.rata',
      `the bundle id is org.mailrata.mail, the iPhone and Mac App Store apps' (D9), unless APPLE_BUNDLE_ID says otherwise`);
    const broken = apple.appleVerifiers({ APPLE_TEST_ROOT: 'bm90IGEgY2VydGlmaWNhdGU=' });
    check(!!broken.off, `a root that cannot be read switches App Store purchases off: ${broken.off}`);

    const sandboxNote = notification(chain, 'TEST', null, { environment: 'Sandbox' });
    const sn = await apple.verifyApple('notification', sandboxNote, made);
    check(sn.ok && sn.environment === 'Sandbox', `a sandbox notification, which carries no appAppleId, verifies: ${sn.ok ? sn.environment : sn.status}`);
    const wrongApp = await apple.verifyApple('notification', notification(chain, 'TEST', null, { environment: 'Production', appAppleId: 42 }), made);
    check(!wrongApp.ok && wrongApp.status === 'INVALID_APP_IDENTIFIER', `a Production notification for another app id is refused: ${wrongApp.status}`);
  }

  console.log('\n— what counts as live, and which wins —');
  {
    const settings = apple.appleSettings({});
    const now = Date.now();
    const why = (over) => apple.appleIgnored(txn(over), settings, now);
    check(why({}) === null, 'a Pro subscription, purchased and not expired: live');
    check(why({ inAppOwnershipType: 'FAMILY_SHARED' }) === null, 'shared through Family Sharing: live too');
    check(why({ expiresDate: now - 1000 }) === 'expired', 'past its expiry: not');
    check(why({ revocationDate: now - 1000 }) === 'revoked', 'refunded or revoked: not');
    check(why({ isUpgraded: true }) === 'upgraded' && why({ isUpgraded: false }) === null,
      'upgraded from (the subscription it was upgraded to took its place): not');
    check(why({ productId: 'org.mailrata.enterprise.monthly' }) === 'product' && why({ productId: 'constructor' }) === 'product', 'a product RATA does not sell here: not');
    check(why({ type: 'Non-Consumable' }) === 'type', 'not an auto-renewable subscription: not');
    check(why({ bundleId: 'com.example' }) === 'bundle', 'another app\'s: not');
    check(why({ originalTransactionId: 'a@b.c' }) === 'id', 'an id that is not Apple\'s kind of number: not');
    const base = txn({ productId: BASE, expiresDate: now + 60 * DAY });
    const pro = txn({ expiresDate: now + 5 * DAY });
    const pro2 = txn({ expiresDate: now + 9 * DAY });
    const order = apple.applePreferred([base, pro, pro2]).map((t) => t.originalTransactionId);
    check(JSON.stringify(order) === JSON.stringify([pro2, pro, base].map((t) => t.originalTransactionId)),
      'Pro before Base whatever the expiry, then the later expiry first');
    const row = { plan: 'pro', expires_at: new Date(now + DAY).toISOString(), grace_until: null, revoked_at: null };
    check(apple.appleLive(row, now) && !apple.appleLive({ ...row, revoked_at: new Date(now).toISOString() }, now), 'a row is live until revoked');
    check(!apple.appleLive({ ...row, expires_at: new Date(now - DAY).toISOString() }, now)
      && apple.appleLive({ ...row, expires_at: new Date(now - DAY).toISOString(), grace_until: new Date(now + DAY).toISOString() }, now),
      'and until it expires, or its billing grace period ends');
    check(!apple.appleLive({ ...row, plan: 'enterprise' }, now) && !apple.appleLive(null, now), 'and only for a plan sold here');

    /* What the AI relay answers: a row not revoked, until the end a licence
       issued from it would have (the margin past what it pays for). */
    const ai = (over) => apple.appleAiLive({ ...row, environment: 'Production', ...over }, now);
    check(ai({}) && ai({ expires_at: new Date(now - 3600e3).toISOString() }) && ai({ expires_at: new Date(now - 2.9 * DAY).toISOString() }),
      'the AI relay answers a live row, and one that expired less than 3 days ago (a renewal not yet recorded)');
    check(!ai({ expires_at: new Date(now - 3.1 * DAY).toISOString() }), 'but not one past the margin');
    check(ai({ environment: 'Sandbox', expires_at: new Date(now - 30 * 60e3).toISOString() }) && !ai({ environment: 'Sandbox', expires_at: new Date(now - 2 * 3600e3).toISOString() }),
      'in the sandbox, an hour');
    check(ai({ expires_at: new Date(now - 10 * DAY).toISOString(), grace_until: new Date(now - DAY).toISOString() }), 'counted from a billing grace period\'s end');
    check(!ai({ revoked_at: new Date(now).toISOString() }) && !ai({ expires_at: new Date(now - 3600e3).toISOString(), revoked_at: new Date(now - 7200e3).toISOString() }),
      'never a refunded or revoked one, at a period\'s end or not');
    check(!apple.appleAiLive(null, now) && !ai({ plan: 'enterprise' }) && !ai({ expires_at: null }), 'nor no row, a plan not sold here, or a row with no end');
    check(apple.appleAiPlan('pro', { plan: 'base' }) === 'base' && apple.appleAiPlan('base', { plan: 'pro' }) === 'base' && apple.appleAiPlan('pro', { plan: 'pro' }) === 'pro',
      'the plan the relay validates against is the lower of the licence\'s and the row\'s');
  }

  console.log('\n— the products, as App Store Connect has them —');
  {
    check(JSON.stringify(APPLE_PRODUCTS) === JSON.stringify({ [PRO]: 'pro', [BASE]: 'base' }),
      `two products, Pro and Base: ${JSON.stringify(APPLE_PRODUCTS)}`);
    check(Object.values(APPLE_PRODUCTS).every((p) => PLANS[p] && PLANS[p].sellable), 'each is a plan on sale');
    check(ORDER.indexOf('pro') > ORDER.indexOf('base'), 'and Pro ranks above Base, as in the subscription group');
    /* The Rust side keeps the same ids, beside the StoreKit commands
       (claude/iap-native). Until a .rs file names one, there is nothing to
       compare.
       TODO(claude/iap-native): read the const by its name once it exists,
       and compare product id to plan, not only the ids. */
    const SRC = join(REPO, 'desktop', 'rata-app');
    const found = new Map();
    for (const f of rustFiles(SRC)) {
      for (const m of readFileSync(f, 'utf8').matchAll(/"(org\.mailrata\.[a-z]+\.(?:monthly|yearly))"/g)) found.set(m[1], relative(REPO, f));
    }
    if (!found.size) {
      console.log('  SKIP  APPLE_PRODUCTS agrees with the Rust const beside the StoreKit commands: no .rs file names a product yet (TODO: claude/iap-native)');
    } else {
      check(JSON.stringify([...found.keys()].sort()) === JSON.stringify(Object.keys(APPLE_PRODUCTS).sort()),
        `APPLE_PRODUCTS agrees with the Rust side (${[...new Set(found.values())].join(', ')}): ${[...found.keys()].join(', ')}`);
    }
  }

  console.log('\n— the licence\'s subject is the purchase, never an address —');
  {
    const id = '2000000123456789';
    const sub = apple.appleSub(id);
    const l = checkLicence(issue({ email: sub, plan: 'pro' }, keys.privateKey), keys.publicKey);
    check(l.ok && l.licence.sub === `apple:${id}` && l.licence.plan === 'pro', `issue() takes apple:<id> as the subject, unchanged: ${l.licence && l.licence.sub}`);
    check(!/@/.test(sub) && apple.appleIdOf(sub) === id, 'it is no address, and reads back as the id');
    check(apple.appleIdOf('buyer@example.com') === null && apple.appleIdOf('apple:12a') === null && apple.appleIdOf('xapple:12') === null && apple.appleIdOf('apple:1@x.y') === null,
      'and nothing else reads as one');
    let threw = false;
    try { apple.appleSub('12; drop'); } catch { threw = true; }
    check(threw, 'a subject is made only from Apple\'s kind of number');
  }

  console.log('\n— the stand-in for Supabase reads PostgREST\'s filters as PostgREST does —');
  {
    const P = '2026-01-10T00:00:00.000Z';
    const S = '2026-01-12T00:00:00.000Z';
    const q = new URLSearchParams();
    q.append('original_transaction_id', 'eq.7');
    q.append('or', `(period_from.lt.${P},and(period_from.eq.${P},signed_at.lte.${S}))`);
    const fs = pgFilters(q);
    const row = (from, signed, id = '7') => ({ original_transaction_id: id, period_from: from, signed_at: signed });
    const cases = [
      ['an earlier period, signed later', row('2026-01-01T00:00:00.000Z', '2026-02-01T00:00:00.000Z'), true],
      ['the same period, signed earlier', row(P, '2026-01-11T00:00:00.000Z'), true],
      ['the same period, signed at the same moment', row(P, S), true],
      ['the same period, the time written as Postgres writes it', row('2026-01-10T00:00:00+00:00', S), true],
      ['the same period, signed later', row(P, '2026-01-13T00:00:00.000Z'), false],
      ['a later period, signed earlier', row('2026-01-11T00:00:00.000Z', '2026-01-01T00:00:00.000Z'), false],
      ['no period (NULL)', row(null, '2026-01-01T00:00:00.000Z'), false],
      ['another subscription', row('2026-01-01T00:00:00.000Z', S, '8'), false],
    ];
    const wrong = cases.filter(([, r, want]) => fs.every((f) => pgHolds(r, f)) !== want).map(([w]) => w);
    check(wrong.length === 0, `or=(lt, and(eq, lte)) holds where Postgres would, and only there: ${wrong.join('; ') || `${cases.length} cases`}`);
    const isNull = pgFilters(new URLSearchParams('period_from=is.null'))[0];
    check(pgHolds(row(null, S), isNull) && !pgHolds(row(P, S), isNull), 'is.null asks for NULL');
    const nested = pgFilters(new URLSearchParams('or=(and(a.eq.1,or(b.eq.2,c.eq.3)),d.eq.4)'));
    const nests = [[{ a: '1', b: '2' }, true], [{ a: '1', c: '3' }, true], [{ a: '1' }, false], [{ d: '4' }, true], [{ b: '2', c: '3' }, false]];
    check(nests.every(([r, want]) => nested.every((f) => pgHolds(r, f)) === want), 'and() and or() nest to any depth');
    const unread = ['a=not.eq.1', 'a=like.x*', 'or=(a.eq.1', 'or=a.eq.1', 'a=eq."1,2"', 'a=is.true', 'or=(a.eq.1,b)'].filter((bad) => {
      try { pgFilters(new URLSearchParams(bad)); return true; } catch { return false; }
    });
    check(unread.length === 0, `and whatever it does not read is refused, not ignored: ${unread.join(', ') || 'all seven'}`);
  }

  /* The servers. Each case that changes a setting needs its own. */
  const sbx = await fakeSupabase();

  console.log('\n— what appleWrite asks the database —');
  {
    /* Through supabase-js itself, so the stand-in is shown the query
       PostgREST would be. */
    const sb = createClient(sbx.url, fakeSupabaseKey('service_role'), { auth: { persistSession: false } });
    const t = txn();
    const id = t.originalTransactionId;
    const from = new Date(t.purchaseDate).toISOString();
    const signed = new Date(t.signedDate).toISOString();
    const first = await apple.appleWrite(sb, id, apple.applePurchase(t, 'Production'));
    const n = sbx.fake.queries.length;
    const again = await apple.appleWrite(sb, id, apple.applePurchase(t, 'Production'));
    const asked = sbx.fake.queries.slice(n).map((x) => x.split('?')[1].replace(/&select=.*/, ''));
    check(first.wrote && again.wrote && asked.length === 2 && asked[0] === `original_transaction_id=eq.${id}&period_from=lt.${from}`
      && asked[1] === `original_transaction_id=eq.${id}&period_from=eq.${from}&signed_at=lte.${signed}`,
      `a purchase: over a row of an earlier period, else over the same period signed no later; nothing about a grace period: ${asked.join(' | ')}`);
    const wrote = sbx.fake.bodies.slice(n).map((b) => Object.keys(JSON.parse(b || '{}')));
    check(wrote.length === 2 && wrote[0].includes('grace_signed_at') && !wrote[1].some((k) => /grace/.test(k)),
      `over the same period it never writes the grace fields, and over an earlier one it starts them afresh: ${wrote.map((k) => k.filter((x) => /grace/.test(x)).join('+') || 'none').join(' | ')}`);
    const at = Date.now() + 1000;
    const notice = apple.appleChange({ notificationType: 'DID_FAIL_TO_RENEW', subtype: 'GRACE_PERIOD', signedDate: at }, t, { gracePeriodExpiresDate: at + 16 * DAY }, 'Production');
    const m = sbx.fake.queries.length;
    const r = await apple.appleWrite(sb, id, notice);
    const three = sbx.fake.queries.slice(m).map((x) => x.split('?')[1].replace(/&select=.*/, ''));
    const stamp = new Date(at).toISOString();
    check(r.wrote && three.length === 3 && three[0] === `original_transaction_id=eq.${id}&period_from=lt.${from}`
      && three[1] === `original_transaction_id=eq.${id}&period_from=eq.${from}&signed_at=lte.${stamp}`
      && three[2] === `original_transaction_id=eq.${id}&period_from=eq.${from}&grace_signed_at=lte.${stamp}`,
      `a notice with renewal info: the subscription's fields so, then the grace period's, over the same period with its own stamp no later: ${three.join(' | ')}`);
    check(r.row.grace_until === new Date(at + 16 * DAY).toISOString() && r.row.grace_signed_at === stamp && r.row.signed_at === stamp && r.row.period_from === from,
      'and the row records both, each with its stamp');
    /* The phone's purchase sent with its renewal info: the grace period
       under the same guard, stamped when Apple signed the renewal info. */
    const rAt = at + 1000;
    const fromPhone = apple.applePurchase(t, 'Production', { originalTransactionId: id, gracePeriodExpiresDate: t.expiresDate + 9 * DAY, signedDate: rAt });
    const k = sbx.fake.queries.length;
    const pw = await apple.appleWrite(sb, id, fromPhone);
    const phoneAsked = sbx.fake.queries.slice(k).map((x) => x.split('?')[1].replace(/&select=.*/, ''));
    const rStamp = new Date(rAt).toISOString();
    check(pw.wrote && phoneAsked.at(-1) === `original_transaction_id=eq.${id}&period_from=eq.${from}&grace_signed_at=lte.${rStamp}`
      && pw.row.grace_until === new Date(t.expiresDate + 9 * DAY).toISOString() && pw.row.grace_signed_at === rStamp && pw.row.signed_at === stamp,
      `the phone's purchase with its renewal info: the grace period over the same period with the renewal info's stamp no later, the older transaction changing nothing else: ${phoneAsked.at(-1)}`);
    const staleAsked = apple.applePurchase(t, 'Production', { originalTransactionId: id, gracePeriodExpiresDate: t.expiresDate + 20 * DAY, signedDate: rAt - 1 });
    const sw = await apple.appleWrite(sb, id, staleAsked);
    check(!sw.wrote && sw.row.grace_until === new Date(t.expiresDate + 9 * DAY).toISOString() && sw.row.grace_signed_at === rStamp,
      'and an older renewal info changes nothing');
    /* A renewal info the phone sends is about the subscription, not a
       period, so only one stating a grace period after the transaction's
       own expiry is used with it: one giving none, or one ending no later
       (an earlier period's), is as though not sent, and never touches the
       grace fields. */
    const graceAt = (g) => ({ originalTransactionId: id, signedDate: rAt + 5000, ...(g === undefined ? {} : { gracePeriodExpiresDate: g }) });
    const ignored = [
      ['none', graceAt(undefined)], ['an earlier period\'s', graceAt(t.expiresDate - DAY)], ['ending at its expiry', graceAt(t.expiresDate)],
      ['not a number', graceAt(String(t.expiresDate + DAY))], ['not finite', graceAt(Infinity)],
    ].filter(([, r]) => apple.appleGraceFor(t, r) || apple.applePurchase(t, 'Production', r).grace !== null);
    check(ignored.length === 0 && apple.appleGraceFor(t, graceAt(t.expiresDate + 1)) && apple.applePurchase(t, 'Production', graceAt(t.expiresDate + 1)).grace !== null
      && !apple.appleGraceFor(t, null) && !apple.appleGraceFor({ ...t, expiresDate: undefined }, graceAt(t.expiresDate + DAY)),
      `appleGraceFor: only a gracePeriodExpiresDate later than the transaction's expiry is used with it${ignored.length ? `; not: ${ignored.map(([w]) => w).join(', ')}` : ''}`);
    const q = sbx.fake.queries.length;
    const nw = await apple.appleWrite(sb, id, apple.applePurchase(t, 'Production', graceAt(undefined)));
    const noneAsked = sbx.fake.queries.slice(q);
    check(!nw.wrote && !noneAsked.some((x) => /grace_signed_at=/.test(x)) && nw.row.grace_until === new Date(t.expiresDate + 9 * DAY).toISOString() && nw.row.grace_signed_at === rStamp,
      'and one giving no grace period, though Apple signed it later, asks nothing of the grace fields and leaves them as they are');
  }

  console.log('\n— every delivery order ends as signing order does —');
  {
    const sb = createClient('http://stand-in.invalid', fakeSupabaseKey('service_role'), { auth: { persistSession: false }, global: { fetch: sbx.fake.fetch } });
    const started = Date.now();
    const ran = await deliveryOrders({ sb, check, label: '' });
    console.log(`  (${ran.orders} orders, ${ran.deliveries} deliveries, on the stand-in, in ${((Date.now() - started) / 1000).toFixed(1)} s)`);
    /* The same against a real Postgres and PostgREST, when one is given:
       APPLE_TEST_POSTGREST=<its address>, over a database with section 7
       of database.sql (and touch_updated_at from section 1) run. */
    const real = String(process.env.APPLE_TEST_POSTGREST || '').trim();
    if (real) {
      const t0 = Date.now();
      const onReal = await deliveryOrders({ sb: viaPostgrest(real), check, label: 'Postgres and PostgREST: ', width: 8 });
      console.log(`  (${onReal.orders} orders, ${onReal.deliveries} deliveries, against ${real}, in ${((Date.now() - t0) / 1000).toFixed(1)} s)`);
    }
  }

  const base = {
    SUPABASE_URL: sbx.url, SUPABASE_SERVICE_ROLE_KEY: fakeSupabaseKey('service_role'),
    LICENCE_PRIVATE_KEY: keys.privateKey, LICENCE_PUBLIC_KEY: keys.publicKey,
  };
  /* An AI key and an address for the model nothing answers at: the relay's
     row check is all these tests reach, never Anthropic. */
  const s = await startServer({ env: { ...base, APPLE_TEST_ROOT: chain.rootBase64, APPLE_APP_ID: String(APP_ID), ANTHROPIC_API_KEY: 'sk-test-never-sent', AI_API_URL: 'http://127.0.0.1:9/never' } });
  const sent = [];
  const post = async (transactions, { raw, origin, renewals } = {}) => {
    if (Array.isArray(transactions)) sent.push(...transactions);
    if (Array.isArray(renewals)) sent.push(...renewals);
    const r = await fetch(s.url + '/api/licence/apple', {
      method: 'POST', headers: { 'Content-Type': 'application/json', ...(origin ? { Origin: origin } : {}) },
      body: raw !== undefined ? raw : JSON.stringify({ transactions, ...(renewals ? { renewalInfos: renewals } : {}) }),
    });
    return { status: r.status, cors: r.headers.get('access-control-allow-origin'), d: await r.json().catch(() => ({})) };
  };
  const notify = async (jws, raw) => {
    const r = await fetch(s.url + '/api/apple/notifications', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: raw !== undefined ? raw : JSON.stringify({ signedPayload: jws }),
    });
    return { status: r.status, d: await r.json().catch(() => ({})) };
  };
  const renew = async (licence) => {
    const r = await fetch(s.url + '/api/licence/renew', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ licence }) });
    return { status: r.status, d: await r.json().catch(() => ({})) };
  };
  const rowOf = (t) => sbx.fake.rows.get(t.originalTransactionId);
  const subOf = (d) => (d.licence ? checkLicence(d.licence, keys.publicKey) : null);
  /* A refusal in /api/licence/renew's shape, { licensed: false, reason,
     message }; /api/licence/apple repeats the sentence as `error`. */
  const refusal = (d, reason, sentence) => d.licensed === false && d.reason === reason && !d.licence
    && typeof d.message === 'string' && (d.error === undefined || d.error === d.message)
    && (sentence === undefined || d.message === sentence);

  try {
    console.log('\n— POST /api/licence/apple: a purchase gives a licence —');
    {
      const t = txn();
      const { status, d } = await post([signJws(t, chain)]);
      const seen = subOf(d);
      check(status === 200 && d.licensed === true && d.plan === 'pro' && seen && seen.ok && seen.licence.sub === `apple:${t.originalTransactionId}` && seen.licence.plan === 'pro',
        `a Pro purchase gives a Pro licence for apple:${t.originalTransactionId}: ${status} ${d.plan}`);
      check(d.expires === new Date(t.expiresDate).toISOString() && d.planLabel === 'RATA Pro' && !('email' in d),
        `the answer says when the subscription runs to, and carries no address: ${d.expires}`);
      const row = rowOf(t);
      check(!!row && row.product_id === PRO && row.plan === 'pro' && row.environment === 'Production' && row.revoked_at === null
        && row.expires_at === new Date(t.expiresDate).toISOString() && row.signed_at === new Date(t.signedDate).toISOString(),
        `and its row is recorded: ${row && JSON.stringify({ product: row.product_id, plan: row.plan, env: row.environment })}`);
      check(row && row.email === null && row.app_account_token === null && Object.keys(row).every((k) => !/name|apple_?id/i.test(k)),
        'with nothing personal in it');
    }
    console.log('\n— an App Store licence lasts as long as what was paid for, and a margin —');
    {
      const exp = (d) => (d.licence ? subOf(d).licence.exp * 1000 : NaN);
      const near = (a, b) => Math.abs(a - b) <= 2000;
      check(apple.APPLE_MARGIN_DAYS === 3 && apple.APPLE_SANDBOX_MARGIN_HOURS === 1,
        `the margins: ${apple.APPLE_MARGIN_DAYS} days for the App Store, ${apple.APPLE_SANDBOX_MARGIN_HOURS} hour in the sandbox`);
      const month = txn();
      const m = await post([signJws(month, chain)]);
      check(near(exp(m.d), Date.now() + 30 * DAY), `a period that ends in 29 days: the licence's own 30 days, the shorter (${new Date(exp(m.d)).toISOString()})`);
      const week = txn({ expiresDate: Date.now() + 5 * DAY });
      const w = await post([signJws(week, chain)]);
      check(near(exp(w.d), week.expiresDate + 3 * DAY) && w.d.expires === new Date(week.expiresDate).toISOString(),
        `a period that ends in 5 days: to 3 days past its end (${new Date(exp(w.d)).toISOString()})`);
      const sandbox = txn({ environment: 'Sandbox', expiresDate: Date.now() + 5 * 60e3 });
      const sb5 = await post([signJws(sandbox, chain)]);
      check(sb5.d.licensed === true && near(exp(sb5.d), sandbox.expiresDate + 3600e3),
        `a sandbox period of five minutes: to an hour past its end, not 30 days (${new Date(exp(sb5.d)).toISOString()})`);
      const lic = issue({ email: apple.appleSub(week.originalTransactionId), plan: 'pro' }, keys.privateKey);
      const r = await renew(lic);
      check(r.d.licensed === true && near(exp(r.d), week.expiresDate + 3 * DAY) && /^Renewed for RATA Pro\. Good offline until \d{4}-\d\d-\d\d\.$/.test(r.d.message),
        `and so from /api/licence/renew: "${r.d.message}"`);
      const rs = await renew(issue({ email: apple.appleSub(sandbox.originalTransactionId), plan: 'pro' }, keys.privateKey));
      check(near(exp(rs.d), sandbox.expiresDate + 3600e3), 'and a sandbox one renews to an hour past its period');
      const stripe = issue({ email: 'buyer@example.com', plan: 'pro' }, keys.privateKey);
      const seenStripe = checkLicence(stripe, keys.publicKey);
      check(seenStripe.ok && seenStripe.licence.exp - seenStripe.licence.iat === 30 * 86400, 'a licence for an address still lasts its 30 days');
      const row = { plan: 'pro', environment: 'Production', expires_at: new Date(Date.now() - DAY).toISOString(), grace_until: new Date(Date.now() + 10 * DAY).toISOString(), revoked_at: null };
      check(near(apple.appleLicenceUntil(row), Date.now() + 13 * DAY), 'in a billing grace period, to 3 days past the grace period');
      let refused = false;
      try { issue({ email: 'apple:1', plan: 'pro', until: Date.now() - 1000 }, keys.privateKey); } catch { refused = true; }
      check(refused, 'and no licence is issued that would end before it began');
    }
    {
      const b = txn({ productId: BASE, expiresDate: Date.now() + 60 * DAY });
      const p = txn({ expiresDate: Date.now() + 3 * DAY });
      const { d } = await post([signJws(b, chain), signJws(p, chain)]);
      check(d.licensed === true && d.plan === 'pro' && subOf(d).licence.sub === `apple:${p.originalTransactionId}`,
        `Base and Pro both live: Pro, though Base runs longer (${d.plan})`);
      const p1 = txn({ expiresDate: Date.now() + 3 * DAY });
      const p2 = txn({ expiresDate: Date.now() + 20 * DAY });
      const two = await post([signJws(p1, chain), signJws(p2, chain)]);
      check(subOf(two.d).licence.sub === `apple:${p2.originalTransactionId}`, 'two of the same plan: the later expiry');
      const alone = await post([signJws(txn({ productId: BASE }), chain)]);
      check(alone.d.licensed === true && alone.d.plan === 'base' && subOf(alone.d).licence.plan === 'base', 'Base alone: a Base licence');
    }
    {
      const sb = txn({ environment: 'Sandbox' });
      const { d } = await post([signJws(sb, chain)]);
      check(d.licensed === true && rowOf(sb) && rowOf(sb).environment === 'Sandbox', 'a sandbox purchase is accepted, and recorded as Sandbox');
      const family = await post([signJws(txn({ inAppOwnershipType: 'FAMILY_SHARED' }), chain)]);
      check(family.d.licensed === true, 'and one shared through Family Sharing');
    }

    console.log('\n— what unlocks nothing —');
    for (const [what, over] of [
      ['an expired one', { expiresDate: Date.now() - 1000 }],
      ['a refunded one', { revocationDate: Date.now() - 1000, revocationReason: 1 }],
      ['one the customer upgraded from', { isUpgraded: true }],
      ['another app\'s', { bundleId: 'com.example.other' }],
      ['one signed for org.mailrata.desktop, the downloaded desktop app', { bundleId: 'org.mailrata.desktop' }],
      ['an unknown product', { productId: 'org.mailrata.lifetime' }],
      ['a purchase that is not a subscription', { type: 'Non-Consumable' }],
    ]) {
      const t = txn(over);
      const { status, d } = await post([signJws(t, chain)]);
      check(status === 200 && refusal(d, 'no-subscription', NOTHING_LIVE) && !rowOf(t),
        `${what}: 200 no-subscription, "${d.message}", and nothing recorded`);
    }
    {
      /* Another app's purchases unlock nothing, as above, but are not
         dropped without trace: if every purchase says so, APPLE_BUNDLE_ID
         does not match the app, and the log is where that shows. */
      const others = [txn({ bundleId: 'com.example.other' }), txn({ bundleId: 'com.example.other' })];
      const before = s.log().length;
      const r = await post(others.map((t) => signJws(t, chain)));
      await new Promise((res) => setTimeout(res, 200));
      const said = s.log().slice(before);
      check(r.status === 200 && refusal(r.d, 'no-subscription', NOTHING_LIVE) && !/APPLE_BUNDLE_ID|INVALID/.test(JSON.stringify(r.d)),
        `two of another app's purchases: no-subscription, "${r.d.message}", and the answer names no setting`);
      check(/licence\/apple: 2 transaction\(s\) were for another app \(INVALID_APP_IDENTIFIER\)/.test(said) && /APPLE_BUNDLE_ID does not match the app/.test(said),
        'the log says how many were another app\'s, by the library\'s status name, and what that means if every purchase says so');
      check(!others.some((t) => said.includes(t.originalTransactionId) || said.includes(t.transactionId)) && !/eyJ/.test(said),
        'naming no transaction and no id');
    }
    {
      const { status, d } = await post([]);
      check(status === 200 && refusal(d, 'no-subscription', NOTHING_LIVE), 'no purchases at all: no-subscription');
      const forged = await post([signJws(txn(), stranger)]);
      check(forged.status === 200 && refusal(forged.d, 'unverified') && /could not verify/.test(forged.d.message) && forged.d.error === forged.d.message,
        `signed by anybody but Apple: 200 unverified, "${forged.d.message}"`);
      const junk = await post(['abc', 'a.b.c']);
      check(junk.status === 200 && refusal(junk.d, 'unverified'), `not a JWS at all: unverified (${junk.d.reason})`);
      const xcode = await post([unsignedJws(txn({ environment: 'Xcode' }))]);
      check(refusal(xcode.d, 'unverified'), `an Xcode purchase without APPLE_ALLOW_LOCAL: unverified (${xcode.d.reason})`);
      const mixed = await post(['abc', signJws(txn({ expiresDate: Date.now() - 1000 }), chain), signJws(txn({ productId: BASE }), chain)]);
      check(mixed.d.licensed === true && mixed.d.plan === 'base', 'one live purchase among a forgery and an expired one still unlocks');
    }
    {
      const oneT = txn();
      const one = signJws(oneT, chain);
      const oneRenewal = signJws(renewalInfo(oneT), chain);
      for (const [what, raw, want] of [
        ['a body that is not JSON', 'not json', 400],
        ['no list', JSON.stringify({ transaction: one }), 400],
        ['a list of something else', JSON.stringify({ transactions: [42] }), 400],
        ['an empty string in it', JSON.stringify({ transactions: [''] }), 400],
        ['eleven', JSON.stringify({ transactions: Array(11).fill(one) }), 400],
        ['one over 16 KiB', JSON.stringify({ transactions: ['a'.repeat(16 * 1024 + 1)] }), 400],
        ['ten, the most', JSON.stringify({ transactions: Array(10).fill(one) }), 200],
        ['renewalInfos that is not a list', JSON.stringify({ transactions: [one], renewalInfos: one }), 400],
        ['renewalInfos holding something else', JSON.stringify({ transactions: [one], renewalInfos: [7] }), 400],
        ['an empty renewal info', JSON.stringify({ transactions: [one], renewalInfos: [''] }), 400],
        ['eleven renewal infos', JSON.stringify({ transactions: [one], renewalInfos: Array(11).fill(oneRenewal) }), 400],
        ['a renewal info over 16 KiB', JSON.stringify({ transactions: [one], renewalInfos: ['a'.repeat(16 * 1024 + 1)] }), 400],
        ['renewalInfos empty', JSON.stringify({ transactions: [one], renewalInfos: [] }), 200],
        ['renewalInfos null', JSON.stringify({ transactions: [one], renewalInfos: null }), 200],
        ['ten transactions and ten renewal infos, the most', JSON.stringify({ transactions: Array(10).fill(one), renewalInfos: Array(10).fill(oneRenewal) }), 200],
      ]) {
        const r = await post(null, { raw });
        check(r.status === want && (want === 200 ? r.d.licensed === true : refusal(r.d, 'malformed')), `${what}: ${r.status}`);
      }
    }

    console.log('\n— the phone\'s renewal info: a billing grace period with no notification —');
    {
      const iso = (ms) => new Date(ms).toISOString();
      const near = (a, b) => Math.abs(a - b) <= 2000;
      const exp = (d) => (d.licence ? subOf(d).licence.exp * 1000 : NaN);
      /* In a billing grace period StoreKit lists the subscription by the
         transaction that has just expired, and its renewal info says when
         the grace period ends. Here no notification has ever arrived, and
         there is no row. */
      const lapsedOf = (over = {}) => txn({ purchaseDate: Date.now() - 31 * DAY, expiresDate: Date.now() - DAY, signedDate: Date.now() - 31 * DAY, ...over });
      const grace = Date.now() + 10 * DAY;
      const t = lapsedOf();
      const r = renewalInfo(t, { gracePeriodExpiresDate: grace, isInBillingRetryPeriod: true, signedDate: Date.now() - 1000 });
      const { status, d } = await post([signJws(t, chain)], { renewals: [signJws(r, chain)] });
      check(status === 200 && d.licensed === true && d.plan === 'pro' && subOf(d).licence.sub === `apple:${t.originalTransactionId}` && d.expires === iso(grace),
        `the expired transaction with its renewal info, and no notification at all: a Pro licence, the subscription running to the grace period's end (${d.reason || d.expires})`);
      check(near(exp(d), grace + 3 * DAY), `and the licence to 3 days past it (${Number.isFinite(exp(d)) ? iso(exp(d)) : 'none'})`);
      const row = rowOf(t);
      check(!!row && row.period_from === iso(t.purchaseDate) && row.expires_at === iso(t.expiresDate) && row.grace_until === iso(grace)
        && row.grace_signed_at === iso(r.signedDate) && row.signed_at === iso(t.signedDate) && row.revoked_at === null,
        'the row records the period and its grace period, the grace period stamped when Apple signed the renewal info');
      const lic = issue({ email: apple.appleSub(t.originalTransactionId), plan: 'pro' }, keys.privateKey);
      const renewed = await renew(lic);
      check(renewed.d.licensed === true && near(exp(renewed.d), grace + 3 * DAY), `and /api/licence/renew renews from that row (${renewed.d.reason || renewed.d.plan})`);

      /* A row the phone recorded for the period before, and nothing since:
         the period that has ended (and failed to renew) was never seen. */
      const id = nextId();
      const before = { purchaseDate: Date.now() - 62 * DAY, expiresDate: Date.now() - 31 * DAY };
      sbx.fake.rows.set(id, {
        original_transaction_id: id, product_id: PRO, plan: 'pro', environment: 'Production', period_from: iso(before.purchaseDate), expires_at: iso(before.expiresDate),
        grace_until: null, grace_signed_at: iso(0), revoked_at: null, signed_at: iso(before.purchaseDate), app_account_token: null, email: null,
      });
      const m2 = lapsedOf({ originalTransactionId: id });
      const without = await post([signJws(m2, chain)]);
      check(refusal(without.d, 'no-subscription', NOTHING_LIVE) && rowOf(m2).period_from === iso(before.purchaseDate),
        'without its renewal info, the expired transaction of a period the row has not heard of unlocks nothing and writes nothing, as before');
      const withIt = await post([signJws(m2, chain)], { renewals: [signJws(renewalInfo(m2, { gracePeriodExpiresDate: grace }), chain)] });
      check(withIt.d.licensed === true && rowOf(m2).period_from === iso(m2.purchaseDate) && rowOf(m2).grace_until === iso(grace),
        `with it, the row moves on to that period and its grace period, and it unlocks (${withIt.d.reason || withIt.d.plan})`);

      const none = lapsedOf();
      const n = await post([signJws(none, chain)], { renewals: [signJws(renewalInfo(none, { isInBillingRetryPeriod: true }), chain)] });
      check(refusal(n.d, 'no-subscription', NOTHING_LIVE) && !rowOf(none),
        'an expired transaction whose renewal info gives no grace period is as one sent alone: no-subscription, and nothing written');

      const live = txn();
      const lr = renewalInfo(live, { signedDate: Date.now() - 1000 });
      const l = await post([signJws(live, chain)], { renewals: [signJws(lr, chain)] });
      check(l.d.licensed === true && rowOf(live).grace_until === null && rowOf(live).grace_signed_at === iso(0),
        'a live purchase with its renewal info, which gives no grace period: licensed, and recorded as the purchase alone');

      /* Apple has since ended the grace period (EXPIRED, its renewal info
         giving none), and the phone sends renewal info StoreKit signed
         before that, still giving the grace period. */
      const ended = lapsedOf({ signedDate: Date.now() - 3 * 3600e3 });
      const endedAt = Date.now() - 3600e3;
      await notify(notification(chain, 'EXPIRED', ended, {
        subtype: 'BILLING_RETRY', renewal: renewalInfo(ended, { signedDate: endedAt }), signedDate: endedAt,
      }));
      check(!!rowOf(ended) && rowOf(ended).grace_until === null && rowOf(ended).grace_signed_at === iso(endedAt), 'EXPIRED, its renewal info giving no grace period, is recorded');
      const held = JSON.stringify(rowOf(ended));
      const staleInfo = renewalInfo(ended, { gracePeriodExpiresDate: grace, isInBillingRetryPeriod: true, signedDate: Date.now() - 2 * 3600e3 });
      const stale = await post([signJws(ended, chain)], { renewals: [signJws(staleInfo, chain)] });
      check(refusal(stale.d, 'no-subscription', NOTHING_LIVE) && JSON.stringify(rowOf(ended)) === held && rowOf(ended).grace_until === null,
        `renewal info Apple signed before a notification that has since ended the grace period changes nothing: "${stale.d.message}"`);
      const fresh = await post([signJws({ ...ended, signedDate: Date.now() }, chain)], { renewals: [signJws(staleInfo, chain)] });
      check(refusal(fresh.d, 'no-subscription') && rowOf(ended).grace_until === null && rowOf(ended).grace_signed_at === iso(endedAt),
        'nor with the transaction StoreKit has signed again since: a transaction never touches the grace period, and the renewal info is older than what Apple said');

      /* Refused: a renewal info for another subscription, from another
         environment, for a refunded transaction, or not signed by Apple. */
      const a = lapsedOf();
      const b = lapsedOf();
      const at = s.log().length;
      const cross = await post([signJws(a, chain)], { renewals: [signJws(renewalInfo(b, { gracePeriodExpiresDate: grace }), chain)] });
      check(refusal(cross.d, 'no-subscription', NOTHING_LIVE) && !rowOf(a) && !rowOf(b),
        `a renewal info for another subscription is refused: "${cross.d.message}", and nothing is recorded for either`);
      const elsewhere = await post([signJws(a, chain)], { renewals: [signJws(renewalInfo(a, { environment: 'Sandbox', gracePeriodExpiresDate: grace }), chain)] });
      check(refusal(elsewhere.d, 'no-subscription') && !rowOf(a), 'so is one from another environment than its transaction');
      const refunded = txn({ revocationDate: Date.now() - 1000, revocationReason: 0, expiresDate: Date.now() - DAY });
      const ref = await post([signJws(refunded, chain)], { renewals: [signJws(renewalInfo(refunded, { gracePeriodExpiresDate: grace }), chain)] });
      check(refusal(ref.d, 'no-subscription') && !rowOf(refunded), 'and one whose transaction unlocks nothing (refunded)');
      const forged = await post([signJws(a, chain)], { renewals: [signJws(renewalInfo(a, { gracePeriodExpiresDate: grace }), stranger)] });
      check(refusal(forged.d, 'no-subscription') && !rowOf(a), 'and one signed by anybody but Apple');
      await new Promise((res) => setTimeout(res, 200));
      const said = s.log().slice(at);
      check(/licence\/apple: 1 renewal info\(s\) matched no transaction sent with them, so refused/.test(said) && /licence\/apple: 1 renewal info\(s\) did not verify: /.test(said)
        && ![a, b, refunded].some((x) => said.includes(x.originalTransactionId)),
        'the log counts each refused one, naming no transaction');
      const both = await post([signJws(a, chain), signJws(b, chain)], { renewals: [signJws(renewalInfo(b, { gracePeriodExpiresDate: grace }), chain)] });
      check(both.d.licensed === true && subOf(both.d).licence.sub === `apple:${b.originalTransactionId}` && !rowOf(a) && rowOf(b).grace_until === iso(grace),
        'sent with both transactions, it records the grace period of its own subscription only');

      /* Billing recovers while StoreKit's copy lags. P1 ended 5 days ago,
         and Apple's grace notice recorded a grace period to 11 days from
         now; the phone, sending P1 alone, is licensed. Billing then
         recovers (P2, ten minutes ago) and, before Apple's DID_RENEW
         reaches the row, the app sends P1, still in
         Transaction.currentEntitlements, beside the renewal info it has
         just fetched from Apple: signed now, no grace period, renewing at
         P2's end. That renewal info says nothing about P1's period, so it
         changes nothing, and the customer, who has paid, stays licensed
         from the grace period recorded. */
      const p1 = lapsedOf({ purchaseDate: Date.now() - 35 * DAY, expiresDate: Date.now() - 5 * DAY, signedDate: Date.now() - 35 * DAY });
      const graceEnd = Date.now() + 11 * DAY;
      const noticed = await notify(notification(chain, 'DID_FAIL_TO_RENEW', p1, {
        subtype: 'GRACE_PERIOD', signedDate: Date.now() - 5 * DAY,
        renewal: renewalInfo(p1, { gracePeriodExpiresDate: graceEnd, isInBillingRetryPeriod: true, signedDate: Date.now() - 5 * DAY }),
      }));
      const inGrace = await post([signJws(p1, chain)]);
      check(noticed.status === 200 && inGrace.d.licensed === true && !!rowOf(p1) && rowOf(p1).grace_until === iso(graceEnd),
        `a grace notice recorded, and P1 sent alone: licensed to the grace period's end (${inGrace.d.reason || inGrace.d.expires})`);
      const recorded = JSON.stringify(rowOf(p1));
      const p2End = p1.expiresDate + 30 * DAY;
      const recoveredInfo = renewalInfo(p1, { signedDate: Date.now(), renewalDate: p2End });
      const lagging = await post([signJws(p1, chain)], { renewals: [signJws(recoveredInfo, chain)] });
      check(lagging.d.licensed === true && lagging.d.expires === iso(graceEnd) && JSON.stringify(rowOf(p1)) === recorded,
        `P1 beside a fresh renewal info giving no grace period (billing recovered, P2 not yet seen): still licensed, and the row unchanged, its grace period kept (${lagging.d.reason || lagging.d.expires}; ${rowOf(p1).grace_until})`);
      const aloneAgain = await post([signJws(p1, chain)]);
      const p1Licence = issue({ email: apple.appleSub(p1.originalTransactionId), plan: 'pro' }, keys.privateKey);
      const p1Renewed = await renew(p1Licence);
      const aiAt = sbx.fake.calls.length;
      const p1Ai = await fetch(s.url + '/api/ai', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ task: 'translate', subject: 'Hi', text: 'Hello there', to: 'fr', licence: p1Licence }) });
      const p1Reserved = sbx.fake.calls.slice(aiAt).some((c) => c.endsWith('/rest/v1/rpc/ai_reserve'));
      check(aloneAgain.d.licensed === true && p1Renewed.d.licensed === true && p1Ai.status !== 401 && p1Reserved,
        `and afterwards P1 alone still licenses, /api/licence/renew renews, and the AI relay answers (${aloneAgain.d.reason || 'licensed'}, ${p1Renewed.d.reason || 'renewed'}, AI ${p1Ai.status}, here a stand-in with no ai_reserve)`);

      /* The other way round: the next period's transaction beside the
         renewal info of the grace period before it, which StoreKit signed
         nine days ago. That grace period ends before this period does, so
         it is not this period's, and the row of the new period starts with
         none. */
      const q1 = lapsedOf({ purchaseDate: Date.now() - 39 * DAY, expiresDate: Date.now() - 9 * DAY, signedDate: Date.now() - 39 * DAY });
      const oldInfo = renewalInfo(q1, { gracePeriodExpiresDate: q1.expiresDate + 16 * DAY, isInBillingRetryPeriod: true, signedDate: Date.now() - 9 * DAY });
      const q2 = txn({ originalTransactionId: q1.originalTransactionId, purchaseDate: Date.now() - 2 * DAY, expiresDate: q1.expiresDate + 30 * DAY });
      const nextPeriod = await post([signJws(q2, chain)], { renewals: [signJws(oldInfo, chain)] });
      check(nextPeriod.d.licensed === true && rowOf(q2).period_from === iso(q2.purchaseDate) && rowOf(q2).grace_until === null && rowOf(q2).grace_signed_at === iso(0),
        `the next period beside the renewal info of the grace period before it: recorded with no grace period (${rowOf(q2) && rowOf(q2).grace_until})`);

      /* Several renewal infos for one subscription, in either order: of
         those stating a grace period for its transaction, the one Apple
         signed last. One giving none is never chosen, though Apple signed
         it later. */
      for (const [order, put] of [['sent last', (x, y) => [x, y]], ['sent first', (x, y) => [y, x]]]) {
        const t2 = lapsedOf();
        const earlier = renewalInfo(t2, { gracePeriodExpiresDate: Date.now() + 5 * DAY, isInBillingRetryPeriod: true, signedDate: Date.now() - 2 * 3600e3 });
        const later = renewalInfo(t2, { gracePeriodExpiresDate: Date.now() + 10 * DAY, isInBillingRetryPeriod: true, signedDate: Date.now() - 3600e3 });
        const two = await post([signJws(t2, chain)], { renewals: put(earlier, later).map((x) => signJws(x, chain)) });
        check(two.d.licensed === true && two.d.expires === iso(later.gracePeriodExpiresDate) && rowOf(t2).grace_until === iso(later.gracePeriodExpiresDate)
          && rowOf(t2).grace_signed_at === iso(later.signedDate),
          `two renewal infos stating a grace period, the later-signed one ${order}: the one Apple signed last is used (${rowOf(t2) && rowOf(t2).grace_until})`);
        const t3 = lapsedOf();
        const graced = renewalInfo(t3, { gracePeriodExpiresDate: Date.now() + 10 * DAY, isInBillingRetryPeriod: true, signedDate: Date.now() - 2 * 3600e3 });
        const plain = renewalInfo(t3, { signedDate: Date.now() - 3600e3 });
        const mixed = await post([signJws(t3, chain)], { renewals: put(graced, plain).map((x) => signJws(x, chain)) });
        check(mixed.d.licensed === true && rowOf(t3).grace_until === iso(graced.gracePeriodExpiresDate) && rowOf(t3).grace_signed_at === iso(graced.signedDate),
          `beside a later-signed one giving no grace period, ${order}: the one stating it is used (${rowOf(t3) && rowOf(t3).grace_until})`);
      }
    }

    console.log('\n— nothing older than what is recorded overwrites it —');
    {
      const t = txn({ signedDate: Date.now() - 10 * DAY });
      sbx.fake.rows.set(t.originalTransactionId, {
        original_transaction_id: t.originalTransactionId, product_id: PRO, plan: 'pro', environment: 'Production',
        period_from: new Date(t.purchaseDate).toISOString(), expires_at: new Date(t.expiresDate).toISOString(), grace_until: null, revoked_at: new Date(Date.now() - DAY).toISOString(),
        signed_at: new Date(Date.now() - DAY).toISOString(), app_account_token: null, email: null,
      });
      const replay = await post([signJws(t, chain)]);
      check(refusal(replay.d, 'no-subscription', NOTHING_LIVE) && !!rowOf(t).revoked_at,
        'a transaction Apple signed before the refund that is recorded unlocks nothing, and the refund stays');
      const again = txn({ originalTransactionId: t.originalTransactionId, purchaseDate: Date.now() - 1000, signedDate: Date.now() });
      const back = await post([signJws(again, chain)]);
      check(back.d.licensed === true && rowOf(t).revoked_at === null && rowOf(t).signed_at === new Date(again.signedDate).toISOString(),
        'one signed after it, with no revocation (a new subscription), unlocks again and is recorded');

      const b = txn({ productId: BASE });
      await post([signJws(b, chain)]);
      const older = txn({ originalTransactionId: b.originalTransactionId, productId: PRO, purchaseDate: b.purchaseDate, expiresDate: b.expiresDate, signedDate: b.signedDate - DAY });
      const o = await post([signJws(older, chain)]);
      check(o.d.licensed === true && o.d.plan === 'base' && rowOf(b).plan === 'base' && rowOf(b).product_id === BASE,
        `an older Pro transaction for the same period never raises a newer Base row, and the licence follows the row (${o.d.plan})`);
      const earlier = txn({ originalTransactionId: b.originalTransactionId, productId: PRO, purchaseDate: b.purchaseDate - DAY, expiresDate: b.expiresDate + 60 * DAY, signedDate: b.signedDate + DAY });
      const e = await post([signJws(earlier, chain)]);
      check(e.d.plan === 'base' && rowOf(b).plan === 'base' && rowOf(b).expires_at === new Date(b.expiresDate).toISOString(),
        'nor one for an earlier period, though Apple signed it later and it runs longer');
    }

    console.log('\n— a fault at this end issues nothing —');
    {
      sbx.fake.mode = 'broken';
      const t = txn();
      const r = await post([signJws(t, chain)]);
      sbx.fake.mode = 'ok';
      check(r.status === 503 && refusal(r.d, 'unavailable') && /try again/i.test(r.d.message), `the database fails: 503, no licence: "${r.d.message}"`);
      sbx.fake.mode = 'broken';
      const lapsedDown = await post([signJws(txn({ expiresDate: Date.now() - DAY }), chain)]);
      sbx.fake.mode = 'ok';
      check(lapsedDown.status === 503 && refusal(lapsedDown.d, 'unavailable'), `and so does reading the row of an expired one: 503, no licence (${lapsedDown.status})`);
      /* The log names what failed: writing the row (a live transaction, or
         an expired one with a renewal info stating its grace period), or
         only reading it (an expired one without one). */
      const wordAt = s.log().length;
      const said = async (transaction, renewals) => {
        const from = s.log().length;
        sbx.fake.mode = 'broken';
        const r = await post([signJws(transaction, chain)], renewals ? { renewals: renewals.map((x) => signJws(x, chain)) } : {});
        sbx.fake.mode = 'ok';
        await new Promise((res) => setTimeout(res, 200));
        return { status: r.status, line: (s.log().slice(from).match(/licence\/apple: could not (\w+) an App Store purchase/) || [])[1] };
      };
      const expiredOne = txn({ purchaseDate: Date.now() - 31 * DAY, expiresDate: Date.now() - DAY });
      const words = [
        await said(txn()),
        await said(expiredOne),
        await said(expiredOne, [renewalInfo(expiredOne, { gracePeriodExpiresDate: Date.now() + 10 * DAY })]),
        await said(expiredOne, [renewalInfo(expiredOne)]),
      ];
      check(words.every((w) => w.status === 503) && words.map((w) => w.line).join(',') === 'record,read,record,read' && !/eyJ/.test(s.log().slice(wordAt)),
        `the log says which failed, a write or a read: live, ${words[0].line}; expired, ${words[1].line}; expired with its grace period, ${words[2].line}; expired with a renewal info giving none, ${words[3].line}`);
      sbx.fake.mode = 'missing';
      const before = s.log().length;
      const m = await post([signJws(txn(), chain)]);
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(m.status === 503 && refusal(m.d, 'not-configured', OFF), `apple_subscriptions missing: 503 "${m.d.message}"`);
      check(/run section 7 of rata-next\/database\.sql/.test(s.log().slice(before)) && !/section 7|database/i.test(JSON.stringify(m.d)),
        'the server log names section 7, the answer does not');
      sbx.fake.mode = 'behind';
      const behindAt = s.log().length;
      const old = await post([signJws(txn(), chain)]);
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(old.status === 503 && refusal(old.d, 'not-configured', OFF) && /run section 7 of rata-next\/database\.sql/.test(s.log().slice(behindAt)),
        `a table made by an earlier copy of section 7, without a column it now adds (PGRST204): 503 "${old.d.message}", and the log names section 7`);
    }

    console.log('\n— the app may call it, and nobody else may read the answer —');
    {
      const pre = await fetch(s.url + '/api/licence/apple', { method: 'OPTIONS', headers: { Origin: 'tauri://localhost', 'Access-Control-Request-Method': 'POST', 'Access-Control-Request-Headers': 'content-type' } });
      check(pre.status === 204 && pre.headers.get('access-control-allow-origin') === 'tauri://localhost', `preflight from the app: ${pre.status}`);
      const mine = await post([signJws(txn(), chain)], { origin: 'tauri://localhost' });
      const theirs = await post([signJws(txn(), chain)], { origin: 'https://evil.example' });
      check(mine.cors === 'tauri://localhost' && theirs.cors === null, `the app may read the answer, another site may not (${theirs.cors})`);
    }

    console.log('\n— POST /api/apple/notifications —');
    {
      const calls = sbx.fake.calls.length;
      const test = await notify(notification(chain, 'TEST', null, { environment: 'Production' }));
      check(test.status === 200 && test.d.acted === false && sbx.fake.calls.length === calls, `TEST: 200, nothing written (${test.status})`);
      const forged = await notify(notification(chain, 'DID_RENEW', txn(), { signer: stranger }));
      check(forged.status === 400 && JSON.stringify(forged.d) === JSON.stringify({ error: 'Bad request' }), `a signature that is not Apple's: 400, and no detail: ${JSON.stringify(forged.d)}`);
      check((await notify(null, 'not json')).status === 400 && (await notify(null, '{}')).status === 400, 'a body that is not a notification: 400');
      const other = await notify(notification(chain, 'DID_RENEW', txn(), { appAppleId: 42 }));
      check(other.status === 400, `a notification for another app: 400 (${other.status})`);
      /* org.mailrata.desktop is the downloaded desktop app's id, which sells
         nothing through Apple: a notification for it, or one for RATA's App
         Store app carrying a transaction signed for it, is another app's. */
      const asked = sbx.fake.calls.length;
      const desktopTx = txn({ environment: 'Sandbox', bundleId: 'org.mailrata.desktop' });
      const desktopNote = await notify(notification(chain, 'DID_RENEW', desktopTx, { environment: 'Sandbox', bundleId: 'org.mailrata.desktop' }));
      const innerTx = txn({ bundleId: 'org.mailrata.desktop' });
      const inner = await notify(notification(chain, 'DID_RENEW', innerTx));
      check(desktopNote.status === 400 && !rowOf(desktopTx) && JSON.stringify(desktopNote.d) === JSON.stringify({ error: 'Bad request' }),
        `a Sandbox notification for org.mailrata.desktop: 400, no detail, nothing written (${desktopNote.status})`);
      check(inner.status === 400 && !rowOf(innerTx) && sbx.fake.calls.length === asked,
        `a notification for ${BUNDLE} whose transaction is signed for org.mailrata.desktop: 400, and the database is not asked (${inner.status})`);

      const t = txn({ signedDate: Date.now() - 5 * DAY });
      const sub = await notify(notification(chain, 'SUBSCRIBED', t, { subtype: 'INITIAL_BUY', signedDate: Date.now() - 5 * DAY }));
      const row = rowOf(t);
      check(sub.status === 200 && sub.d.acted === true && row && row.plan === 'pro' && row.product_id === PRO && row.environment === 'Production',
        `SUBSCRIBED for a subscription not seen yet creates its row (${sub.status})`);

      const renewed = { ...t, transactionId: nextId(), expiresDate: t.expiresDate + 30 * DAY, signedDate: Date.now() - 4 * DAY };
      await notify(notification(chain, 'DID_RENEW', renewed, { signedDate: Date.now() - 4 * DAY }));
      check(rowOf(t).expires_at === new Date(renewed.expiresDate).toISOString(), 'DID_RENEW moves the expiry on');

      const moved = { ...renewed, productId: BASE };
      await notify(notification(chain, 'DID_CHANGE_RENEWAL_PREF', moved, { subtype: 'DOWNGRADE', signedDate: Date.now() - 3.5 * DAY }));
      check(rowOf(t).plan === 'base' && rowOf(t).product_id === BASE, 'DID_CHANGE_RENEWAL_PREF writes the product and plan its transaction names');

      const offered = { ...renewed, productId: PRO, expiresDate: renewed.expiresDate + 5 * DAY };
      await notify(notification(chain, 'OFFER_REDEEMED', offered, { signedDate: Date.now() - 3 * DAY }));
      check(rowOf(t).plan === 'pro' && rowOf(t).expires_at === new Date(offered.expiresDate).toISOString(), 'OFFER_REDEEMED too, with its expiry');

      const lapsed = { ...offered, expiresDate: Date.now() - DAY };
      const grace = Date.now() + 6 * DAY;
      await notify(notification(chain, 'DID_FAIL_TO_RENEW', lapsed, { subtype: 'GRACE_PERIOD', renewal: renewalInfo(lapsed, { gracePeriodExpiresDate: grace, isInBillingRetryPeriod: true }), signedDate: Date.now() - 2 * DAY }));
      check(rowOf(t).grace_until === new Date(grace).toISOString() && rowOf(t).expires_at === new Date(lapsed.expiresDate).toISOString(),
        'DID_FAIL_TO_RENEW with GRACE_PERIOD records when the grace period ends');
      const lic = issue({ email: apple.appleSub(t.originalTransactionId), plan: 'pro' }, keys.privateKey);
      const inGrace = await renew(lic);
      check(inGrace.d.licensed === true && inGrace.d.plan === 'pro', `renewing while in the grace period: licensed (${inGrace.d.reason || inGrace.d.plan})`);
      /* In a billing grace period StoreKit's current entitlements still
         list the subscription, by the transaction that has just expired;
         the phone asks /api/licence/apple with it, not /renew. The row
         says the grace period has not ended, so it is live; and a
         transaction past its expiry writes nothing over the row. */
      const held = JSON.stringify(rowOf(t));
      const phone = await post([signJws({ ...lapsed, signedDate: Date.now() }, chain)]);
      check(phone.status === 200 && phone.d.licensed === true && phone.d.plan === 'pro' && subOf(phone.d).licence.sub === `apple:${t.originalTransactionId}`,
        `the phone in the grace period, with its expired transaction: a Pro licence (${phone.d.reason || phone.d.plan})`);
      check(phone.d.expires === new Date(grace).toISOString() && JSON.stringify(rowOf(t)) === held,
        `which says the subscription runs to the end of the grace period (${phone.d.expires}), and the row is left as Apple's notification wrote it`);
      const baseLive = txn({ productId: BASE });
      const both = await post([signJws(baseLive, chain), signJws({ ...lapsed, signedDate: Date.now() }, chain)]);
      check(both.d.plan === 'pro' && subOf(both.d).licence.sub === `apple:${t.originalTransactionId}`,
        `a Pro subscription in its grace period still ranks above a live Base one (${both.d.plan})`);

      await notify(notification(chain, 'GRACE_PERIOD_EXPIRED', lapsed, { renewal: renewalInfo(lapsed, { gracePeriodExpiresDate: Date.now() - 1000 }), signedDate: Date.now() - 1.5 * DAY }));
      check(rowOf(t).plan === 'pro' && Date.parse(rowOf(t).grace_until) < Date.now(), 'GRACE_PERIOD_EXPIRED changes only the dates');
      const after = await renew(lic);
      check(refusal(after.d, 'no-subscription', NOTHING_LIVE), `and renewing then: no-subscription, "${after.d.message}"`);
      const ended = await post([signJws({ ...lapsed, signedDate: Date.now() }, chain)]);
      check(refusal(ended.d, 'no-subscription', NOTHING_LIVE) && Date.parse(rowOf(t).grace_until) < Date.now(),
        `nor from the phone, once the grace period has ended: "${ended.d.message}"`);

      const back = { ...lapsed, transactionId: nextId(), expiresDate: Date.now() + 30 * DAY };
      await notify(notification(chain, 'DID_RENEW', back, { subtype: 'BILLING_RECOVERY', signedDate: Date.now() - DAY }));
      check((await renew(lic)).d.licensed === true, 'a renewal that recovers the payment makes it live again');

      const refundedAt = Date.now() - 0.5 * DAY;
      await notify(notification(chain, 'REFUND', { ...back, revocationDate: refundedAt, revocationReason: 0 }, { signedDate: refundedAt }));
      check(rowOf(t).revoked_at === new Date(refundedAt).toISOString() && rowOf(t).expires_at === new Date(back.expiresDate).toISOString() && rowOf(t).plan === 'pro',
        'REFUND records when it was revoked, and nothing else');
      check(refusal((await renew(lic)).d, 'no-subscription', NOTHING_LIVE), 'renewing a refunded one: no-subscription');

      await notify(notification(chain, 'REFUND_REVERSED', back, { signedDate: Date.now() - 0.4 * DAY }));
      check(rowOf(t).revoked_at === null && (await renew(lic)).d.licensed === true, 'REFUND_REVERSED clears it, and it renews again');

      const stale = await notify(notification(chain, 'REFUND', { ...back, revocationDate: Date.now() - 4 * DAY }, { signedDate: Date.now() - 4 * DAY }));
      check(stale.status === 200 && stale.d.acted === false && rowOf(t).revoked_at === null,
        'a notification Apple signed before the one recorded changes nothing, and is still answered 200');

      await notify(notification(chain, 'EXPIRED', { ...back, expiresDate: Date.now() - 1000 }, { subtype: 'VOLUNTARY', signedDate: Date.now() - 0.2 * DAY }));
      check(Date.parse(rowOf(t).expires_at) < Date.now() && rowOf(t).plan === 'pro' && rowOf(t).revoked_at === null, 'EXPIRED changes only the dates');
      check(refusal((await renew(lic)).d, 'no-subscription'), 'and renewing then: no-subscription');

      /* A refund is of one transaction, and can be of a period that has
         already ended while the one paid for now goes on: the row, which
         is the whole subscription, is revoked only by a refund of a period
         no earlier than the one it records. */
      const first = txn({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() - 10 * DAY, signedDate: Date.now() - 40 * DAY });
      await notify(notification(chain, 'SUBSCRIBED', first, { signedDate: Date.now() - 40 * DAY }));
      const second = { ...first, transactionId: nextId(), purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 20 * DAY };
      await notify(notification(chain, 'DID_RENEW', second, { signedDate: Date.now() - 10 * DAY }));
      const paid = issue({ email: apple.appleSub(first.originalTransactionId), plan: 'pro' }, keys.privateKey);
      const early = await notify(notification(chain, 'REFUND', { ...first, revocationDate: Date.now() - DAY, revocationReason: 0 }, { signedDate: Date.now() - DAY }));
      check(early.status === 200 && early.d.acted === false && rowOf(first).revoked_at === null && (await renew(paid)).d.licensed === true,
        'a REFUND of a period that has ended leaves the one paid for now live, and is answered 200');
      await notify(notification(chain, 'REFUND', { ...second, revocationDate: Date.now() - 0.5 * DAY, revocationReason: 0 }, { signedDate: Date.now() - 0.5 * DAY }));
      check(!!rowOf(first).revoked_at && refusal((await renew(paid)).d, 'no-subscription'), 'a REFUND of the current period revokes it');
      const undo = await notify(notification(chain, 'REFUND_REVERSED', first, { signedDate: Date.now() - 0.4 * DAY }));
      check(undo.d.acted === false && !!rowOf(first).revoked_at, 'and reversing the earlier refund does not undo it');
      await notify(notification(chain, 'REFUND_REVERSED', second, { signedDate: Date.now() - 0.3 * DAY }));
      check(rowOf(first).revoked_at === null && (await renew(paid)).d.licensed === true, 'reversing the current period\'s refund does');

      /* The order of a subscription's periods is when each began (its
         transaction's purchaseDate), never when each ends: a period paid
         for in advance (an introductory offer of several months, an offer
         code) can end after the one that replaced it, as can one upgraded
         from. Within one period, when Apple signed it. Each sequence below
         is one Apple can produce; `later` is a further transaction of the
         same subscription, `upFront` a three-month introductory offer paid
         in advance. */
      const later = (t, over) => ({ ...t, transactionId: nextId(), offerType: undefined, offerDiscountType: undefined, ...over });
      const upFront = (over) => txn({ offerType: 1, offerDiscountType: 'PAY_UP_FRONT', ...over });
      const licOf = (t) => issue({ email: apple.appleSub(t.originalTransactionId), plan: 'pro' }, keys.privateKey);
      const at = (ms) => new Date(ms).toISOString();
      const records = (t, p) => rowOf(t).period_from === at(p.purchaseDate) && rowOf(t).expires_at === at(p.expiresDate) && rowOf(t).plan === APPLE_PRODUCTS[p.productId];

      /* Notifications out of order. Apple signs the renewal for the next
         period (P2), then a refund of the period before it (P1). The
         renewal is answered 500 (the database was down) and Apple sends it
         again hours later, after the refund, which finds the row still
         recording P1 and revokes it. The renewal, though Apple signed it
         first, is for a later period, so it still lands; and so does the
         phone's own transaction for P2, signed before the refund. Either
         way the customer who paid for P2 is live until P2 ends; and so
         when P2 is an upgrade that ends before P1, paid for in advance,
         would have. */
      for (const via of ['the late renewal', 'the phone', 'an upgrade, late']) {
        const upgrade = via === 'an upgrade, late';
        const p1 = upgrade
          ? upFront({ productId: BASE, purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() + 50 * DAY, signedDate: Date.now() - 40 * DAY })
          : txn({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() - 10 * DAY, signedDate: Date.now() - 40 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', p1, { signedDate: p1.signedDate }));
        const p2 = later(p1, { productId: PRO, purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 20 * DAY, signedDate: Date.now() - 10 * DAY });
        const renewal = upgrade
          ? notification(chain, 'DID_CHANGE_RENEWAL_PREF', p2, { subtype: 'UPGRADE', signedDate: p2.signedDate })
          : notification(chain, 'DID_RENEW', p2, { signedDate: p2.signedDate });
        sbx.fake.mode = 'broken';
        const lost = await notify(renewal);
        sbx.fake.mode = 'ok';
        const refundAt = Date.now() - DAY;
        const refund = notification(chain, 'REFUND', { ...p1, revocationDate: refundAt, revocationReason: 0 }, { signedDate: refundAt });
        const refunded = await notify(refund);
        const lic = licOf(p1);
        check(lost.status === 500 && refunded.d.acted === true && !!rowOf(p1).revoked_at && refusal((await renew(lic)).d, 'no-subscription'),
          `(${via}) P2's ${upgrade ? 'upgrade' : 'renewal'} answered 500, then P1's refund, signed after it, revokes the row still recording P1`);
        if (via === 'the phone') {
          const answer = await post([signJws({ ...p2, signedDate: refundAt - DAY }, chain)]);
          check(answer.status === 200 && answer.d.licensed === true && answer.d.plan === 'pro' && answer.d.expires === at(p2.expiresDate)
            && subOf(answer.d).licence.sub === `apple:${p1.originalTransactionId}`,
            `the phone sends P2's transaction, signed before the refund: a Pro licence, to P2's end (${answer.d.reason || answer.d.expires})`);
        } else {
          const again = await notify(renewal);
          check(again.status === 200 && again.d.acted === true, `(${via}) Apple sends it again: 200, recorded (${again.status})`);
        }
        check(rowOf(p1).revoked_at === null && records(p1, p2), `(${via}) the row records P2, not revoked, with P2's start and expiry`);
        const renewed = await renew(lic);
        check(renewed.d.licensed === true && renewed.d.plan === 'pro', `(${via}) and /api/licence/renew renews it (${renewed.d.reason || renewed.d.plan})`);
        if (via !== 'the phone') {
          const phone = await post([signJws(p2, chain)]);
          check(phone.d.licensed === true && phone.d.expires === at(p2.expiresDate), `(${via}) as does the phone, with P2's transaction`);
        }
        /* News of the earlier period, delivered again, still changes
           nothing. */
        const replayed = [await notify(refund), await notify(notification(chain, 'SUBSCRIBED', p1, { signedDate: p1.signedDate }))];
        check(replayed.every((r) => r.status === 200 && r.d.acted === false) && rowOf(p1).revoked_at === null && records(p1, p2),
          `(${via}) P1's refund and P1's purchase delivered again: each answered 200, neither changes the row`);
        if (via === 'the phone') {
          const old = await notify(renewal);
          check(old.d.acted === false && rowOf(p1).signed_at === at(refundAt - DAY),
            `(${via}) nor does P2's renewal, signed before the transaction the phone sent`);
        }
        if (upgrade) {
          const before = await post([signJws(p1, chain)]);
          check(before.d.plan === 'pro' && records(p1, p2), `(${via}) nor P1's own transaction from before the refund, though it would run longer (${before.d.plan})`);
        }
      }

      {
        /* The key (period start, then when Apple signed it) never goes
           down. P2's renewal lands after P1's refund with its own
           signedDate, older than the refund's; news of P1 that Apple
           signed in between, here a change of renewal preference carrying
           P1's refunded transaction, is still about an earlier period and
           changes nothing. */
        const p1 = txn({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() - 10 * DAY, signedDate: Date.now() - 40 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', p1, { signedDate: p1.signedDate }));
        const p2 = later(p1, { purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 20 * DAY, signedDate: Date.now() - 10 * DAY });
        const renewal = notification(chain, 'DID_RENEW', p2, { signedDate: p2.signedDate });
        sbx.fake.mode = 'broken';
        await notify(renewal);
        sbx.fake.mode = 'ok';
        await notify(notification(chain, 'REFUND', { ...p1, revocationDate: Date.now() - DAY, revocationReason: 0 }, { signedDate: Date.now() - DAY }));
        await notify(renewal);
        const gap = await notify(notification(chain, 'DID_CHANGE_RENEWAL_PREF', { ...p1, revocationDate: Date.now() - 6 * DAY, revocationReason: 0 }, { subtype: 'DOWNGRADE', signedDate: Date.now() - 5 * DAY }));
        check(gap.status === 200 && gap.d.acted === false && rowOf(p1).revoked_at === null && records(p1, p2) && (await renew(licOf(p1))).d.licensed === true,
          'the signed_at gap: news of P1 signed between P2\'s renewal and P1\'s refund does not lower the row back to P1, refunded');
      }

      {
        /* S1. Pro bought with three months paid in advance; the phone
           records it; Apple refunds it; the customer resubscribes to Base
           for a month, which then expires. Replaying the first purchase's
           own transaction, from before the refund, ends later than
           anything since, and still unlocks nothing: it is an earlier
           period. */
        const t1 = upFront({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() + 50 * DAY, signedDate: Date.now() - 40 * DAY });
        const jws1 = signJws(t1, chain);
        const first = await post([jws1]);
        await notify(notification(chain, 'REFUND', { ...t1, revocationDate: Date.now() - 39 * DAY, revocationReason: 1 }, { signedDate: Date.now() - 39 * DAY }));
        const t2 = later(t1, { productId: BASE, purchaseDate: Date.now() - 38 * DAY, expiresDate: Date.now() - 8 * DAY, signedDate: Date.now() - 38 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', t2, { subtype: 'RESUBSCRIBE', signedDate: t2.signedDate }));
        await notify(notification(chain, 'EXPIRED', t2, { subtype: 'VOLUNTARY', signedDate: t2.expiresDate }));
        const lapsed = await renew(licOf(t1));
        const replay = await post([jws1]);
        check(first.d.licensed === true && refusal(lapsed.d, 'no-subscription') && refusal(replay.d, 'no-subscription', NOTHING_LIVE) && records(t1, t2) && rowOf(t1).revoked_at === null,
          `S1: a refunded purchase paid in advance, replayed after a resubscription has expired, unlocks nothing (${replay.d.reason || replay.d.plan})`);
      }

      {
        /* S2. Base bought with three months in advance, upgraded to Pro
           monthly, Apple refunds what was left of Base, and Pro is
           cancelled and expires. The Base transaction from before the
           upgrade, which does not say it was upgraded from, ends later
           than Pro did, and unlocks nothing. */
        const tb = upFront({ productId: BASE, purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() + 50 * DAY, signedDate: Date.now() - 40 * DAY });
        const jwsB = signJws(tb, chain);
        await post([jwsB]);
        const tp = later(tb, { productId: PRO, purchaseDate: Date.now() - 35 * DAY, expiresDate: Date.now() - 5 * DAY, signedDate: Date.now() - 35 * DAY });
        await notify(notification(chain, 'DID_CHANGE_RENEWAL_PREF', tp, { subtype: 'UPGRADE', signedDate: tp.signedDate }));
        const refundB = await notify(notification(chain, 'REFUND', { ...tb, isUpgraded: true, revocationDate: tp.signedDate + 3600e3, revocationReason: 0 }, { signedDate: tp.signedDate + 3600e3 }));
        await notify(notification(chain, 'EXPIRED', tp, { subtype: 'VOLUNTARY', signedDate: tp.expiresDate }));
        const replay = await post([jwsB]);
        check(refundB.d.acted === false && refusal(replay.d, 'no-subscription', NOTHING_LIVE) && records(tb, tp) && rowOf(tb).revoked_at === null,
          `S2: the transaction upgraded from, replayed after the upgrade has expired, unlocks nothing (${replay.d.reason || replay.d.plan})`);
      }

      {
        /* S5, with no one replaying anything. The first purchase, three
           months in advance, is answered 500; Apple refunds it; the
           customer resubscribes to Base for a month; then Apple's retry of
           the first purchase arrives. It is an earlier period, and changes
           nothing. */
        const t1 = upFront({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() + 50 * DAY, signedDate: Date.now() - 40 * DAY });
        const subscribed = notification(chain, 'SUBSCRIBED', t1, { subtype: 'INITIAL_BUY', signedDate: t1.signedDate });
        sbx.fake.mode = 'broken';
        const lost = await notify(subscribed);
        sbx.fake.mode = 'ok';
        await notify(notification(chain, 'REFUND', { ...t1, revocationDate: Date.now() - 39 * DAY, revocationReason: 1 }, { signedDate: Date.now() - 39 * DAY }));
        const t2 = later(t1, { productId: BASE, purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 20 * DAY, signedDate: Date.now() - 10 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', t2, { subtype: 'RESUBSCRIBE', signedDate: t2.signedDate }));
        const retry = await notify(subscribed);
        const r = await renew(licOf(t1));
        check(lost.status === 500 && retry.status === 200 && retry.d.acted === false && records(t1, t2) && rowOf(t1).revoked_at === null && r.d.licensed === true && r.d.plan === 'base',
          `S5: Apple's retry of a first purchase paid in advance, after its refund and a resubscription, changes nothing (${r.d.plan || r.d.reason})`);
      }

      for (const marked of [true, false]) {
        /* A refund of the period someone upgraded from. Base, three months
           in advance, upgraded to Pro monthly, which is paid for; then
           Apple refunds the Base transaction. Pro is a later period, so
           the refund changes nothing, whether or not the refunded
           transaction says it was upgraded from. */
        const tb = upFront({ productId: BASE, purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 80 * DAY, signedDate: Date.now() - 10 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', tb, { signedDate: tb.signedDate }));
        const tp = later(tb, { productId: PRO, purchaseDate: Date.now() - 5 * DAY, expiresDate: Date.now() + 25 * DAY, signedDate: Date.now() - 5 * DAY });
        await notify(notification(chain, 'DID_CHANGE_RENEWAL_PREF', tp, { subtype: 'UPGRADE', signedDate: tp.signedDate }));
        const refund = await notify(notification(chain, 'REFUND', { ...tb, ...(marked ? { isUpgraded: true } : {}), revocationDate: Date.now() - 4 * DAY, revocationReason: 0 }, { signedDate: Date.now() - 4 * DAY }));
        const r = await renew(licOf(tb));
        const phone = await post([signJws(tp, chain)]);
        check(refund.d.acted === false && rowOf(tb).revoked_at === null && records(tb, tp) && r.d.licensed === true && r.d.plan === 'pro' && phone.d.licensed === true && phone.d.plan === 'pro',
          `a REFUND of the Base period upgraded from (${marked ? 'marked' : 'not marked'} isUpgraded) leaves the Pro subscription live (${r.d.plan || r.d.reason})`);
      }

      {
        /* A notification carrying the transaction upgraded from changes
           nothing, even before the row has heard of the upgrade: the
           upgrade's own transaction writes the row when it arrives. */
        const tb = upFront({ productId: BASE, purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 80 * DAY, signedDate: Date.now() - 10 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', tb, { signedDate: tb.signedDate }));
        const tp = later(tb, { productId: PRO, purchaseDate: Date.now() - 5 * DAY, expiresDate: Date.now() + 25 * DAY, signedDate: Date.now() - 5 * DAY });
        const upgrade = notification(chain, 'DID_CHANGE_RENEWAL_PREF', tp, { subtype: 'UPGRADE', signedDate: tp.signedDate });
        sbx.fake.mode = 'broken';
        await notify(upgrade);
        sbx.fake.mode = 'ok';
        const early = await notify(notification(chain, 'REFUND', { ...tb, isUpgraded: true, revocationDate: Date.now() - 4 * DAY, revocationReason: 0 }, { signedDate: Date.now() - 4 * DAY }));
        check(early.d.acted === false && rowOf(tb).revoked_at === null && records(tb, tb) && (await renew(licOf(tb))).d.licensed === true,
          'a REFUND of the transaction upgraded from, before the upgrade is recorded, changes nothing');
        const landed = await notify(upgrade);
        const stale = await notify(notification(chain, 'DID_CHANGE_RENEWAL_PREF', { ...tb, isUpgraded: true }, { subtype: 'DOWNGRADE', signedDate: Date.now() - 3 * DAY }));
        check(landed.d.acted === true && stale.d.acted === false && records(tb, tp) && rowOf(tb).revoked_at === null,
          'the upgrade then lands, and a later notification carrying the transaction upgraded from changes nothing');
      }

      for (const lostRefund of [false, true]) {
        /* REFUND_REVERSED of a later period than the row records. P1, paid
           in advance, is refunded; the customer resubscribes to Base (P2),
           whose purchase is never recorded; P2 is refunded and the refund
           reversed. Whether or not the refund of P2 was recorded, the
           reversal leaves the row recording P2, paid for. */
        const p1 = upFront({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() + 50 * DAY, signedDate: Date.now() - 40 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', p1, { signedDate: p1.signedDate }));
        await notify(notification(chain, 'REFUND', { ...p1, revocationDate: Date.now() - 39 * DAY, revocationReason: 1 }, { signedDate: Date.now() - 39 * DAY }));
        const p2 = later(p1, { productId: BASE, purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 20 * DAY, signedDate: Date.now() - 10 * DAY });
        const refund2 = notification(chain, 'REFUND', { ...p2, revocationDate: Date.now() - 5 * DAY, revocationReason: 0 }, { signedDate: Date.now() - 5 * DAY });
        if (lostRefund) sbx.fake.mode = 'broken';
        const refunded = await notify(refund2);
        sbx.fake.mode = 'ok';
        if (!lostRefund) {
          check(refunded.d.acted === true && !!rowOf(p1).revoked_at && records(p1, p2),
            'a REFUND of a later period the row has not heard of records that period (its start and expiry), revoked');
        }
        const reversed = await notify(notification(chain, 'REFUND_REVERSED', p2, { signedDate: Date.now() - 4 * DAY }));
        const retried = lostRefund ? await notify(refund2) : null;
        const r = await renew(licOf(p1));
        check(reversed.d.acted === true && (!retried || retried.d.acted === false) && rowOf(p1).revoked_at === null && records(p1, p2) && r.d.licensed === true && r.d.plan === 'base',
          `REFUND_REVERSED of a later period${lostRefund ? ', its refund lost and retried after' : ''}: the row records it, live (${r.d.plan || r.d.reason})`);
      }
      {
        /* Notifications about the same period out of order. Each states
           the whole of what it knows when Apple signed it, so the one
           Apple signed last wins, whatever arrives last: a refund answered
           500 and sent again after a later grace notice is not lost (that
           notice says the period is refunded too); a grace notice answered
           500 and sent again after a later change of renewal preference
           still stands (that change carries the grace period in its renewal
           info, and the grace period has its own stamp); a reversal
           answered 500 and sent again after a later grace notice is not
           lost either. */
        const p = txn({ purchaseDate: Date.now() - 35 * DAY, expiresDate: Date.now() - 5 * DAY, signedDate: Date.now() - 35 * DAY });
        const refundAt = Date.now() - 20 * DAY;
        const grace = Date.now() + 11 * DAY;
        const inGrace = (t, signedDate) => notification(chain, 'DID_FAIL_TO_RENEW', t, { subtype: 'GRACE_PERIOD', renewal: renewalInfo(t, { gracePeriodExpiresDate: grace, isInBillingRetryPeriod: true }), signedDate });
        const lostThenLate = async (first, later) => {
          sbx.fake.mode = 'broken';
          const lost = await notify(first);
          sbx.fake.mode = 'ok';
          const landed = await notify(later);
          const again = await notify(first);
          return lost.status === 500 && landed.status === 200 && again.status === 200;
        };

        const a = txn({ ...p, originalTransactionId: nextId() });
        await notify(notification(chain, 'SUBSCRIBED', a, { signedDate: a.signedDate }));
        const okA = await lostThenLate(
          notification(chain, 'REFUND', { ...a, revocationDate: refundAt, revocationReason: 0 }, { signedDate: refundAt }),
          inGrace({ ...a, revocationDate: refundAt, revocationReason: 0 }, Date.now() - 5 * DAY),
        );
        check(okA && rowOf(a).revoked_at === at(refundAt) && rowOf(a).grace_until === at(grace) && refusal((await renew(licOf(a))).d, 'no-subscription'),
          'a REFUND answered 500, then a grace notice Apple signed after it: the period ends refunded, and renews nothing');

        const b = txn({ ...p, originalTransactionId: nextId() });
        await notify(notification(chain, 'SUBSCRIBED', b, { signedDate: b.signedDate }));
        const okB = await lostThenLate(
          inGrace(b, Date.now() - 5 * DAY),
          notification(chain, 'DID_CHANGE_RENEWAL_PREF', b, { subtype: 'DOWNGRADE', renewal: renewalInfo(b, { autoRenewProductId: BASE, gracePeriodExpiresDate: grace }), signedDate: Date.now() - 3 * DAY }),
        );
        const rb = await renew(licOf(b));
        check(okB && rowOf(b).grace_until === at(grace) && rb.d.licensed === true,
          `a grace notice answered 500, then a change of renewal preference Apple signed after it: still in grace, and renews (${rb.d.reason || rb.d.plan})`);
        const offered = txn({ ...p, originalTransactionId: nextId() });
        await notify(notification(chain, 'SUBSCRIBED', offered, { signedDate: offered.signedDate }));
        const okO = await lostThenLate(
          inGrace(offered, Date.now() - 5 * DAY),
          notification(chain, 'OFFER_REDEEMED', offered, { renewal: renewalInfo(offered, { gracePeriodExpiresDate: grace }), signedDate: Date.now() - 3 * DAY }),
        );
        check(okO && rowOf(offered).grace_until === at(grace) && (await renew(licOf(offered))).d.licensed === true,
          'and so with an offer redeemed after it');

        const c = txn({ ...p, originalTransactionId: nextId() });
        await notify(notification(chain, 'SUBSCRIBED', c, { signedDate: c.signedDate }));
        await notify(notification(chain, 'REFUND', { ...c, revocationDate: refundAt, revocationReason: 0 }, { signedDate: refundAt }));
        const okC = await lostThenLate(
          notification(chain, 'REFUND_REVERSED', c, { signedDate: Date.now() - 10 * DAY }),
          inGrace(c, Date.now() - 5 * DAY),
        );
        const rc = await renew(licOf(c));
        check(okC && rowOf(c).revoked_at === null && rc.d.licensed === true,
          `a REFUND_REVERSED answered 500, then a grace notice Apple signed after it: not revoked, and renews (${rc.d.reason || rc.d.plan})`);
      }
      {
        /* A refund of a period the row has not heard of yet is recorded as
           that period's: P2's renewal is lost, then P2 is refunded. P2's
           transaction from before the refund, from the phone or as the
           late renewal, then finds the refund and unlocks nothing. */
        const p1 = txn({ purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() - 10 * DAY, signedDate: Date.now() - 40 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', p1, { signedDate: p1.signedDate }));
        const p2 = { ...p1, transactionId: nextId(), purchaseDate: Date.now() - 10 * DAY, expiresDate: Date.now() + 20 * DAY, signedDate: Date.now() - 10 * DAY };
        const refundAt = Date.now() - DAY;
        await notify(notification(chain, 'REFUND', { ...p2, revocationDate: refundAt, revocationReason: 0 }, { signedDate: refundAt }));
        check(!!rowOf(p1).revoked_at && rowOf(p1).expires_at === new Date(p2.expiresDate).toISOString(),
          'a REFUND of a period the row has not heard of records that period, revoked');
        const phone = await post([signJws({ ...p2, signedDate: refundAt - DAY }, chain)]);
        const late = await notify(notification(chain, 'DID_RENEW', p2, { signedDate: p2.signedDate }));
        check(refusal(phone.d, 'no-subscription', NOTHING_LIVE) && late.d.acted === false && !!rowOf(p1).revoked_at,
          'and P2\'s transaction from before the refund, from the phone or as the late renewal, unlocks nothing');
      }
      {
        /* A billing grace period after a refunded period: P2 has ended and
           its renewal failed, Apple reports the grace period, and P1's
           refund, signed after that, arrived first. The grace period is
           P2's, a later period, so it lands and clears P1's refund. */
        const p1 = txn({ purchaseDate: Date.now() - 70 * DAY, expiresDate: Date.now() - 40 * DAY, signedDate: Date.now() - 70 * DAY });
        await notify(notification(chain, 'SUBSCRIBED', p1, { signedDate: p1.signedDate }));
        const p2 = { ...p1, transactionId: nextId(), purchaseDate: Date.now() - 40 * DAY, expiresDate: Date.now() - DAY };
        const refundAt = Date.now() - 0.5 * DAY;
        await notify(notification(chain, 'REFUND', { ...p1, revocationDate: refundAt, revocationReason: 0 }, { signedDate: refundAt }));
        const grace = Date.now() + 5 * DAY;
        const failed = await notify(notification(chain, 'DID_FAIL_TO_RENEW', p2, {
          subtype: 'GRACE_PERIOD', renewal: renewalInfo(p2, { gracePeriodExpiresDate: grace, isInBillingRetryPeriod: true }), signedDate: p2.expiresDate,
        }));
        const lic = issue({ email: apple.appleSub(p1.originalTransactionId), plan: 'pro' }, keys.privateKey);
        check(failed.d.acted === true && rowOf(p1).revoked_at === null && rowOf(p1).grace_until === new Date(grace).toISOString()
          && (await renew(lic)).d.licensed === true,
          'DID_FAIL_TO_RENEW for the period after a refunded one, signed before the refund, records the grace period and renews');
      }

      const fam = txn({ inAppOwnershipType: 'FAMILY_SHARED' });
      await notify(notification(chain, 'SUBSCRIBED', fam, { signedDate: Date.now() - DAY }));
      await notify(notification(chain, 'REVOKE', { ...fam, revocationDate: Date.now() - 1000, revocationReason: 0 }));
      check(!!rowOf(fam).revoked_at, 'REVOKE (Family Sharing withdrawn) records it as revoked');

      const unseen = txn();
      const refundFirst = await notify(notification(chain, 'REFUND', { ...unseen, revocationDate: Date.now() - 1000 }));
      check(refundFirst.d.acted === true && rowOf(unseen) && !!rowOf(unseen).revoked_at && rowOf(unseen).plan === 'pro',
        'a REFUND for a subscription not seen yet creates its row, revoked');

      const ignored = txn();
      const price = await notify(notification(chain, 'PRICE_INCREASE', ignored, { subtype: 'PENDING' }));
      check(price.status === 200 && price.d.acted === false && !rowOf(ignored), 'any other type: 200 and left alone');
      /* RESCIND_CONSENT (a parent or guardian withdrew consent, under the
         age laws Apple applies in Texas and elsewhere) carries appData, an
         app transaction, instead of data: no subscription to change, and
         Apple stops the app launching itself. Answered 200 like any other
         type, never 400, or Apple would send it again for days. */
      const unasked = sbx.fake.calls.length;
      const consent = await notify(signJws({
        notificationType: 'RESCIND_CONSENT', notificationUUID: randomUUID(), version: '2.0', signedDate: Date.now(),
        appData: { appAppleId: APP_ID, bundleId: BUNDLE, environment: 'Production', signedAppTransactionInfo: signJws({ bundleId: BUNDLE, appAppleId: APP_ID, receiptType: 'Production', signedDate: Date.now() }, chain) },
      }, chain));
      check(consent.status === 200 && consent.d.acted === false && sbx.fake.calls.length === unasked,
        `RESCIND_CONSENT, which carries appData and no subscription: 200, nothing written (${consent.status})`);
      const notOurs = txn({ productId: 'org.mailrata.lifetime' });
      const no = await notify(notification(chain, 'DID_RENEW', notOurs));
      check(no.status === 200 && no.d.acted === false && !rowOf(notOurs), 'a product RATA does not sell here: 200 and left alone');

      const sandbox = txn({ environment: 'Sandbox' });
      const sn = await notify(notification(chain, 'SUBSCRIBED', sandbox));
      check(sn.status === 200 && rowOf(sandbox) && rowOf(sandbox).environment === 'Sandbox', `a sandbox notification is recorded as Sandbox (${sn.status})`);

      sbx.fake.mode = 'broken';
      const down = await notify(notification(chain, 'DID_RENEW', txn()));
      sbx.fake.mode = 'missing';
      const before = s.log().length;
      const missing = await notify(notification(chain, 'DID_RENEW', txn()));
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(down.status === 500 && missing.status === 500, `a database that fails or has no table: 500, so Apple sends it again (${down.status}, ${missing.status})`);
      check(/run section 7 of rata-next\/database\.sql/.test(s.log().slice(before)), 'and the log names section 7');
      sbx.fake.mode = 'behind';
      const behindAt = s.log().length;
      const old = await notify(notification(chain, 'DID_RENEW', txn(), { renewal: renewalInfo(txn()) }));
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(old.status === 500 && /run section 7 of rata-next\/database\.sql/.test(s.log().slice(behindAt)),
        `a table without a column section 7 now adds: 500, and the log names section 7 (${old.status})`);
    }

    console.log('\n— renewing an App Store licence —');
    {
      const live = txn({ productId: BASE });
      await post([signJws(live, chain)]);
      const lic = issue({ email: apple.appleSub(live.originalTransactionId), plan: 'pro', now: Date.now() - 27 * DAY }, keys.privateKey);
      const calls = sbx.fake.calls.length;
      const r = await renew(lic);
      const seen = subOf(r.d);
      check(r.status === 200 && r.d.licensed === true && r.d.plan === 'base' && seen && seen.ok && seen.licence.sub === `apple:${live.originalTransactionId}`,
        `a live row renews it, with the row's plan (${r.d.plan})`);
      check(!('email' in r.d) && !JSON.stringify(r.d).includes('@'), 'with no address in the answer');
      const asked = sbx.fake.calls.slice(calls);
      check(asked.length > 0 && asked.every((c) => c.endsWith('/rest/v1/apple_subscriptions')), `from the App Store's record, never Stripe's: ${asked.join(', ')}`);

      const none = await renew(issue({ email: 'apple:2999999999999999', plan: 'pro' }, keys.privateKey));
      check(refusal(none.d, 'no-subscription', NOTHING_LIVE) && !/mailrata\.org/.test(none.d.message), `no row: no-subscription, with nothing to buy elsewhere: "${none.d.message}"`);
      const calls2 = sbx.fake.calls.length;
      const odd = await renew(issue({ email: 'apple:not-a-number', plan: 'pro' }, keys.privateKey));
      check(refusal(odd.d, 'no-subscription') && sbx.fake.calls.length === calls2, 'an apple: subject that names no purchase asks nobody, Stripe included');

      const stripe = await renew(issue({ email: 'buyer@example.com', plan: 'pro' }, keys.privateKey));
      check(refusal(stripe.d, 'no-subscription') && sbx.fake.calls.at(-1).endsWith('/rest/v1/subscriptions') && stripe.d.email === 'buyer@example.com',
        'a licence for an address still renews from Stripe\'s record, as before');

      /* A licence from the App Store has no account at mailrata.org, so no
         refusal of one sends anybody to sign in there; inside the iPhone
         app that would also send them to the website that sells. What
         decides the words is the subject, read before the age and, when
         the signature does not verify, from the payload for the words
         alone. */
      const otherKeys = generateKeys();
      const longAgo = Date.now() - (30 + 100) * DAY;
      const appleLic = issue({ email: 'apple:2000000123456789', plan: 'pro', now: longAgo }, keys.privateKey);
      const callsBefore = sbx.fake.calls.length;
      const told = {
        'too old': await renew(appleLic),
        'signed by another key': await renew(issue({ email: 'apple:2000000123456789', plan: 'pro' }, otherKeys.privateKey)),
        'unreadable': await renew(issue({ email: 'apple:2000000123456789', plan: 'pro' }, keys.privateKey).replace(/^v1\./, 'v9.')),
      };
      check(told['too old'].status === 200 && refusal(told['too old'].d, 'too-old', apple.APPLE_SAID.tooOld),
        `an App Store licence that expired over 90 days ago: too-old, "${told['too old'].d.message}"`);
      check(refusal(told['signed by another key'].d, 'bad-signature', apple.APPLE_SAID.unreadable) && refusal(told.unreadable.d, 'malformed', apple.APPLE_SAID.unreadable),
        `one that does not verify, or cannot be read: "${told.unreadable.d.message}"`);
      check(sbx.fake.calls.length === callsBefore, 'each answered before the database, as for an address');
      const elsewhere = Object.entries(told).filter(([, r]) => /mailrata\.org|sign in/i.test(r.d.message || '')).map(([k]) => k);
      check(elsewhere.length === 0, `none of them names mailrata.org or a sign-in (${elsewhere.join(', ') || 'none'})`);
      const oldAddr = await renew(issue({ email: 'buyer@example.com', plan: 'pro', now: longAgo }, keys.privateKey));
      const forgedAddr = await renew(issue({ email: 'buyer@example.com', plan: 'pro' }, otherKeys.privateKey));
      check(refusal(oldAddr.d, 'too-old', 'This licence expired more than 90 days ago, so it cannot renew itself. Sign in at mailrata.org to get a new one.')
        && refusal(forgedAddr.d, 'bad-signature', 'mailrata.org did not recognise the signature on this licence. Sign in at mailrata.org to get a new one.'),
        'a licence for an address is still told to sign in at mailrata.org, word for word as before');

      sbx.fake.mode = 'broken';
      const down = await renew(lic);
      sbx.fake.mode = 'missing';
      const before = s.log().length;
      const missing = await renew(lic);
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(down.status === 503 && refusal(down.d, 'unavailable') && /keeps the licence it has/.test(down.d.message), `the database fails: 503, "${down.d.message}"`);
      check(missing.status === 503 && refusal(missing.d, 'not-configured') && missing.d.message === OFF && /run section 7/.test(s.log().slice(before)),
        'apple_subscriptions missing: 503 not-configured, and the log names section 7');
      sbx.fake.mode = 'behind';
      const behindAt = s.log().length;
      const old = await renew(lic);
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(old.status === 503 && refusal(old.d, 'not-configured') && old.d.message === OFF && /run section 7/.test(s.log().slice(behindAt)),
        'a table without a column section 7 now adds (42703 on reading it): 503 not-configured, and the log names section 7');
      const sendsAway = [r, none, odd, down, missing].filter((x) => /sign in|choose a plan|at mailrata\.org|mailrata\.org\//i.test(x.d.message || ''));
      check(sendsAway.length === 0, 'and no answer to an App Store licence sends anyone to mailrata.org');
    }

    console.log('\n— the AI relay asks an App Store licence\'s row first —');
    {
      const ask = async (licence) => {
        const r = await fetch(s.url + '/api/ai', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ task: 'translate', subject: 'Hi', text: 'Hello there', to: 'fr', licence }) });
        return { status: r.status, d: await r.json().catch(() => ({})) };
      };
      const reserved = (from) => sbx.fake.calls.slice(from).some((c) => c.endsWith('/rest/v1/rpc/ai_reserve'));
      const licOf3 = (t) => issue({ email: apple.appleSub(t.originalTransactionId), plan: 'pro' }, keys.privateKey);

      const live = txn();
      await post([signJws(live, chain)]);
      let at = sbx.fake.calls.length;
      const ok = await ask(licOf3(live));
      check(reserved(at) && sbx.fake.calls.slice(at).some((c) => c.endsWith('/rest/v1/apple_subscriptions')),
        `a live subscription: its row is read, then the allowance is asked (${ok.status}, here a stand-in with no ai_reserve)`);

      const refunded = txn();
      await post([signJws(refunded, chain)]);
      await notify(notification(chain, 'REFUND', { ...refunded, revocationDate: Date.now() - 1000, revocationReason: 0 }));
      at = sbx.fake.calls.length;
      const r = await ask(licOf3(refunded));
      check(r.status === 401 && r.d.reason === 'expired' && r.d.error === NOTHING_LIVE && !reserved(at),
        `refunded, while its licence still runs: 401 expired, "${r.d.error}", and nothing reserved`);
      at = sbx.fake.calls.length;
      const none = await ask(issue({ email: 'apple:2999999999999998', plan: 'pro' }, keys.privateKey));
      check(none.status === 401 && none.d.reason === 'expired' && !reserved(at), 'no row at all: the same');

      /* A period's end, with its renewal not yet recorded: Apple's
         DID_RENEW is late, and the phone has not been opened. The row has
         expired an hour ago; renewing the licence is refused until the
         renewal arrives, and the licence still runs, so AI still answers,
         to the margin a licence from that row would have. */
      const iso = (ms) => new Date(ms).toISOString();
      const seed = ({ licencePlan = 'pro', ...over }) => {
        const id = nextId();
        sbx.fake.rows.set(id, {
          original_transaction_id: id, product_id: PRO, plan: 'pro', environment: 'Production', period_from: iso(Date.now() - 30 * DAY),
          expires_at: iso(Date.now() + DAY), grace_until: null, grace_signed_at: iso(0), revoked_at: null, signed_at: iso(Date.now() - 30 * DAY),
          app_account_token: null, email: null, ...over,
        });
        return issue({ email: apple.appleSub(id), plan: licencePlan }, keys.privateKey);
      };
      const boundary = seed({ expires_at: iso(Date.now() - 3600e3) });
      at = sbx.fake.calls.length;
      const b = await ask(boundary);
      check(reserved(at) && b.status !== 401, `at a period's end, the renewal not yet recorded: the relay answers (${b.status}, here a stand-in with no ai_reserve)`);
      check(refusal((await renew(boundary)).d, 'no-subscription'), 'though renewing the licence waits for the renewal');
      at = sbx.fake.calls.length;
      const sandboxEdge = await ask(seed({ environment: 'Sandbox', expires_at: iso(Date.now() - 30 * 60e3) }));
      check(reserved(at) && sandboxEdge.status !== 401, 'in the sandbox, within its hour, too');
      at = sbx.fake.calls.length;
      const pastMargin = await ask(seed({ expires_at: iso(Date.now() - 4 * DAY) }));
      const sandboxPast = await ask(seed({ environment: 'Sandbox', expires_at: iso(Date.now() - 2 * 3600e3) }));
      check(pastMargin.status === 401 && pastMargin.d.reason === 'expired' && sandboxPast.status === 401 && !reserved(at),
        'past the margin, 3 days (an hour in the sandbox): 401 expired, nothing reserved');
      at = sbx.fake.calls.length;
      const graceRow = await ask(seed({ expires_at: iso(Date.now() - 5 * DAY), grace_until: iso(Date.now() + 2 * DAY) }));
      check(reserved(at) && graceRow.status !== 401, 'in a billing grace period: answered');
      at = sbx.fake.calls.length;
      const refundedEdge = await ask(seed({ expires_at: iso(Date.now() - 3600e3), revoked_at: iso(Date.now() - 60e3) }));
      const refundedLive = await ask(seed({ revoked_at: iso(Date.now() - 60e3) }));
      check(refundedEdge.status === 401 && refundedEdge.d.reason === 'expired' && refundedEdge.d.error === NOTHING_LIVE
        && refundedLive.status === 401 && !reserved(at),
        'a refund is refused at once, at a period\'s end or in the middle of one');

      /* The plan: the lower of the licence's and the row's. A Pro licence
         whose row is now Base is answered as expired, so the app renews to
         Base; a Base licence whose row is now Pro is answered as Base. */
      at = sbx.fake.calls.length;
      const lowered = await ask(seed({ plan: 'base', product_id: BASE }));
      check(lowered.status === 401 && lowered.d.reason === 'expired' && !reserved(at), `a Pro licence, its row now Base: 401 expired, so the app renews to Base ("${lowered.d.error}")`);
      at = sbx.fake.calls.length;
      const up = await ask(seed({ licencePlan: 'base' }));
      check(reserved(at) && up.status !== 401, 'a Base licence, its row now Pro: answered, for what Base allows');
      const upSummary = await fetch(s.url + '/api/ai', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ task: 'summarize', text: 'Hello there', licence: seed({ licencePlan: 'base' }) }) });
      check(upSummary.status === 403, `and not for what only Pro allows (${upSummary.status})`);

      /* A Pro licence whose row is now Base and has only reached its
         period's end, the renewal not yet recorded: renewing is refused
         (no-subscription), so the app keeps the Pro licence, and `expired`
         would refuse every request until the next period reached the row.
         So the relay answers what Base allows, and refuses only the rest. */
      const lowEdge = (environment, ago) => seed({ plan: 'base', product_id: BASE, environment, expires_at: iso(Date.now() - ago) });
      const sandboxLow = lowEdge('Sandbox', 10 * 60e3);
      at = sbx.fake.calls.length;
      const lowTranslate = await ask(sandboxLow);
      check(reserved(at) && lowTranslate.status !== 401,
        `a Pro licence, its Base row expired ten minutes ago in the sandbox: a translation is answered (${lowTranslate.status}, here a stand-in with no ai_reserve)`);
      check(refusal((await renew(sandboxLow)).d, 'no-subscription'), 'while renewing it to Base is refused until the renewal is recorded');
      const prodLow = lowEdge('Production', DAY);
      at = sbx.fake.calls.length;
      const prodTranslate = await ask(prodLow);
      check(reserved(at) && prodTranslate.status !== 401, 'in Production a day after the period\'s end, too');
      at = sbx.fake.calls.length;
      const lowSummaryR = await fetch(s.url + '/api/ai', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ task: 'summarize', text: 'Hello there', licence: prodLow }) });
      const lowSummary = { status: lowSummaryR.status, d: await lowSummaryR.json().catch(() => ({})) };
      check(lowSummary.status === 403 && lowSummary.d.reason === 'plan' && !reserved(at),
        `and a summary, which Base does not include: 403 plan, nothing reserved ("${lowSummary.d.error}")`);
      at = sbx.fake.calls.length;
      const lowPast = await ask(lowEdge('Production', 4 * DAY));
      check(lowPast.status === 401 && lowPast.d.reason === 'expired' && !reserved(at), 'past the margin: 401 expired, as before');

      /* Too many requests are refused before the row is read. */
      const busy = issue({ email: 'apple:2999999999999997', plan: 'pro' }, keys.privateKey);
      for (let i = 0; i < 20; i++) await ask(busy);
      at = sbx.fake.calls.length;
      const limited = await ask(busy);
      check(limited.status === 429 && limited.d.reason === 'rate' && sbx.fake.calls.length === at, 'the twenty-first request in a minute: 429, and no row is read');

      sbx.fake.mode = 'broken';
      at = sbx.fake.calls.length;
      const down = await ask(licOf3(live));
      sbx.fake.mode = 'missing';
      const before = s.log().length;
      const missing = await ask(licOf3(live));
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(down.status === 503 && !down.d.reason && !reserved(at), `a database that fails: 503, nothing reserved (fails closed): "${down.d.error}"`);
      check(missing.status === 503 && missing.d.reason === 'not-configured' && /ai: .*run section 7 of rata-next\/database\.sql/.test(s.log().slice(before)),
        'no apple_subscriptions table: 503 not-configured, and the log names section 7');
      at = sbx.fake.calls.length;
      await ask(issue({ email: 'buyer@example.com', plan: 'pro' }, keys.privateKey));
      check(!sbx.fake.calls.slice(at).some((c) => c.endsWith('/rest/v1/apple_subscriptions')), 'a licence for an address never reads an App Store row');
    }

    console.log('\n— the log —');
    {
      const log = s.log();
      const leaked = sent.filter((j) => typeof j === 'string' && j.length > 40 && log.includes(j.slice(-40)));
      check(leaked.length === 0 && !/eyJ[A-Za-z0-9_-]{20,}/.test(log), 'no transaction or notification, nor any part of one, is ever logged');
      check(!/20000000000\d{5}/.test(log), 'nor any transaction id');
    }
  } finally {
    await s.stop();
  }

  console.log('\n— a server with Apple\'s real roots, and one for local testing —');
  {
    const real = await startServer({ env: { ...base, APPLE_APP_ID: String(APP_ID) } });
    try {
      const r = await fetch(real.url + '/api/licence/apple', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ transactions: [signJws(txn(), chain)] }) });
      const d = await r.json();
      check(r.status === 200 && refusal(d, 'unverified'), `Apple's real roots refuse the test chain: ${d.reason}`);
      const n = await fetch(real.url + '/api/apple/notifications', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ signedPayload: notification(chain, 'DID_RENEW', txn()) }) });
      check(n.status === 400, `and so does the notifications route: ${n.status}`);
      const g = await (await fetch(real.url + '/api/apple/notifications')).json();
      check(g.configured === true && JSON.stringify(Object.keys(g)) === JSON.stringify(['endpoint', 'configured', 'note']),
        `a browser visiting it is told what it is, and nothing about the settings: ${JSON.stringify(g)}`);
    } finally { await real.stop(); }

    /* An APPLE_BUNDLE_ID that is not the app's: every genuine purchase is
       another app's to the library, so the customer is told they have no
       subscription, and only the log says why. */
    const wrong = await startServer({ env: { ...base, APPLE_TEST_ROOT: chain.rootBase64, APPLE_APP_ID: String(APP_ID), APPLE_BUNDLE_ID: 'org.example.notrata' } });
    try {
      const t = txn();
      const r = await fetch(wrong.url + '/api/licence/apple', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ transactions: [signJws(t, chain)] }) });
      const d = await r.json();
      await new Promise((res) => setTimeout(res, 200));
      const log = wrong.log();
      check(r.status === 200 && refusal(d, 'no-subscription', NOTHING_LIVE) && !sbx.fake.rows.has(t.originalTransactionId),
        `with an APPLE_BUNDLE_ID that is not the app's, a genuine purchase is "${d.message}", and nothing is recorded`);
      check(/1 transaction\(s\) were for another app \(INVALID_APP_IDENTIFIER\).*APPLE_BUNDLE_ID does not match the app/.test(log) && !log.includes(t.originalTransactionId),
        'and the log says APPLE_BUNDLE_ID does not match the app, naming no transaction');
    } finally { await wrong.stop(); }

    const local = await startServer({ env: { ...base, APPLE_TEST_ROOT: chain.rootBase64, APPLE_ALLOW_LOCAL: '1' } });
    try {
      const t = txn({ environment: 'Xcode' });
      const r = await fetch(local.url + '/api/licence/apple', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ transactions: [unsignedJws(t)] }) });
      const d = await r.json();
      check(d.licensed === true && sbx.fake.rows.get(t.originalTransactionId).environment === 'Xcode', `with APPLE_ALLOW_LOCAL=1 an Xcode purchase is accepted (${d.reason || d.plan})`);
      const p = await fetch(local.url + '/api/licence/apple', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ transactions: [signJws(txn(), chain)] }) });
      const pd = await p.json();
      check(p.status === 503 && refusal(pd, 'not-configured', OFF), `without APPLE_APP_ID a Production purchase is "not switched on": ${p.status}`);
      await new Promise((res) => setTimeout(res, 200));
      const log = local.log();
      check(/APPLE_APP_ID is not set/.test(log) && /APPLE_TEST_ROOT is set/.test(log) && /APPLE_ALLOW_LOCAL=1/.test(log),
        'and the log says why, and that the test-only settings are on');
      const g = await (await fetch(local.url + '/api/apple/notifications')).json();
      check(g.configured === false, `and the notifications address says it is not configured, since nothing from the App Store itself would verify: ${JSON.stringify(g.configured)}`);
    } finally { await local.stop(); }
  }
  await sbx.close();
}
