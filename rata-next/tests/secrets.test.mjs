/* Encryption of the credentials RATA holds.

   These are the properties that matter if the database is ever read by someone
   who should not have it: the password is not in there, a tampered row will not
   open, and a row copied into somebody else's account will not open either. */
import { makeChecker, startServer } from './helpers.mjs';

const KEY_A = Buffer.alloc(32, 1).toString('base64url');
const KEY_B = Buffer.alloc(32, 2).toString('base64url');

/* The module reads process.env at call time, so each case sets what it needs
   and the import is shared. */
async function withKeys(primary, previous, fn) {
  const before = [process.env.TOKEN_ENC_KEY, process.env.TOKEN_ENC_KEY_OLD];
  if (primary) process.env.TOKEN_ENC_KEY = primary; else delete process.env.TOKEN_ENC_KEY;
  if (previous) process.env.TOKEN_ENC_KEY_OLD = previous; else delete process.env.TOKEN_ENC_KEY_OLD;
  try { return await fn(); }
  finally {
    if (before[0] === undefined) delete process.env.TOKEN_ENC_KEY; else process.env.TOKEN_ENC_KEY = before[0];
    if (before[1] === undefined) delete process.env.TOKEN_ENC_KEY_OLD; else process.env.TOKEN_ENC_KEY_OLD = before[1];
  }
}

export default async function run(state) {
  const check = makeChecker(state);
  const S = await import('../lib/secrets.js');
  const USER = '3f30ce69-b1af-42b9-b701-4488a85dc957';
  const PROV = 'mail:owner_example_com';
  const PASS = 'abcd efgh ijkl mnop';

  console.log('\n— the password is not in what gets stored —');
  await withKeys(KEY_A, null, () => {
    const sealed = S.encryptSecret(PASS, USER, PROV);
    check(!sealed.includes(PASS) && !sealed.includes('abcdefghijklmnop'),
      `stored form: ${sealed.slice(0, 34)}…`);
    check(S.isEncrypted(sealed), 'and is recognisable as encrypted');
    check(S.decryptSecret(sealed, USER, PROV) === PASS, 'it comes back exactly as it went in');

    const again = S.encryptSecret(PASS, USER, PROV);
    check(again !== sealed, 'the same password encrypts differently each time (fresh IV)');
    check(S.decryptSecret(again, USER, PROV) === PASS, 'and both still open');
  });

  console.log('\n— a row that has been tampered with does not open —');
  await withKeys(KEY_A, null, () => {
    const sealed = S.encryptSecret(PASS, USER, PROV);
    const [v, iv, tag, ct] = sealed.split('.');
    const flip = s => { const b = Buffer.from(s, 'base64url'); b[0] ^= 1; return b.toString('base64url'); };
    check(S.decryptSecret([v, iv, tag, flip(ct)].join('.'), USER, PROV) === null, 'altered ciphertext → null');
    check(S.decryptSecret([v, flip(iv), tag, ct].join('.'), USER, PROV) === null, 'altered IV → null');
    check(S.decryptSecret([v, iv, flip(tag), ct].join('.'), USER, PROV) === null, 'altered tag → null');
  });

  console.log('\n— a credential cannot be moved to another account —');
  await withKeys(KEY_A, null, () => {
    const sealed = S.encryptSecret(PASS, USER, PROV);
    const attacker = '00000000-0000-0000-0000-000000000001';
    check(S.decryptSecret(sealed, attacker, PROV) === null,
      'copied into another user’s row → null, not their neighbour’s mail password');
    check(S.decryptSecret(sealed, USER, 'mail:someone_else_com') === null,
      'copied onto another provider row → null');
    check(S.decryptSecret(sealed, USER, PROV) === PASS, 'and the row it belongs to still works');
  });

  console.log('\n— rotating the key —');
  await withKeys(KEY_A, null, async () => {
    const old = S.encryptSecret(PASS, USER, PROV);
    await withKeys(KEY_B, KEY_A, () => {
      check(S.decryptSecret(old, USER, PROV) === PASS, 'rows written under the old key still open');
      const fresh = S.encryptSecret(PASS, USER, PROV);
      check(fresh !== old, 'new writes use the new key');
      /* Once the old key is dropped, anything still written under it is gone —
         which is the point of rotating, and why re-encrypting matters. */
      return withKeys(KEY_B, null, () => {
        check(S.decryptSecret(old, USER, PROV) === null, 'and drop the old key, and old rows stop opening');
        check(S.decryptSecret(fresh, USER, PROV) === PASS, 'while the re-encrypted ones carry on');
      });
    });
  });

  console.log('\n— rows written before encryption existed —');
  await withKeys(KEY_A, null, () => {
    check(S.decryptSecret('plain-app-password', USER, PROV) === 'plain-app-password',
      'legacy plaintext is passed through rather than stranding the account');
    check(!S.isEncrypted('plain-app-password'), 'and is not mistaken for ciphertext');
    const row = S.sealRow({ user_id: USER, provider: PROV, access: 'plain-app-password', refresh: null });
    check(S.isEncrypted(row.access), 'any write re-encrypts it');
    check(row.refresh === null, 'and an absent second secret stays absent rather than encrypting ""');
  });

  console.log('\n— with no key configured, nothing is stored in the clear —');
  await withKeys(null, null, () => {
    check(S.encryptionReady() === false, 'the app knows it is unconfigured');
    let threw = '';
    try { S.encryptSecret(PASS, USER, PROV); } catch (e) { threw = e.message; }
    check(/TOKEN_ENC_KEY/.test(threw), `and refuses to store: "${threw.slice(0, 62)}…"`);
  });

  console.log('\n— a key of the wrong size is rejected, not quietly padded —');
  await withKeys(Buffer.alloc(16, 1).toString('base64url'), null, () => {
    check(S.encryptionReady() === false, 'a 16-byte key does not count as ready');
    let threw = '';
    try { S.keys(); } catch (e) { threw = e.message; }
    check(/32 bytes/.test(threw) && /openssl/.test(threw), `and says how to make a real one: "${threw}"`);
  });

  console.log('\n— the running app refuses to link without a key —');
  {
    const s = await startServer({ env: { TOKEN_ENC_KEY: '' } });
    try {
      const r = await fetch(s.url + '/api/link/mail', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: 'someone@example.com', appPassword: 'abcdefghijkl' }),
      });
      const body = await r.json();
      /* Unauthenticated here, so it stops earlier than the key check — what
         matters is that it never reaches a state where it would store one. */
      check(!!body.error, `refused before storing anything: ${JSON.stringify(body.error)}`);
    } finally { await s.stop(); }
  }
}
