/* Subscriptions bought in the iPhone app (the App Store design's server
   half): verifying what Apple signed, issuing and renewing the licence, and
   App Store Server Notifications.

   What is worth proving is that only Apple's signature unlocks anything,
   and only for a live RATA subscription; that nothing older Apple signed
   ever undoes something newer; that a fault at this end issues nothing it
   cannot record; and that an `apple:` subject is never taken for an
   address. Apple is played by a certificate chain made in the test
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
import { generateKeys, issue, check as checkLicence } from '../lib/licence.js';
import { APPLE_PRODUCTS, PLANS, ORDER } from '../lib/plan.js';
import * as apple from '../lib/apple.js';

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
const BUNDLE = 'org.mailrata.desktop';
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
function notification(chain, type, t, { subtype, renewal, signedDate = Date.now(), environment, appAppleId, signer } = {}) {
  const env = environment || (t ? t.environment : 'Production');
  const data = { bundleId: BUNDLE, bundleVersion: '1', environment: env, status: 1 };
  /* Apple sends appAppleId only for an app downloaded from the App Store:
     never in the sandbox. */
  if (env === 'Production') data.appAppleId = appAppleId ?? APP_ID;
  if (t) data.signedTransactionInfo = signJws(t, chain);
  if (renewal) data.signedRenewalInfo = signJws(renewal, chain);
  return signJws({
    notificationType: type, ...(subtype ? { subtype } : {}), notificationUUID: randomUUID(), data, version: '2.0', signedDate,
  }, signer || chain);
}

/* Supabase, for apple_subscriptions (and subscriptions, which only a
   Stripe licence's renewal may ask). `mode`: 'ok', 'missing' (database.sql
   section 7 never ran: PostgREST's PGRST205) or 'broken'. */
