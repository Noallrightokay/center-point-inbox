import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import {
  APPLE_ACTED, appleVerifiers, verifyApple, verifyAppleIn, readNotification, appleChange, appleWrite, tableMissing,
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
   What each type changes is appleChange's; every write is ordered by when
   Apple signed the notification, so one delivered late never undoes a later
   one. A subscription not seen yet gets its row from the notification.

   Apple retries anything not answered 200, for days. So a refusal of the
   signature is a 400 that says nothing more, and a fault at this end (the
   database, a setting) is a 500 on purpose, so the notification comes back
   once it is fixed. Everything else is 200 at once: TEST, and the types
   RATA has no use for. Nothing about the purchase is logged. */
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
    console.warn(`apple/notifications: refused (${seen.status})`);
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
    if (tableMissing(written.error)) {
      console.warn('apple/notifications: the database has no apple_subscriptions table; run section 7 of rata-next/database.sql');
    } else {
      console.warn('apple/notifications: could not record a notification', written.error.code || written.error.message);
    }
    return fault();
  }
  return done(written.wrote);
}

/* A browser visiting this address gets an explanation. */
export async function GET() {
  return NextResponse.json({
    endpoint: 'App Store Server Notifications V2',
    configured: !appleVerifiers().off,
    note: 'POST only, signed by Apple. See rata-next/LAUNCH.md section 11.',
  });
}
