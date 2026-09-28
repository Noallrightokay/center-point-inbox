# MVP evidence

What has been seen working on a real system, for each row of
[MVP-PLAN Part 1](MVP-PLAN.md#part-1-what-mvp-means). Each task that
proves a row adds its own section here. The MVP is done only when every
Part 1 row links to a result in this file ([G3](MVP-PLAN.md#g3)).

---

## B2: live provider smoke

The checklist is [`SMOKE.md`](SMOKE.md). Row numbers match it.

### What goes in a cell

One line per machine, separated by `<br>`:

```
<version> · <OS> · <result>
```

- **version**: from the window title, for example `0.1.38`.
- **OS**: for example `Win 11`, `macOS 15 arm`, `Ubuntu 24.04`.
- **result**: one of
  - `pass`, with the short note the row asks for (for example
    `pass, 4 s` for row 15, or `pass, Inbox ok, Sent ok, …` for row 4);
  - `fail · "<exact error text>" · #<issue>`. The error text is copied,
    never retyped or summarised. If it is too long for the cell, put it
    under **Failures** below and write `fail, see F<n> · #<issue>`;
  - `expected fail · "<exact text>"` (Outlook.com, row 2, until
    [C1](MVP-PLAN.md#c1) to [C4](MVP-PLAN.md#c4) ship);
  - `blocked (<task>)`, for example `blocked (D4)` for row 17 before the
    website is deployed, with the note RATA showed;
  - `n/a`, with the reason (for example Outlook rows 3 to 20 after row 2
    failed as expected).

Example: `0.1.38 · Win 11 · pass<br>0.1.38 · macOS 15 arm · fail · "…" · #61`

### What must never go in

This repository is public. Never write here, or in an issue:

- a password or app password, or any part of one;
- a licence key, or the contents of `mailboxes.json` (it holds the key);
- message content: the subject, text, sender or attachment names of real
  mail. Use the `RATA smoke …` test subjects from SMOKE.md;
- any address other than the tester's own. Write `me@<provider>` if you
  would rather not show your own.

### Runs

| Date | Machine and OS | Version | Who | Notes |
|---|---|---|---|---|
| | | | | |

### Results

Row 1 and row 17 are once per machine: use the Gmail column.

| # | Row | Gmail | iCloud | Yahoo | Fastmail | Hostinger domain | Outlook.com |
|---|---|---|---|---|---|---|---|
| 1 | Licence accepted | | n/a | n/a | n/a | n/a | n/a |
| 2 | Link the mailbox | | | | | | |
| 3 | The provider it named | | | | | | |
| 4 | First sync, per folder | | | | | | |
| 5 | Keychain and `mailboxes.json` | | | | | | |
| 6 | Open an HTML message | | | | | | |
| 7 | Send to self with an attachment | | | | | | |
| 8 | Save an attachment | | | | | | |
| 9 | Reply | | | | | | |
| 10 | Forward with the attachment | | | | | | |
| 11 | Mark read, then unread | | | | | | |
| 12 | Archive | | | | | | |
| 13 | Delete | | | | | | |
| 14 | Move to a folder | | | | | | |
| 15 | New mail within a minute | | | | | | |
| 16 | Notification | | | | | | |
| 17 | Summarize (Pro) | | n/a | n/a | n/a | n/a | n/a |
| 18 | Relaunch keeps mail | | | | | | |
| 19 | Offline start | | | | | | |
| 20 | Remove deletes the keychain entry | | | | | | |
| X1 | Load older mail (optional) | | | | | | |
| X2 | Reply all, Cc and Bcc (optional) | | | | | | |
| X3 | Continue a draft (optional) | | | | | | |
| X4 | Not spam (optional) | | | | | | |

### Failures

Error text too long for its cell. One entry per failure.

| Id | Row, column | Version, OS | Exact error text | Issue |
|---|---|---|---|---|
| | | | | |
