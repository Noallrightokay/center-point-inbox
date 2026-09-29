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

## 1. The two products

Stripe Dashboard → **Product catalogue** → add a product for each plan, each
with a **recurring monthly** price:

| Product | Price | What it is |
|---|---|---|
| RATA Base | $12.99 / month | Up to two mailboxes in one Center Point inbox (the app's name for its unified view) |
| RATA Pro | $23.99 / month | More than two mailboxes, side by side, summaries |
| ~~RATA Enterprise~~ | ~~$72 / month~~ | **Not on sale.** The CRM, texts and automations are not built, so there is no product to charge for yet. Create this one when they are. |

**Do not create the "Your own domain" add-on.** `lib/plan.js` still prices it
($1.50 a month, "host mail on your own domain"), but RATA does not host
mail: it is read on the customer's own machine, and
`mailrata.org` has no connection to anyone's mailbox (see `CLAUDE.md`, *The
one-line version*, and
[`docs/MVP-PLAN.md` Part 3](../docs/MVP-PLAN.md#part-3-integrations-plug-ins-what-connects-to-what):
*RATA-hosted mail does not exist*). Selling it would be charging for nothing.

Open each price and copy its **price ID** (`price_…`). You need Base and Pro, two. Enterprise waits until its features exist; `sellable` in `lib/plan.js` is what turns it back on.

## 2. A payment link for each

Dashboard → **Payment Links** → create one per price.

On each link, under **After payment**, choose *Redirect customers to a page*
and set it to:

```
https://mailrata.org/account?checkout=success
```

**The licence page, not the app.** What somebody needs in the ten seconds after
paying is the key that makes the desktop app run, and `/account` is the only
page that has it. Sending them to `/app` instead leaves them inside the web
version wondering what they just bought.

The redirect grants nothing. The webhook is what records the plan, and the
licence page asks the server — which reads the subscription row and not the URL,
so `?checkout=success` on its own buys nobody anything.

`?checkout=success` is not decoration. Stripe redirects the customer and
delivers the webhook independently, and the redirect usually wins by a second or
two — so a brand-new subscriber would otherwise land here and be told they have
no active plan, which is the single worst sentence to show somebody who has just
paid. The page reads that parameter, treats "no subscription" as *not yet*, and
waits up to twenty seconds for the webhook before it says anything
discouraging. Change the redirect and you lose that.

Copy the two `https://buy.stripe.com/…` URLs.

## 3. The webhook

Dashboard → **Developers → Webhooks → Add endpoint**.

Endpoint URL:

```
https://mailrata.org/api/stripe/webhook
```

Select exactly these five events and no others:

- `checkout.session.completed` — the only one that carries the buyer's email,
  and what creates their row. It does **not** say which plan was bought: a
  Checkout Session carries `line_items` only when it is retrieved with
  `expand[]=line_items`, and a webhook delivery is never expanded (RATA holds
  no secret key to retrieve it with). So the row is written as `incomplete`,
  which is not entitled, until the subscription event below names the plan.
  A checkout paid by a delayed method (a bank debit) completes before the
  money arrives and stays `incomplete` until that clears too
- `checkout.session.async_payment_succeeded` — that delayed payment cleared:
  the row becomes `active`, keeping the plan the subscription named
- `customer.subscription.created` — **the event that names the plan.** It
  carries the subscription's prices (`items.data[].price`), which is where
  the plan and any add-on come from, and it makes a new customer's row live.
  Leave it out and a card checkout that Stripe creates already active is
  never followed by another subscription event, so that customer's row stays
  `incomplete` and they get no licence
- `customer.subscription.updated` — upgrades, downgrades, failed payments,
  and a subscription turning `active` when its first payment clears
- `customer.subscription.deleted` — cancellations

Copy the **signing secret** (`whsec_…`).

## 4. Set the environment variables

hPanel → your site → Environment. Public ones (they reach the browser, which
is fine — payment links are meant to be shared):

| Variable | Value |
|---|---|
| `STRIPE_BASE` | the Base payment link |
| `STRIPE_PRO` | the Pro payment link |
| `STRIPE_ENTERPRISE` | leave unset: Enterprise is not on sale |
| `STRIPE_PORTAL` | the customer portal link (section 5) |

Server-only. These must never appear in the browser:

| Variable | Value |
|---|---|
| `STRIPE_WEBHOOK_SECRET` | the `whsec_…` from step 3 |
| `STRIPE_PRICE_BASE` | the Base `price_…` |
| `STRIPE_PRICE_PRO` | the Pro `price_…` |
| `STRIPE_PRICE_ENTERPRISE` | leave unset: Enterprise is not on sale |
| `STRIPE_PRICE_DOMAIN` | **leave unset**: the domain add-on must not be sold (section 1) |

With `STRIPE_PRICE_DOMAIN` unset no domain is ever granted, by accident or
otherwise: the webhook counts no add-on line at all.

Redeploy.

## 5. Customer portal

Dashboard → **Settings → Billing → Customer portal** → activate, allow plan
switching and cancellation, copy the login link into `STRIPE_PORTAL`. That one
screen handles cards, invoices, upgrades and cancellation, so RATA does not
have to build any of it.

---

## What happens, in order

1. Someone with no plan clicks a buy link (the home page, `/account`, or
   Settings) and pays on Stripe's page. Signed in, the link carries their
   address (`prefilled_email`) and account id (`client_reference_id`), so
   they pay with the address the licence is keyed on. Someone who already
   has a plan switches in the customer portal, never through a Payment
   Link: that would make a second Stripe customer, whose checkout the
   webhook refuses as a conflict (charged twice, nothing applied).
2. Stripe raises `customer.subscription.created` and then
   `checkout.session.completed`, and POSTs both to the webhook, signed, in
   no promised order.
3. The checkout event creates the row: email, the Stripe customer id, and
   status `incomplete` (not entitled), with the table's default plan `base`
   and no add-ons as placeholders. It never writes a plan it was not told,
   so it can never lower one a subscription event set; and it never replaces
   a live row with an `incomplete` one. The row is stamped with when the
   buyer opened the checkout, so no subscription event of this purchase is
   refused as older than it.
4. The subscription event, matched on the customer id (it carries no
   email), writes the plan, any custom-domain add-on quantity (always zero,
   since `STRIPE_PRICE_DOMAIN` stays unset) and Stripe's status, which makes
   the row live. If it arrived before the checkout event, which is the usual
   order, it finds no row: the webhook keeps it in `pending_subscriptions`
   (keyed by the customer id, no address; `database.sql` section 6) and
   answers `200`, and the checkout event applies it in the same request,
   straight after writing the row. No retry is needed. Until the row is
   live, `/api/licence` answers `pending` and the licence page says
   "Setting up your licence" with no buy buttons, rather than handing a Pro
   buyer a Base licence good for 30 days. A `created` event stamped the
   same second as an `updated` is the earlier of the two, so it never
   replaces it, in either table.

   The one case still left to Stripe's retry: a database where section 6
   has not run has nowhere to keep the event, so it gets a `409` as before
   and the server log says to run section 6.

   The plan is found by scanning every line rather than reading the first. A
   subscription carrying both Pro and the domain add-on can arrive in either
   order, and reading line one would have cleared the plan of a customer whose
   only mistake was buying something extra.
5. The licence routes read that row (`entitlementsForUser` in `lib/plan.js`)
   and sign its plan into the licence key that `/account` shows and the app
   renews. The desktop app enforces the plan from that signed key (Base's
   two-mailbox limit included), so entitlement does not depend on the
   browser being honest, and a row that is no longer live gets no new key.
6. Later changes arrive as subscription events, matched on the customer id
   stored in step 3. A delayed payment clearing sends
   `checkout.session.async_payment_succeeded`, which sets the status only.

**Dropping the add-on is recorded, not ignored.** A customer who removes their
extra domain sends a subscription with no add-on line, and that writes zero —
skipping it would leave them entitled to a domain they stopped paying for.

**A cancellation does not delete anyone's data.** Status goes to `canceled`,
the plan clears, and the licence is not renewed, so the app stops at the end
of its current 30 days. Mail already synced stays where it is.

**A failed payment does not lock anyone out.** `past_due` is still entitled —
a card that failed this morning should not cut off someone's mail while Stripe
retries it. `LIVE_STATUSES` in `lib/stripe.js` is where that decision lives.

## Testing before going live

Stripe test mode has its own products, links, price IDs and signing secret, so
set the test values, then:

1. Pay with `4242 4242 4242 4242`, any future expiry, any CVC.
2. Dashboard → Webhooks → your endpoint shows the deliveries:
   `checkout.session.completed` and `customer.subscription.created` each
   with a `200` on the first delivery, in whichever order they arrived. A
   `409` means `pending_subscriptions` is missing: re-run `database.sql`
   (section 6).
3. The `subscriptions` row appears, `active`, with the plan that was bought
   (Pro for the Pro link, not Base).
4. Reload the app — the plan shows in Settings, and the gates for that tier open.

If the delivery shows `400`, the signing secret does not match the one in the
environment. A `500` means the webhook could not write to Supabase — check
`SUPABASE_SERVICE_ROLE_KEY`. Stripe retries every failed delivery, a `400`
included, for up to three days in live mode (three times over a few hours in
test mode), so once the signing secret or the key is corrected the next retry
lands and no payment is lost.

`npm test` covers the parts that do not need Stripe: signature verification
including replay and tolerance, price-to-plan mapping, what each event does to
the record, and that the live endpoint refuses a forged POST.

## Adding a plan later

Add the product and price in Stripe and set `STRIPE_<NAME>` and
`STRIPE_PRICE_<NAME>`. Then, in code: the tier in `PLANS` in `lib/plan.js`
and `TIERS` in `app.html`; the price in `planForPrice` in `lib/stripe.js`,
which compares each `STRIPE_PRICE_*` variable by name; the payment link in
`PUBLIC_CONFIG` in `app/api/config/route.js`; and the plan in `plan_def` in
`desktop/rata-app/src-tauri/src/licence.rs` (an app that does not know a
plan treats it as unlimited, so ship that before selling it).
