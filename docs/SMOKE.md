# RATA live smoke test

A checklist for the project owner, run on real machines with real
mailboxes. It is task [B2](MVP-PLAN.md#b2). No real mailbox has ever been
opened by this code, so every row here is a first.

Write each result in [`MVP-EVIDENCE.md`](MVP-EVIDENCE.md). Row numbers are
the same in both files. File every failure as a *Beta bug* issue (see
[§4](#4-recording-a-failure)).

**Never write down** a password, a licence key, the text or subject of real
mail, or anyone's address but your own. Use the test subjects given in the
steps, so the only mail you describe is mail you wrote for this test.

---

## 1. Before you start

### What you need

- A Windows machine and a Mac. Linux is optional.
- A **Pro** licence key. Base allows only two mailboxes and has no
  Summarize.
- One mailbox at each provider below, each with its app password.
- A second way to send mail to each mailbox: your phone, or one of the
  other test mailboxes.
- A small PDF you do not mind sending to yourself (under 5 MB).
- About two hours per machine.

### Which release to install

1. Open https://github.com/Noallrightokay/center-point-inbox/releases
2. Take the newest release. This checklist is written for **v0.1.42**
   and later. Rows X5 to X7 (RATA's drafts saved to the mailbox) need
   v0.1.41 or later, and X7's Trash step needs v0.1.42.
3. Install as in BETA.md §1:
   - Windows: `RATA_<version>_x64-setup.exe`. Install it with the setup
     program and start RATA from the Start menu. Row 16 needs the
     installed copy.
   - macOS: the `aarch64` dmg (Apple silicon) or `x64` dmg (Intel). Drag
     RATA to Applications and open it once. macOS says it could not
     check RATA: choose **Done**. Then System Settings, **Privacy &
     Security**, scroll to the line about RATA, **Open Anyway**, and
     confirm when macOS asks again (it may want your password). Right-click, **Open** no longer gets
     past this on macOS 15. If macOS says instead that RATA is damaged and
     can't be opened, run `xattr -dr com.apple.quarantine
     /Applications/RATA.app` in Terminal, and write down which message
     you saw: it says whether the unsigned build needs that step.
   - Linux: `sudo apt install ./RATA_<version>_amd64.deb`, or the
     `RATA_<version>_amd64.AppImage` made executable (`chmod +x`).
4. Check the window title. It must read `RATA <version> beta`, with the
   version you installed. Write that version in every result.

### The licence key

Either:

- mint one on the VPS: `sudo infrastructure/licence/mint.sh you@example.com pro 30`
  (from this repository; `sudo ./mint.sh check` first)
  (BETA.md §2), or
- once [D5](MVP-PLAN.md#d5) is done, buy Pro and copy the key from
  https://mailrata.org/account

Keep the key in a password manager. It is also stored in RATA's
`mailboxes.json` (§5), so never paste that file anywhere.

### App passwords

Your normal password will be refused. Make one app password per mailbox.

| Column | Where to make it | Webmail | Note |
|---|---|---|---|
| Gmail | https://myaccount.google.com/apppasswords | mail.google.com | Needs 2-Step Verification on. If Gmail settings show an IMAP switch, turn it on. |
| iCloud | https://account.apple.com, Sign-In and Security, App-Specific Passwords | icloud.com/mail | |
| Yahoo | login.yahoo.com, Account security, Generate app password | mail.yahoo.com | |
| Fastmail | Settings, Privacy & Security, App passwords | app.fastmail.com | Give it mail (IMAP and SMTP) access. |
| Hostinger domain | hPanel, Emails, the mailbox's own password | mail.hostinger.com | Hostinger has no app passwords. Use the mailbox password. |
| Outlook.com | none: Microsoft's own sign-in page, no app password | outlook.live.com | **Fails until C4, then must pass.** See below. |

**Outlook.com fails until [C4](MVP-PLAN.md#c4), then must pass.**
Microsoft turned off password sign-in over IMAP for Outlook.com in September
2024; from v0.1.39 RATA signs in to Microsoft mailboxes with Microsoft's own
page (**Sign in with Microsoft**, [C1](MVP-PLAN.md#c1) to
[C3](MVP-PLAN.md#c3)). That button appears only in a build made with the
owner's Microsoft client id (C4: the `RATA_MS_CLIENT_ID` repository
variable, `rata-next/LAUNCH.md` §10). Until then, row 2 shows, in the
**＋ Email account** form and before any password box, "Outlook.com, Hotmail and Live
mailboxes cannot be added to RATA yet. …": copied exactly, that is the
expected result for this column, and rows 3 to 20 are `n/a`. From the first
release built with the client id, this column is no longer expected to
fail: row 2 is **Sign in with Microsoft** (your browser opens Microsoft's
page, then an account picker), and every row must pass like any other
column's. Record any failure's exact text as usual.

### Set up each mailbox in webmail first

1. Make a folder (a label in Gmail) called `RATA test`.
2. Make sure Archive, Spam and Drafts each hold at least one message.
   Archive and Spam show in RATA only when they hold something.
3. Have one formatted newsletter in the inbox, with pictures and links.
4. Have one unread message in the inbox.

---

## 2. The grid

Tick on paper, then copy results into `MVP-EVIDENCE.md`. Do the rows in
order. Rows 2 to 5 for every mailbox first is easiest, then the rest a
column at a time.

| # | Row | Gmail | iCloud | Yahoo | Fastmail | Hostinger | Outlook |
|---|---|---|---|---|---|---|---|
| 1 | Licence accepted (once per machine) | ☐ | | | | | |
| 2 | Link the mailbox | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 3 | The provider it named | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 4 | First sync, per folder | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 5 | Keychain and `mailboxes.json` (§5) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 6 | Open an HTML message | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 7 | Send to self with an attachment | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 8 | Save an attachment | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 9 | Reply | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 10 | Forward with the attachment | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 11 | Mark read, then unread | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 12 | Archive | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 13 | Delete | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 14 | Move to a folder | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 15 | New mail within a minute | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 16 | Notification (§5) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 17 | Summarize (once per machine) | ☐ | | | | | |
| 18 | Relaunch keeps mail (§6) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 19 | Offline start (§6) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| 20 | Remove deletes the keychain entry (§5) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X1 | Load older mail (if time) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X2 | Reply all, Cc and Bcc (if time) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X3 | Continue a draft (if time) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X4 | Not spam (if time) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X5 | A draft saved in RATA reaches the phone (if time) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X6 | Saved twice, one copy (if time) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| X7 | Gmail's All Mail after two saves (if time) | ☐ | | | | | |

Where a step says **refresh**, press **⟳ Sync linked inboxes** above the
mail list, or **Sync now** on the mailbox's row in Settings, Linked
accounts.

Error text appears in a small message at the bottom of the window (it
fades after a few seconds, so screenshot it at once), or on the mailbox's
row in **Settings**, **Linked accounts**, at the end of the grey line under
the address.

---

## 3. The rows

Each row says which feature it proves. The reference is to the version
entry in `CLAUDE.md` → Status.

### Row 1. Licence accepted
*Proves: licensing (CLAUDE.md → Licensing), v0.1.24 plan from the licence.*

1. Start RATA for the first time.
2. The box **Your licence key** appears. Paste the key and press **Use this
   licence**.

**Pass:** the box closes and the app loads. **Settings**, **Current plan**
reads "Licensed for RATA Pro until" a date.
**Fail:** the red line under the key field. Copy it exactly.

### Row 2. Link the mailbox
*Proves: adding a mailbox by address and app password (Status → Works).*

1. **Settings** → **Linked accounts** → **＋ Email account**.
2. Enter the address and the app password. Press **Link**. It reads
   "Checking…" while it works.
3. If a server field appears, RATA could not find the server. Copy the
   sentence above the field first, then enter the IMAP server from the
   provider's help page.

**Pass:** a message says the address is linked and pulling messages. The
mailbox's row shows **LIVE**.
**Fail:** any error. Copy it exactly: it names the servers tried and what
each said.
**Outlook.com:** see §1. Copy the message whatever it is.

### Row 3. The provider it named
*Proves: server discovery, SRV then MX then usual names (Status → Works).*

1. Look at the mailbox's row in **Settings** → **Linked accounts**.

**Pass:** the grey line under the address names the provider and server:

| Column | Expected |
|---|---|
| Gmail | Gmail · imap.gmail.com |
| iCloud | iCloud Mail · imap.mail.me.com |
| Yahoo | Yahoo Mail · imap.mail.yahoo.com |
| Fastmail | Fastmail · imap.fastmail.com |
| Hostinger | Hostinger Email · imap.hostinger.com (the link message also says the domain's mail is at Hostinger Email) |
| Outlook.com | Outlook · outlook.office365.com, if it got this far |

**Record:** the line exactly as shown.

### Row 4. First sync, per folder
*Proves: v0.1.22 Sent, v0.1.25 Archive and Spam, v0.1.31 Drafts, v0.1.38
Gmail's archive, v0.1.5 the first page of older mail.*

1. Go to **Mail**. Wait until the list stops changing (under a minute).
2. Click each chip in turn: **All**, **Sent**, **Drafts**, **Archive**,
   **Spam**.
3. For each, compare with the same folder in webmail: is the newest
   message the same? Scroll to the bottom of RATA's list. What is the date
   of the oldest one shown?

**Pass:** each chip exists and its newest message matches webmail. A new
mailbox brings up to about 65 messages per folder (the newest 15, then one
page of 50). Where webmail holds fewer, RATA shows all of them.
Gmail's Archive chip shows mail that is in All Mail but not in Inbox, Sent
or Drafts.
**Record:** one word per folder, for example
`Inbox ok, Sent ok, Drafts ok, Archive missing, Spam ok`. Do not write
subjects.
**Webmail:** each folder's own view.

### Row 5. Keychain and `mailboxes.json`
*Proves: the password lives in the OS keychain and nowhere else
(BETA.md §4, MVP M11).*

Do §5 "The password checks" for this mailbox, straight after linking.

### Row 6. Open an HTML message
*Proves: v0.1.7 decoding, v0.1.12 HTML mail, v0.1.14 links.*

1. In **Mail**, open the newsletter.
2. Above the message, **Formatted** and **Text** should show. Try both.
3. If pictures are missing, press **Load images**.
4. Click a link in the formatted view.

**Pass:** it looks like the newsletter in webmail. Pictures appear after
**Load images**. The link opens a box, **Open this link in your browser?**,
naming the site. **Open in browser** opens it in your browser, and RATA
stays as it was.
**Fail:** raw code in the text (lines like `--000abc`, `Content-Type:` or
`=20`), a blank pane, or a link that does nothing. Record the sender's
name only if it is a newsletter.
**Webmail:** nothing changes. Only compare how it looks.

### Row 7. Send to self with an attachment
*Proves: sending (Status → Works), v0.1.9 attachments, v0.1.22 Sent without
doubles.*

1. Press **Compose**.
2. To: this mailbox's own address. Subject: `RATA smoke 7 <column>`, for
   example `RATA smoke 7 Yahoo`. Type one line.
3. Press **📎 Attach** and pick the PDF.
4. Check From is this mailbox. Press **Send**.

**Pass:** a message starting "Sent from" and this address. It arrives in
RATA's inbox with 📎 in the list. After a refresh, **Sent** shows it once,
not twice.
**Webmail:** Inbox has it, with the PDF, which opens and has the same name.
Sent has it too.

### Row 8. Save an attachment
*Proves: v0.1.8 whole messages and saving attachments.*

1. Open the message from row 7.
2. Click the PDF's card under the message (the one with ⤓).

**Pass:** a message starting "Saved to" and a path in Downloads. The file
opens in your usual PDF app and matches the one you sent. RATA does not
open it for you; that is by design.
**Record:** write `Downloads`, not the full path.
**Webmail:** nothing changes.

### Row 9. Reply
*Proves: v0.1.6 Reply, threading, v0.1.26 reply from the right mailbox.*

1. From your phone or another test mailbox, send this mailbox a message
   with subject `RATA smoke 9 <column>`.
2. In RATA, open it and press **Reply**. Type one line. Press **Send**.

**Pass:** a message starting "Sent from" and **this** mailbox's address,
not another one.
**Webmail (the other mailbox):** the reply arrives in the same
conversation as the original, subject `Re: RATA smoke 9 …`, from this
mailbox.

### Row 10. Forward with the attachment
*Proves: v0.1.11 Forward, which carries the original's attachments.*

1. Open the message from row 7.
2. Press **Forward**. The composer title reads **Forward**, and a line says
   it is forwarding with 1 file.
3. To: another test mailbox. Press **Send**.

**Pass:** a message starting "Sent from".
**Webmail (the other mailbox):** the forward arrives with the PDF, which
opens.

### Row 11. Mark read, then unread
*Proves: v0.1.4 read and unread on the server.*

1. In webmail, note one unread message in the inbox.
2. In RATA, open it. Opening marks it read.
3. Refresh webmail. Then in RATA press **Mark unread** on the message.
   Refresh webmail again.

**Pass:** RATA shows it read after opening. **Mark unread** says "Marked
unread" and the message is bold again in the list.
**Webmail:** read after step 2, unread after step 3.

### Row 12. Archive
*Proves: v0.1.4 archive, v0.1.25 Archive and Move to inbox, v0.1.38 Gmail's
archive.*

1. Send this mailbox a message, subject `RATA smoke 12 <column>`.
2. In RATA, open it and press **Archive**.
3. Refresh RATA, then click the **Archive** chip.
4. Open the message there and press **Move to inbox**.

**Pass:** step 2 says "Archived" and the message leaves the list. Step 3
shows it under **Archive**. Step 4 shows a message starting "Moved to your
inbox", and after a refresh it is back in the inbox.
**Fail:** a message starting "Archived in RATA, but your mailbox was not
updated". Copy all of it. A mailbox with no Archive folder says so here.
**Webmail:** after step 2, gone from Inbox and in Archive. Gmail has no
Archive folder: look in **All Mail**; it must not be in Inbox. After step
4, back in Inbox. (In Gmail, Move to inbox adds the Inbox label back.)

### Row 13. Delete
*Proves: v0.1.4 delete, which is a move to Trash and never a permanent
delete.*

1. Send this mailbox a message, subject `RATA smoke 13 <column>`.
2. In RATA, open it and press **Delete**.
3. Refresh RATA.

**Pass:** "Deleted", the message leaves the list, and it does not come
back after the refresh.
**Fail:** a message starting "Deleted in RATA, but your mailbox was not
updated". Copy all of it.
**Webmail:** the message is in **Trash** (Gmail and iCloud may say Bin),
not gone for good, and not in Inbox.

### Row 14. Move to a folder
*Proves: v0.1.26 your own folders and Move to.*

1. Send this mailbox a message, subject `RATA smoke 14 <column>`.
2. In RATA, open it and press **Move to ▾**. Pick `RATA test`.
3. Click the **Folders** chip, then `RATA test` under this mailbox.

**Pass:** step 2 says "Moved to RATA test" and the message leaves the list.
Step 3 shows it in that folder.
**Webmail:** it is in `RATA test` (a label in Gmail) and not in Inbox.

### Row 15. New mail within a minute
*Proves: v0.1.34 IMAP IDLE. Never seen against a real server.*

1. Leave RATA open for at least two minutes after it starts.
2. From another mailbox or your phone, send this mailbox
   `RATA smoke 15 <column>`. Note the time.
3. Watch RATA's inbox. Do not press anything.

**Pass:** it appears within a minute of webmail showing it, usually within
a few seconds.
**Record:** the seconds it took. If it only came after about five
minutes, write `slow, N min`: this provider may not offer IDLE, and that
is worth knowing.

### Row 16. Notification
*Proves: v0.1.29 desktop notifications. Never seen on Windows or macOS.*

Do §5 "The notification check" for this mailbox.

### Row 17. Summarize (Pro)
*Proves: v0.1.17 AI through RATA's relay (MVP M8). Needs [D4](MVP-PLAN.md#d4)
and [D5](MVP-PLAN.md#d5).*

1. Open your own message from row 7. Use your own test mail so no one
   else's words go to the relay.
2. Press **✦ Summarize**.
3. The first time, RATA explains that the message text goes to its AI
   service, run by Anthropic, and is not kept. Press **OK**.

**Pass:** a box titled **Summary by AI**.
**Before D4 is deployed:** a box titled **Quick summary**, with a note
ending "This one was made on this computer, without AI." Record it as
`blocked (D4)` with the note's first sentence, not as a fail.

### Row 18. Relaunch keeps mail
See §6.

### Row 19. Offline start
See §6.

### Row 20. Remove deletes the keychain entry
*Proves: Remove really removes (2026-09-23 review fixes), MVP M11.*

Do this last for each mailbox. See §5 "After Remove".

### If you have time

These are not MVP rows, but none has been seen on a real server.

**X1. Load older mail** (v0.1.5, v0.1.28). In **Mail**, **All**, scroll to
the bottom and press **Load older mail**. **Pass:** a message saying how
many older messages loaded, or "That's everything in your inbox". The
list goes further back, in the same order as webmail.

**X2. Reply all, Cc and Bcc** (v0.1.32, v0.1.33). From mailbox B, send
this mailbox a message with mailbox C in Cc, subject
`RATA smoke X2 <column>`. In RATA open it and press **Reply all**.
**Pass:** To holds B, Cc holds C, and your own address is in neither.
Press **Bcc**, add mailbox D, and send. **Webmail:** B, C and D all
receive it. In C's copy, D appears nowhere in the headers (use webmail's
"show original").

**X3. Continue a draft** (v0.1.31). In webmail, save a draft to yourself
with subject `RATA smoke X3 <column>` and the PDF attached. In RATA,
refresh, click **Drafts**, open it and press **Continue**. **Pass:** the
composer holds the address, subject, text and the PDF. Send it.
**Webmail:** the message arrives with the PDF, and the draft has moved
from Drafts to Trash.

**X4. Not spam** (v0.1.25). In webmail, mark a test message
`RATA smoke X4 <column>` as spam. In RATA, refresh, click **Spam** and
open it. **Pass:** a warning "This was in your Spam folder." Press **Not
spam**: a message starting "Marked not spam". **Webmail:** it is back in
Inbox.

**X5. A draft saved in RATA reaches the phone** (v0.1.41). In RATA press
**Compose**, choose this mailbox in From, address it to yourself, type the
subject `RATA smoke X5 <column>` and a line of text, then close the
composer. **Pass:** a message "Saved to Drafts", and the draft is under
**Drafts** in RATA. **Phone:** within a few minutes the draft is in the
phone's Drafts (the provider's app, or webmail on the phone), with the
same subject and text. If the mailbox has no Drafts folder, RATA says it
keeps the draft on this computer: write that down instead of a tick.

**X6. Saved twice, one copy** (v0.1.41). In RATA open **Drafts**, open
`RATA smoke X5 <column>`, press **Continue**, add a second line and close
the composer; wait for "Saved to Drafts". Refresh. **Pass:** RATA's
Drafts holds one copy, with both lines. **Webmail:** Drafts holds one
copy of it, with both lines, and nothing else in Drafts has changed.
On Gmail, do X7 now. Then open it in RATA, press **Continue** and **Discard**: it is asked
about first, and **webmail** shows it in Trash.

**X7. Gmail's All Mail after two saves** (v0.1.41, Gmail only). In X6,
before **Discard**, open **All Mail** in Gmail's webmail and search
`subject:"RATA smoke X5"`. **Pass:** one message. **Fail:** two or more,
the older with only the first line: Gmail kept the copy RATA removed
from Drafts. Also check RATA's **Archive** after a refresh: an old copy
of the draft must not be there. Screenshot either way; this decides how
RATA saves drafts on Gmail. From v0.1.42 RATA moves its older copy to
Gmail's **Trash** (review P3-2), so an older copy in Trash is expected;
All Mail must still show one.

---

## 4. Recording a failure

1. Write the result in `MVP-EVIDENCE.md`: version, OS, `fail`, the exact
   error text, and the issue number once filed.
2. Open an issue with the **Beta bug** template:
   https://github.com/Noallrightokay/center-point-inbox/issues/new?template=beta-bug.md
3. Title: `[beta] Row <n> <row name>, <column>`, for example
   `[beta] Row 12 Archive, Yahoo`.
4. Fill in:
   - **Version**: the version in the title bar, for example `RATA 0.1.42 beta`.
   - **OS**: for example `Windows 11` or `macOS 15, Apple silicon`.
   - **Mail provider**: the column name.
   - **What I did**: the row number and its steps.
   - **Exact error text**: copied, never retyped or summarised. It names
     the servers RATA tried and what each said, which is what makes it
     fixable.
   - A screenshot only if the problem is something you see. Crop it to
     RATA's own words.

**The repository is public.** Never put in an issue a password, an app
password, a licence key, the contents of `mailboxes.json`, the subject or
text of real mail, or anyone's address but your own. Your own address is
fine, or write `me@<provider>`.

The project manager turns each issue into a bug card. The agent that
fixes it writes a failing test from your error text first, so the exact
words matter more than anything else in the report.

---

## 5. Per machine: keychain, `mailboxes.json`, notifications

### The password checks (row 5)

**The password is in the keychain.**

- **Windows:** Control Panel → Credential Manager → Windows Credentials.
  Under Generic Credentials there is an entry whose name contains your
  address and `org.mailrata.desktop`. Or in Command Prompt:
  `cmdkey /list | findstr mailrata` (it lists names only, never the
  password).
- **macOS:** Keychain Access → login → search `org.mailrata.desktop`. There
  is one entry per mailbox, with your address as the account. Or in
  Terminal: `security find-generic-password -s org.mailrata.desktop -a you@example.com`.
  Do not add `-w` or `-g`: those print the password.
- **Linux:** Passwords and Keys (Seahorse) or KWallet → Login. There is an
  entry that mentions `org.mailrata.desktop`.

**The password is not in RATA's own file.** Open `mailboxes.json` in a text
editor (Notepad, TextEdit, gedit). Do not paste it anywhere: it also holds
your licence key.

| OS | Folder |
|---|---|
| Windows | `%APPDATA%\org.mailrata.desktop\` (paste into Explorer's address bar) |
| macOS | `~/Library/Application Support/org.mailrata.desktop/` (Finder → Go → Go to Folder) |
| Linux | `~/.local/share/org.mailrata.desktop/` |

1. The file lists your address and the server.
2. Use the editor's Find for the first six characters of the app password.

**Pass:** zero matches. **If you find it, stop and report that first.** It
is the most serious bug possible here. Say which row and provider, never
the password.

### After Remove (row 20)

1. **Settings** → **Linked accounts** → **Remove** on the mailbox's row.
2. Confirm. The question starts "Remove" and the mailbox's name, and says
   "Messages already in your workspace stay".

**Pass:** the row is gone. The keychain entry for that address is gone
(the `cmdkey` or `security` command above now finds nothing), and
`mailboxes.json` no longer lists the address. Mail already in RATA staying
is by design.
**Fail:** a message starting "Could not remove". Copy it.

### The notification check (row 16)

Windows and macOS have never shown one. Linux has only been seen on the
system's message bus, not on screen.

1. In RATA: **Settings** → **New-mail notifications** → **Show sender and
   subject**.
2. Put RATA in the background: click another app, or minimise RATA.
3. From another mailbox, send this mailbox `RATA smoke 16 <column>`.
4. Wait up to a minute.

**Pass:** a desktop notification with the sender and the subject
`RATA smoke 16 …`.

What to know per system:

- **Windows:** notifications only work for the **installed** app, because
  Windows ties them to the installed identifier (`org.mailrata.desktop`,
  its AppUserModelID). Use the copy the setup program installed, started
  from the Start menu, not an unpacked or copied `.exe`. If none appears,
  check Settings → System → Notifications: is RATA listed, is it on, and is
  Do not disturb off? Write down what you found.
- **macOS:** the first time, macOS may ask whether RATA may send
  notifications. Allow it, then send another test message. If none
  appears, check System Settings → Notifications → RATA and write down
  what it shows.
- **Linux (optional):** any desktop with a notification area.

Notifications are never shown for mail you fetched by pressing a sync
button, for Spam, or while RATA is in front. That is by design, not a
fail.

---

## 6. Relaunch and offline

### Row 18. Relaunch keeps mail
*Proves: v0.1.13 mail kept on disk, v0.1.21 text on disk.*

1. In RATA, press **☆ Star** on one message. Open one message you have not
   opened before.
2. Quit RATA fully. macOS: RATA → Quit (⌘Q). Windows and Linux: close the
   window.
3. Start RATA again.

**Pass:** no licence box. Every mailbox's mail is there, the star is
kept, and the message from step 1 opens with its text. The Sent, Drafts,
Archive and Spam chips are still there.

### Row 19. Offline start
*Proves: the licence works offline (thirty days at a time), and mail is
kept on this computer (MVP M9).*

1. Quit RATA.
2. Turn off Wi-Fi and unplug any network cable.
3. Start RATA.
4. Open a message you opened before. Then **More** → **Search**, type
   `RATA smoke` and press **Search**.
5. Look at **Settings** → **Linked accounts**. Copy what each row says.
6. Turn the network back on. Send this mailbox `RATA smoke 19 <column>`
   from your phone. Wait up to five minutes, or press **Sync now**.

**Pass:** no licence box. The mail list shows. The message opens with its
text. Search finds the smoke-test mail. Nothing crashes. After step 6
the new message arrives.
**Record:** the text on the Linked accounts rows while offline, exactly.
It should say the network could not be reached, not that the password
was wrong. A mailbox marked **NEEDS APP PASSWORD** after an offline start
is a fail.

---

## 7. After the run

Every cell in `MVP-EVIDENCE.md` needs a result on Windows and macOS. Then
the rows count towards these MVP rows in MVP-PLAN Part 1:

| MVP row | What it needs | Smoke rows |
|---|---|---|
| M5 | the app accepts the key | 1 |
| M6 | a real Gmail, iCloud, Yahoo, Fastmail and custom-domain mailbox links; Outlook if C0 to C4 ship | 2, 3, 4 (Outlook column: 2 only, until C4) |
| M7 | read, send, reply, forward with attachments, archive, delete, move, seen in webmail | 6, 7, 8, 9, 10, 11, 12, 13, 14, 15 |
| M8 | AI summary on Pro | 17 (with [D5](MVP-PLAN.md#d5)) |
| M9 | keeps working offline | 18, 19 (renewal is [D5](MVP-PLAN.md#d5)'s) |
| M11 | the password never leaves the machine | 5, 20 |

Rows 5 and 20 are also the "OS keychains" line of MVP-PLAN Part 3, and row
16 is its "OS notifications" line. Rows X1 to X7 map to no MVP row.

Then:

1. Check every failure has an issue linked in its cell.
2. Tell the project manager the run is done. Each failure becomes a bug
   card with its error text.
3. B2 is done when every row has a result ([B2](MVP-PLAN.md#b2)).
