import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import { entitlementsForUser, planDef } from '../../../../lib/plan';
import { check, issue, renewable, LICENCE_DAYS, RENEW_GRACE_DAYS } from '../../../../lib/licence';
import { corsHeaders, preflight } from '../../../../lib/cors';

export const dynamic = 'force-dynamic';

/* Renew a licence the app already holds.

   The desktop app has no sign-in. It never did: there is no cloud account
   behind it, only a signed token, and asking somebody to type an email and
   password every month to keep reading mail that is already on their computer
   would undo the point of signing the licence in the first place.

   So the old licence is the credential. It is signed with a key only this
   server holds, so presenting one proves it was issued here — and the address
   inside it is the address the subscription is keyed on. An expired licence is
   accepted, deliberately: renewing an expired licence is the entire job. But
   only for RENEW_GRACE_DAYS after it expired, so a token that leaked long ago
   (an old backup, a pasted screenshot) cannot come back to life.

   What this is not: a way to look up somebody else's plan. The only thing that
   comes back is a licence for the address inside the licence presented, so the
   worst an attacker with a stolen token can do is obtain a copy of a token they
   already have.

   The signature is checked before anything touches the database, so an
   unsigned guess costs one Ed25519 verification and no query. */
/* The app calls this from its own origin, so the answer has to say the app
   may read it (lib/cors.js) — without that, renewal failed as "offline". */
export function OPTIONS(req) {
  return preflight(req);
}

export async function POST(req) {
  const res = await renew(req);
  for (const [k, v] of Object.entries(corsHeaders(req))) res.headers.set(k, v);
  return res;
}

async function renew(req) {
  let body;
  try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }

  /* White space is never part of a licence (base64url and dots), and a key
     that went through a mail client arrives wrapped, with line breaks inside
     it. Taken out, so a wrapped key renews like the key it is. */
  const presented = String(body.licence || '').replace(/\s+/g, '');
  if (!presented) return NextResponse.json({ error: 'No licence to renew.' });

  /* `check` refuses a forged token and reports an expired one separately, with
     the payload still readable — which is what makes renewing one possible. */
  const seen = check(presented, process.env.LICENCE_PUBLIC_KEY);
  const licence = seen.licence;
  if (!licence && !seen.ok) {
    /* A licence this server cannot read is not renewable by any means, and
       saying which of the two it is helps a support conversation. The app
       keeps the licence it has whatever this says (bridge.js renew()) and
       shows the sentence, so each one is for the customer: a deployment
       with no public key is our fault, named in the server log only. */
    if (seen.reason === 'no-public-key') {
      console.warn('licence/renew: LICENCE_PUBLIC_KEY is not set or cannot be read, so no licence can be renewed; see rata-next/LAUNCH.md section 2');
    }
    return NextResponse.json({
      licensed: false,
      reason: seen.reason,
      message: seen.reason === 'no-public-key'
        ? 'mailrata.org cannot check licences at the moment, so RATA keeps the licence it has and tries again later.'
        : seen.reason === 'bad-signature'
          ? 'mailrata.org did not recognise the signature on this licence. Sign in at mailrata.org to get a new one.'
          : 'That licence could not be read. Sign in at mailrata.org to get a new one.',
    }, seen.reason === 'no-public-key' ? { status: 500 } : undefined);
  }

  if (!renewable(seen)) {
    /* Genuine, but expired too long ago to renew itself (lib/licence.js):
       asked before the database, like a forgery. The customer still gets a
       new one by signing in. */
    return NextResponse.json({
      licensed: false,
      reason: 'too-old',
      message: `This licence expired more than ${RENEW_GRACE_DAYS} days ago, so it cannot renew itself. Sign in at mailrata.org to get a new one.`,
    });
  }

  const email = String(licence.sub || '').toLowerCase();
  const sb = admin();
  if (!sb) {
    return NextResponse.json({ error: 'The licence service is not configured.' }, { status: 500 });
  }

  const { plan, pending } = await entitlementsForUser(sb, email);
  if (!plan && pending) {
    /* A purchase still being set up (settingUp in lib/plan.js): the checkout
       reached us and the subscription has not named its plan yet, or a bank
       transfer has not cleared. /api/licence answers `pending` for the same
       row, and so does this: it is not "no subscription", and the app keeps
       the licence it holds and asks again later rather than showing a price
       to somebody who has just paid. */
    return NextResponse.json({
      licensed: false,
      reason: 'pending',
      email,
      message: 'Your licence is being set up: your purchase has reached us and is not confirmed yet. If you paid by bank transfer, that happens once the transfer clears. RATA keeps the licence it has and tries again later.',
    });
  }
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
