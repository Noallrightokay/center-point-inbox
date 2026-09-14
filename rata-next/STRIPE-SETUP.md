# RATA × Stripe — taking payments

RATA sells through **Stripe Payment Links**. Stripe hosts the card form, the
receipts, the tax handling and the customer portal; the app never sees a card
number and holds no Stripe secret key. What it needs from Stripe is one signed
message saying who paid, which `/api/stripe/webhook` handles.

About 30 minutes end to end. Test mode first — the last section covers that.

**Prerequisites:** the app deployed with `SUPABASE_URL` and
`SUPABASE_SERVICE_ROLE_KEY` set, and `database.sql` run (it creates the
`subscriptions` table this writes to).

---

## 1. The three products

Stripe Dashboard → **Product catalogue** → add a product for each plan, each
with a **recurring monthly** price:

| Product | Price | What it is |
|---|---|---|
| RATA Base | $8 / month | Up to two mailboxes in one Center Point inbox |
| RATA Pro | $16 / month | More than two mailboxes, side by side, summaries |
| RATA Enterprise | $72 / month | CRM, texts, automations |

Open each price and copy its **price ID** (`price_…`). You need all three.

## 2. A payment link for each

Dashboard → **Payment Links** → create one per price.

On each link, under **After payment**, choose *Redirect customers to a page*
and set it to:

```
https://mailrata.org/app?checkout=success
```

The redirect is only there so the user lands back in the app. It grants
nothing — the webhook is what records the plan, and the app treats the absence
of a subscription row as the absence of a plan no matter what the URL says.

Copy the three `https://buy.stripe.com/…` URLs.

## 3. The webhook

Dashboard → **Developers → Webhooks → Add endpoint**.

Endpoint URL:

```
https://mailrata.org/api/stripe/webhook
```

Select these events and no others:

- `checkout.session.completed` — the only one that carries the buyer's email,
  and what creates their row
- `customer.subscription.updated` — upgrades, downgrades, failed payments
- `customer.subscription.deleted` — cancellations

Copy the **signing secret** (`whsec_…`).

## 4. Set the environment variables

hPanel → your site → Environment. Public ones (they reach the browser, which
is fine — payment links are meant to be shared):

| Variable | Value |
|---|---|
| `STRIPE_BASE` | the Base payment link |
| `STRIPE_PRO` | the Pro payment link |
| `STRIPE_ENTERPRISE` | the Enterprise payment link |
| `STRIPE_PORTAL` | the customer portal link (section 5) |

Server-only. These must never appear in the browser:

| Variable | Value |
|---|---|
| `STRIPE_WEBHOOK_SECRET` | the `whsec_…` from step 3 |
| `STRIPE_PRICE_BASE` | the Base `price_…` |
| `STRIPE_PRICE_PRO` | the Pro `price_…` |
| `STRIPE_PRICE_ENTERPRISE` | the Enterprise `price_…` |

Redeploy.

## 5. Customer portal

Dashboard → **Settings → Billing → Customer portal** → activate, allow plan
switching and cancellation, copy the login link into `STRIPE_PORTAL`. That one
screen handles cards, invoices, upgrades and cancellation, so RATA does not
have to build any of it.

---

## What happens, in order

1. Someone clicks a plan in Settings and pays on Stripe's page.
2. Stripe POSTs `checkout.session.completed` to the webhook, signed.
3. The webhook verifies the signature, maps the price to a plan, and writes
   `subscriptions`: email, plan, status `active`, and the Stripe customer id.
4. The app reads that row. The **server** reads it too, through `planForUser`,
   and that is what refuses an over-limit mailbox — so entitlement does not
   depend on the browser being honest.
5. Later changes arrive as subscription events. Those carry no email, so they
   are matched on the customer id stored in step 3.

**A cancellation does not delete anyone's data.** Status goes to `canceled`,
the plan clears, and linking a new mailbox stops. Mail already synced stays
where it is.

**A failed payment does not lock anyone out.** `past_due` is still entitled —
a card that failed this morning should not cut off someone's mail while Stripe
retries it. `LIVE_STATUSES` in `lib/stripe.js` is where that decision lives.

## Testing before going live

Stripe test mode has its own products, links, price IDs and signing secret, so
set the test values, then:

1. Pay with `4242 4242 4242 4242`, any future expiry, any CVC.
2. Dashboard → Webhooks → your endpoint shows the delivery and a `200`.
3. The `subscriptions` row appears with the right plan.
4. Reload the app — the plan shows in Settings, and the gates for that tier open.

If the delivery shows `400`, the signing secret does not match the one in the
environment. A `500` means the webhook could not write to Supabase — check
`SUPABASE_SERVICE_ROLE_KEY`. Stripe retries `500`s for up to three days, so a
payment is not lost while that is fixed.

`npm test` covers the parts that do not need Stripe: signature verification
including replay and tolerance, price-to-plan mapping, what each event does to
the record, and that the live endpoint refuses a forged POST.

## Adding a plan later

Add the product and price in Stripe, add `STRIPE_<NAME>` and
`STRIPE_PRICE_<NAME>`, and add the tier to `PLANS` in `lib/plan.js` and `TIERS`
in `app.html`. The webhook needs no change — it maps whatever price IDs the
environment names.
