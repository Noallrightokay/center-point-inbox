# Launch runbook

What the website does now: the landing page, sign-up and sign-in, the
licence page (`/account`), the Stripe webhook that records who paid, licence
issue (`/api/licence`) and renewal (`/api/licence/renew`), the AI relay
(`/api/ai`), account deletion, and `/api/health`. Mailboxes are linked and
read only in the desktop app; the site holds no mail password and has no
route that reads anybody's mail. The code builds with `npm run build`, and
the website's nine test suites pass with `npm test` after it.
What is left is configuration, and all of it needs credentials only you hold.

Work top to bottom. Each step has a way to tell whether it worked; do not move
on from one that has not.

**Roughly 45 minutes**, most of it in the Stripe dashboard.

What is live right now (checked 2026-09-28): an old deploy. The landing page
still says **$8 / $79** and sells texts, Slack and Discord, and
`/api/health`, `/api/licence/renew`, `/api/ai` and the screenshots in
`/shots/` all answer 404. So licence renewal and AI have never worked for
anyone, and **launch is the configuration below plus the deploy**. Supabase
at `db.mailrata.org` is up: the live `/api/config` serves its URL.

---

## 0. Before anything — the two hard blockers

Neither of these can be worked around, and both fail quietly if skipped:

| | What happens without it |
|---|---|
| **`LICENCE_PRIVATE_KEY` is not set, or is not the existing key** | Unset: nobody can be issued a licence, so every copy of the app stops working within thirty days and no new customer can start at all. A *new* key is worse: the site issues licences that every installed app refuses with a signature error. Either way it is a variable, not a crash, and the site looks perfectly healthy. `/api/health` reports a missing or malformed key; only a key pasted into a released app proves it is the right one (section 6, check 7). |
| **The current code is not deployed** | The live site is an old build: it still shows $8 / $79, and has no licence renewal, no AI relay and no `/api/health`. |

---

## 1. Database

Supabase → SQL Editor → paste `database.sql` → Run. Safe to re-run; it is all
`if not exists`. This release adds two columns to `subscriptions`
(`domain_addons`, `event_at`) and the `ai_usage` table (its section 5), so
it must be run even though the other tables exist.

**Check:** Table Editor → `subscriptions` shows both new columns, and
`ai_usage` exists.

## 2. The licence signing key

`LICENCE_PRIVATE_KEY` signs every licence the site issues and renews. It is
an **Ed25519 private key in PEM form**, not a random string, and it must be
**the key that already exists** on the VPS at `/etc/rata/licence.key`. Its
public half is compiled into every installer released since v0.1.3 (the
GitHub secret `RATA_LICENCE_PUBLIC_KEY`). A newly generated key would make
every licence the site issues fail, with a signature error, in every copy of
the app already installed. **Do not generate one.** LICENSING.md §1 is only
for a deliberate rotation, which also means shipping a new app build.

On the VPS, read both halves from the existing key and paste them straight
into hPanel (step 4). Never through chat, an issue or a commit:

```bash
cat /etc/rata/licence.key                        # -> LICENCE_PRIVATE_KEY
openssl pkey -in /etc/rata/licence.key -pubout   # -> LICENCE_PUBLIC_KEY
```

The second is the same PEM as the GitHub secret `RATA_LICENCE_PUBLIC_KEY`.
If `openssl pkey` refuses the file, it is not a PEM key: stop and find out
how `/etc/rata/mint.sh` reads it before setting anything. Hosting panels
mangle multi-line values; the site accepts the PEM with real newlines or
with them written as `\n` (LICENSING.md §2).

Keep a copy somewhere durable and **separate from your database backups** (a
password manager is fine). Lose it and no licence can be issued or renewed
until a new app build ships with a new public key, which every customer then
has to install. Keep it beside the backup and one stolen file lets
anyone mint licences. The reasoning is in LICENSING.md and
`infrastructure/backup/README.md`.

**Check:** after deploying, `/api/health?token=<HEALTH_TOKEN>` shows
`licensing` ok, which proves the key is present and well-formed. Only
section 6, check 7 (a key from `/account` accepted by a released app)
proves it is the right one.

