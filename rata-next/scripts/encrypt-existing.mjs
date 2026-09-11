/* Encrypt credentials that were stored before encryption existed.

   Every write already re-encrypts, so rows heal themselves the next time an
   account is relinked or a Microsoft token refreshes. This converts the rest
   in one pass, so there is no plaintext sitting in the table waiting for
   someone to get around to it.

   Run it once after deploying, with the same environment the app has:

     SUPABASE_URL=... SUPABASE_SERVICE_ROLE_KEY=... TOKEN_ENC_KEY=... \
       node scripts/encrypt-existing.mjs

   It is safe to run repeatedly: rows already encrypted are left alone. Add
   --dry-run to see what it would touch without writing. */

import { createClient } from '@supabase/supabase-js';
import { isEncrypted, encryptSecret, encryptionReady } from '../lib/secrets.js';

const dry = process.argv.includes('--dry-run');

const url = process.env.SUPABASE_URL;
const key = process.env.SUPABASE_SERVICE_ROLE_KEY;
if (!url || !key) {
  console.error('Set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY.');
  process.exit(2);
}
if (!encryptionReady()) {
  console.error('Set TOKEN_ENC_KEY (32 bytes). Generate one with: openssl rand -base64 32');
  process.exit(2);
}

const sb = createClient(url, key, { auth: { persistSession: false } });
const { data: rows, error } = await sb.from('provider_tokens').select('user_id,provider,label,access,refresh');
if (error) { console.error('Could not read provider_tokens: ' + error.message); process.exit(1); }

let done = 0, already = 0, failed = 0;

for (const r of rows || []) {
  const needs = ['access', 'refresh'].filter(f => r[f] && !isEncrypted(r[f]));
  if (!needs.length) { already++; continue; }

  const patch = {};
  for (const f of needs) patch[f] = encryptSecret(r[f], r.user_id, r.provider);

  /* The label is the mailbox address, not a secret — printing it is what makes
     the output useful if one row fails. The credential never is. */
  if (dry) {
    console.log(`would encrypt  ${r.provider}  ${r.label || ''}  (${needs.join(', ')})`);
    done++;
    continue;
  }
  const { error: e } = await sb.from('provider_tokens').update(patch)
    .eq('user_id', r.user_id).eq('provider', r.provider);
  if (e) { console.error(`FAILED  ${r.provider}  ${r.label || ''}: ${e.message}`); failed++; }
  else { console.log(`encrypted  ${r.provider}  ${r.label || ''}  (${needs.join(', ')})`); done++; }
}

console.log(`\n${dry ? 'would encrypt' : 'encrypted'} ${done}, already encrypted ${already}, failed ${failed}`);
process.exit(failed ? 1 : 0);
