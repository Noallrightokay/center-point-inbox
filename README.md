# RATA

Your mail, on your machine.

RATA links the mailboxes you already have — Gmail, Fastmail, a work IMAP server —
and reads them on your own computer. The mail password is kept in the operating
system's keychain and never leaves the device. There is no server holding
anybody's mail, because there is no server in the path at all.

That last sentence is the product. It is also why this repository looks the way
it does: the mail code is a Rust library that runs on the customer's machine,
and the website exists only to sell licences.

---

## How mail actually flows

```
   ┌──────────────────────┐                    ┌─────────────────┐
   │  RATA on your PC     │  IMAP 993 ────────▶│  imap.gmail.com │
   │                      │  SMTP 465/587 ────▶│  (or whoever)   │
   │  password in the     │                    └─────────────────┘
   │  OS keychain         │
   └──────────┬───────────┘
              │
              │  licence check only — a signed token, verified offline.
              │  Contacted roughly monthly to renew. Never sees mail.
              ▼
      ┌────────────────┐
      │  mailrata.org  │
      └────────────────┘
```

The website cannot read your mail even if it wanted to. It has no credentials,
no connection, and no code for it.

---

## Repository layout

| Path | What it is |
|---|---|
| `desktop/rata-mail/` | The mail engine. A standalone Rust library: DNS discovery, IMAP, SMTP, and the outbound guard. Knows nothing about the app. |
| `desktop/rata-app/` | The desktop application. Tauri shell, keychain, licence verification, and the glue to the interface. |
| `rata-next/` | The website — marketing pages, Stripe checkout, licence issuing and renewal. Next.js on a Hostinger VPS. |
| `rata-next/public/app.html` | The interface. One copy, shared: the desktop app builds its own from this file. |
| `infrastructure/backup/` | Postgres backups for the VPS, with a self-test. |
| `docs/hostinger-mcp.md` | Managing the VPS and DNS from the repo. |

There is no `src/`. An earlier version of this product was nine .NET
microservices on Kubernetes; it was abandoned and the code removed in favour of
the two halves above. If you find a reference to `centerpoint-inbox.com`,
an API gateway, or a translation worker, it is a ghost — report it.

---

## Building and testing

### The mail engine

```bash
cd desktop/rata-mail
cargo test          # 138 tests, no network required
cargo clippy --all-targets -- -D warnings
```

Toolchain is pinned to **1.94.1** in CI; anything from that release on works.

The tests never open a socket. Protocol handling is tested against recorded
server dialogue, which means the suite is fast and honest about what it proves —
and about what it does not. See *What has never been tested* below.

### The desktop app

```bash
cd desktop/rata-app
./sync-ui.sh        # MUST run first — see the trap below
cd src-tauri
cargo test          # 44 tests
```

On Linux you need the system webview first:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf
```

Building installers, and where things land on a customer's machine, are covered
in **[`desktop/rata-app/README.md`](desktop/rata-app/README.md)** — read that
before packaging anything.

### The website

```bash
cd rata-next
npm ci
npm test            # no network, no database required
npm run dev
```

---

## Traps

Every one of these cost someone a day. They are here so it is not you.

**`sync-ui.sh` must run before `cargo build`, every time.**
Tauri compiles the web assets *into* the binary at build time. Editing
`app.html` and rebuilding does nothing until you re-run `sync-ui.sh`, and the
app will keep showing the previous interface with no warning. If a change
refuses to appear, this is why.

**There is only one copy of the interface, and it lives in `rata-next/public`.**
`sync-ui.sh` copies it into `desktop/rata-app/ui/` and applies the desktop
differences loudly, in one script you can read. Do not create a second
`app.html`. The generated `ui/` directory is never committed.

**Inline `style="..."` attributes used to be silently dropped in the shipped
app.** Tauri adds a nonce to `style-src`, and per CSP a nonce makes
`'unsafe-inline'` ignored — even though the config asks for it. It never
reproduced in `tauri dev`, only in the packaged build, and it shipped in v0.1.2
as a first screen with two forms on it. `tauri.conf.json` now sets
`dangerousDisableAssetCspModification: ["style-src"]` so the declared policy is
the one that applies; scripts keep Tauri's protection. Leave it there, and check
interface changes in a packaged build.

**`bridge.js` replaces the browser's `fetch`.**
The interface was written to call a server. Rather than rewrite it, the bridge
intercepts `fetch` and routes those calls to Rust instead. It is the single
most surprising file in the repository and the first one to read. A call it
does not implement is refused with a message rather than failing silently.

**The interface is email only.** Slack, Discord, texts, Google and Microsoft
sign-in, translation and the AI brief were switches with nothing behind them,
and translation and the AI brief sent the text of your mail to Google or
Anthropic. They were removed in v0.1.6, together with the screens that held
them; a workspace saved by an older build is tidied on load (`tidyV6`). Do not
bring any of them back as a control that leads nowhere.

**The licence public key is compiled in at build time.**
The build reads `RATA_LICENCE_PUBLIC_KEY`. A binary built without it cannot
verify any licence, and a binary built with the wrong one rejects every
legitimate licence with a signature error. The private half lives only on the
VPS at `/etc/rata/licence.key` and is not in this repository.

**An unset GitHub secret is an empty string, not absent.**
`std::env::var` returns `Ok("")`, which Tauri reads as "yes, sign this build",
and macOS codesigning then fails confusingly. The release workflow has an
explicit opt-in step for the Apple variables because of this.

**`tauri.conf.json` builds `nsis`, not `msi`.**
The Windows installer is `RATA_0.1.0_x64-setup.exe`. Anything promising a
`.msi` is wrong.

---

## How licensing works

A licence is a signed token, not a subscription check:

```
v1.<base64url payload>.<base64url Ed25519 signature>
    {"v":1,"sub":"you@example.com","plan":"pro","iat":…,"exp":…}
