# Setting up connected accounts (OneDrive, Google Drive, Slack)

RATA Pro can open, convert and save files in a customer's cloud storage,
and send a message or a file to Slack (Workstream K in `MVP-PLAN.md`).
iCloud Drive and Adobe's Creative Cloud Files need nothing from you: RATA
uses the folders their own apps keep on the computer (K5). The other three
need RATA registered with each company, which only the owner can do. This
page is the owner's side: what to register, what to copy where, and how to
tell it worked.

Nothing here goes on a server. Each registration gives a **public client
id** that is compiled into every copy of the app, like Microsoft's
`RATA_MS_CLIENT_ID` (`rata-next/LAUNCH.md` §10). The customer signs in on
the company's own page in their browser, and the tokens stay in that
customer's keychain. Put each id in GitHub → the repository → Settings →
Secrets and variables → Actions → **Variables** (not Secrets).

The Rust side of each connection is its own card (K2, K3, K4). K3
(Slack) is built and reads `RATA_SLACK_CLIENT_ID`; K2 (OneDrive) is built
and reads `RATA_MS_CLIENT_ID`; K4 is not built yet, and the variable names
below are the ones that card will read. Registering first costs nothing and
lets each card be tested the day it is built.

## OneDrive (K2)

Uses the Microsoft registration you make for mail sign-in (LAUNCH.md §10).
Do that first. OneDrive is in every build that has `RATA_MS_CLIENT_ID`,
exactly as Sign in with Microsoft is; there is no separate switch.

