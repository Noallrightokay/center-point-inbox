import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';
import { fetchInbox, isMailKey, mailKey, describe, candidateHosts } from '../../../../lib/mail';
import { decryptSecret } from '../../../../lib/secrets';

export const dynamic = 'force-dynamic';

/* Pull every linked mailbox into one stream.

   The combined inbox is the product, so the default is all of them, merged and
   newest first. ?email= narrows it to one account for a per-inbox refresh.

   One mailbox failing must not lose the others: each account reports its own
   error alongside whatever did arrive. */

/* How many mailboxes are opened at once.

   Not all of them. A Pro account has no mailbox limit, so "all of them" is a
   number the user chooses — and thirty simultaneous IMAP sessions is a burst
   that ties up thirty sockets here, looks like abuse from the far end, and
   makes the slowest of the thirty decide how long everyone waits. Four at a
   time keeps a large account's refresh a few seconds longer than a small one's
   instead of a different kind of event. */
const AT_ONCE = 4;

/* And a ceiling on any single one. imapflow's own timeouts cover a socket that
   stalls, but a server that answers every step slowly can still pass all of
   them and hold the whole response open. */
const PER_MAILBOX_MS = 25000;

/* A ceiling on the whole refresh, which is the one that actually matters.

   Four at a time bounds how many sockets are open, not how long the request
   runs: twelve mailboxes is three waves, and three waves of the per-mailbox
   ceiling is seventy-five seconds in a single HTTP request. Gateways do not
   wait that long — the customer gets a network error rather than their mail,
   and because unlimited mailboxes is what Pro sells, it is the customers
   paying most who hit it first.

   So the refresh returns what it has when the budget runs out and names what
   it did not reach, which is a true answer. Under thirty seconds is comfortably
   inside every proxy timeout worth worrying about. */
const TOTAL_MS = 28000;

/* Not enough time left to be worth opening a connection that will only be cut
   off part way through. */
const WORTH_TRYING_MS = 3000;

async function mapLimit(items, n, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i]);
    }
  }));
  return out;
}

function withDeadline(promise, ms, onTimeout) {
  let timer;
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise(resolve => { timer = setTimeout(() => resolve(onTimeout()), ms); }),
  ]);
}

export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const only = String(new URL(req.url).searchParams.get('email') || '').trim().toLowerCase();

  let q = sb.from('provider_tokens').select('provider,label,access,extra').eq('user_id', user.id);
  if (only) q = q.eq('provider', mailKey(only));
  const { data: rows } = await q;

  const mailboxes = (rows || []).filter(r => isMailKey(r.provider));
  if (!mailboxes.length) {
    return NextResponse.json({ error: only ? 'That mailbox is not linked' : 'No mailbox linked yet' });
  }

  const deadline = Date.now() + TOTAL_MS;

  /* Where in the list to start, rotating each minute.

     Without this the same mailboxes are always attempted first, so the ones at
     the end of a long list would never be reached on an account whose servers
     are slow — they would be permanently the ones that ran out of budget.
     Rotating means every refresh covers a different part of the list and
     everything lands within a few. Stable within the minute, so a user
     hammering refresh gets consistent answers rather than a shuffle. */
  const from = mailboxes.length ? Math.floor(Date.now() / 60000) % mailboxes.length : 0;
  const ordered = [...mailboxes.slice(from), ...mailboxes.slice(0, from)];

  const results = await mapLimit(ordered, AT_ONCE, async r => {
    const host = r.extra?.host || candidateHosts(r.label)[0];
    const port = r.extra?.port || undefined;
    const label = r.extra?.provider_label || describe(r.label)?.label || host;
    const base = { email: r.label, label, provider: r.provider };

    /* A mailbox whose password was rejected last time is not tried again.
       Retrying a revoked app password on every refresh does not recover the
       mailbox — it only walks the account towards the provider's own lockout,
       and RATA would be the one doing the walking. Relinking clears this. */
    if (r.extra?.auth_failed_at) {
      return { ...base, needsRelink: true,
        error: `${r.label} needs its app password again — RATA stopped trying so your provider does not lock the account. Relink it in Accounts.` };
    }

    const pass = decryptSecret(r.access, user.id, r.provider);
    /* An unreadable credential is a key problem, not a mail problem, and
       saying "sync failed" would send the user looking in the wrong place. */
    if (!pass) return { ...base, needsRelink: true, error: `${r.label} could not be unlocked — relink it in Accounts.` };

    /* Checked here rather than before the loop: by the time a worker reaches
       this mailbox, earlier ones may have spent the budget. */
    const left = deadline - Date.now();
    if (left < WORTH_TRYING_MS) {
      return { ...base, deferred: true,
        error: `${r.label} was not reached in this refresh — the next one starts with it.` };
    }

    const out = await withDeadline(
      fetchInbox({ host, port, email: r.label, pass, label }),
      Math.min(PER_MAILBOX_MS, left),
      () => ({ kind: 'net', error: `${r.label} took too long to answer and was left out of this refresh.` }),
    );
    return { ...base, ...out };
  });

  /* Remember the rejections, so the skip above has something to read. Only a
     refused sign-in is recorded: a server that was unreachable is a network
     that will probably be back, and marking that would strand a working
     mailbox behind a relink it does not need. */
  const refused = results.filter(r => r.kind === 'auth');
  if (refused.length) {
    const at = new Date().toISOString();
    await Promise.all(refused.map(async r => {
      const row = mailboxes.find(m => m.provider === r.provider);
      await sb.from('provider_tokens')
        .update({ extra: { ...(row?.extra || {}), auth_failed_at: at } })
        .eq('user_id', user.id).eq('provider', r.provider);
    })).catch(() => { /* the sync still reports it; the note is an optimisation */ });
  }

  const messages = results.flatMap(r => r.messages || []).sort((a, b) => b.ts - a.ts);

  /* A mailbox left for the next refresh is not a failure and must not be
     reported as one — nothing is wrong with it, and telling somebody to check
     an app password that is working would be a lie that costs a support
     email. */
  const deferred = results.filter(r => r.deferred).map(r => r.email);
  const failures = results.filter(r => r.error && !r.deferred)
    .map(r => ({ email: r.email, error: r.error, needsRelink: !!(r.needsRelink || r.kind === 'auth') }));

  return NextResponse.json({
    messages,
    accounts: results.map(r => ({
      email: r.email, label: r.label,
      count: (r.messages || []).length,
      error: r.error || null,
      needsRelink: !!(r.needsRelink || r.kind === 'auth'),
      deferred: !!r.deferred,
    })),
    ...(deferred.length ? { deferred } : {}),
    ...(failures.length ? { partial: failures } : {}),
    /* Only a refresh where everything genuinely failed is an error. One that
       ran out of time still delivered mail. */
    ...(failures.length && failures.length === results.length ? { error: failures[0].error } : {}),
  });
}
