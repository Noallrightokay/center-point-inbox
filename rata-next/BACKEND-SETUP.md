# RATA Backend — Live Outlook, Slack & iCloud Sync

The Next.js deployment IS the RATA backend. These steps light up real, server-side
inbox sync. Gmail is not covered here — it runs entirely in the browser and only
needs `GOOGLE_CLIENT_ID` set in the environment (see DEPLOY.md).

**Prerequisites:** a working Node.js Web App deploy with `APP_URL`, `SUPABASE_URL`,
`SUPABASE_SERVICE_ROLE_KEY` and `TOKEN_ENC_KEY` already set, and `database.sql`
already run. All of
that is DEPLOY.md steps 1–3 — do those first. If `/api/sync/ms` in a browser tab
returns the landing page instead of JSON, stop and fix the deploy; nothing below
will work until it returns JSON.

Add the provider variables below in hPanel → your app → Environment, then redeploy.
Each provider is independent — set up only the ones you want.

---

## TOKEN_ENC_KEY — required before any account can be linked

RATA holds the thing that opens a user's mailbox: an app password, or an OAuth
access and refresh token. Those are encrypted before they reach the database
(AES-256-GCM, in `lib/secrets.js`), and the key lives in the environment rather
than in the database, so reading the database is not enough to read the
credentials in it.

Generate one:

```bash
openssl rand -base64 32
```

Set it in hPanel → your app → Environment as `TOKEN_ENC_KEY`, then redeploy.
**Set it before deploying the code that needs it** — a deployment without it
refuses to store credentials rather than falling back to plaintext, so linking
an account will fail with a message naming this variable.

Rows written before this existed still work: unencrypted values are read as-is,
and any write re-encrypts them. To convert them all in one pass:

```bash
SUPABASE_URL=... SUPABASE_SERVICE_ROLE_KEY=... TOKEN_ENC_KEY=... \
  node scripts/encrypt-existing.mjs --dry-run   # see what it would touch
```

Drop `--dry-run` to write. It is safe to run more than once.

### Rotating the key

Set the new key as `TOKEN_ENC_KEY`, move the current one to `TOKEN_ENC_KEY_OLD`,
and redeploy. Existing rows keep opening under the old key while new writes use
the new one; run `scripts/encrypt-existing.mjs` — it rewrites anything still on
the old key — then remove `TOKEN_ENC_KEY_OLD`.

**Losing the key is not recoverable.** Encrypted credentials cannot be read back
without it, and every user has to relink their accounts. Keep it wherever you
keep `SUPABASE_SERVICE_ROLE_KEY`, and do not rotate one expecting the other to
cover for it.

---

## Microsoft (Outlook) — Azure app, free, ~10 min

1. portal.azure.com → Microsoft Entra ID → **App registrations → New**.
   - Supported accounts: **Accounts in any organizational directory and personal
     Microsoft accounts**.
   - Redirect URI (Web): `https://yourdomain.com/api/link/ms/callback`
2. **API permissions** → Microsoft Graph → Delegated → add `Mail.Read`, `User.Read`,
   `offline_access`.
3. **Certificates & secrets** → New client secret → copy the **Value** (not the ID —
   the Value is shown once and never again).
4. Overview page → copy **Application (client) ID**.

→ set `MS_CLIENT_ID` + `MS_CLIENT_SECRET`.

The redirect URI must match `APP_URL` exactly, scheme and all. A mismatch here is
the single most common cause of "Microsoft token exchange failed".

## Slack — Slack app, free, ~5 min

1. api.slack.com/apps → **Create New App → From scratch** → pick your workspace.
2. **OAuth & Permissions**:
   - Redirect URL: `https://yourdomain.com/api/link/slack/callback`
   - **User Token Scopes** (not Bot Token Scopes): `channels:history`,
     `groups:history`, `im:history`, `mpim:history`, `users:read`, `team:read`
3. Basic Information → copy **Client ID** and **Client Secret**.

→ set `SLACK_CLIENT_ID` + `SLACK_CLIENT_SECRET`.

## iCloud Mail and Gmail over IMAP — no setup required

Two providers link with an **app-specific password** instead of OAuth. Both need no
environment variables, no developer account, and no work from you — each user adds
their own address and password in Settings, and RATA verifies it by opening a real
IMAP connection before storing anything.

| Provider | Host | Where the user gets the password |
|---|---|---|
| iCloud Mail | `imap.mail.me.com` | appleid.apple.com → Sign-In and Security → App-Specific Passwords |
| Gmail | `imap.gmail.com` | myaccount.google.com/apppasswords (requires 2-Step Verification) |

Apple offers no OAuth for Mail at all, so IMAP is the only route. Gmail *does* offer
OAuth, and it is the nicer experience — but `gmail.readonly` is a **restricted** scope:
opening one-click Gmail beyond ~100 test users requires Google verification plus a
third-party CASA security assessment, renewed annually at real cost. The IMAP link
needs none of that, so it is what lets ordinary users connect Gmail today. Ship both:
OAuth for those you can add as test users, IMAP for everyone else.

