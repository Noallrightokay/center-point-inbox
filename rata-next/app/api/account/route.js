import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../lib/server';
import { blocksDeletion, DELETION_REMOVES, DELETION_KEEPS } from '../../../lib/account';

export const dynamic = 'force-dynamic';

/* Deleting an account, and meaning it.

   Everything RATA holds for a person comes out: the preferences, the
   subscription record, what the AI relay charged them, and the login itself.
   Only workspaces cascades from auth.users (database.sql §1); subscriptions
   and ai_usage are keyed by email with no link to the login, so without these
   deletes they would outlive it. Each is deleted explicitly so the reply can
   say what went.

   There is much less to delete than there was, and that is the point of the
   move to the device: the mailbox passwords RATA used to hold are in the
   customer's own keychain now, so this route cannot reach them and no longer
   needs to.

   Three things this cannot reach, and says so rather than implying otherwise:
   the mail, which never left the customer's provider; anything the app holds
   on their own machine; and Stripe's billing records, which Stripe is required
   to keep. */

/* PostgREST's answer for a table that is not there: PGRST205 ("Could not find
   the table … in the schema cache") from current versions, and Postgres's own
   42P01 (undefined_table) from older ones. Anything else is a real failure. */
function tableMissing(e) {
  return e?.code === 'PGRST205' || e?.code === '42P01';
}

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
  const wipe = async (table, column, value, { mayBeMissing = false } = {}) => {
    const { data, error: e } = await sb.from(table).delete().eq(column, value).select(column);
    if (e && mayBeMissing && tableMissing(e)) { removed[table] = 0; return; }
    if (e) throw new Error(`${table}: ${e.message}`);
    removed[table] = data ? data.length : 0;
  };

  try {
    await wipe('workspaces', 'id', user.id);
    await wipe('subscriptions', 'email', email);
    /* What the AI relay charged this address, month by month. The table
       exists only once database.sql §5 has been run; on a deploy without it
       there is nothing to delete, which is no reason to leave the login
       behind, and the reply says 0 rather than passing on the database's
       message. */
    await wipe('ai_usage', 'email', email, { mayBeMissing: true });

    const { error: e } = await sb.auth.admin.deleteUser(user.id);
    if (e) throw new Error('login: ' + e.message);

    return NextResponse.json({
      ok: true,
      deleted: { ...removed, login: 1 },
      /* Said plainly, because a deletion confirmation that overstates itself
         is worse than none. */
      note: 'Your mail was never stored by RATA and is untouched at your provider. Anything the app holds is on your own computer — uninstall it to remove that. Stripe keeps its billing records, which it is required to do.',
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
  return NextResponse.json({
    email,
    subscription: sub ? { plan: sub.plan, status: sub.status } : null,
    blocked: blocksDeletion(sub),
    removes: DELETION_REMOVES,
    keeps: DELETION_KEEPS,
  });
}
