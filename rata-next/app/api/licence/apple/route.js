import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import { planDef } from '../../../../lib/plan';
import { issue, LICENCE_DAYS } from '../../../../lib/licence';
import { corsHeaders, preflight } from '../../../../lib/cors';
import {
  APPLE_SAID, appleVerifiers, verifyApple, readTransactions, appleIgnored, applePreferred,
  appleGraceFor, applePurchase, appleWrite, appleRow, appleLive, appleUntil, appleLicenceUntil, appleSub, appleTableBehind,
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
   period that a later one has replaced, changes nothing and unlocks
   nothing of its own (only what the row says is live), and one for the
   period after a refunded one is recorded whenever Apple signed it. A
   transaction alone never touches the grace period, which only a renewal
   info reports.

   Beside the transactions the app may send each subscription's renewal
   info as Apple signed it (`renewalInfos`, lib/apple.js readTransactions),
   which says when a billing grace period ends. Each is verified by the
   verifier its own environment picks, and is used only with a transaction
   of this request that has the same original transaction id and was
   verified in the same environment (so by the same verifier, and for
   RATA's bundle id, which the transaction carries and the renewal info
   does not). One that matches no such transaction is refused: it records
   nothing and unlocks nothing, and the log counts it.

   A matched one is used only when it states a grace period for the period
   that transaction pays for: a gracePeriodExpiresDate later than the
   transaction's expiry (lib/apple.js, appleGraceFor). StoreKit's
   transactions are its local copy and the renewal info is fetched from
   Apple, and neither names a period, so after billing recovers the phone
   can send the lapsed transaction beside a fresh renewal info that gives
   no grace period because the next period runs; one like that, or one
   from an earlier grace period, is left out as though not sent, and never
   ends a grace period the row holds. A used one's grace period is
   written with the transaction (applePurchase), under the guard a
   notification's is written under, stamped with when Apple signed the
   renewal info; so one older than what Apple has since said of the
   period changes nothing.

   A transaction past its expiry unlocks only what its row says is live.
   That is a billing grace period: StoreKit still lists the subscription
   by the transaction that has just expired, and the row holds when the
   grace period ends (grace_until), from Apple's DID_FAIL_TO_RENEW
   notification or from the renewal info the phone sends with the
   transaction. With a renewal info stating its grace period the expired
   transaction is written as a live one is, so the phone alone records the
   grace period when no notification has reached mailrata.org; without
   one, it only reads its row, and without the row it unlocks nothing.

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
     its expiry (`lapsed`), so only its row, or its renewal info, can make
     it live. */
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

  /* The renewal infos, each matched to a transaction above by original
     transaction id and environment, and used with it only when it states
     a grace period for that transaction's period (appleGraceFor); of
     several so for one, the one Apple signed last. A refused one is only
     counted in the log, by the library's status name, never its contents.
     A matched one that states no grace period for its transaction is
     neither used nor counted: the phone sends one for every subscription,
     and outside a grace period none states one. */
  const renewalOf = new Map();
  const renewalRefused = [];
  let unmatched = 0;
  const signedOf = (p) => (typeof p.signedDate === 'number' && Number.isFinite(p.signedDate) ? p.signedDate : 0);
  const renewalsSeen = await Promise.all(asked.renewals.map((jws) => verifyApple('renewal', jws, made)));
  for (const r of renewalsSeen) {
    if (!r.ok) {
      renewalRefused.push(r.status);
      continue;
    }
    const id = r.payload && r.payload.originalTransactionId;
    const twins = live.filter((l) => l.payload.originalTransactionId === id && l.environment === r.environment);
    if (!twins.length) {
      unmatched += 1;
      continue;
    }
    for (const twin of twins) {
      if (!appleGraceFor(twin.payload, r.payload)) continue;
      const held = renewalOf.get(twin.payload);
      if (!held || signedOf(r.payload) > signedOf(held)) renewalOf.set(twin.payload, r.payload);
    }
  }
  if (renewalRefused.length) console.warn(`licence/apple: ${renewalRefused.length} renewal info(s) did not verify: ${[...new Set(renewalRefused)].join(', ')}`);
  if (unmatched) console.warn(`licence/apple: ${unmatched} renewal info(s) matched no transaction sent with them, so refused`);

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
    const renewal = renewalOf.get(s) || null;
    /* A lapsed transaction with no renewal info stating its grace period
       only reads its row; anything else is written. */
    const read = lapsed && !renewal;
    const written = read
      ? await appleRow(sb, s.originalTransactionId)
      : await appleWrite(sb, s.originalTransactionId, applePurchase(s, environment, renewal));
    if (written.error) {
      /* Fail closed: a licence that cannot be recorded, or checked against
         its row, is not issued. */
      if (appleTableBehind(written.error)) {
        console.warn('licence/apple: the database has no apple_subscriptions table, or one without a column section 7 now adds; run section 7 of rata-next/database.sql');
        return refuse('not-configured', APPLE_SAID.off, 503);
      }
      console.warn(`licence/apple: could not ${read ? 'read' : 'record'} an App Store purchase`, written.error.code || written.error.message);
      return refuse('unavailable', APPLE_SAID.busy, 503);
    }
    if (!appleLive(written.row, now)) continue;

    const plan = written.row.plan;
    let licence;
    try {
      /* No longer than what the row pays for, and a margin (lib/apple.js,
         appleLicenceUntil): at most LICENCE_DAYS. */
      licence = issue({ email: appleSub(s.originalTransactionId), plan, until: appleLicenceUntil(written.row, now), now });
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