## 3. Stripe

Full detail in `STRIPE-SETUP.md`. In order:

1. **Products** — two, each a recurring monthly price: RATA Base **$12.99**,
   RATA Pro **$23.99**. Copy both `price_…` ids.
   *(Enterprise is deliberately not for sale — its features are not built.)*
2. **Do not create the domain add-on price.** STRIPE-SETUP.md describes a
   $1.50/month "Your own domain" add-on, and `lib/plan.js` prices it, but
   RATA-hosted mail does not exist, so there is nothing to sell. Leave
   `STRIPE_PRICE_DOMAIN` unset.
3. **Payment links** — one per plan. After-payment redirect to
   `https://mailrata.org/account?checkout=success`: the licence page, which
   waits for the webhook when it sees `?checkout=success` (STRIPE-SETUP.md §2
   says why).
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
| `LICENCE_PRIVATE_KEY` | the existing key from `/etc/rata/licence.key` (step 2) |
| `LICENCE_PUBLIC_KEY` | its public half (step 2), the same as the GitHub secret `RATA_LICENCE_PUBLIC_KEY` — renewal and the AI relay check licences with it |
| `HEALTH_TOKEN` | any long random string, e.g. from `openssl rand -hex 24`. With it, `/api/health?token=…` says which check fails and why; without it, only which |
| `ANTHROPIC_API_KEY` | an API key from console.anthropic.com, for the AI relay. Paste it into hPanel only — never into a chat, an issue or a commit |
| `AI_MONTHLY_CAP_USD` | optional; what AI may cost per customer per month. Default `2` |
| `AI_MODEL` | optional; default `claude-haiku-4-5-20251001` |
| `STRIPE_WEBHOOK_SECRET` | the `whsec_…` |
| `STRIPE_PRICE_BASE` | the Base `price_…` |
| `STRIPE_PRICE_PRO` | the Pro `price_…` |
| `STRIPE_PRICE_DOMAIN` | **leave unset** — there is no domain add-on to sell (step 3) |
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
off without it), and `STRIPE_ENTERPRISE` / `STRIPE_PRICE_ENTERPRISE`
(Enterprise is not for sale). The Microsoft and Slack variables are gone:
mailboxes are linked in the desktop app, and the server has no route that
reads anybody's messages.

### The AI relay

`/api/ai` does summaries, translation and task flags for the desktop app, and
RATA pays for them. It is off (answers "AI is not switched on") until
`ANTHROPIC_API_KEY` is set **and** section 5 of `database.sql` has been run —
that section creates `ai_usage`, the running total that caps each customer at
`AI_MONTHLY_CAP_USD` a month. Without the table it refuses rather than
spending without a limit. It passes each request's text to Anthropic and
back and writes none of it anywhere: the table holds an address, a month and
a number. In the Anthropic console, set a monthly spend limit on the key as
well — the cap here is per customer, that one is for the whole bill.

## 5. Deploy

Deploy `rata-next` to the Hostinger Node.js site, from the current branch.

> There is no deploy workflow, deliberately. The one that used to exist pushed
> an abandoned Kubernetes architecture to `centerpoint-inbox.com` and has been
> deleted along with the code it deployed. `rata-next-ci.yml` builds and tests
> only; `release.yml` cuts desktop installers. Neither touches the live site.

The build command is `npm run build`, the start command `npm start`, Node 22.

## 6. Smoke test on the live site

The website links no mailboxes any more, so this tests what it does do: hand
out and renew licences, take payment and relay AI. The first block needs no
credentials; the second needs you.

**Public checks** (anyone, no account):

1. **`/api/config`** — `supabaseUrl`, `supabaseKey` and the Stripe links are
   present, and the key is the anon key, not the service role (a service-role
   key is refused and left out, and the browser console and server log say so).
2. **`/api/health`** answers `200` with `"ok":true`. With
   `?token=<HEALTH_TOKEN>`, `database`, `licensing` and `billing` are all ok
   and `ready` is `true`.
