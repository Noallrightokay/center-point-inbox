# RATA beta

Thanks for testing. This page is everything you need: how to install, what to
expect, and, most importantly, how to report what breaks so it can be fixed
the same day.

**Help page:** https://mailrata.org/help has the install steps, each
provider's own app-password page, what every message RATA shows when a link
fails means, and how to report a bug.

**The single most useful thing you can do is connect a real mailbox.** No
customer's mailbox has been opened by this code yet: it is tested against
real mail servers set up for testing, but not against Gmail, Outlook or any
other provider. Whatever happens when you try, success or an error, is the
most valuable data this project has.

---

## 1. Install

Download from the latest release:
**https://github.com/Noallrightokay/center-point-inbox/releases**

| Platform | File | First launch |
|---|---|---|
| Windows | `RATA_<version>_x64-setup.exe` | "Windows protected your PC" → **More info** → **Run anyway** |
| macOS, Apple silicon | `RATA_<version>_aarch64.dmg` | Open it once and choose **Done**, then System Settings → **Privacy & Security** → **Open Anyway** |
| macOS, Intel | `RATA_<version>_x64.dmg` | Open it once and choose **Done**, then System Settings → **Privacy & Security** → **Open Anyway** |
| Linux | `RATA_<version>_amd64.deb`, or `RATA_<version>_amd64.AppImage` for other distributions | `sudo apt install ./RATA_<version>_amd64.deb`, or make the `.AppImage` executable (`chmod +x`) and run it |

The warnings are because the installers are not code-signed yet. That is
expected for the beta. On macOS 15, right-click → **Open** no longer gets past
the warning; System Settings → **Privacy & Security** → **Open Anyway** does.
If macOS says instead that **RATA is damaged and can't be opened**, there is no
Open Anyway: run `xattr -dr com.apple.quarantine /Applications/RATA.app` in
Terminal, open RATA again, and tell us which of the two messages you saw.

**Check your version:** it is in the window's title bar, as `RATA <version> beta`.
Put it in every bug report.

## 2. Licence key

You need your own key, tied to your email address. Ask the project owner for
one; paste it into the box RATA shows on first launch. One line, no spaces.

*(Owner: mint one per tester on the VPS
with `infrastructure/licence/mint.sh` from this repository (`sudo ./mint.sh check` first, then
`sudo ./mint.sh their@email.com pro 30`) and send it privately. Keys are
signed, not secret-encrypted, but they are still a year's access if you mint
them for a year; 30 days is plenty for a beta.)*

## 3. Add a mailbox with an app password

Your normal account password will be refused. Mail providers require an
**app password** for mail apps:

| Provider | Where |
|---|---|
| Gmail | https://myaccount.google.com/apppasswords (needs 2-Step Verification on) |
| iCloud | https://account.apple.com → Sign-In and Security → App-Specific Passwords |
| Yahoo | Account Security → Generate app password |
| Fastmail | Settings → Privacy & Security → App passwords |
| Hostinger (your own domain) | hPanel → Emails → the mailbox's own password. Hostinger has no app passwords. |

Outlook, Hotmail, Live and Microsoft 365 mailboxes do not use a password at
all. Microsoft accepts only its own sign-in (OAuth) for mail apps now, so
from v0.1.39 RATA shows **Sign in with Microsoft** instead of the password
box: it opens Microsoft's page in your browser, and your password goes only
to Microsoft. That button appears only once RATA's Microsoft registration
is in the build; until then the **＋ Email account** form (Settings →
Linked accounts) says these mailboxes cannot be added yet. A work account may need its IT administrator to allow
RATA first.

Enter your email address and the app password. RATA works out the server
itself. Only if it asks, enter your provider's IMAP server name
(e.g. `imap.example.com`; a pasted `imaps://…:993` is fine too).

## 4. Two checks worth a minute

These verify the product's whole promise: that your password never leaves
your computer.

- **The password is in your OS keychain.** Windows: Credential Manager →
  Windows Credentials → look for `org.mailrata.desktop`. macOS: Keychain Access,
  search `org.mailrata.desktop`. Linux: Seahorse / KWallet. A Microsoft
  mailbox's entry holds a sign-in token that begins `rata-oauth2:`, not a
  password, and a long token (usual for work accounts) also makes entries
  under `org.mailrata.desktop.oauth-piece`.
