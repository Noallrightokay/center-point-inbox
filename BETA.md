# RATA beta

Thanks for testing. This page is everything you need: how to install, what to
expect, and — most importantly — how to report what breaks so it can be fixed
the same day.

**The single most useful thing you can do is connect a real mailbox.** No
mailbox has ever been opened by this code outside a test. Whatever happens when
you try — success or an error — is the most valuable data this project has.

---

## 1. Install

Download from the latest release:
**https://github.com/Noallrightokay/center-point-inbox/releases**

| Platform | File | First launch |
|---|---|---|
| Windows | `RATA_<version>_x64-setup.exe` | "Windows protected your PC" → **More info** → **Run anyway** |
| macOS, Apple silicon | `RATA_<version>_aarch64.dmg` | Right-click the app → **Open** → **Open** |
| macOS, Intel | `RATA_<version>_x64.dmg` | Right-click the app → **Open** → **Open** |
| Linux | `.deb` (or `.AppImage` from the run's artifacts) | `sudo apt install ./RATA_<version>_amd64.deb` |

The warnings are because the installers are not code-signed yet. That is
expected for the beta.

**Check your version:** it is in the window's title bar — `RATA 0.1.6 beta`.
Put it in every bug report.

## 2. Licence key

You need your own key, tied to your email address. Ask the project owner for
one; paste it into the box RATA shows on first launch. One line, no spaces.

*(Owner: mint one per tester on the VPS —
`/etc/rata/mint.sh their@email.com pro 30` — and send it privately. Keys are
signed, not secret-encrypted, but they are still a year's access if you mint
them for a year; 30 days is plenty for a beta.)*

## 3. Add a mailbox — use an app password

Your normal account password will be refused. Mail providers require an
**app password** for mail apps:

| Provider | Where |
|---|---|
| Gmail | https://myaccount.google.com/apppasswords (needs 2-Step Verification on) |
| Outlook / Hotmail | https://account.live.com/proofs/AppPassword |
| iCloud | https://account.apple.com → Sign-In and Security → App-Specific Passwords |
| Yahoo | Account Security → Generate app password |
| Fastmail | Settings → Privacy & Security → App passwords |

Enter your email address and the app password. RATA works out the server
itself. Only if it asks, enter your provider's IMAP server name
(e.g. `imap.example.com` — a pasted `imaps://…:993` is fine too).

## 4. Two checks worth a minute

These verify the product's whole promise — that your password never leaves
your computer.

- **The password is in your OS keychain.** Windows: Credential Manager →
  Windows Credentials → look for `org.mailrata.desktop`. macOS: Keychain Access,
  search `org.mailrata.desktop`. Linux: Seahorse / KWallet.
- **The password is NOT in RATA's own file.** Open `mailboxes.json`:
  - Windows: `%APPDATA%\org.mailrata.desktop\`
  - macOS: `~/Library/Application Support/org.mailrata.desktop/`
  - Linux: `~/.local/share/org.mailrata.desktop/`

  It should list your address and server — and no password. Search it for part
  of your app password; zero matches is correct. **If you find it, report that
  first — it is the most serious bug possible here.**

And one more: **Remove** a mailbox in Accounts, then check the keychain entry is
gone too.

## 5. What works, and what doesn't yet

Don't spend time reporting these — they are known and planned:

- Older mail comes in **50 at a time**: a new mailbox arrives with its newest
  ~65, and **Load older mail** at the bottom of the list walks further back.
  RATA stops loading older mail once its storage on your computer is nearly
  full (a few MB — somewhere between one and three thousand messages,
  depending on how long they are) and says so; a
  larger store is coming.
- No other folders yet — only the inbox is shown.
- **HTML mail is shown formatted from v0.1.12** — layout, colours, tables and
  the pictures carried inside the message. Pictures from the internet stay off
  until you click **Load images** (loading them tells the sender you opened
  it). Links in the formatted view do not open; switch to **Text** to see each
  link's address and copy it. Report any message that looks broken
  formatted — with its sender — as well as any that shows raw code. From v0.1.8, opening a long
  message loads all of it, and **attachments** are listed on the message —
  click one to save it to your Downloads folder (RATA never opens it for you).
  From v0.1.9 you can **attach files** when writing, with 📎 Attach — up to
  18 MB in all, since most providers refuse mail over 25 MB once encoded.
  From v0.1.11 **Forward** on a message sends its attachments along too.
  **Please report any message that still shows raw code** — lines
  like `--000abc`, `Content-Type:` or `=20` — with its sender (a newsletter
  name is fine); that is a decoding bug.
- Mail that arrived under an older beta is re-read and cleaned up after the
  first refresh on v0.1.7, 50 messages per mailbox at a time, and any one you
  open is fixed at once.
- No auto-update — new betas are a new download.

What *should* work: adding a mailbox, seeing recent mail from several
mailboxes, sending, **replying** (the Reply button on a message, from v0.1.6 —
check that it lands in the same thread at the other end), removing a mailbox,
the licence — and,
from v0.1.4, **read, unread, star, delete and archive reach your real
mailbox**. Delete moves the message to your provider's Trash (never deletes it
for good); Archive moves it to your Archive folder, or Gmail's All Mail. Please
check the result in your provider's own webmail or phone app — that is the
real test. Mail that was already in RATA before v0.1.4 is changed in RATA only
until the next refresh picks up its server reference.

## 6. Reporting a bug

Open an issue with the **Beta bug** template:
**https://github.com/Noallrightokay/center-point-inbox/issues/new?template=beta-bug.md**

⚠️ This repository is **public**. Never paste an app password or a licence key.
Your own email address is fine to include if you are comfortable with that; if
not, replace it with `me@<provider>` — the provider is the useful part.

What makes a report fixable in minutes rather than days:

1. **The exact error text**, copied, not paraphrased. RATA's errors are written
   to be diagnostic — they name the servers it tried and what each one said.
2. **Your provider** (Gmail, Outlook, a work domain on Google Workspace…).
3. **Version** from the title bar, and your OS.
4. **What you did, what you expected, what happened.**
5. A screenshot if the problem is something you see rather than an error.

---

## For testers who want to fix things too

Start with `README.md` (layout, build, the traps) and `CLAUDE.md` (current
state and the rules of the road). Short version:

```bash
# mail engine — no network needed
cd desktop/rata-mail && cargo test

# desktop app — sync-ui.sh FIRST, every time
cd desktop/rata-app && ./sync-ui.sh && cd src-tauri && cargo test
cargo tauri dev          # run it (from desktop/rata-app)

# website
cd rata-next && npm ci && npm test
```

Rust 1.94.1. On Linux you also need
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf`.

One rule that is not negotiable: **mail and mail passwords never go to a
server.** If a change would put them there, it is wrong however convenient.

When a real mailbox misbehaves, the three files to read first are
`desktop/rata-mail/src/resolve.rs` (finding the server), `imap.rs` (sign-in and
fetch) and `guard.rs` (hosts RATA refuses to connect to).

Work on a branch, open a PR against `main`. CI runs the Rust and website suites.
