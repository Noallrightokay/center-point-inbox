# RATA × Stripe — Payments Setup

Base plan: **$8/month** or **$79/year** (two months free). Future paid tiers
(Automations, CRM) are shown as "coming soon" on the pricing page.

## 1. Create the product (Stripe Dashboard, 10 min)
1. dashboard.stripe.com → **Product catalog → Add product**
   - Name: `RATA Base`
   - Price 1: **$8.00 USD, Recurring, Monthly**
   - Price 2: **$79.00 USD, Recurring, Yearly**
2. **Payment Links → New** (one per price):
   - Select RATA Base → the monthly price → Create link
   - Repeat for the yearly price
3. On each link → **After payment** → *Don't show confirmation page* →
   redirect to: `https://yourdomain.com/app.html?checkout=success`
4. Copy both `https://buy.stripe.com/…` URLs.

## 2. Wire the links into the site (2 min)
- **Landing page:** edit `index.html` in Hostinger File Manager → find
  `STRIPE_LINKS` near the bottom → paste the two URLs. The pricing buttons go live.
- **Console:** Settings → Deployment → paste the same two links (monthly/annual).
  The Plan & billing section lights up with checkout buttons.

## 3. Customer portal (2 min)
Stripe Dashboard → **Settings → Billing → Customer portal** → Activate.
Copy the portal login link (`https://billing.stripe.com/p/login/…`) → paste into
Settings → Deployment → *Stripe customer portal link*. Customers can now update
cards, switch monthly↔annual, download invoices, and cancel themselves.

## 4. What happens on purchase (as shipped)
Buyer pays on Stripe → Stripe redirects to `app.html?checkout=success` → the
console marks the account **RATA Base**, logs it to the audit chain, and shows the
plan in Settings. Money lands in your Stripe balance; Stripe emails receipts and
handles renewals, failed cards, and cancellation automatically.

⚠ Honest note: that redirect flag is *convenience*, not proof of payment — a
static site can't verify it. For real, per-user verified entitlements, do step 5.

## 5. Real entitlements — Stripe webhook → Supabase (30 min, optional but right)
Requires the Supabase project from DEPLOY.md.

**a. Table** (SQL editor):
```sql
create table subscriptions (
  email text primary key,
  plan text default 'base',
  status text,
  stripe_customer text,
  updated_at timestamptz default now()
);
alter table subscriptions enable row level security;
create policy "read own sub" on subscriptions
  for select using (lower(auth.jwt()->>'email') = email);
```

**b. Edge Function** — `supabase/functions/stripe-webhook/index.ts`:
```ts
import Stripe from "npm:stripe@14";
import { createClient } from "npm:@supabase/supabase-js@2";

const stripe = new Stripe(Deno.env.get("STRIPE_SECRET_KEY")!);

Deno.serve(async (req) => {
  const sig = req.headers.get("stripe-signature")!;
  const body = await req.text();
  let ev: Stripe.Event;
  try {
    ev = await stripe.webhooks.constructEventAsync(
      body, sig, Deno.env.get("STRIPE_WEBHOOK_SECRET")!
    );
  } catch { return new Response("bad signature", { status: 400 }); }

  const sb = createClient(
    Deno.env.get("SUPABASE_URL")!,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!
  );

  const t = ev.type;
  if (t === "checkout.session.completed" ||
      t === "customer.subscription.updated" ||
      t === "customer.subscription.deleted") {
    const o = ev.data.object as any;
    let email = (o.customer_details?.email || o.customer_email || "").toLowerCase();
    if (!email && o.customer) {
      const c = await stripe.customers.retrieve(o.customer as string);
      email = ((c as any).email || "").toLowerCase();
    }
    if (email) {
      const status = t === "customer.subscription.deleted"
        ? "canceled" : (o.status || "active");
      await sb.from("subscriptions").upsert({
        email, plan: "base", status,
        stripe_customer: o.customer ?? null,
        updated_at: new Date().toISOString(),
      });
    }
  }
  return new Response("ok");
});
```

**c. Deploy & connect:**
```bash
supabase functions deploy stripe-webhook --no-verify-jwt
supabase secrets set STRIPE_SECRET_KEY=sk_live_…
supabase secrets set STRIPE_WEBHOOK_SECRET=whsec_…   # from the next step
```
Stripe Dashboard → **Developers → Webhooks → Add endpoint** →
`https://<project>.supabase.co/functions/v1/stripe-webhook` → events:
`checkout.session.completed`, `customer.subscription.updated`,
`customer.subscription.deleted` → copy the signing secret into the secret above.

Done: on every purchase/cancel, Stripe writes the truth into `subscriptions`;
the console reads it at sign-in (matched by account email) and sets the plan
accordingly — spoof-proof. Important: buyers must check out with the **same email
they use for their RATA account** (Payment Links can lock/prefill the email field).

## 6. Test before going live
Toggle Stripe test mode → make test payment links → card `4242 4242 4242 4242`,
any future date/CVC → confirm the redirect flips the plan and (if step 5 is done)
the `subscriptions` row appears. Then rebuild the links in live mode.

## Future tiers
When Automations/CRM ship: add new Stripe products/prices, new payment links, and
extend the `plan` value ('automations', 'crm') — the webhook and table already
carry it. Feature-gating those tiers in the app is a one-line check on
`S.settings.plan`.
