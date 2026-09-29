import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import {
  verifySignature, rowForEvent, isNewer, checkoutConflict, checkoutWrite, LIVE_STATUSES, PENDING,
  orderGuard, subscriptionPatch, pendingBelongs, tableMissing,
} from '../../../../lib/stripe';

export const dynamic = 'force-dynamic';

/* Stripe tells the server who has paid.

   This is the only thing standing between "money arrived" and "the app knows
   it": the subscriptions table it writes is what both the server and the
   client read a plan from, so nothing else in RATA needs to know Stripe
   exists.

   Two rules shape the replies. Stripe retries anything it does not get a 2xx
   for, whatever the status — in live mode for up to three days with
   exponential back-off, in a sandbox three times over a few hours — so an
   event we do not act on still gets a 200 ("understood, nothing to do")
   rather than an error that brings it back again and again. And a failure on
   our side gets a 500 on purpose, because that retry is exactly what we
   want. */
export async function POST(req) {
  /* The raw body, before any parsing: the signature covers the exact bytes
     Stripe sent, so re-serialising parsed JSON would never match. */
  const raw = await req.text();

  const sig = verifySignature(raw, req.headers.get('stripe-signature'), process.env.STRIPE_WEBHOOK_SECRET);
  if (!sig.ok) {
    /* 400: the request is refused, and nothing is written. A 4xx does not
       stop Stripe retrying; it retries this like any other failure, and signs
       each retry afresh. That is harmless for a forgery, which never came from
       Stripe, and useful for a real event refused because
       STRIPE_WEBHOOK_SECRET was wrong or unset: once it is corrected, the
       next retry within the three days verifies and lands. */
    return NextResponse.json({ error: sig.error }, { status: 400 });
  }

  let event;
  try { event = JSON.parse(raw); } catch { return NextResponse.json({ error: 'body was not JSON' }, { status: 400 }); }

  const row = rowForEvent(event);
  if (!row) return NextResponse.json({ received: true, acted: false, type: event.type });

  const sb = admin();
  if (!sb) return NextResponse.json({ error: 'backend not configured' }, { status: 500 });

  const now = new Date().toISOString();

  try {
    if (row.by === 'email') {
      /* Read before writing, so a checkout event that took a long way round
         cannot land on top of a subscription change that already happened. */
      const { data: held, error: readErr } = await sb.from('subscriptions')
        .select('event_at,stripe_customer,status').eq('email', row.email).maybeSingle();
      if (readErr) throw new Error(readErr.message);
      if (held && !isNewer(row.event_at, held.event_at)) {
        return NextResponse.json({ received: true, acted: false, stale: true, type: event.type });
      }
      if (checkoutConflict(held, row)) {
        /* A second Stripe customer paying for an address that already has a
           live subscription under another: never replace the first (see
           checkoutConflict). 200 so Stripe stops retrying; the owner
           reconciles by hand in the Stripe dashboard, refunding the second
           payment. The log line names neither the address nor a customer. */
        console.warn('stripe: checkout conflict: an address with a live subscription under another customer; row left alone');
        return NextResponse.json({ received: true, acted: false, conflict: true, type: event.type });
      }
      /* A checkout event does not know the plan (lib/stripe.js): the
         subscription events name it, and checkoutWrite leaves out any
         column this event cannot vouch for, so the upsert keeps what the
         row holds there. Until a subscription event has named the plan, the
         row is PENDING even when paid. */
      const write = checkoutWrite(held, row, now);
      if (write.status === PENDING && held && LIVE_STATUSES.includes(String(held.status || '').toLowerCase())) {
        /* A checkout still waiting for its money (a bank transfer), or for
           its subscription to name the plan, never takes away a
           subscription that is live now: the row stays as it is until the
           subscription events say otherwise. */
        return NextResponse.json({ received: true, acted: false, pending: true, type: event.type });
      }

      const { error } = await sb.from('subscriptions').upsert(write);
      if (error) throw new Error(error.message);

      /* A subscription event of this purchase that was delivered before this
         checkout event found no row with its customer and was kept in
         pending_subscriptions (below). Apply it now, in this request, through
         the same guarded update a subscription event uses, so the plan lands
         with the checkout rather than on Stripe's retry hours later. Read
         after the upsert, never before: a subscription event racing this one
         writes its pending row and then retries its own update, so whichever
         of the two goes second sees the other's write. */
      const applied = await applyPending(sb, row, now);
      if (applied) {
        return NextResponse.json({ received: true, acted: true, email: row.email, plan: applied.plan || write.plan || null,
          status: applied.status, pending_applied: true });
      }
      return NextResponse.json({ received: true, acted: true, email: row.email, plan: write.plan || null, status: write.status });
    }

    /* Keyed by customer. The row is created by the checkout event, which
       Stripe usually raises after this one and delivers in no promised order,
       so there is often no row yet. */
    const patch = subscriptionPatch(row, now);
    const guard = orderGuard(event.type, row.event_at);
    const matched = await updateByCustomer(sb, 'subscriptions', row.stripe_customer, patch, guard);
    if (matched) return NextResponse.json({ received: true, acted: true, customers: matched, status: row.status });

    /* Either no row for this customer yet, or the stored one is newer. */
    const { data: exists, error: existsErr } = await sb.from('subscriptions')
      .select('email').eq('stripe_customer', row.stripe_customer).limit(1);
    if (existsErr) throw new Error(existsErr.message);
    if (exists && exists.length) {
      return NextResponse.json({ received: true, acted: false, stale: true, type: event.type });
    }

    /* No row: keep the event against its customer until the checkout event
       writes the row, which applies it in the same request (applyPending).
       The same ordering guard holds between pending events, so a `created`
       delivered after its `updated` does not replace it. */
    const kept = await keepPending(sb, row.stripe_customer, patch, guard);
    if (kept === 'missing') {
      /* The one case left for Stripe's retry: pending_subscriptions does not
         exist because database.sql section 6 has not been run on this
         database. 409 (not 200) so Stripe delivers it again, by which time
         the checkout event has usually made the row. Logged without the
         customer. */
      console.warn('stripe: pending_subscriptions is missing (run database.sql section 6); subscription event left to Stripe\'s retry');
      return NextResponse.json(
        { error: 'no subscription row for that customer yet, and nowhere to keep the event', customer: row.stripe_customer },
        { status: 409 });
    }

    /* The checkout may have written its row between the update above and the
       pending write, and read pending_subscriptions before this event was in
       it. Trying the update once more closes that gap. */
    const late = await updateByCustomer(sb, 'subscriptions', row.stripe_customer, patch, guard);
    if (late) {
      await dropPending(sb, row.stripe_customer);
      return NextResponse.json({ received: true, acted: true, customers: late, status: row.status });
    }
    return NextResponse.json({ received: true, acted: true, pending: true, kept, status: row.status, type: event.type });
  } catch (e) {
    /* 500 so Stripe retries: the payment happened, and the record has to catch
       up rather than being quietly lost. */
    return NextResponse.json({ error: 'could not record the subscription — ' + e.message }, { status: 500 });
  }
}

