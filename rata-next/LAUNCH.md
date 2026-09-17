# Launch runbook

Everything in the code is ready: `npm run build` is clean and 364 checks pass.
What is left is configuration, and all of it needs credentials only you hold.

Work top to bottom. Each step has a way to tell whether it worked; do not move
on from one that has not.

**Roughly 45 minutes**, most of it in the Stripe dashboard.

What is live right now, checked today: the pricing page still says **$8 / $79**
— the pricing from two revisions ago — and `/api/config` still emits the old
`stripeMonthly` / `stripeAnnual` keys. `app.html` contains none of the
multi-mailbox linking, DNS discovery, relink flow or plan tiers. Supabase at
`db.mailrata.org` is up and healthy, and the `.com` redirect works. So the
foundation is sound and **launch is essentially the deploy**.

---

## 0. Before anything — the two hard blockers

Neither of these can be worked around, and both fail quietly if skipped:

| | What happens without it |
|---|---|
| **`LICENCE_PRIVATE_KEY` is not set** | Nobody can be issued a licence, so every copy of the app stops working within thirty days and no new customer can start at all. It is a variable, not a crash: the site looks perfectly healthy. `/api/health` reports it. |
| **Nothing since `4a19f94` is deployed** | The live site is running a build from before any of this: it still shows $8 / $79, and has no multi-mailbox linking, no MX discovery, no credential encryption and no Stripe. |

---

## 1. Database

Supabase → SQL Editor → paste `database.sql` → Run. Safe to re-run; it is all
`if not exists`. This release adds two columns to `subscriptions`
(`domain_addons`, `event_at`), so it must be run even though the tables exist.

**Check:** Table Editor → `subscriptions` shows both new columns.

## 2. The encryption key

Generate it on your own machine and paste it straight into the host's
environment settings. Do not send it through chat, and do not commit it.

```
openssl rand -base64 32
```

Set as **`LICENCE_PRIVATE_KEY`**. See LICENSING.md, which also covers the public half that goes into the app build.

Then put a copy somewhere durable and **separate from your database backups** —
a password manager is fine. Lose it and every customer has to relink every
mailbox; store it beside the backup and one stolen file is both halves. The
reasoning is in `infrastructure/backup/README.md`.

**Check:** after deploying, link a mailbox. If it saves, the key is good.

## 3. Stripe

Full detail in `STRIPE-SETUP.md`. In order:

1. **Products** — two, each a recurring monthly price: RATA Base **$12.99**,
   RATA Pro **$23.99**. Copy both `price_…` ids.
   *(Enterprise is deliberately not for sale — its features are not built.)*
2. **Add-on** (optional, only if selling custom domains) — a $1.50/month price
   with **"customers can adjust quantity"** on.
3. **Payment links** — one per plan. After-payment redirect to
   `https://mailrata.org/app?checkout=success`.
4. **Webhook** → `https://mailrata.org/api/stripe/webhook`, subscribed to
   exactly `checkout.session.completed`, `customer.subscription.updated`,
   `customer.subscription.deleted`. Copy the `whsec_…`.
5. **Customer portal** — activate it, allow plan changes and cancellation, copy
   the login link.

**Do this in test mode first.** Test mode has its own ids and its own signing
secret; pay with `4242 4242 4242 4242`.

## 4. Environment variables

hPanel → the site → Environment. Server-only — these must never reach a browser:

| Variable | Value |
|---|---|
| `SUPABASE_URL` | your Supabase URL |
| `SUPABASE_SERVICE_ROLE_KEY` | the service-role key |
| `LICENCE_PRIVATE_KEY` | from step 2 |
| `STRIPE_WEBHOOK_SECRET` | the `whsec_…` |
| `STRIPE_PRICE_BASE` | the Base `price_…` |
| `STRIPE_PRICE_PRO` | the Pro `price_…` |
| `STRIPE_PRICE_DOMAIN` | the add-on `price_…`, if you made one |
| `APP_URL` | `https://mailrata.org` |