3. **The app's preflight is allowed, anyone else's is not.** An `OPTIONS`
   request to `/api/ai` and to `/api/licence/renew` with
   `Origin: tauri://localhost` answers `204` with
   `Access-Control-Allow-Origin: tauri://localhost`; the same with
   `Origin: https://evil.example` answers with no such header:

   ```bash
   for o in tauri://localhost https://evil.example; do for p in ai licence/renew; do
     echo "$o $p"; curl -si -X OPTIONS -H "Origin: $o" \
       -H 'Access-Control-Request-Method: POST' "https://mailrata.org/api/$p" \
       | grep -iE '^HTTP|^access-control-allow-origin'; done; done
   ```
4. **`/shots/app-light.webp`** answers `200`, so the new landing page is live.
5. **The landing page makes no third-party requests** (its fonts are served
   by the site itself), and its copy claims nothing the app does not do.

**Your checks** (in this order, because each depends on the last):

6. **Sign up** with a real address, then **pay** with the test card
   `4242 4242 4242 4242`. Stripe's webhook log shows `200`, the
   `subscriptions` row appears, and the redirect lands on `/account`, which
   shows a licence key.
7. **Paste that key into a released app** (the current beta). It must be
   accepted. *A signature error here means `LICENCE_PRIVATE_KEY` is not the
   existing key: go back to step 2.*
8. **Pro: AI.** With a Pro key, Summarize on a message gives an AI summary,
   not the local one with a reason.
9. **Renewal.** The app renews when it starts, online, with a licence inside
   its last week. Mint a short key on the VPS for the address that paid in
   check 6 (`/etc/rata/mint.sh <that address> pro 6`), paste it into the app,
   then quit and reopen RATA: Settings → Current plan now runs about 30 days
   out, not 6. Renewal from the app has never worked before this deploy.
10. **Delete the account** (on the website, Settings → Delete). It should
    refuse while the subscription is live — that refusal is the feature.

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

## 9. The update key (so RATA updates itself)

From v0.1.23 the desktop app can update itself — but only with installers
signed by a key that is yours. Without it every release still works exactly
as before; testers just download each new version by hand. Do this once, on
your own computer, **before** the release you want to be the first that
updates itself:

1. Make the key: `npx @tauri-apps/cli signer generate -w ~/.tauri/rata-updates.key`
   and choose a password. It writes two files: `rata-updates.key` (private)
   and `rata-updates.key.pub` (public).
2. In GitHub → the repository → Settings → Secrets and variables → Actions,
   add three repository secrets:
   - `TAURI_SIGNING_PRIVATE_KEY` — the contents of `rata-updates.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password you chose
   - `RATA_UPDATER_PUBKEY` — the contents of `rata-updates.key.pub`
3. Back up `rata-updates.key` and its password somewhere offline. Lose them
   and every installed copy can only be updated by reinstalling by hand.

The private key and its password go into GitHub's secrets and nowhere else —
never into a chat, an issue or a commit. The release workflow refuses to run
with only half of them set.

**How to tell it worked:** the next release's run log says "Signing this
build's installers for updates", and afterwards a release called *Update
feed (not an installer)* has a `latest.json` naming that version. Copies
installed before that release have no key and cannot update themselves: each
tester installs that one release by hand, and from then on RATA offers every
new version itself ("RATA x.y.z is ready — Restart to update").

---

## What is deliberately not in this launch

- **Enterprise** — defined, not for sale. The CRM, texts and automations it
  advertises do not exist. `sellable` in `lib/plan.js` turns it back on.
- **RATA-hosted mailboxes, and the domain add-on that would sell them** —
  there is no mail server, and the code that anticipated one went with the
  rest of the server-side mail handling. Do not create the domain add-on
  price in Stripe, and leave `STRIPE_PRICE_DOMAIN` unset.
- **Outlook, Hotmail and Microsoft 365 mailboxes** — Microsoft requires OAuth
  sign-in for IMAP, and RATA signs in with app passwords only.
- **Google one-click sign-in** — no client id. Gmail still links by app
  password, which is the path most people take anyway.
- **The affiliate programme** — modelled, not built.
