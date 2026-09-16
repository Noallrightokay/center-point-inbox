import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../lib/server';
import { entitlementsForUser, planDef } from '../../../lib/plan';
import { issue, LICENCE_DAYS } from '../../../lib/licence';

export const dynamic = 'force-dynamic';

/* Hand the desktop app a licence.

   This is the whole of the server's involvement in whether RATA runs. The app
   asks once, gets a signed token, and then checks it locally for the next
   month — so a customer's mail does not stop working because this endpoint is
   having a bad morning, and RATA never sees a mailbox, a password or a message.

   Issued against the signed-in account, never against an address in the
   request body: the subscription is keyed on email, so accepting one from the
   caller would hand a licence to anyone who could guess a customer's address. */
export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const email = (user.email || '').toLowerCase();
  const { plan } = await entitlementsForUser(sb, email);

  if (!plan) {
    /* Not an error — this is the ordinary state of somebody who has signed up
       and not yet paid, and the app shows them the price rather than a fault. */
    return NextResponse.json({
      licensed: false,
      reason: 'no-subscription',
      message: 'No active subscription on this account. Choose a plan to use RATA on this device.',
    });
  }

  let licence;
  try {
    licence = issue({ email, plan });
  } catch (e) {
    /* The signing key is missing or unreadable. That is a deployment fault,
       not the customer's, and a 500 says so — it must not read as "you have
       not paid". */
    return NextResponse.json({ error: e.message }, { status: 500 });
  }

  const def = planDef(plan);
  return NextResponse.json({
    licensed: true,
    licence,
    /* Repeated in the clear so the app can show it without unpacking the
       token, and so a support conversation can compare the two. */
    plan,
    planLabel: def.label,
    email,
    renewWithinDays: LICENCE_DAYS,
    message: `Licensed for ${def.label}. RATA will keep working offline for ${LICENCE_DAYS} days and renew itself whenever it is online.`,
  });
}
