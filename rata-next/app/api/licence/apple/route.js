import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import { planDef } from '../../../../lib/plan';
import { issue, LICENCE_DAYS } from '../../../../lib/licence';
import { corsHeaders, preflight } from '../../../../lib/cors';
import {
  APPLE_SAID, appleVerifiers, verifyApple, readTransactions, appleIgnored, applePreferred,
  appleRowOf, appleWrite, appleLive, appleUntil, appleSub, tableMissing,
} from '../../../../lib/apple';

export const dynamic = 'force-dynamic';

/* A licence for a subscription bought in the iPhone app.

   The app sends what StoreKit says it is entitled to now
   (Transaction.currentEntitlements, after a purchase, a restore or when its
   licence is due): each transaction as Apple signed it. Apple's signature is
   the credential, as the old licence is for /api/licence/renew, so there is
   no sign-in and no account: a purchase on the phone needs neither.

   Each transaction is verified (lib/apple.js). One for another app, another
   product, something other than an auto-renewable subscription, refunded,
   revoked or expired unlocks nothing. Of the live ones the higher plan wins,
   then the later expiry; its row in apple_subscriptions is written, never
   over a row Apple signed something newer for, and the licence follows the
   row as it then stands. So a transaction replayed after its refund finds
   the refund already recorded and unlocks nothing.

   A refusal is { licensed: false, reason, message }, the shape
   /api/licence/renew answers with: the app keeps any licence that still
   works and shows the sentence. The same sentence is also `error`, which is
   what the design named it. Nothing is issued that is not recorded first. */
export function OPTIONS(req) {
  return preflight(req);
}

export async function POST(req) {
  const res = await licenceFor(req);
  for (const [k, v] of Object.entries(corsHeaders(req))) res.headers.set(k, v);
  return res;
}

const refuse = (reason, message, status = 200) =>
  NextResponse.json({ licensed: false, reason, message, error: message }, { status });

async function licenceFor(req) {
  const asked = readTransactions(await req.text());
  if (!asked.ok) return refuse('malformed', APPLE_SAID.malformed, 400);
  if (!asked.list.length) return refuse('no-subscription', APPLE_SAID.nothingLive);

  const made = appleVerifiers();
  if (made.off) {
    console.warn(`licence/apple: ${made.off}; see rata-next/LAUNCH.md section 11`);
    return refuse('not-configured', APPLE_SAID.off, 503);
  }

  const now = Date.now();
  const seen = await Promise.all(asked.list.map((jws) => verifyApple('transaction', jws, made)));
  const live = [];
  const refused = [];
  let noAppId = false;
  for (const s of seen) {
    if (s.ok) {
      if (!appleIgnored(s.payload, made.settings, now)) live.push(s);
    } else if (s.noAppId) {
      noAppId = true;
    } else if (s.status !== 'INVALID_APP_IDENTIFIER') {
      /* Another app's purchase (the library's INVALID_APP_IDENTIFIER, a
         bundle id not RATA's) is ignored like any other that unlocks
         nothing; anything else that does not verify is a refusal. The log
         gets the library's status name, never the transaction. */
      refused.push(s.status);
    }
  }
  if (refused.length) console.warn(`licence/apple: ${refused.length} transaction(s) did not verify: ${[...new Set(refused)].join(', ')}`);

  if (!live.length) {
    if (noAppId) {
      console.warn('licence/apple: a Production purchase arrived and APPLE_APP_ID is not set; see rata-next/LAUNCH.md section 11');
      return refuse('not-configured', APPLE_SAID.off, 503);
    }
    return refused.length ? refuse('unverified', APPLE_SAID.unverified) : refuse('no-subscription', APPLE_SAID.nothingLive);
  }

  const sb = admin();
  if (!sb) {
    console.warn('licence/apple: SUPABASE_URL or SUPABASE_SERVICE_ROLE_KEY is not set, so no App Store purchase can be recorded');
    return refuse('not-configured', APPLE_SAID.off, 503);
  }

  for (const s of applePreferred(live.map((l) => l.payload))) {
    const env = live.find((l) => l.payload === s).environment;
    const row = appleRowOf(s, env);
    const written = await appleWrite(sb, s.originalTransactionId, { patch: row, insert: row });
    if (written.error) {
      /* Fail closed: a licence that cannot be recorded is not issued. */
      if (tableMissing(written.error)) {
        console.warn('licence/apple: the database has no apple_subscriptions table; run section 7 of rata-next/database.sql');
        return refuse('not-configured', APPLE_SAID.off, 503);
      }
      console.warn('licence/apple: could not record an App Store purchase', written.error.code || written.error.message);
      return refuse('unavailable', APPLE_SAID.busy, 503);
    }
    if (!appleLive(written.row, now)) continue;

    const plan = written.row.plan;
    let licence;
    try {
      licence = issue({ email: appleSub(s.originalTransactionId), plan });
    } catch (e) {
      /* The signing key is missing or unreadable: ours, for the log only
         (SEC-7), and never "you have not paid". */
      console.error(`licence/apple: no licence could be issued: ${e.message}`);
      return NextResponse.json({
        licensed: false,
        reason: 'not-configured',
        message: 'mailrata.org cannot issue licences at the moment. This is at our end, not yours. Try again later.',
      }, { status: 500 });
    }
    const def = planDef(plan);
    return NextResponse.json({
      licensed: true,
      licence,
      plan,
      planLabel: def.label,
      /* When the subscription, as Apple last told mailrata.org, stops
         being live if nothing renews it: its expiry, or the end of a
         billing grace period. The licence's own expiry is in the licence. */
      expires: appleUntil(written.row),
      renewWithinDays: LICENCE_DAYS,
      message: `Licensed for ${def.label} through the App Store. RATA renews the licence by itself while the subscription lasts.`,
    });
  }
  return refuse('no-subscription', APPLE_SAID.nothingLive);
}