Because the password is stored (it has to be — IMAP re-authenticates on every sync),
read the credential-storage note at the bottom of this file before enabling iCloud
for users other than yourself.

---

## Use it

Sign in with a **cloud account** → Settings → Linked accounts → **＋ Microsoft ·
live**, **＋ Slack · live**, or **＋ iCloud** → real OAuth consent (or the
app-specific password prompt) → you land back in the inbox and RATA pulls the real
messages immediately.

After that: the sources strip at the top of Messages shows each linked provider with
its last-sync time — tap any source (or ⟳ Sync linked inboxes) to refresh. Microsoft
tokens auto-refresh.

## How it works

```
app.html → /api/link/{provider}/state   (authenticated: Supabase JWT in the header)
         → one-time state row in link_states
           + the same value in an HttpOnly SameSite=Lax cookie, so the flow is
           bound to this browser and not just to whoever holds the state value
         → /api/link/{provider}/start   → provider consent screen
         → /api/link/{provider}/callback
              exchanges the code server-side (client secret never leaves the server)
              stores tokens in provider_tokens
              redirects to /app.html?linked=…
         → /api/sync/{provider}         → normalized messages
         → merged into your inbox, tagged to the linked account,
           synced to your workspace in the database
```

Both `/start` and `/callback` require the cookie to match the state in the URL,
and both enforce the 10-minute TTL. The cookie is cleared however the flow ends.

IMAP providers skip the OAuth legs entirely: `POST /api/link/imap/{provider}`
verifies the credentials against the real mail host and stores them, then
`/api/sync/imap/{provider}` connects and returns normalized messages. `{provider}`
is `apple` or `gmail`; both share one implementation in `lib/imap.js`.

## What is stored, and where

RATA runs on the device it was installed on. The account exists online so you
can sign in from more than one machine and so billing has somewhere to live.
Almost nothing else travels.

**On the server**

| What | Where | Why it has to be there |
|---|---|---|
| Email address, password hash | Supabase auth | signing in |
| Plan, status, Stripe customer id | `subscriptions` | entitlement, written by the Stripe webhook |
| Mailbox credentials, encrypted | `provider_tokens` | the server performs the IMAP fetch, so it needs them; AES-256-GCM with the key outside the database, each value bound to its own row |
| Preferences: name, plan, plugin switches, folder names, rules, job tags, which mailboxes are linked | `workspaces` | so a second device looks like the first |

**On the device, and nowhere else**

Messages and their content. Contacts. Document text and file bytes (IndexedDB).
The audit chain. Counters. Any AI key, which lives in that browser's
localStorage and is never uploaded.

Mail is not uploaded because it does not need to be: the mailbox is the source
of truth and every device fetches from it directly, so a message appears
everywhere without RATA keeping a copy. A file made on one machine stays
there; a file that travelled through RATA arrives with the mail that carried
it, wherever that mail is read.

`forCloud()` in app.html is the whole of it — an allow-list, so a field added
to the workspace later is private unless somebody deliberately adds it.

### What you can tell a customer

> Your account login, your subscription, and the encrypted passwords for the
> mailboxes you connect are stored on RATA's server. Your mail, your files and
> your contacts stay on your device. Files live on the device that made them;
> anything delivered through RATA travels with the mail. You can disconnect a
> mailbox at any time, and revoke its password at your provider independently
> of us.

Do not claim RATA holds no credentials for other services. It holds the
mailbox passwords, encrypted — it cannot read mail without them. Encrypted and
revocable is a strong claim and a true one.

## Known limits

- **One account per provider, per user — except mail.** `provider_tokens` is keyed
  on `(user_id, provider)`. Mailboxes work around this by taking a key of their
  own per address (`mail:<address>`), so a user may hold several; Microsoft and
  Slack still overwrite rather than add.
- **Credentials are encrypted with `TOKEN_ENC_KEY`** before they reach
  `provider_tokens`, and each value is bound to the row that holds it, so a
  ciphertext moved to another user's row will not open. The service-role key on
  its own therefore no longer yields anyone's mail password — both it and
  `TOKEN_ENC_KEY` are needed. They are still worth storing separately: an
  attacker holding both is back to holding everything. What is not encrypted is
  the `label` column, which is the mailbox address.
- **There is no deletion path.** Cancelling a subscription leaves the
  `workspaces` and `provider_tokens` rows in place. Nothing deletes an account
  and everything attached to it. That is the gap most likely to matter legally
  and it is small to build.
- **Discord**: their API does not permit reading user DMs via OAuth — by policy. A
  bot-based bridge for servers you own is the viable path (future build).
- **SMS**: needs a telephony provider (Twilio) — planned, not free.
- Live sync requires cloud accounts (Supabase mode), since tokens are stored per
  registered user. Local-only accounts cannot link providers.