```

The app verifies the signature against the compiled-in public key and reads the
expiry. **No network involved** — RATA works on a plane. The token is signed,
not encrypted, and is meant to be readable.

Tokens are issued for **30 days** (`LICENCE_DAYS` in `rata-next/lib/licence.js`)
and the app renews itself by presenting the old token to `/api/licence/renew`
once it is inside its final week. Thirty days covers a holiday or a dead
laptop without anyone noticing. The trade is that a cancelled subscription
stops working at the next renewal rather than instantly — accepted deliberately,
because the alternative is phoning home.

Releases are built by the **Release installers** workflow, which produces a
`.deb` and `.AppImage` for Linux, a `.dmg` for each Mac architecture, and an
NSIS `.exe` for Windows. They are **unsigned**, so Windows shows a SmartScreen
warning and macOS requires right-click → Open.

---

## Status: what works and what does not

Be realistic about this before promising anything to a customer.

**Works, and is tested:**

- Licence verification, including offline and self-renewal
- Adding a mailbox by address and app password
- Server discovery — asks the domain's DNS (SRV, then MX, then conventional
  names), which is what makes `you@yourcompany.com` work when it is really Google
- Fetching the newest messages from INBOX, several mailboxes at once
- Replying from the message itself, threaded with `In-Reply-To` and sent to
  the sender's Reply-To address when they gave one
- A guard that stops a hostile mail server redirecting the app at your own LAN

**Not built yet:**

- **Mail is stored in one blob with a cap.** The interface keeps everything
  it has fetched, so mail persists between launches and is searchable;
  read/unread, star, delete (to the server's Trash) and archive reach the real
  mailbox; and older mail loads 50 at a time on request. But everything sits
  in one `localStorage` blob with a cap of a few MB, so loading older mail
  stops near it. A per-message store is the largest gap.
- **INBOX only.** No other folders are shown.
- **Mail is shown as text.** Each message's first 64 KB is fetched and decoded
  — MIME, quoted-printable, base64, any charset — and an HTML-only message is
  turned into text with its links' addresses kept (`body.rs`, `html.rs`).
  Opening a long message, or one with attachments, fetches all of it; an
  attachment is saved to Downloads under a cleaned-up name and never opened by
  RATA. Files can be attached when sending, up to 18 MB in all. There is no
  HTML rendering.
- No auto-update, and no code signing.

### What has never been tested

**No real mailbox has ever been opened by this code.** The test suite covers
every path up to the socket and stops there. IMAP and SMTP handshakes, the
authentication classification that decides "wrong password" versus "server
unreachable", and the discovery chain against real DNS have never run against a
live provider.

If you are picking this up, that is the first thing to do, and the three files
worth reading first when it misbehaves are `resolve.rs` (finding the server),
`imap.rs` (handshake and authentication), and `guard.rs` (host refusal).
Errors are typed rather than stringly, so the message you get back names the
hosts it tried and what each one said.
