# RATA — Production Deploy Guide

RATA ships as a blank-slate product: no demo data, no guest door. Visitors land on
the landing page, create a real account, and arrive in an empty inbox that fills
only when they connect their own accounts. Installs as an app on Windows, Android,
iOS/iPadOS, and macOS straight from your domain (PWA — no app store needed).

---

## Before you start: pick the right Hostinger plan

**RATA is a Next.js application, not a static site.** The landing page, auth page,
and console are static files, but account linking and inbox sync run in server-side
API routes (`/api/link/*`, `/api/sync/*`). Those routes need a Node process.

| You have | What happens |
|---|---|
| **Business Web Hosting**, or any **Cloud** plan (Startup / Professional / Enterprise) | ✅ Node.js Web Apps available — deploy as below, everything works |
| **VPS** | ✅ Works — run `npm ci && npm run build && npm start` behind your own reverse proxy |
| Premium / entry shared hosting (`public_html` file manager only) | ❌ No Node runtime. Pages load, **every live integration silently fails** |

> **Do not upload the project as a zip into `public_html`.** Files dropped there are
> served as static assets — the API routes never execute, so Outlook, Slack, and
> iCloud linking return "backend missing" and no inbox ever syncs. Earlier drafts of
> this guide described that path; it was wrong. Use the Node.js Web App flow below.
>
> Verify with `npm run build`: every `/api/*` row prints `ƒ (Dynamic) server-rendered
> on demand`. Dynamic routes require a running server, by definition.

Supported Node.js versions on Hostinger: 18.x, 20.x, 22.x, 24.x. RATA needs **20 or
newer** (`imapflow` and the Supabase SDK both require it).

---

## The launch checklist (≈25 min total)

### 1. Database — Supabase, free (10 min) ← do this first
This is where every registered account and workspace lives.

1. supabase.com → New project → copy **Project URL** + **anon public key**
   (Settings → API).
2. SQL editor → paste the whole of **`database.sql`** from this project → Run.
   That creates `workspaces`, `subscriptions`, `provider_tokens`, and `link_states`
   with row-level security on all four. It is safe to re-run.
3. Settings → API → also copy the **`service_role`** key. You need it in step 3.
   It bypasses RLS — treat it like a root password. It goes in
   `SUPABASE_SERVICE_ROLE_KEY` and nowhere else; it must never end up in
   `SUPABASE_ANON_KEY`, which is published to every visitor.
4. Auth → URL Configuration → add `https://yourdomain.com`.
   (While testing you can disable email confirmation: Auth → Providers → Email.)

### 2. Nothing to edit
There is no config file to hand-edit. `/config.js` is generated per request from
environment variables (see the table in step 3) and served with `no-store`, so a
changed value takes effect on the next page load. Set them once in hPanel and
they apply to every visitor.

Everything in that response ships to the browser, so only publishable values are
read: the Supabase URL and **anon** key (which row-level security is designed to
expose), the Google client ID, and Stripe payment links. `SUPABASE_SERVICE_ROLE_KEY`
is never read there. If an anon key is ever mistaken for the service_role key, the
route refuses to publish it and logs why rather than leaking it.

### 3. Deploy as a Node.js Web App (8 min)
hPanel → **Websites → Add Website → Deploy Web App**. Three ways in — pick one:

- **Git** — authorize GitHub, select the repo. Rebuilds on every push.
- **Zip upload** — compress the project (excluding `node_modules/` and `.next/`).
- **Hostinger Connector** — deploy from VS Code / Cursor / Windsurf.

Confirm the detected framework is **Next.js**, and that the build settings are
`npm run build` / `npm start`. The stock `next.config.mjs` needs no changes.

Then set the environment variables (hPanel → your app → Environment) **before the
first deploy that needs them**, and redeploy after any change:

| Variable | Value | Needed for | Public? |
|---|---|---|---|
| `APP_URL` | `https://yourdomain.com` (no trailing slash) | linking | server |
| `SUPABASE_URL` | Supabase → Settings → API → Project URL | accounts, linking | **shipped to browser** |
| `SUPABASE_ANON_KEY` | Settings → API → **anon** key | accounts | **shipped to browser** |
| `SUPABASE_SERVICE_ROLE_KEY` | Settings → API → **service_role** (SECRET) | linking | server only |
| `GOOGLE_CLIENT_ID` | Google Cloud OAuth client | Gmail | **shipped to browser** |
| `MS_CLIENT_ID` / `MS_CLIENT_SECRET` | see BACKEND-SETUP.md | Outlook | server only |
| `SLACK_CLIENT_ID` / `SLACK_CLIENT_SECRET` | see BACKEND-SETUP.md | Slack | server only |
| `STRIPE_MONTHLY` / `STRIPE_ANNUAL` / `STRIPE_PORTAL` | see STRIPE-SETUP.md | billing | **shipped to browser** |