Public — these are emitted to every visitor, which is correct for all of them:

| Variable | Value |
|---|---|
| `SUPABASE_ANON_KEY` | the anon key, **not** the service-role key |
| `STRIPE_BASE` | the Base payment link |
| `STRIPE_PRO` | the Pro payment link |
| `STRIPE_PORTAL` | the billing portal link |

`/api/config` refuses to publish a service-role key if one is pasted into
`SUPABASE_ANON_KEY` by mistake — the two look alike and sit on the same page —
and logs why. That refusal is covered by a test, but check the browser console
after deploying anyway.

Leave unset unless you mean them: `GOOGLE_CLIENT_ID` (one-click sign-in stays
off without it). The Microsoft and Slack variables are gone — those links are
made in the desktop app now, and the server has no route that reads anybody's
messages.

## 5. Deploy

Deploy `rata-next` to the Hostinger Node.js site, from the current branch.

> There is no deploy workflow, deliberately. The one that used to exist pushed
> an abandoned Kubernetes architecture to `centerpoint-inbox.com` and has been
> deleted along with the code it deployed. `rata-next-ci.yml` builds and tests
> only; `release.yml` cuts desktop installers. Neither touches the live site.

The build command is `npm run build`, the start command `npm start`, Node 22.

## 6. Smoke test on the live site

In this order, because each depends on the last:

1. **`/api/config`** in a browser — Supabase URL and anon key present, no
   `service_role` warning in the console.
2. **Sign up** with a real address. The badge must say the account syncs, not
   that it is device-only.
3. **Link a mailbox** — a Gmail or iCloud address with an app password. It
   should report which provider it found. *If this errors about
   `LICENCE_PRIVATE_KEY`, stop and finish step 2.*
4. **Link a second mailbox** on Base — must be refused with the Pro price named.
5. **Pay** with a test card. Stripe's webhook log shows `200`; the
   `subscriptions` row appears; reload and the plan shows in Settings.
6. **Delete the account** (Settings → Delete). It should refuse while the
   subscription is live — that refusal is the feature.

## 7. Two things to know about signups before you open the doors

Checked against the live backend today:

**Signups are auto-confirmed** (`mailer_autoconfirm` is on), so anyone can
register any address without proving they own it. Entitlement is looked up by
that address — `subscriptions` is keyed on email — so the link between "who
paid at Stripe" and "who holds the account" rests on an address nobody
verified. Two consequences, one common and one nasty:

- *Common:* somebody registers with a personal address, pays with the one their
  card is under, and the payment lands on a row no account reads. They have been
  charged and see no plan. **This is now mitigated** — checkout links carry
  `prefilled_email` for the signed-in address and `client_reference_id` for
  their account id, so the two only diverge if the buyer deliberately edits the
  field.
- *Nasty:* somebody registers an address they do not own before its real owner
  does. The owner cannot then register at all, and if they pay using it, the
  plan lands on the squatter's account.

The real fix is email confirmation, which needs an SMTP sender configured in
GoTrue — turning `mailer_autoconfirm` off without one would break signup
completely, so **do not flip it on launch day**. Configure a sender first, then
turn it off, and the whole class goes away.

## 8. Before you sleep, not after

- **Backups.** `infrastructure/backup/backup.sh`, nightly, off the box. There
  is currently no copy of the database anywhere. At a few hundred paying
  customers this is the largest single risk in the system, and it is an hour's
  work.
- **Uptime and error alerts.** Without them you find out about an outage from
  a customer. Free at this size.

---

## What is deliberately not in this launch

- **Enterprise** — defined, not for sale. The CRM, texts and automations it
  advertises do not exist. `sellable` in `lib/plan.js` turns it back on.
- **RATA-hosted mailboxes** — there is no mail server, and the code that
  anticipated one went with the rest of the server-side mail handling.
- **Google one-click sign-in** — no client id. Gmail still links by app
  password, which is the path most people take anyway.
- **The affiliate programme** — modelled, not built.
