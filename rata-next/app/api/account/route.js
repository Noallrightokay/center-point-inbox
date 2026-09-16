import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../lib/server';
import { blocksDeletion, DELETION_REMOVES, DELETION_KEEPS } from '../../../lib/account';

export const dynamic = 'force-dynamic';

/* Deleting an account, and meaning it.

   Everything RATA holds for a person comes out: the encrypted mailbox
   credentials, the preferences, any half-finished link, the subscription
   record, and the login itself. Three of those tables cascade from auth.users
   anyway, but they are deleted explicitly so the reply can say what went and
   so this keeps working if the schema's cascades ever change.

   Two things this cannot reach, and says so rather than implying otherwise:
   the mail, which never left the customer's provider, and Stripe's billing
   records, which Stripe is required to keep. */

export async function DELETE(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const email = (user.email || '').toLowerCase();

  /* Typing the address is the confirmation. A dialog nobody reads is not one,
     and this is the one action in RATA with nothing behind it. */
  let body = {};
  try { body = await req.json(); } catch { /* no body is a failed confirmation */ }
  if (String(body.confirm || '').trim().toLowerCase() !== email) {
    return NextResponse.json({ error: `Type ${email} to confirm.` }, { status: 400 });
  }

  const { data: sub } = await sb.from('subscriptions')
    .select('plan,status').eq('email', email).maybeSingle();
  const blocked = blocksDeletion(sub);
  if (blocked) return NextResponse.json({ error: blocked, subscription: sub?.status }, { status: 409 });

  const removed = {};
  const wipe = async (table, column, value) => {
    const { data, error: e } = await sb.from(table).delete().eq(column, value).select(column);
    if (e) throw new Error(`${table}: ${e.message}`);
    removed[table] = data ? data.length : 0;
  };

  try {
    /* Credentials first. If anything later fails, the thing worth losing
       sleep over is already gone. */
    await wipe('provider_tokens', 'user_id', user.id);
    await wipe('link_states', 'user_id', user.id);
    await wipe('workspaces', 'id', user.id);
    await wipe('subscriptions', 'email', email);

    const { error: e } = await sb.auth.admin.deleteUser(user.id);
    if (e) throw new Error('login: ' + e.message);

    return NextResponse.json({
      ok: true,
      deleted: { ...removed, login: 1 },
      /* Said plainly, because a deletion confirmation that overstates itself
         is worse than none. */
      note: 'Your mail was never stored by RATA and is untouched at your provider. Stripe keeps its billing records, which it is required to do.',
    });
  } catch (e) {
    return NextResponse.json({
      error: 'Could not finish deleting the account — ' + e.message,
      deletedSoFar: removed,
    }, { status: 500 });
  }
}

/* What deletion would remove, so the warning in the app is generated from the
   thing that does the work rather than written twice. */
export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });
  const email = (user.email || '').toLowerCase();

  const { data: sub } = await sb.from('subscriptions').select('plan,status').eq('email', email).maybeSingle();
  const { data: links } = await sb.from('provider_tokens').select('provider').eq('user_id', user.id);

  return NextResponse.json({
    email,
    mailboxes: (links || []).length,
    subscription: sub ? { plan: sub.plan, status: sub.status } : null,
    blocked: blocksDeletion(sub),
    removes: DELETION_REMOVES,
    keeps: DELETION_KEEPS,
  });
}