**Every one of these is optional.** With none set, RATA deploys and runs on
on-device accounts — inbox, People, Documents, DLP, audit chain and the Format
Bridge all work; what you lose is cross-device sync and live provider linking.
Add Supabase later by setting the variables and redeploying; nothing needs
rebuilding or rewriting.

Finally, confirm SSL is on (Security → SSL). HTTPS is required for accounts, PWA
installs, and Google OAuth.

### 4. Smoke test (5 min)
- Visit your domain → Create account → you land in the Messages inbox: **empty**. ✓
- Auth page shows "Cloud accounts active". ✓
- Supabase → Authentication → Users: your new account is registered. ✓
- Add a person in People, send them a message from their thread → workspace row
  appears in the `workspaces` table. ✓
- Sign in from a second device → same workspace follows you. ✓
- Open `https://yourdomain.com/api/sync/ms` directly in a browser tab. You should
  get JSON (`{"error":"Not signed in"}` is the correct answer here) — **not** the
  RATA landing page and not a 404. Either of those means the deploy is being served
  statically and no integration will work. ✓

### 5. Install the apps
| Platform | How |
|---|---|
| **Windows / macOS** | Chrome or Edge → install icon in the address bar (or the ⬇ Install button). Own window, Start-menu/dock entry. |
| **Android** | Chrome prompts to Install (or menu → *Install app*). Real app icon + splash. |
| **iPhone / iPad** | **Safari** → Share → **Add to Home Screen**. Runs full-screen standalone. |

Store listings later: point **pwabuilder.com** at your URL for Microsoft Store &
Google Play packages; the iOS App Store needs a thin Capacitor wrapper. Same code.

---

## Optional integrations (any time)

- **Outlook + Slack live sync** — server-side OAuth. Follow **BACKEND-SETUP.md**.
- **iCloud Mail** — no setup on your side; each user adds their own iCloud address
  and an app-specific password in Settings.
- **Real Gmail inbox** — Google Cloud → OAuth client ID (Web) with your domain as an
  authorized origin → set as `GOOGLE_CLIENT_ID`. Add BOTH scopes on the consent screen:
  `gmail.readonly` and `gmail.send`. Users then link Google accounts in Settings —
  each account really syncs its inbox AND really sends email from compose. This one
  runs entirely in the browser; tokens live in memory only, never stored.
- **AI briefings** — each user pastes their own Anthropic key in Settings.
  (Personal secret — deliberately never in the site configuration.)
- **Payments** — free for now. When ready to charge $8/mo · $79/yr, follow
  **STRIPE-SETUP.md**: create the payment links, set them as `STRIPE_MONTHLY`,
  `STRIPE_ANNUAL` and `STRIPE_PORTAL`, and
  (recommended) deploy the included webhook for verified entitlements. The pricing
  page, billing settings, and plan system are already live and waiting.

## What's real vs. pending backend
| Feature | Status |
|---|---|
| Accounts registered to your database, cross-device workspaces | **Real** (Supabase) |
| Blank-slate inboxes, People, Documents, DLP, audit chain, Assist | **Real**, in-app |
| Outlook, Slack, iCloud feeds | **Real** on a Node.js Web App deploy (see BACKEND-SETUP.md) |
| Gmail feed | **Real** with a Google client ID — browser-side, no server needed |
| Stripe checkout, portal, entitlements | **Ready** — activates when links added |
| Discord / SMS live feeds | Pending. Discord's API does not permit reading user DMs via OAuth; SMS needs a telephony provider. UI and threading are built and waiting. |

## The Format Bridge — two files, not one

Converting a document keeps the original and produces the new one as a second
file. Both land in Documents, linked as a pair ("⇄ translated from …"), and both
can be downloaded back byte-for-byte. Nothing is overwritten.

Where the two halves live is worth knowing, because it decides what follows a
user between devices:

| | Stored in | Syncs across devices |
|---|---|---|
| Names, formats, sizes, extracted text, the pair link | the workspace (Supabase) | **yes** — and stays searchable |
| The actual file bytes | IndexedDB on the device | **no** |

That split is deliberate. The workspace is written to `localStorage` on every
change and upserted to Supabase as one JSON blob about 1.5s later; file bytes in
there would exhaust the ~5MB browser quota after a couple of documents and
re-upload every byte on every edit. So the light half travels and the heavy half
stays put. Open a pair on a second device and you see both entries and their
text; the ⤓ File button tells you the bytes live on the device that made them.

Every conversion engine (Word, Excel, PDF) is served from your own domain and
precached by the service worker at install, so a conversion never downloads
anything — it works with the network off. Nothing is fetched from a third-party
CDN at runtime. See `scripts/vendor-libs.mjs` to refresh them.

## Updating the site
Push to the connected branch (Git deploys), or re-upload and redeploy. Pages and
`/config.js` load network-first, so visitors and installed apps pick up changes on
next open. Configuration changes need only a redeploy, not a rebuild of the pages.
