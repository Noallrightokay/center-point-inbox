import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import { entitlementsForUser, planDef } from '../../../../lib/plan';
import { check, issue, LICENCE_DAYS } from '../../../../lib/licence';

export const dynamic = 'force-dynamic';

/* Renew a licence the app already holds.

   The desktop app has no sign-in. It never did: there is no cloud account
   behind it, only a signed token, and asking somebody to type an email and
   password every month to keep reading mail that is already on their computer
   would undo the point of signing the licence in the first place.

   So the old licence is the credential. It is signed with a key only this
   server holds, so presenting one proves it was issued here — and the address
   inside it is the address the subscription is keyed on. An expired licence is
   accepted, deliberately: renewing an expired licence is the entire job.

   What this is not: a way to look up somebody else's plan. The only thing that
   comes back is a licence for the address inside the licence presented, so the
   worst an attacker with a stolen token can do is obtain a copy of a token they
   already have.

   The signature is checked before anything touches the database, so an
   unsigned guess costs one Ed25519 verification and no query. */
export async function POST(req) {
  let body;
  try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }

  const presented = String(body.licence || '').trim();
  if (!presented) return NextResponse.json({ error: 'No licence to renew.' });

  /* `check` refuses a forged token and reports an expired one separately, with
     the payload still readable — which is what makes renewing one possible. */
  const seen = check(presented, process.env.LICENCE_PUBLIC_KEY);
  const licence = seen.licence;
  if (!licence && !seen.ok) {
    /* A licence this server cannot read is not renewable by any means, and
       saying which of the two it is helps a support conversation. */
    return NextResponse.json({
      licensed: false,
      reason: seen.reason,
      message: seen.reason === 'no-public-key'
        ? 'This deployment cannot check licences — LICENCE_PUBLIC_KEY is not set. See LICENSING.md.'
        : 'That licence could not be read. Sign in at mailrata.org to get a new one.',
    }, seen.reason === 'no-public-key' ? { status: 500 } : undefined);
  }

  const email = String(licence.sub || '').toLowerCase();
  const sb = admin();
  if (!sb) {
    return NextResponse.json({ error: 'The licence service is not configured.' }, { status: 500 });
  }

  const { plan } = await entitlementsForUser(sb, email);
  if (!plan) {
    /* Cancelled, refunded, or a failed payment that has stopped retrying. Not
       an error and not an accusation — the app shows the price. */
    return NextResponse.json({
      licensed: false,
      reason: 'no-subscription',
      email,
      message: 'There is no active subscription on this account. Choose a plan at mailrata.org to keep using RATA.',
    });
  }

  let renewed;
  try {
    renewed = issue({ email, plan });
  } catch (e) {
    /* The signing key is missing. A deployment fault, not the customer's, and
       it must not read as "you have not paid". */
    return NextResponse.json({ error: e.message }, { status: 500 });
  }

  const def = planDef(plan);
  return NextResponse.json({
    licensed: true,
    licence: renewed,
    plan,
    planLabel: def.label,
    email,
    renewWithinDays: LICENCE_DAYS,
    message: `Renewed for ${def.label}. Good for another ${LICENCE_DAYS} days offline.`,
  });
}
