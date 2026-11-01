/* A certificate chain shaped like the App Store's, made here, and JWS
   signed by it: what tests/apple.test.mjs gives the verifier instead of
   Apple's own roots, so a purchase can be signed without Apple.

   Apple signs a transaction, renewal info or a notification as a JWS
   (ES256) whose header carries x5c: the leaf, the intermediate and the root,
   base64 DER. Apple's library checks that the intermediate was issued by a
   root it was given, the leaf by the intermediate, the intermediate is a CA,
   the leaf carries Apple's marker 1.2.840.113635.100.6.11.1 and the
   intermediate 1.2.840.113635.100.6.2.1, every date is valid at the
   payload's signedDate, and the JWS verifies with the leaf's key.

   The certificates are written out byte by byte (DER) rather than with a
   tool, so the tests need nothing but Node, and so a chain can leave a
   marker out on purpose. */
import { generateKeyPairSync, sign } from 'node:crypto';

export const LEAF_MARKER = '1.2.840.113635.100.6.11.1';
export const INTERMEDIATE_MARKER = '1.2.840.113635.100.6.2.1';

const b64url = (b) => Buffer.from(b).toString('base64url');

function len(n) {
  if (n < 0x80) return Buffer.from([n]);
  const bytes = [];
  for (let v = n; v > 0; v >>= 8) bytes.unshift(v & 0xff);
  return Buffer.from([0x80 | bytes.length, ...bytes]);
}
const tlv = (tag, ...parts) => {
  const body = Buffer.concat(parts);
  return Buffer.concat([Buffer.from([tag]), len(body.length), body]);
};
const seq = (...p) => tlv(0x30, ...p);
const set = (...p) => tlv(0x31, ...p);
const explicit = (n, ...p) => tlv(0xa0 + n, ...p);
const bool = (v) => tlv(0x01, Buffer.from([v ? 0xff : 0x00]));
const nul = () => Buffer.from([0x05, 0x00]);
const octets = (b) => tlv(0x04, b);
const bits = (b) => tlv(0x03, Buffer.from([0]), b);
const utf8 = (s) => tlv(0x0c, Buffer.from(s, 'utf8'));
function int(n) {
  let hex = BigInt(n).toString(16);
  if (hex.length % 2) hex = '0' + hex;
  let b = Buffer.from(hex, 'hex');
  if (b[0] & 0x80) b = Buffer.concat([Buffer.from([0]), b]);
  return tlv(0x02, b);
}
function oid(s) {
  const [a, b, ...rest] = s.split('.').map(Number);
  const out = [40 * a + b];
  for (const v of rest) {
    const stack = [v & 0x7f];
    for (let x = v >> 7; x > 0; x >>= 7) stack.unshift(0x80 | (x & 0x7f));
    out.push(...stack);
  }
  return tlv(0x06, Buffer.from(out));
}
function time(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, '0');
  const s = `${p(d.getUTCFullYear() % 100)}${p(d.getUTCMonth() + 1)}${p(d.getUTCDate())}${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}Z`;
  return tlv(0x17, Buffer.from(s, 'ascii'));
}
const name = (cn) => seq(set(seq(oid('2.5.4.3'), utf8(cn))));
const ECDSA_SHA256 = seq(oid('1.2.840.10045.4.3.2'));

/* One certificate: `key` is the subject's key pair, `signer` the issuer's
   private key (the subject's own for a root). */
function certificate({ subject, issuer, key, signer, serial, from, to, ca, marker }) {
  const exts = [seq(oid('2.5.29.19'), bool(true), octets(ca ? seq(bool(true)) : seq()))];
  if (marker) exts.push(seq(oid(marker), octets(nul())));
  const tbs = seq(
    explicit(0, int(2)),
    int(serial),
    ECDSA_SHA256,
    name(issuer),
    seq(time(from), time(to)),
    name(subject),
    key.publicKey.export({ type: 'spki', format: 'der' }),
    explicit(3, seq(...exts)),
  );
  return seq(tbs, ECDSA_SHA256, bits(sign('sha256', tbs, signer)));
}

/* A root, an intermediate and a leaf, valid from a year before `now` to a
   year after it. `leafMarker` and `intermediateMarker` false leave Apple's
   markers out, which the library must refuse. */
export function makeChain({ now = Date.now(), leafMarker = true, intermediateMarker = true, tag = 'RATA test' } = {}) {
  const year = 365 * 86400e3;
  const from = now - year, to = now + year;
  const rootKey = generateKeyPairSync('ec', { namedCurve: 'P-256' });
  const midKey = generateKeyPairSync('ec', { namedCurve: 'P-256' });
  const leafKey = generateKeyPairSync('ec', { namedCurve: 'P-256' });
  const root = certificate({ subject: `${tag} Root CA`, issuer: `${tag} Root CA`, key: rootKey, signer: rootKey.privateKey, serial: 1, from, to, ca: true });
  const intermediate = certificate({ subject: `${tag} Intermediate CA`, issuer: `${tag} Root CA`, key: midKey, signer: rootKey.privateKey, serial: 2, from, to, ca: true,
    marker: intermediateMarker ? INTERMEDIATE_MARKER : null });
  const leaf = certificate({ subject: `${tag} Store Signing`, issuer: `${tag} Intermediate CA`, key: leafKey, signer: midKey.privateKey, serial: 3, from, to, ca: false,
    marker: leafMarker ? LEAF_MARKER : null });
  return { root, intermediate, leaf, leafKey: leafKey.privateKey, rootBase64: root.toString('base64') };
}

/* A JWS as the App Store writes one: ES256, x5c leaf first. */
export function signJws(payload, chain) {
  const header = { alg: 'ES256', x5c: [chain.leaf, chain.intermediate, chain.root].map((c) => c.toString('base64')) };
  const data = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(payload))}`;
  const sig = sign('sha256', Buffer.from(data), { key: chain.leafKey, dsaEncoding: 'ieee-p1363' });
  return `${data}.${b64url(sig)}`;
}

/* What Xcode's StoreKit testing signs with: its own certificate, which no
   App Store root issued. The library does not check a signature at all in
   the Xcode and LocalTesting environments; this one is signed by a key
   nobody trusts, so it verifies nowhere else. */
export function unsignedJws(payload) {
  const stray = makeChain({ tag: 'Xcode stand-in' });
  return signJws(payload, stray);
}