- **The password is NOT in RATA's own file.** Open `mailboxes.json`:
  - Windows: `%APPDATA%\org.mailrata.desktop\`
  - macOS: `~/Library/Application Support/org.mailrata.desktop/`
  - Linux: `~/.local/share/org.mailrata.desktop/`

  It should list your address and server, and no password (a Microsoft
  mailbox shows `"auth": "oauth"` and no token). Search it for part of your
  app password; zero matches is correct. **If you find it, report that
  first: it is the most serious bug possible here.** This file also holds
  your licence key, so never paste it, attach it or screenshot it anywhere.

And one more: in **Settings → Linked accounts**, press **Remove** on a
mailbox, then check its keychain entry is gone too (for a Microsoft mailbox,
the `oauth-piece` entries as well).

## 5. What works, and what doesn't yet

Don't spend time reporting these; they are known and planned:

- **Microsoft mailboxes** (Outlook.com, Hotmail, Live, Microsoft 365) can
  be added only in a build that has RATA's Microsoft registration; in one
  without it, the **＋ Email account** form says they cannot be added yet. Until a release
  note says Sign in with Microsoft is switched on, that message is expected.
- **v0.1.39:** Microsoft mailboxes get **Sign in with Microsoft** (above),
  and if one later needs you again its row in Settings says **Sign in to
  Microsoft again**. A message RATA cannot read no longer stops a refresh:
  the rest of your mail arrives, and that one is listed with a sentence
  saying RATA could not read it; opening it asks for it again. **Archive**
  now works on providers whose Archive folder is only named "Archive"
  (before, RATA showed the folder but would not archive into it); on Gmail,
  a label called exactly "Archive" is now where Archive files mail. When
  you type your server's address yourself and it fails, RATA now says why,
  for example that the server's certificate could not be trusted. Copy any
  such sentence exactly into your report, with the provider.
- **v0.1.40:** attachments you save and files you convert are marked as
  downloaded from the internet, the way a browser marks them, so Windows
  and macOS may warn before opening one; that is intended. A program named
  to look like a document (like `invoice.pdf.exe`) is labelled, and RATA
  asks before saving it. The website's app page now says that mail is read
  in the desktop app.
- **v0.1.41:** in the app, a draft is saved to your mailbox's Drafts folder
  when you close the composer and every two minutes while you write, so you
  can finish it on your phone; if your mailbox has no Drafts folder it stays
  in RATA. Saving again replaces RATA's own earlier copy, never anyone
  else's. A half-typed address (in To, Cc or Bcc) stops the save: closing
  the composer then says "Not saved to Drafts" with the reason, and the
  draft is kept in RATA (**Compose** opens it again) until you fix it. On
  Gmail, after saving a draft twice, look in **All Mail**: if old copies of
  it pile up there (or show under Archive in RATA), tell us. Once
  mailrata.org runs its new version, a licence that expired more than 90
  days ago says to sign in there for a new one instead of trying to renew.
- **v0.1.42:** on Gmail, each time RATA saves a draft again its older copy
  now goes to Gmail's **Trash** rather than staying in **All Mail**; the
  All Mail check above still applies (one copy is right). A program named
  to look like a document is now also caught when an invisible character
  or a dot-like character hides its real extension, and names ending in a
  document type such as `.csv`, `.rtf` or `.html` followed by `.iso`,
  `.url`, `.reg`, `.cpl` or `.one` are asked about too. When a server
  refuses your password, its message no longer repeats the password.
  Once mailrata.org runs its new version: a Pro purchase is recorded as
  Pro (it had been recorded as Base), your account page may say "not yet"
  for a short while after paying, and deleting your account waits 30 days
  after a subscription ends (the page gives the date).

- Older mail comes in **50 at a time**: a new mailbox arrives with its newest
  ~65, and **Load older mail** at the bottom of the list walks further back.
  From v0.1.13 there is no storage limit to run into: mail is kept one
  message at a time, with room for as much as your disk holds, and from
  v0.1.16 the list stays quick however long it is. From v0.1.21 the text of
  your messages stays on disk until you open one, so RATA uses far less
  memory with a big mailbox; the first start after updating to v0.1.21
  takes a few seconds longer while it reorganises what it has kept, once.
  Searching a very big mailbox takes a moment (about half a second at ten
  thousand messages). Tell us when anything starts to feel slow, and at
  roughly how many messages.
- From v0.1.22 RATA shows your **Sent** mail too, including what you sent
  from your phone or webmail, in the Sent filter and in each person's
  thread in People. From v0.1.25 **Archive** and **Spam** have filters of
  their own (Gmail's archive from v0.1.38); a message in Spam can be marked **Not spam**. From v0.1.26 **Folders**
  (in the row of filters) lists your own folders (Gmail labels too) and
  opens one, and **Move to** on a message files it into one. From v0.1.30
  **Select** several messages to **Archive**, **Move to inbox** or **Move
  to** a folder in one go. From v0.1.31 **Drafts** has a filter too: a
  draft begun on your phone or in webmail opens with **Continue**, which
  puts it in RATA's composer (everyone it was for, the subject, the words, its attachments
  and the thread it answers), and once you send it, the copy left in Drafts
  goes to your Trash. A formatted draft continues as plain text. From
  v0.1.41 a draft you write in RATA is saved to Drafts too (above);
  **Discard** moves RATA's copy to the Trash. From v0.1.32 the composer
  has a **Cc** line (and from v0.1.33 **Bcc**, which the other recipients
  don't see), and a message that went to several people offers
  **Reply all** (not for mail RATA fetched before v0.1.31). If a reply to
  everyone goes to someone it should not, tell us who, and how they were
  on the original. If real mail turns up in Spam, or a folder is
  missing, tell us which provider. If something you sent from RATA shows up twice in Sent, tell
  us which provider it was.
- **With more than one mailbox linked, before v0.1.26 a reply went out from
  the first one**, not the mailbox the message came to, and each
  mailbox's own filter and Side by side column showed nothing. If you
  replied to mail in another mailbox on an earlier version, that reply is
  in the first mailbox's Sent. Fixed in v0.1.26; tell us if a reply still
  goes from the wrong address.
- **Converting files (Files → Format Bridge), rebuilt in v0.1.18:** Word
  output is now a real .docx that Word, Google Docs and Pages open without
  complaint; headings, bold and italic, lists and tables survive; every
  sheet of a spreadsheet is kept. In the app, the converted file is saved
  to your Downloads folder (before v0.1.18 it silently went nowhere). From
  v0.1.19 it reads **PDFs** too: the text, headings, lists and simple
  tables come across, rebuilt from the page; the layout and pictures don't,
  and scanned PDFs (pictures of pages) can't be read yet. It can't read
  old .doc files. From v0.1.20 an attachment RATA can read has a
  **⇄ Convert** button beside it: one click puts it in the Format Bridge,
  ready to turn into another format, without saving it first. Send us the
  files that come out wrong.
- **AI, from v0.1.17:** **✦ Summarize** and **⇄ Translate** on a message, and
  **Run briefing** in Assist, which flags the mail that needs you to do
  something (⚑ in the list). The first time, RATA tells you what it sends:
  the text of that message (or the start of your recent mail, for the
  briefing) goes to RATA's AI service, run by Anthropic, and is not kept.
  Summaries and flags are Pro; translation is in every plan. These only work
  once mailrata.org is updated; until then they say the AI service could
  not be reached, and Summarize and the briefing fall back to a quick version
  made on your computer.
- **HTML mail is shown formatted from v0.1.12**: layout, colours, tables and
  the pictures carried inside the message. Pictures from the internet stay off
  until you click **Load images** (loading them tells the sender you opened
  it). **From v0.1.14 links open in your browser.** In a formatted message
  RATA first asks, showing the site the link really goes to (the words of a
  link can say anything, so check it is the site you expected). In the
  **Text** view each address is shown in full and opens straight away. A
  link to an email address opens RATA's own composer. Report any link that
  does nothing, or opens the wrong thing. From v0.1.15, closing the composer
  keeps the whole draft (a forward keeps its files, a reply its thread), and
  **Discard** at the top starts a blank message. Report any message that looks broken
  formatted, with its sender, as well as any that shows raw code. From v0.1.8, opening a long
  message loads all of it, and **attachments** are listed on the message:
  click one to save it to your Downloads folder (RATA never opens it for you).
  From v0.1.40, on Windows a saved attachment or converted file may say it
  came from the internet (SmartScreen, or Office's Protected View), and on a
  Mac Gatekeeper may ask before opening it. That is intended: RATA marks what
  it saves the way a browser does. A program named to look like a document
  is labelled and RATA asks before saving it; Cancel is the default.
  From v0.1.9 you can **attach files** when writing, with 📎 Attach, up to
  18 MB in all, since most providers refuse mail over 25 MB once encoded.
  From v0.1.11 **Forward** on a message sends its attachments along too.
  **Please report any message that still shows raw code** (lines
  like `--000abc`, `Content-Type:` or `=20`) with its sender (a newsletter
  name is fine); that is a decoding bug.
- Mail that arrived under an older beta is re-read and cleaned up after the
  first refresh on v0.1.7, 50 messages per mailbox at a time, and any one you
  open is fixed at once.
- **Updates:** from v0.1.23 RATA can update itself, once the update key is
  set up on our side: a bar at the top says "RATA x is ready." with a
  **Restart to update** button. The first version that has it is installed by hand, once; after
  that RATA offers each new one. Until then, new betas are a new download.
  If an update fails, the bar says why and offers the download; please send
  us that exact sentence.

- **New mail comes in by itself from v0.1.27**: a moment after RATA starts,
  every five minutes while it is open, and when you come back to it. Before
  that, mail only arrived when you pressed Sync. If RATA was closed while a
  lot arrived (more than 200 in a folder), it brings the newest first and
  the rest over the next refreshes (or at once with **Load older mail**);
  before v0.1.28 the ones in between never came. From v0.1.29 new mail that
  arrives while RATA is in the background shows a **desktop notification**;
  Settings → New-mail notifications can hide the sender and subject, or turn
  them off. Tell us if none ever appears, and say which system you are on. If your provider starts
  refusing RATA with messages about too many sign-ins, tell us which
  provider: every five minutes is a sign-in every five minutes. From
  v0.1.34 new mail should appear **within a few seconds** of arriving: RATA
  keeps one connection per mailbox open and your provider tells it when mail
  lands (the five-minute check still runs underneath). If mail still only
  turns up every few minutes, tell us which provider: it may not offer
  this, and that is worth knowing. From v0.1.35 RATA signs in far less
  often: a mailbox with that live connection is only checked every half
  hour (it says when mail comes), and new mail in one mailbox no longer
  makes RATA sign in to all of them. Coming back to RATA still checks
  everything.
- From v0.1.36 each mailbox can have a **signature** (Settings →
  Signatures). It goes under new messages, and above the quoted text of
  replies and forwards unless you turn that off; changing From swaps it.
  Signatures stay on your computer; they are not part of what RATA keeps
  in your online account.
- From v0.1.38 **Gmail's archive** shows under Archive. Gmail keeps
  archived mail inside "All Mail" with everything else, so RATA asks Gmail
  which of it is archived; mail you archive on your phone should appear
  within a refresh or two, and mail you move back to the inbox elsewhere
  should leave. **Move to inbox** and **Move to** on Gmail's archive add
  the label, as Gmail does, so a message moved into a folder is still in
  Archive too. If archived mail is missing, doubled, or does not leave,
  say so and say it was Gmail.
- v0.1.37 **looks different**: calmer colours, one accent, a new typeface
  (Geist), flatter buttons, and a mail list with badges on the sender's line
  so more of it fits. Nothing moved or was taken away. If something is now
  hard to read, too faint, or cut off, send a screenshot with the window
  size, and say whether your computer is in light or dark mode.

What *should* work: adding a mailbox, seeing recent mail from several
mailboxes, sending, **replying** (the Reply button on a message, from v0.1.6;
check that it lands in the same thread at the other end), removing a mailbox,
the licence and,
from v0.1.4, **read, unread, star, delete and archive reach your real
mailbox**. Delete moves the message to your provider's Trash (never deletes it
for good); Archive moves it to your Archive folder, or Gmail's All Mail. Please
check the result in your provider's own webmail or phone app: that is the
real test. Mail that was already in RATA before v0.1.4 is changed in RATA only
until the next refresh picks up its server reference.

## 6. Reporting a bug

Open an issue with the **Beta bug** template:
**https://github.com/Noallrightokay/center-point-inbox/issues/new?template=beta-bug.md**

⚠️ This repository is **public**. Never paste an app password or a licence key.
Your own email address is fine to include if you are comfortable with that; if
not, replace it with `me@<provider>`; the provider is the useful part.

Running the full check on real mailboxes, provider by provider? Follow
`docs/SMOKE.md` and record the results in `docs/MVP-EVIDENCE.md`.

What makes a report fixable in minutes rather than days:

1. **The exact error text**, copied, not paraphrased. RATA's errors are written
   to be diagnostic: they name the servers it tried and what each one said.
2. **Your provider** (Gmail, iCloud, a work domain on Google Workspace…).
3. **Version** from the title bar, and your OS.
4. **What you did, what you expected, what happened.**
5. A screenshot if the problem is something you see rather than an error.

---

## For testers who want to fix things too

Start with `README.md` (layout, build, the traps) and `CLAUDE.md` (current
state and the rules of the road). Short version:

```bash
# mail engine, no network needed
cd desktop/rata-mail && cargo test --all-targets

# desktop app: sync-ui.sh FIRST, every time
cd desktop/rata-app && ./sync-ui.sh && cd src-tauri && cargo test
cargo tauri dev          # run it (from desktop/rata-app)

# website (npm test reads the build)
cd rata-next && npm ci && npm run build && npm test
```

Rust 1.94.1. On Linux you also need
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf`.

One rule that is not negotiable: **mail and mail passwords never go to a
server.** If a change would put them there, it is wrong however convenient.

When a real mailbox misbehaves, the three files to read first are
`desktop/rata-mail/src/resolve.rs` (finding the server), `imap.rs` (sign-in and
fetch) and `guard.rs` (hosts RATA refuses to connect to).

Work on a branch, open a PR against `main`. CI runs the Rust and website
suites, the interface harness, and the mail engine against real Dovecot and
GreenMail servers.
