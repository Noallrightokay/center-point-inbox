import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import { planDef } from '../../../../lib/plan';
import { issue, LICENCE_DAYS } from '../../../../lib/licence';
import { corsHeaders, preflight } from '../../../../lib/cors';
import {
  APPLE_SAID, appleVerifiers, verifyApple, readTransactions, appleIgnored, applePreferred,
  applePurchase, appleWrite, appleRow, appleLive, appleUntil, appleSub, tableMissing,
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
   revoked or upgraded from unlocks nothing. Of the rest the higher plan
   wins, then the later expiry; a live one's row in apple_subscriptions is
   written in the order of the subscription's periods, then of when Apple
   signed it (lib/apple.js, appleWrite), and the licence follows the row as
   it then stands. So a transaction replayed after its refund, or one for a
   period that a later one has replaced, unlocks nothing, and one for the
   period after a refunded one is recorded whenever Apple signed it.

   One past its expiry writes nothing, and unlocks only what its row says
   is live. That is a billing grace period: StoreKit still lists the
   subscription by the transaction that has just expired, and Apple's
   DID_FAIL_TO_RENEW notification has put when the grace period ends in
   the row (grace_until). Without the row, it unlocks nothing.

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
  /* Each verified transaction worth asking about, with whether it is past
     its expiry (`lapsed`), so only its row can make it live. */
  const live = [];
  const refused = [];
  let otherApp = 0;
  let noAppId = false;
  for (const s of seen) {
    if (s.ok) {
      const why = appleIgnored(s.payload, made.settings, now);
      if (!why || why === 'expired') live.push({ ...s, lapsed: !!why });
    } else if (s.noAppId) {
      noAppId = true;
    } else if (s.status === 'INVALID_APP_IDENTIFIER') {
      /* Another app's purchase (a bundle id not RATA's) is ignored like any
         other that unlocks nothing, but logged: when every purchase says
         so, APPLE_BUNDLE_ID does not match the app (LAUNCH.md section 11),
         and each paying customer would otherwise be told they have no
         subscription with nothing in the log. */
      otherApp += 1;
    } else {
      /* Anything else that does not verify is a refusal. The log gets the
         library's status name, never the transaction. */
      refused.push(s.status);
    }
  }
  if (otherApp) console.warn(`licence/apple: ${otherApp} transaction(s) were for another app (INVALID_APP_IDENTIFIER), so ignored; if every purchase says so, APPLE_BUNDLE_ID does not match the app (rata-next/LAUNCH.md section 11)`);
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
    const { environment, lapsed } = live.find((l) => l.payload === s);
    let written;
    if (lapsed) {
      written = await appleRow(sb, s.originalTransactionId);
    } else {
      written = await appleWrite(sb, s.originalTransactionId, applePurchase(s, environment));
    }
    if (written.error) {
      /* Fail closed: a licence that cannot be recorded, or checked against
         its row, is not issued. */
      if (tableMissing(written.error)) {
        console.warn('licence/apple: the database has no apple_subscriptions table; run section 7 of rata-next/database.sql');
        return refuse('not-configured', APPLE_SAID.off, 503);
      }
      console.warn(`licence/apple: could not ${lapsed ? 'read' : 'record'} an App Store purchase`, written.error.code || written.error.message);
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
