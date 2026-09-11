import { createCipheriv, createDecipheriv, randomBytes, timingSafeEqual } from 'node:crypto';

/* ---------------------------------------------------------------------------
   Encrypting the credentials RATA holds.

   provider_tokens stores the thing that opens a user's mailbox: an app
   password, or an OAuth access and refresh token. Those are not RATA's secrets
   to lose. Anything that reads the database — a backup copied somewhere it
   should not be, a stolen service-role key, an operator glancing at a table —
   otherwise reads live mail credentials for every account, and with SMTP added
   those credentials send as well as read.

   AES-256-GCM, one random IV per value, and the authentication tag stored with
   it, so a tampered ciphertext fails to decrypt rather than yielding rubbish.

   The additional authenticated data is the row's own identity — user id and
   provider. That is what stops a ciphertext being *moved*: someone with write
   access to the table could otherwise copy another user's encrypted password
   into their own row and have the server decrypt it for them on the next sync.
   Bound this way, a value only decrypts in the row it was written for.

   Stored form:  v1.<iv>.<tag>.<ciphertext>, all base64url.

   A value without that prefix is treated as legacy plaintext and passed
   through, so this change does not strand rows written before it; every write
   re-encrypts, and scripts/encrypt-existing.mjs converts the rest in one pass.
   --------------------------------------------------------------------------- */

const PREFIX = 'v1';
const ALGO = 'aes-256-gcm';
const b64 = b => Buffer.from(b).toString('base64url');
const un64 = s => Buffer.from(s, 'base64url');

/* Accepts base64, base64url or hex, and insists on a full 32 bytes: a short
   key is a silently weak key, which is worse than no key at all. */
function parseKey(raw, name) {
  if (!raw) return null;
  const s = String(raw).trim();
  let buf;
  if (/^[0-9a-f]{64}$/i.test(s)) buf = Buffer.from(s, 'hex');
  else buf = Buffer.from(s, 'base64url');
  if (buf.length !== 32) {
    throw new Error(`${name} must be 32 bytes (got ${buf.length}). Generate one with: openssl rand -base64 32`);
  }
  return buf;
}

/* The first key encrypts. The rest only decrypt, which is what makes rotation
   possible: set the new key as TOKEN_ENC_KEY, move the old one to
   TOKEN_ENC_KEY_OLD, and existing rows keep opening until they are rewritten. */
export function keys() {
  const primary = parseKey(process.env.TOKEN_ENC_KEY, 'TOKEN_ENC_KEY');
  const previous = parseKey(process.env.TOKEN_ENC_KEY_OLD, 'TOKEN_ENC_KEY_OLD');
  return { primary, all: [primary, previous].filter(Boolean) };
}

export function encryptionReady() {
  try { return !!keys().primary; } catch { return false; }
}

export function isEncrypted(v) {
  return typeof v === 'string' && v.startsWith(PREFIX + '.') && v.split('.').length === 4;
}

function aadFor(userId, provider) {
  return Buffer.from(`${userId}:${provider}`, 'utf8');
}

/* Refuses rather than falling back to plaintext. A missing key is a deployment
   that has not been configured, and storing a mailbox password in the clear
   because a variable is unset is not a reasonable thing to do quietly. */
export function encryptSecret(plain, userId, provider) {
  if (plain === null || plain === undefined || plain === '') return null;
  const { primary } = keys();
  if (!primary) {
    throw new Error('TOKEN_ENC_KEY is not set — RATA will not store mail credentials unencrypted. See BACKEND-SETUP.md.');
  }
  const iv = randomBytes(12);
  const c = createCipheriv(ALGO, primary, iv);
  c.setAAD(aadFor(userId, provider));
  const ct = Buffer.concat([c.update(String(plain), 'utf8'), c.final()]);
  return [PREFIX, b64(iv), b64(c.getAuthTag()), b64(ct)].join('.');
}

/* Returns the plaintext, or null when the value cannot be opened — a wrong
   key, a tampered row, or one moved between users. Never throws at the call
   site, because every caller's honest response to an unreadable credential is
   the same: tell the user to relink that account. */
export function decryptSecret(stored, userId, provider) {
  if (stored === null || stored === undefined || stored === '') return null;
  if (!isEncrypted(stored)) return String(stored);   // written before encryption

  const [, ivS, tagS, ctS] = String(stored).split('.');
  let all;
  try { all = keys().all; } catch { return null; }

  for (const key of all) {
    try {
      const d = createDecipheriv(ALGO, key, un64(ivS));
      d.setAAD(aadFor(userId, provider));
      d.setAuthTag(un64(tagS));
      return Buffer.concat([d.update(un64(ctS)), d.final()]).toString('utf8');
    } catch { /* try the previous key */ }
  }
  return null;
}

/* Encrypt every secret-bearing field of a row about to be written. */
export function sealRow(row) {
  const out = { ...row };
  for (const f of ['access', 'refresh']) {
    if (f in out) out[f] = encryptSecret(out[f], row.user_id, row.provider);
  }
  return out;
}

/* And open them again on the way back. */
export function openRow(row, userId) {
  if (!row) return row;
  const out = { ...row };
  for (const f of ['access', 'refresh']) {
    if (f in out) out[f] = decryptSecret(out[f], userId ?? row.user_id, row.provider);
  }
  return out;
}

/* Constant-time equality, for comparing anything secret-derived. */
export function sameSecret(a, b) {
  const x = Buffer.from(String(a ?? ''), 'utf8');
  const y = Buffer.from(String(b ?? ''), 'utf8');
  if (x.length !== y.length) return false;
  return timingSafeEqual(x, y);
}