/* Update every row of `table` with this customer under the ordering guard,
   in the database so two deliveries racing each other cannot both pass a
   check and then both write. A row never stamped predates the guard and is
   always updated. Returns how many rows it wrote. */
async function updateByCustomer(sb, table, customer, patch, guard) {
  let q = sb.from(table).update(patch).eq('stripe_customer', customer);
  if (guard) q = q.or(guard);
  const { data, error } = await q.select('stripe_customer');
  if (error) throw new Error(error.message);
  return data ? data.length : 0;
}

/* Keep a subscription event that found no row: insert it if the customer
   has no pending row, otherwise update that row under the ordering guard.
   Returns 'new', 'updated', 'stale' (a newer event is already kept) or
   'missing' (the table does not exist). */
async function keepPending(sb, customer, patch, guard) {
  const { data: inserted, error } = await sb.from('pending_subscriptions')
    .upsert({ stripe_customer: customer, ...patch }, { onConflict: 'stripe_customer', ignoreDuplicates: true })
    .select('stripe_customer');
  if (error) {
    if (tableMissing(error)) return 'missing';
    throw new Error(error.message);
  }
  if (inserted && inserted.length) return 'new';
  const n = await updateByCustomer(sb, 'pending_subscriptions', customer, patch, guard);
  return n ? 'updated' : 'stale';
}

/* After a checkout wrote its row: apply the pending subscription event of
   this purchase, if one is kept, as if it had arrived now. Returns what it
   applied, or null. A missing table means there is nothing kept. */
async function applyPending(sb, row, now) {
  if (!row.stripe_customer) return null;
  const { data: held, error } = await sb.from('pending_subscriptions')
    .select('plan,domain_addons,status,event_at').eq('stripe_customer', row.stripe_customer).maybeSingle();
  if (error) {
    if (tableMissing(error)) return null;
    throw new Error(error.message);
  }
  if (!held) return null;
  /* One from an earlier subscription of the same customer says nothing
     about this purchase; it is dropped rather than applied. */
  if (!pendingBelongs(held, row)) { await dropPending(sb, row.stripe_customer); return null; }
  const patch = subscriptionPatch({
    status: held.status, plan: held.plan, event_at: held.event_at,
    domain_addons: typeof held.domain_addons === 'number' ? held.domain_addons : null,
  }, now);
  const n = await updateByCustomer(sb, 'subscriptions', row.stripe_customer, patch, orderGuard('customer.subscription.updated', held.event_at));
  await dropPending(sb, row.stripe_customer);
  return n ? { plan: held.plan, status: held.status } : null;
}

/* Best effort: a pending row left behind is harmless, since pendingBelongs
   refuses it for any later checkout. */
async function dropPending(sb, customer) {
  try {
    const { error } = await sb.from('pending_subscriptions').delete().eq('stripe_customer', customer);
    if (error && !tableMissing(error)) console.warn('stripe: could not clear a pending subscription event; it is ignored from now on');
  } catch { /* the same */ }
}

/* A browser visiting this URL should get an explanation, not a stack trace. */
export async function GET() {
  return NextResponse.json({
    endpoint: 'Stripe webhook',
    configured: !!process.env.STRIPE_WEBHOOK_SECRET,
    entitled: LIVE_STATUSES,
    note: 'POST only, signed by Stripe. See STRIPE-SETUP.md.',
  });
}