1. [entra.microsoft.com](https://entra.microsoft.com) → App registrations →
   your RATA registration → API permissions → Add a permission →
   **Microsoft Graph** → Delegated → `Files.ReadWrite`. Not
   `Files.ReadWrite.All`: the `.All` permissions need an organisation's
   administrator. `offline_access` is there already from §10. `User.Read`
   is not needed: RATA names the drive's owner from `GET /me/drive`, which
   `Files.ReadWrite` already allows, and never asks for the address.
2. Nothing else to change in the registration. The redirect URI
   `http://127.0.0.1` under **Mobile and desktop applications** (§10) is
   the one OneDrive's sign-in comes back to too (any port), and **Allow
   public client flows** stays on. No client secret.
3. Nothing new to copy: K2 uses `RATA_MS_CLIENT_ID`.
4. Strongly recommended: **publisher verification** (free). It needs a
   Microsoft AI Cloud Partner Program account, the registration made from a
   work account, and a verified publisher domain (mailrata.org).
   [Publisher verification](https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview).
   Without it, organisations that use risk-based consent show a warning or
   block RATA.

Know this before promising work accounts: since Microsoft's change
MC1163922 (late 2025), organisations on the default consent policy no
longer let their people approve mail access (`IMAP.AccessAsUser.All`) for
an app like RATA; an administrator must. Personal Outlook.com accounts are
not affected. `help.html` says so.

Connecting OneDrive is a sign-in of its own, apart from any Microsoft
mailbox: Microsoft issues a token for one service at a time, so the
customer approves "files" on Microsoft's page once more even when their
Outlook mailbox is already linked, and Disconnect in Settings removes only
OneDrive. RATA reaches the customer's own OneDrive (`/me/drive`), not other
SharePoint sites or files shared with them.

**How to tell it worked:** in a build made after the permission was added,
Settings → Connected accounts → Microsoft OneDrive → Connect opens
Microsoft's page in the browser, asking to let RATA open and change your
files, and Files then lists the account's OneDrive.

## Slack: Share to Slack (K3)

Send only: a message or a file to a channel or a person. RATA does not read
Slack (apps outside the Slack Marketplace may read history only one request
a minute, 15 messages each, which is too little to be useful).

1. [api.slack.com/apps](https://api.slack.com/apps) → Create New App →
   From scratch → name it RATA, pick a workspace you own for development.
2. **OAuth & Permissions** → Redirect URLs → add these three, exactly,
   and Save URLs:
   - `http://localhost:28417`
   - `http://localhost:28418`
   - `http://localhost:28419`

   Slack's docs say a redirect must match, or sit under, a registered
   Redirect URL, and treat a `localhost` redirect as a desktop one once
   PKCE is on. Reports from other desktop apps (2026) show the port has to
   match too, and that `127.0.0.1` is refused where `localhost` works, so
   a bare `http://localhost` is not enough. RATA listens on the first of
   the three ports no other program is using, so register all three. They
   are fixed in the app (`PORTS` in
   `desktop/rata-app/src-tauri/src/slack.rs`); change both together or not
   at all. If Slack's page says `bad_redirect_uri`, this step is why.
3. **User Token Scopes** (not Bot Token Scopes): `chat:write`,
   `files:write`, `im:write`, `channels:read`, `groups:read`, `im:read`,
   `users:read`.
4. Turn on **PKCE**. It is one-way: once on, the app can no longer use a
   client secret, which RATA would never ship anyway.
   [Using PKCE](https://docs.slack.dev/authentication/using-pkce/).
   Recommended as well, under OAuth & Permissions: opt in to **token
   rotation** (also one-way), so an access token lasts 12 hours and is
   renewed by RATA, rather than lasting until it is revoked. RATA works
   either way. [Token rotation](https://docs.slack.dev/authentication/using-token-rotation/).
5. **Manage Distribution** → make the app distributable so other
   workspaces can install it. A Marketplace listing is not needed for
   sending (and needs 10 active workspaces before Slack will review it).
6. Copy the **Client ID** into a GitHub variable `RATA_SLACK_CLIENT_ID`.
   Do not copy the client secret anywhere.

Refresh tokens of a PKCE app expire after 30 days, so a customer who has
not shared anything for a month signs in to Slack again.

Share to Slack is in builds made after `RATA_SLACK_CLIENT_ID` is set; a
build without it shows no Slack anywhere, as before.

**How to tell it worked:** in a build made after the variable was set,
Connect on the Slack row opens Slack's page, and Share to Slack on a
message lists the workspace's
channels.

## Google Drive (K4)

RATA asks only for `drive.file`: it sees the files the customer picks in
Google's own chooser (which opens in the browser) and the files RATA saves.
That scope needs no security assessment.

1. [console.cloud.google.com](https://console.cloud.google.com) → create a
   project "RATA" → APIs & Services → enable the **Google Drive API** and
   the **Google Picker API**.
2. **OAuth consent screen** (Google Auth Platform → Branding): app name
   RATA, support email, the logo, `mailrata.org` as the authorised domain,
   links to `https://mailrata.org/privacy` and `/terms`. Audience:
   External. Data access: add `https://www.googleapis.com/auth/drive.file`
   only.
3. Clients → Create client → **Desktop app**. Google treats a desktop
   client's secret as not secret, so both values go in GitHub
   **Variables**: `RATA_GOOGLE_CLIENT_ID` and `RATA_GOOGLE_CLIENT_SECRET`.
4. Publish the app (Audience → Publish) and ask for **brand
   verification**; it is often automatic, otherwise a few business days.
   In Testing status only listed test users can sign in and their tokens
   expire after 7 days.
   [Desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app),
   [Picker for desktop apps](https://developers.google.com/workspace/drive/picker/guides/overview-desktop).

Do not add `drive`, `drive.readonly` or Gmail's `https://mail.google.com/`:
those are restricted scopes that need a yearly third-party security
assessment (CASA), which outside sources price at roughly $500 to $4,500 a
year. Gmail keeps working with app passwords.

**How to tell it worked:** once K4 ships, Connect on the Google Drive row
opens Google's chooser in the browser, and the files picked there show in
Files.

## Not planned, and why

- **Apple iCloud Drive and Adobe, as accounts:** neither has a public API
  for a person's files. RATA already uses the folders on the computer (K5).
- **Box:** its sign-in appears to need a client secret, which would mean a
  server holding one. Box Drive's folder on the computer is the way in.
- **Dropbox:** possible (it supports PKCE), not requested yet.
