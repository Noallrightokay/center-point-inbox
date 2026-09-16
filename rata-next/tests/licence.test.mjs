/* Licences.

   This is the only thing standing between "paid" and "the app runs", and the
   app checks it on the customer's own machine — where they can read the code,
   read the token, and change the clock. So what is worth testing is that none
   of that helps: a forged licence must fail, an edited one must fail, and the
   difference between "expired" and "forged" must survive, because telling a
   paying customer their licence is fake is a support call you do not recover
   from. */
import { startServer, makeChecker } from './helpers.mjs';
import { generateKeys, issue, check, explain, LICENCE_DAYS } from '../lib/licence.js';

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
}