async function fakeSupabase() {
  const fake = { rows: new Map(), mode: 'ok', calls: [] };
  const filtersOf = (url) => [...url.searchParams].filter(([k]) => !['select', 'on_conflict'].includes(k))
    .map(([k, v]) => { const i = v.indexOf('.'); return { col: k, op: v.slice(0, i), val: v.slice(i + 1) }; });
  const matches = (row, fs) => fs.every(({ col, op, val }) => {
    if (op === 'eq') return String(row[col]) === val;
    if (op === 'lte') return row[col] != null && Date.parse(row[col]) <= Date.parse(val);
    return false;
  });
  const srv = createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    let raw = '';
    req.on('data', (d) => { raw += d; });
    req.on('end', () => {
      const json = (status, body) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(body)); };
      fake.calls.push(`${req.method} ${url.pathname}`);
      if (url.pathname === '/rest/v1/subscriptions') return json(200, []);
      if (url.pathname !== '/rest/v1/apple_subscriptions') return json(404, { message: 'not in the stand-in' });
      if (fake.mode === 'missing') return json(404, { code: 'PGRST205', details: null, hint: null, message: "Could not find the table 'public.apple_subscriptions' in the schema cache" });
      if (fake.mode === 'broken') return json(500, { code: 'XX000', details: null, hint: null, message: 'disk on fire' });
      const fs = filtersOf(url);
      if (req.method === 'GET') return json(200, [...fake.rows.values()].filter((r) => matches(r, fs)));
      if (req.method === 'PATCH') {
        const patch = JSON.parse(raw);
        const hit = [...fake.rows.values()].filter((r) => matches(r, fs));
        for (const r of hit) Object.assign(r, patch, { updated_at: new Date().toISOString() });
        return json(200, hit);
      }
      if (req.method === 'POST') {
        const body = JSON.parse(raw);
        const ignore = /resolution=ignore-duplicates/.test(req.headers.prefer || '');
        const out = [];
        for (const row of Array.isArray(body) ? body : [body]) {
          if (fake.rows.has(row.original_transaction_id)) {
            if (!ignore) return json(409, { code: '23505', message: 'duplicate key value violates unique constraint' });
            continue;
          }
          const stored = { grace_until: null, revoked_at: null, app_account_token: null, email: null, ...row, updated_at: new Date().toISOString() };
          fake.rows.set(row.original_transaction_id, stored);
          out.push(stored);
        }
        return json(201, out);
      }
      json(405, { message: 'not in the stand-in' });
    });
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  return { fake, url: `http://127.0.0.1:${srv.address().port}`, close: () => new Promise((r) => { srv.close(() => r()); srv.closeAllConnections(); }) };
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
    check(apple.appleSettings({}).bundleId === BUNDLE && apple.appleSettings({ APPLE_BUNDLE_ID: ' org.example.rata ' }).bundleId === 'org.example.rata',
      `the bundle id is ${BUNDLE} unless APPLE_BUNDLE_ID says otherwise`);
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

  /* The servers. Each case that changes a setting needs its own. */
  const sbx = await fakeSupabase();
  const base = {
    SUPABASE_URL: sbx.url, SUPABASE_SERVICE_ROLE_KEY: fakeSupabaseKey('service_role'),
    LICENCE_PRIVATE_KEY: keys.privateKey, LICENCE_PUBLIC_KEY: keys.publicKey,
  };
  const s = await startServer({ env: { ...base, APPLE_TEST_ROOT: chain.rootBase64, APPLE_APP_ID: String(APP_ID) } });
  const sent = [];
  const post = async (transactions, { raw, origin } = {}) => {
    if (Array.isArray(transactions)) sent.push(...transactions);
    const r = await fetch(s.url + '/api/licence/apple', {
      method: 'POST', headers: { 'Content-Type': 'application/json', ...(origin ? { Origin: origin } : {}) },
      body: raw !== undefined ? raw : JSON.stringify({ transactions }),
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
      ['another app\'s', { bundleId: 'com.example.other' }],
      ['an unknown product', { productId: 'org.mailrata.lifetime' }],
      ['a purchase that is not a subscription', { type: 'Non-Consumable' }],
    ]) {
      const t = txn(over);
      const { status, d } = await post([signJws(t, chain)]);
      check(status === 200 && refusal(d, 'no-subscription', NOTHING_LIVE) && !rowOf(t),
        `${what}: 200 no-subscription, "${d.message}", and nothing recorded`);
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
      const one = signJws(txn(), chain);
      for (const [what, raw, want] of [
        ['a body that is not JSON', 'not json', 400],
        ['no list', JSON.stringify({ transaction: one }), 400],
        ['a list of something else', JSON.stringify({ transactions: [42] }), 400],
        ['an empty string in it', JSON.stringify({ transactions: [''] }), 400],
        ['eleven', JSON.stringify({ transactions: Array(11).fill(one) }), 400],
        ['one over 16 KiB', JSON.stringify({ transactions: ['a'.repeat(16 * 1024 + 1)] }), 400],
        ['ten, the most', JSON.stringify({ transactions: Array(10).fill(one) }), 200],
      ]) {
        const r = await post(null, { raw });
        check(r.status === want && (want === 200 ? r.d.licensed === true : refusal(r.d, 'malformed')), `${what}: ${r.status}`);
      }
    }

    console.log('\n— nothing older than what is recorded overwrites it —');
    {
      const t = txn({ signedDate: Date.now() - 10 * DAY });
      sbx.fake.rows.set(t.originalTransactionId, {
        original_transaction_id: t.originalTransactionId, product_id: PRO, plan: 'pro', environment: 'Production',
        expires_at: new Date(t.expiresDate).toISOString(), grace_until: null, revoked_at: new Date(Date.now() - DAY).toISOString(),
        signed_at: new Date(Date.now() - DAY).toISOString(), app_account_token: null, email: null,
      });
      const replay = await post([signJws(t, chain)]);
      check(refusal(replay.d, 'no-subscription', NOTHING_LIVE) && !!rowOf(t).revoked_at,
        'a transaction Apple signed before the refund that is recorded unlocks nothing, and the refund stays');
      const again = txn({ originalTransactionId: t.originalTransactionId, signedDate: Date.now() });
      const back = await post([signJws(again, chain)]);
      check(back.d.licensed === true && rowOf(t).revoked_at === null && rowOf(t).signed_at === new Date(again.signedDate).toISOString(),
        'one signed after it, with no revocation (a new subscription), unlocks again and is recorded');

      const b = txn({ productId: BASE });
      await post([signJws(b, chain)]);
      const older = txn({ originalTransactionId: b.originalTransactionId, productId: PRO, signedDate: b.signedDate - DAY });
      const o = await post([signJws(older, chain)]);
      check(o.d.licensed === true && o.d.plan === 'base' && rowOf(b).plan === 'base' && rowOf(b).product_id === BASE,
        `an older Pro transaction never raises a newer Base row, and the licence follows the row (${o.d.plan})`);
    }

    console.log('\n— a fault at this end issues nothing —');
    {
      sbx.fake.mode = 'broken';
      const t = txn();
      const r = await post([signJws(t, chain)]);
      sbx.fake.mode = 'ok';
      check(r.status === 503 && refusal(r.d, 'unavailable') && /try again/i.test(r.d.message), `the database fails: 503, no licence: "${r.d.message}"`);
      sbx.fake.mode = 'missing';
      const before = s.log().length;
      const m = await post([signJws(txn(), chain)]);
      sbx.fake.mode = 'ok';
      await new Promise((res) => setTimeout(res, 200));
      check(m.status === 503 && refusal(m.d, 'not-configured', OFF), `apple_subscriptions missing: 503 "${m.d.message}"`);
      check(/run section 7 of rata-next\/database\.sql/.test(s.log().slice(before)) && !/section 7|database/i.test(JSON.stringify(m.d)),
        'the server log names section 7, the answer does not');
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

      await notify(notification(chain, 'GRACE_PERIOD_EXPIRED', lapsed, { renewal: renewalInfo(lapsed, { gracePeriodExpiresDate: Date.now() - 1000 }), signedDate: Date.now() - 1.5 * DAY }));
      check(rowOf(t).plan === 'pro' && Date.parse(rowOf(t).grace_until) < Date.now(), 'GRACE_PERIOD_EXPIRED changes only the dates');
      const after = await renew(lic);
      check(refusal(after.d, 'no-subscription', NOTHING_LIVE), `and renewing then: no-subscription, "${after.d.message}"`);

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
    } finally { await local.stop(); }
  }
  await sbx.close();
}
