# RATA Backend — Live Outlook, Slack & iCloud Sync

The Next.js deployment IS the RATA backend. These steps light up real, server-side
inbox sync. Gmail is not covered here — it runs entirely in the browser and only
needs `GOOGLE_CLIENT_ID` set in the environment (see DEPLOY.md).

**Prerequisites:** a working Node.js Web App deploy with `APP_URL`, `SUPABASE_URL`,
and `SUPABASE_SERVICE_ROLE_KEY` already set, and `database.sql` already run. All of
that is DEPLOY.md steps 1–3 — do those first. If `/api/sync/ms` in a browser tab
returns the landing page instead of JSON, stop and fix the deploy; nothing below
will work until it returns JSON.

Add the provider variables below in hPanel → your app → Environment, then redeploy.
Each provider is independent — set up only the ones you want.

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

## iCloud Mail — no setup required

Apple publishes no OAuth for Mail. The sanctioned mechanism is an **app-specific
password** from appleid.apple.com — a revocable per-app token. There are no
environment variables and no developer account to register: each user adds their own
iCloud address and app-specific password in Settings, and RATA verifies it by
opening a real IMAP connection to `imap.mail.me.com` before storing anything.

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

iCloud skips the OAuth legs: `POST /api/link/apple` verifies and stores the
app-specific password directly, then `/api/sync/apple` connects over IMAP.

## Known limits

- **One account per provider, per user.** `provider_tokens` is keyed on
  `(user_id, provider)`, so linking a second Microsoft account overwrites the first
  rather than adding it. The UI's "add another" affordance does not yet reflect this.
- **Credentials are stored as plaintext columns** in `provider_tokens` — OAuth
  refresh tokens and iCloud app-specific passwords alike. The table has RLS enabled
  with no policies, so only the service-role key can read it, and Supabase encrypts
  at rest. But anyone holding `SUPABASE_SERVICE_ROLE_KEY` can read every user's mail
  credentials. Guard that key accordingly, and rotate it if it is ever exposed.
- **Discord**: their API does not permit reading user DMs via OAuth — by policy. A
  bot-based bridge for servers you own is the viable path (future build).
- **SMS**: needs a telephony provider (Twilio) — planned, not free.
- Live sync requires cloud accounts (Supabase mode), since tokens are stored per
  registered user. Local-only accounts cannot link providers.
