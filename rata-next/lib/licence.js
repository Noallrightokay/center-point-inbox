import { createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from 'node:crypto';

/* ---------------------------------------------------------------------------
   Licences.

   The app runs on the customer's machine and talks to their mail servers
   directly, so it cannot ask a server whether they have paid — not without
   being useless on a train, and not without making RATA's uptime a condition
   of reading your own mail. A licence therefore has to be checkable offline.

   So it is signed, not looked up. The server holds an Ed25519 private key and
   issues a small token saying who paid and for what; the app ships with the
   public key and can verify that token with no network at all. Forging one
   requires the private key, which never leaves the server.

   Ed25519 rather than RSA or an HMAC: signatures are 64 bytes, keys are 32, and
   an HMAC would need the verifying secret inside the app — where every customer
   could read it and mint their own licences.

   The token is deliberately readable. Base64url of JSON, not encryption: there
   is nothing secret in "this person bought Pro", and a customer being able to
   see what they were issued is a feature when something goes wrong.

   Stored form:  v1.<payload>.<signature>
   --------------------------------------------------------------------------- */

const PREFIX = 'v1';
const b64 = b => Buffer.from(b).toString('base64url');
const un64 = s => Buffer.from(s, 'base64url');

/* How long an issued licence stands before the app should ask for a fresh one.

   Not the subscription's length — the subscription is monthly and a licence
   that expired with it would lock someone out the moment their card was
   retried. This is the offline grace period: how long RATA keeps working with
   no contact at all. Thirty days is long enough to cover a holiday, a dead
   laptop battery and a fortnight of bad wifi, and short enough that a
   cancellation stops mattering within a billing cycle or two. */
export const LICENCE_DAYS = 30;

export function generateKeys() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return {
    privateKey: privateKey.export({ type: 'pkcs8', format: 'pem' }),
    publicKey: publicKey.export({ type: 'spki', format: 'pem' }),
  };
}

function privateKeyFrom(pem) {
  if (!pem) throw new Error('LICENCE_PRIVATE_KEY is not set — no licence can be issued. See LICENSING.md.');
  /* Hosting panels mangle multi-line values, so the key is accepted either as
     a real PEM or with its newlines written as \n. */
  return createPrivateKey(String(pem).includes('-----') ? String(pem).replace(/\\n/g, '\n') : pem);
}

function publicKeyFrom(pem) {
  if (!pem) return null;
  try { return createPublicKey(String(pem).replace(/\\n/g, '\n')); } catch { return null; }
}

/* Issue a licence. Everything the app needs to decide what to unlock, and
   nothing it does not: no name, no mailbox list, no card details. */
export function issue({ email, plan, days = LICENCE_DAYS, now = Date.now() }, privatePem = process.env.LICENCE_PRIVATE_KEY) {
  const addr = String(email || '').trim().toLowerCase();
  if (!addr) throw new Error('a licence needs an address to be issued to');
  if (!plan) throw new Error('a licence needs a plan');

  const payload = {
    v: 1,
    sub: addr,
    plan,
    iat: Math.floor(now / 1000),
    exp: Math.floor(now / 1000) + days * 86400,
  };
  const body = b64(JSON.stringify(payload));
  const sig = sign(null, Buffer.from(body, 'utf8'), privateKeyFrom(privatePem));
  return `${PREFIX}.${body}.${b64(sig)}`;
}

/* Check one. Returns the payload when it is genuine and current, or an object
   saying why not — never throws, because every caller's honest response to a
   bad licence is the same: ask the customer to sign in again.

   `expired` is reported separately from `invalid` on purpose. An expired
   licence is a real one that needs refreshing, and the app should say so
   rather than accusing the customer of forging it. */
export function check(token, publicPem = process.env.LICENCE_PUBLIC_KEY, now = Date.now()) {
  const key = publicKeyFrom(publicPem);
  if (!key) return { ok: false, reason: 'no-public-key' };

  const parts = String(token || '').split('.');
  if (parts.length !== 3 || parts[0] !== PREFIX) return { ok: false, reason: 'malformed' };

  let good = false;
  try { good = verify(null, Buffer.from(parts[1], 'utf8'), key, un64(parts[2])); } catch { good = false; }
  if (!good) return { ok: false, reason: 'bad-signature' };

  let payload;
  try { payload = JSON.parse(un64(parts[1]).toString('utf8')); } catch { return { ok: false, reason: 'malformed' }; }
  if (payload.v !== 1 || !payload.sub || !payload.plan) return { ok: false, reason: 'malformed' };

  /* The signature is checked before the clock, so a forged token is never
     reported as merely expired. */
  if (typeof payload.exp !== 'number' || payload.exp * 1000 < now) {
    return { ok: false, reason: 'expired', expiredAt: payload.exp, licence: payload };
  }

  return { ok: true, licence: payload };
}

/* What the app should do about a licence it cannot use, in words a customer
   can act on. Kept here so the desktop app and the website say the same thing. */
export function explain(reason) {
  switch (reason) {
    case 'expired':
      return 'This licence needs refreshing. Open RATA while online and it will renew itself.';
    case 'bad-signature':
    case 'malformed':
      return 'This licence could not be read. Sign in at mailrata.org to get a new one.';
    case 'no-public-key':
      return 'This copy of RATA was built without a licence key and cannot check licences. Reinstall it from mailrata.org.';
    default:
      return 'This licence could not be used. Sign in at mailrata.org to get a new one.';
  }
}
