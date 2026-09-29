/* Licences.

   This is the only thing standing between "paid" and "the app runs", and the
   app checks it on the customer's own machine — where they can read the code,
   read the token, and change the clock. So what is worth testing is that none
   of that helps: a forged licence must fail, an edited one must fail, and the
   difference between "expired" and "forged" must survive, because telling a
   paying customer their licence is fake is a support call you do not recover
   from. */
import { startServer, makeChecker } from './helpers.mjs';
import * as licence from '../lib/licence.js';
import { generateKeys, issue, check, explain, LICENCE_DAYS } from '../lib/licence.js';

const { RENEW_GRACE_DAYS, renewable } = licence;

export default async function run(state) {
  const check_ = makeChecker(state);

  const keys = generateKeys();
  const other = generateKeys();
  const now = Date.UTC(2026, 8, 16);

  console.log('\n— a licence the app can check with no network at all —');
  {
    const t = issue({ email: 'Buyer@Example.com', plan: 'pro', now }, keys.privateKey);
    check_(/^v1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/.test(t), `shaped as v1.payload.signature (${t.length} chars)`);

    const r = check(t, keys.publicKey, now);
    check_(r.ok, 'it verifies against the public key alone');
    check_(r.licence.sub === 'buyer@example.com', `issued to the address, lowercased: ${r.licence.sub}`);
    check_(r.licence.plan === 'pro', 'carrying the plan');

    /* Nothing about the mailboxes, the password or the card belongs in here. */
    const inside = JSON.stringify(r.licence);
    check_(!/pass|card|token|mailbox/i.test(inside), `and nothing else: ${inside}`);

    check_(r.licence.exp - r.licence.iat === LICENCE_DAYS * 86400,
      `good for ${LICENCE_DAYS} days offline, which is the point of signing it`);
  }

  console.log('\n— forging one requires the private key —');
  {
    const t = issue({ email: 'buyer@example.com', plan: 'pro', now }, keys.privateKey);

    check_(!check(t, other.publicKey, now).ok, 'signed by a different key: refused');

    /* The attack a customer would actually try: take a real Base licence and
       edit the word. */
    const base = issue({ email: 'buyer@example.com', plan: 'base', now }, keys.privateKey);
    const [, body, sig] = base.split('.');
    const upgraded = JSON.parse(Buffer.from(body, 'base64url').toString('utf8'));
    upgraded.plan = 'pro';
    const forged = `v1.${Buffer.from(JSON.stringify(upgraded)).toString('base64url')}.${sig}`;
    const fr = check(forged, keys.publicKey, now);
    check_(!fr.ok && fr.reason === 'bad-signature',
      `a Base licence edited to say Pro: ${fr.reason}`);

    check_(!check('v1.not.base64url!!', keys.publicKey, now).ok, 'nonsense: refused');
    check_(!check('', keys.publicKey, now).ok, 'nothing at all: refused');
    check_(check(t, null, now).reason === 'no-public-key',
      'and a build with no public key refuses rather than waving everything through');
  }

  console.log('\n— expired is not the same as forged —');
  {
    const t = issue({ email: 'buyer@example.com', plan: 'pro', days: 30, now }, keys.privateKey);
    const later = now + 31 * 86400 * 1000;

    const r = check(t, keys.publicKey, later);
    check_(!r.ok && r.reason === 'expired', `a month and a day on: ${r.reason}`);
    check_(!!r.licence && r.licence.plan === 'pro',
      'the licence is still readable, so the app knows what it is renewing');
    check_(/renew itself/i.test(explain('expired')),
      `and the customer is told to reconnect, not accused: "${explain('expired')}"`);
    check_(/could not be read/i.test(explain('bad-signature')),
      'while a forged one gets different words entirely');

    /* Signature before clock: a forged token must never be reported as merely
       expired, or the app would offer to renew something nobody issued. */
    const junk = 'v1.' + Buffer.from(JSON.stringify({ v: 1, sub: 'x@y.z', plan: 'pro', iat: 1, exp: 1 })).toString('base64url') + '.AAAA';
    check_(check(junk, keys.publicKey, later).reason === 'bad-signature',
      'and an unsigned, long-expired token reads as forged rather than expired');

    check_(check(t, keys.publicKey, now + 29 * 86400 * 1000).ok,
      'a day before expiry it still works, offline, with no server involved');

    /* Renewable while expired no more than RENEW_GRACE_DAYS: the boundary
       itself still renews, a second past it does not. */
    check_(typeof renewable === 'function', 'lib/licence.js decides it in one place (renewable)');
    if (typeof renewable === 'function') {
      const exp = check(t, keys.publicKey, now).licence.exp * 1000;
      const at = (ms) => renewable(check(t, keys.publicKey, ms), ms);
      check_(at(now), 'a current licence is renewable');
      check_(at(exp + 1000), 'one that expired a second ago is');
      check_(at(exp + RENEW_GRACE_DAYS * 86400e3), `one that expired exactly ${RENEW_GRACE_DAYS} days ago is`);
      check_(!at(exp + RENEW_GRACE_DAYS * 86400e3 + 1000), `one that expired ${RENEW_GRACE_DAYS} days and a second ago is not`);
      check_(!renewable(check(junk, keys.publicKey, later), later), 'and a forged one never is');
    }
  }

  console.log('\n— issuing needs a key, and a plan —');
  {
    let threw = '';
    try { issue({ email: 'a@b.c', plan: 'pro' }, ''); } catch (e) { threw = e.message; }
    check_(/LICENCE_PRIVATE_KEY/.test(threw), `no signing key is a loud failure: "${threw}"`);

    threw = '';
    try { issue({ email: '', plan: 'pro' }, keys.privateKey); } catch (e) { threw = e.message; }
    check_(/address/.test(threw), 'and a licence issued to nobody is refused');
  }

  console.log('\n— the endpoint —');
  {
    const s = await startServer();
    try {
      const anon = await (await fetch(s.url + '/api/licence')).json();
      check_(!!anon.error, `no session, no licence: ${JSON.stringify(anon.error)}`);
    } finally { await s.stop(); }
  }

  /* Renewal is what makes "RATA renews itself whenever it is online" true, and
     it is the one endpoint with no sign-in behind it — the old licence is the
     credential. So what is worth testing is that presenting a licence nobody
     issued gets nothing, and that the signature is what decides. */
  console.log('\n— renewing, with the old licence as the credential —');
  {
    const s = await startServer({ env: { LICENCE_PUBLIC_KEY: keys.publicKey, LICENCE_PRIVATE_KEY: keys.privateKey } });
    const post = async licence => (await fetch(s.url + '/api/licence/renew', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ licence }),
    })).json();

    try {
      check_(!!(await post('')).error, 'nothing to renew is refused');
      check_(!!(await post('not-a-licence')).message, 'a licence that is not one is refused');

      /* The attack: mint a licence with your own key and present it. Forging
         requires the private key, which is the whole point of signing. */
      const theirs = issue({ email: 'attacker@example.com', plan: 'enterprise' }, other.privateKey);
      const r = await post(theirs);
      check_(!r.licensed, `a licence signed by somebody else's key gets nothing back: ${JSON.stringify(r.message || r.error)}`);
      check_(!r.licence, 'and certainly no token');

      /* A real, expired licence must be *accepted* for renewal — refusing one
         would mean the only licences that can be renewed are the ones that did
         not need it. Without a database configured the answer stops at "not
         configured", which still proves the signature check let it through. */
      const DAY = 86400e3;
      const lapsed = (daysAgo, slack) => issue({ email: 'buyer@example.com', plan: 'pro', days: 30,
        now: Date.now() - (daysAgo + 30) * DAY + slack }, keys.privateKey);
      const mine = lapsed(10, 0);
      const expired = await post(mine);
      check_(!/could not be read/.test(expired.message || ''),
        `an expired licence is accepted for renewal rather than dismissed: ${JSON.stringify(expired.error || expired.message)}`);

      /* But not one of any age: a token copied off a machine, or shared,
         would otherwise renew for as long as the subscription lives. Past
         RENEW_GRACE_DAYS the customer signs in at mailrata.org for a new
         one, and the database is never asked. */
      check_(RENEW_GRACE_DAYS === 90, `renewal takes a licence expired up to ${RENEW_GRACE_DAYS} days ago`);
      const inside = await post(lapsed(RENEW_GRACE_DAYS, 60e3));
      check_(!inside.reason && /not configured/.test(inside.error || ''),
        `expired ${RENEW_GRACE_DAYS} days less a minute ago: still renewed (reaches the database): ${JSON.stringify(inside)}`);
      const outside = await post(lapsed(RENEW_GRACE_DAYS, -60e3));
      check_(outside.licensed === false && outside.reason === 'too-old' && !outside.licence,
        `expired ${RENEW_GRACE_DAYS} days and a minute ago: refused: ${JSON.stringify(outside)}`);
      check_(/sign in at mailrata\.org/i.test(outside.message || ''),
        `saying where to get a new one: ${JSON.stringify(outside.message)}`);
      const ancient = await post(issue({ email: 'buyer@example.com', plan: 'pro', days: 30, now: Date.UTC(2020, 0, 1) }, keys.privateKey));
      check_(ancient.reason === 'too-old', `a licence from 2020 is refused: ${JSON.stringify(ancient.reason)}`);
    } finally { await s.stop(); }
  }

  /* A deployment with no signing key must say so rather than telling every
     paying customer they have not paid. */
  console.log('\n— a misconfigured deployment does not read as "you have not paid" —');
  {
    const s = await startServer();
    try {
      const r = await fetch(s.url + '/api/licence/renew', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ licence: issue({ email: 'buyer@example.com', plan: 'pro' }, keys.privateKey) }),
      });
      const d = await r.json();
      check_(r.status === 500, `no LICENCE_PUBLIC_KEY is a server fault (${r.status}), not a refusal`);
      check_(!/have not paid|no active subscription/i.test(JSON.stringify(d)),
        `and never blames the customer: ${JSON.stringify(d.message || d.error)}`);
    } finally { await s.stop(); }
  }
}
