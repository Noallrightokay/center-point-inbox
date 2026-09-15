import { NextResponse } from 'next/server';
import { admin } from '../../../../lib/server';
import { verifySignature, rowForEvent, LIVE_STATUSES } from '../../../../lib/stripe';

export const dynamic = 'force-dynamic';

/* Stripe tells the server who has paid.

   This is the only thing standing between "money arrived" and "the app knows
   it": the subscriptions table it writes is what both the server and the
   client read a plan from, so nothing else in RATA needs to know Stripe
   exists.

   Two rules shape the replies. Anything Stripe does not get a 200 for is
   retried, so an event we do not act on still gets a 200 — "understood,
   nothing to do" — rather than an error that brings it back every few minutes
   for days. And a failure on our side gets a 500 on purpose, because that
   retry is exactly what we want. */
export async function POST(req) {
  /* The raw body, before any parsing: the signature covers the exact bytes
     Stripe sent, so re-serialising parsed JSON would never match. */
  const raw = await req.text();

  const sig = verifySignature(raw, req.headers.get('stripe-signature'), process.env.STRIPE_WEBHOOK_SECRET);
  if (!sig.ok) {
    /* 400, not 500: a bad signature is not something a retry will fix, and
       Stripe stops rather than hammering an endpoint that rejects it. */
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
      const { error } = await sb.from('subscriptions').upsert({
        email: row.email,
        plan: row.plan,
        status: row.status,
        stripe_customer: row.stripe_customer,
        domain_addons: row.domain_addons || 0,
        updated_at: now,
      });
      if (error) throw new Error(error.message);
      return NextResponse.json({ received: true, acted: true, email: row.email, plan: row.plan });
    }

    /* Keyed by customer. The row was created by the checkout event, so if
       there is none yet this is an event arriving out of order — Stripe's
       retries will bring it round again once checkout has landed. */
    const patch = { status: row.status, updated_at: now };
    if (row.plan) patch.plan = row.plan;
    /* Written on every subscription event rather than only when non-zero: a
       customer who removes their extra domain sends a quantity of nothing, and
       skipping the zero would leave them entitled to a domain they stopped
       paying for. */
    if (typeof row.domain_addons === 'number') patch.domain_addons = row.domain_addons;

    const { data, error } = await sb.from('subscriptions')
      .update(patch).eq('stripe_customer', row.stripe_customer).select('email');
    if (error) throw new Error(error.message);

    if (!data || !data.length) {
      return NextResponse.json(
        { error: 'no subscription row for that customer yet', customer: row.stripe_customer },
        { status: 409 });
    }
    return NextResponse.json({ received: true, acted: true, customers: data.length, status: row.status });
  } catch (e) {
    /* 500 so Stripe retries: the payment happened, and the record has to catch
       up rather than being quietly lost. */
    return NextResponse.json({ error: 'could not record the subscription — ' + e.message }, { status: 500 });
  }
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
