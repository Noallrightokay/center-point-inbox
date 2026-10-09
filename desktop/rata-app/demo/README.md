# Working on the RATA demo

The demo is the real RATA interface running in a browser, with sample mail
instead of a real mailbox. Anyone can change it: you need a browser, Git,
Python 3 and a Bash shell. You do not need Rust, Node or an account
anywhere.

## How it fits together

RATA's desktop app has two halves:

- **The interface**, `rata-next/public/app.html`: one HTML file with its CSS
  and JavaScript, the same file the released app ships. `bridge.js`
  (`desktop/rata-app/ui-src/bridge.js`) sits under it and turns each
  request the page makes (`/api/mail/refresh`, `/api/created`…) into a
  call to the app's Rust side, `window.__TAURI__.core.invoke(command, args)`.
- **The Rust side** (`desktop/rata-app/src-tauri`), which reads mail over
  IMAP, sends over SMTP, keeps passwords in the keychain and so on.

The demo keeps the interface and `bridge.js` exactly as they ship and
replaces only the Rust side with **`demo-backend.js`**: a plain JavaScript
file that defines `window.__TAURI__.core.invoke` and answers each command
with sample data. That file is the demo's whole "backend". It sends
nothing, opens no mailbox and talks to no server. The two things
`bridge.js` asks of mailrata.org itself, the AI relay (Summarize,
Translate, Run briefing) and licence renewal, are answered in the page
too, with words written for the demo and marked as its own ("Demo
summary: …"); any other request off the page fails at once. Sign out and
Delete account start the demo again.

| File | What it is |
|---|---|
| `demo-backend.js` | The fake backend: mailboxes, mail, saved people, attachments, cloud folders, Slack, Create file, the Base/Pro switch, and one `switch (cmd)` answering every command |
| `demo.css` | The "Demo" label in the corner |
| `build.sh` | Assembles the demo into one folder of static files, or with `--single` into one HTML file |
| `single.py` | What `--single` runs: folds the folder into one file |
| `check.mjs` | Builds both forms and drives them in Chromium to check the rules below (*Check it*) |
| `out/` | Where `build.sh` writes by default (not committed) |

## Run it

From the repository root, on macOS, Linux, or Windows with WSL or Git Bash:

```bash
git clone https://github.com/Noallrightokay/center-point-inbox.git
cd center-point-inbox
desktop/rata-app/demo/build.sh                 # writes desktop/rata-app/demo/out
python3 -m http.server 8000 --directory desktop/rata-app/demo/out
```

Open http://localhost:8000/rata-demo.html. Run `build.sh` again after every
change, then reload the page. `build.sh <folder>` builds somewhere else; it
empties that folder first, so it refuses one that holds anything but an
earlier demo build. It must be served over http; opening the file
directly does not work, because the page loads scripts and fonts beside it.
For one file that does open directly, see *Send it as one file* below.

The label at the bottom (**Demo**, the Base/Pro switch and **Reset**) folds
to a small **Demo** tag when you click the word Demo, for when it covers
something; it stays folded in that tab.

What you click is kept in this browser between reloads. **Reset** in the
demo's label starts again, and so does raising `DEMO_VERSION` at the top of
`demo-backend.js`, which clears every visitor's saved copy the next time
they open the demo. Raise it whenever you change the sample mail, or people
who opened the demo before keep seeing the old mail.

## Send it as one file

```bash
desktop/rata-app/demo/build.sh --single        # writes desktop/rata-app/demo/out/RATA-demo.html
desktop/rata-app/demo/build.sh --single ~/Desktop/RATA-demo.html
```

This writes the whole demo as one HTML file of about 6 MB, small enough
to attach to an email or drop in a chat. Whoever gets it opens it by
double-click, in Chrome, Edge, Firefox or Safari. It needs no server and
no internet connection. Everything the folder build loads from files
beside the page is inside it: scripts, styles, fonts, pictures, and the
Word, Excel and PDF libraries. The libraries are loaded from `blob:`
URLs the page makes when it starts, so reading a PDF, writing Word, Excel
and PDF, and Convert all work as they do in the folder build. There is no
service worker. It still sends nothing and calls no server.

What is kept between reloads depends on the browser, because a page
opened from a file gets different storage in each:

- **Chrome and Edge** keep what you did, as the served demo does (in one
  store shared by every file opened from that computer's disk). Reset,
  the Base/Pro switch and a new `DEMO_VERSION` remove only the demo's own
  entries there (those starting `centra_` or `rata_`), never another
  file's.
- **Firefox and Safari** may keep it, keep it only for that file, or keep
  nothing. Where the browser refuses storage, the demo still runs but
  starts fresh on every reload. The Base/Pro choice survives a reload
  either way: it is kept in the address too (`#plan=base`), so a link or
  bookmark ending in `#plan=base` opens the demo on Base.
- Where a browser will not keep files in IndexedDB for the page, the
  sample documents in Files show with **⇄ Bridge** but without **⤓ File**,
  and Files cannot keep a converted file. Converting and downloading
  still work.

Rebuild the file after every change, as with the folder.

## Common changes

All in `demo-backend.js`:

- **Mailboxes**: `MAILBOXES`. On Base the demo shows the first two.
- **Mail**: `MAIL`, built with `msg({...})`. `acct` is which mailbox it
  arrived in, `from`/`fromName` the sender, `ago` how long ago in
  milliseconds (`25 * MIN`, `3 * HOUR`, `2 * DAY`), `body` the text,
  `unread`/`starred`, `folder` (`'sent'`, `'drafts'`, `'archive'`,
  `'junk'` for Spam, or `{ named: 'Folder' }`; the inbox when left out), `atts` for attachments, and `mid`/`inReplyTo`
  to thread replies into one conversation. Give each message its own `uid`.
- **Mail that arrives while you watch**: the `setTimeout` after `MAIL`.
- **Saved people**: `CONTACTS`.
- **Attachments**: `FILES`. Documents are written by RATA's own Word,
  Excel and PDF writers from blocks (`H`, `P`, `LI`, `TABLE`), so they are
  real files that open in Word and Excel.
- **Plans**: `PLANS`, and what each allows (`mail`, `split`, `ai`,
  `connect`).
- **AI answers**: `SUMMARIES` and `TASKS`, by subject. A message with
  neither gets a summary built from its sender and subject, and no flag.
- **Cloud folders and Slack**: `SERVICES`, `TREES`, `SLACK`.
- **Create file**: `MADE` (the files it starts with) and the
  `create_file` case.

**A new command.** If you change `app.html` so the page asks for something
new, it reaches `demo-backend.js` as a new `cmd`. Add a `case` for it.
A command it does not know fails with "This demo does not do that
(<command>).", which names the one that is missing. What each command takes and answers is written
in comments in `bridge.js`, beside the route that calls it; match that
shape, since the real Rust side answers the same way.

## Rules for the demo

- **Invent everything.** Every person, organisation, address and message
  is made up. No real patient, customer or colleague information, ever,
  even as a joke: the demo is shown to people outside the project.
  Addresses, domains and mail servers use names reserved for examples
  (RFC 2606: `riley.carter@mail.example`, `imap.tidepoolcpr.example`),
  so none can be anybody's; `check.mjs` fails on any other.
- **Nothing leaves the page.** The demo must not call any server. Links
  open in a new tab, and that is all.
- **Leave other pages alone.** Clear only the demo's own storage, never
  `localStorage.clear()`: a page opened from disk shares its store with
  every other.
- **Keep it honest.** The demo shows what RATA does. If you add a screen
  to the demo that the real app does not have, say so in your pull request.

## Check it

```bash
(cd rata-next && npm ci && npx playwright install chromium)   # once; needs Node
node desktop/rata-app/demo/check.mjs
```

This builds the folder and the one file in a temporary place and drives
both in Chromium: no request leaves the page (AI and renewal included),
another page's storage survives opening, Reset, the Base/Pro switch and
Delete account, Sign out and Delete account come back to the demo, every
address is a reserved one, and `build.sh` refuses a folder that is not a
demo build. Every check must pass.

## Changing the interface itself

`rata-next/public/app.html` is the real product, not a copy. A change there
ships in the next release of the desktop app, and the website's app page
uses it too. Before opening a pull request that touches it, run the
interface checks. They drive the page in Chromium against another fake
backend, and include accessibility scans:

```bash
(cd rata-next && npm ci && npx playwright install chromium)   # once; needs Node
(cd desktop/rata-app && ./sync-ui.sh)
(cd desktop/rata-app/ui && python3 -m http.server 3181 --bind 127.0.0.1 &)
node desktop/rata-app/harness/ui-harness.mjs http://127.0.0.1:3181
```

Every check must pass. Read `DESIGN.md` for how RATA looks (colours, type,
spacing, wording), and the root `README.md` and `CLAUDE.md` for everything
else, including the Rust side.

## Sending your change

Fork the repository on GitHub, push a branch, and open a pull request
against `main`. Say what you changed and how you checked it: a screenshot of
the demo helps.
