import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import {
  APPLE_ACTED, appleVerifiers, appleTakesProduction, verifyApple, verifyAppleIn, readNotification, appleChange, appleWrite, appleTableBehind,
  appleOtherApp,
} from '../../../../lib/apple';

export const dynamic = 'force-dynamic';

/* App Store Server Notifications, version 2.

   Apple tells mailrata.org when a subscription bought in the iPhone app
   renews, fails to renew, expires, is refunded or is revoked, so the row
   /api/licence/renew reads stays true even when the phone has not been
   opened. The owner points App Store Connect here, for Production and
   Sandbox (LAUNCH.md section 11).

   The notification is signed like a transaction, and the transaction and
   renewal info inside it are signed again; all three are verified
   (lib/apple.js), each by the verifier of the notification's environment.
   Each notification states the whole of what it knows at the moment Apple
   signed it (appleChange): the subscription's fields from its
   transaction, and the grace period from its renewal info when it carries
   one. The subscription's fields are ordered by when the period the
   transaction pays for began (or when Apple signed the notification, if
   that was earlier: a renewal Apple charges ahead of time, lib/apple.js
   periodFrom), then by when Apple signed the notification; the grace
   period by when Apple signed it, within its period (the product and
   expiry), wherever the row placed that period (appleWrite). So
   one delivered late or twice never undoes a later one: a renewal
   delivered after a refund of the period before it still lands, a refund
   is not lost behind a later grace notice, nor a grace period behind a
   later change of renewal preference. A subscription not seen yet gets
   its row from the notification.

   Apple sends a notification again when it is not answered with success,
   a 400 as much as a 500, but only five times, 1, 12, 24, 48 and 72 hours
   after the attempt before, and only in Production: a Sandbox one is sent
   once (Apple's *Responding to App Store Server Notifications*). So a
   refusal is a 400 with no detail, and a fault at this end (the database,
   a setting) a 500. Either comes back, but not after those days, so what
   was refused while a setting was wrong is lost once they pass, and each
   refusal and fault is logged at once. A notification refused as another
   app's names, in the log, the app Apple signed it for and the setting
   that differs (appleOtherApp): an APPLE_APP_ID that is not the app's
   refuses every Production notification and nothing else. Everything
   else is 200 at once: TEST, and the types RATA has no use for. Nothing
   about the purchase is logged. */
const bad = () => NextResponse.json({ error: 'Bad request' }, { status: 400 });
const fault = () => NextResponse.json({ error: 'not recorded' }, { status: 500 });
const done = (acted) => NextResponse.json({ received: true, acted });

export async function POST(req) {
  const signed = readNotification(await req.text());
  if (!signed) return bad();

  const made = appleVerifiers();
  if (made.off) {
    console.warn(`apple/notifications: ${made.off}; see rata-next/LAUNCH.md section 11`);
    return fault();
  }

  const seen = await verifyApple('notification', signed, made);
  if (!seen.ok) {
    if (seen.noAppId) {
      console.warn('apple/notifications: a Production notification arrived and APPLE_APP_ID is not set; see rata-next/LAUNCH.md section 11');
      return fault();
    }
    /* The library says another app's only once the signature has
       verified, so what the notification names is Apple's. */
    console.warn(seen.status === 'INVALID_APP_IDENTIFIER' ? appleOtherApp(signed, made.settings) : `apple/notifications: refused (${seen.status})`);
    return bad();
  }

  const n = seen.payload;
  if (!APPLE_ACTED.includes(n.notificationType)) return done(false);
  const data = n.data || {};
  if (typeof data.signedTransactionInfo !== 'string') return done(false);

  const t = await verifyAppleIn(seen.environment, 'transaction', data.signedTransactionInfo, made);
  if (!t.ok) {
    console.warn(`apple/notifications: its transaction was refused (${t.status})`);
    return bad();
  }
  let renewal = null;
  if (typeof data.signedRenewalInfo === 'string') {
    const r = await verifyAppleIn(seen.environment, 'renewal', data.signedRenewalInfo, made);
    if (!r.ok) {
      console.warn(`apple/notifications: its renewal info was refused (${r.status})`);
      return bad();
    }
    renewal = r.payload;
  }

  const change = appleChange(n, t.payload, renewal, seen.environment);
  if (!change) return done(false);

  const sb = admin();
  if (!sb) {
    console.warn('apple/notifications: SUPABASE_URL or SUPABASE_SERVICE_ROLE_KEY is not set, so nothing can be recorded');
    return fault();
  }
  const written = await appleWrite(sb, t.payload.originalTransactionId, change);
  if (written.error) {
    if (appleTableBehind(written.error)) {
      console.warn('apple/notifications: the database has no apple_subscriptions table, or one without a column section 7 now adds; run section 7 of rata-next/database.sql');
    } else {
      console.warn('apple/notifications: could not record a notification', written.error.code || written.error.message);
    }
    return fault();
  }
  return done(written.wrote);
}

/* A browser visiting this address gets an explanation, and whether it can
   take what Apple sends from the App Store itself: Apple's roots are
   readable and APPLE_APP_ID is set, without which every Production
   purchase and notification is refused. A yes or a no, never the
   settings themselves. It cannot say whether APPLE_APP_ID is the app's
   own number: only a Production notification shows that (appleOtherApp,
   LAUNCH.md section 11). */
export async function GET() {
  return NextResponse.json({
    endpoint: 'App Store Server Notifications V2',
    configured: appleTakesProduction(appleVerifiers()),
    note: 'POST only, signed by Apple. See rata-next/LAUNCH.md section 11.',
  });
}
