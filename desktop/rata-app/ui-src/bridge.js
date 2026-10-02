/* The seam between the interface and the machine it is running on.

   RATA's interface is one file, and it is the same file on the website and in
   the desktop app. Everything it needs from outside itself goes through one
   function — `apiFetch(path, opts)` — which on the web calls the Next.js API.
   Here it calls into Rust instead, and the interface does not know the
   difference.

   That is the whole reason this file exists rather than a second frontend. Two
   copies of a 250KB interface diverge within a month, and the desktop copy is
   the one nobody remembers to update.

   Nothing here decides anything. It translates a path and a JSON body into a
   command and translates the answer back into the shape the interface already
   expects. Where the desktop genuinely cannot do something, it says so in a
   sentence rather than failing in a way that reads as a bug. */

(function () {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return; // Running on the web; leave apiFetch alone.

  /* The interface's own vocabulary for a message. The Rust side uses its own
     names, and translating here keeps the mail layer from being shaped by one
     particular screen's field names. */
  /* An attachment as the interface draws it: a short type label, the name,
     a readable size, the index a download asks for, and whether it is a
     program named to look like a document (rata_mail::names::looks_disguised),
     which the page labels and asks about before saving. `b` is the size in
     bytes (0 when the engine could not tell), which decides whether a
     picture is small enough to show in the message (H11). */
  function asAttachment(a) {
    const ext = (String(a.name || '').match(/\.([A-Za-z0-9]{1,5})$/) || [])[1];
    const kind = (ext || String(a.mime || '').split('/')[1] || 'file').replace(/[^A-Za-z0-9]/g, '').slice(0, 5).toUpperCase() || 'FILE';
    const n = Number(a.size) || 0;
    const size = !n ? '' : n < 1024 ? n + ' B' : n < 1048576 ? Math.round(n / 1024) + ' KB' : (n / 1048576).toFixed(1) + ' MB';
    return { i: a.index, n: a.name, f: kind, s: size, b: n, mime: a.mime, disguised: a.disguised === true };
  }

  function asMessage(m) {
    /* From the Sent folder: mail the customer sent, from RATA or anywhere
       else. The interface already knows sent mail by `sent` and who it went
       to by `toName`. */
    const sent = m.folder === 'sent';
    /* From Drafts (0.1.31): begun somewhere and not sent. Who it is for, and
       what it answers, are what finishing it here starts from. */
    const draft = m.folder === 'drafts';
    /* One of the customer's own folders arrives as {named: <server name>};
       the interface keeps it as folder 'named' with the name in `box`, and
       sends {named: box} back whenever it asks about the message. */
    const named = !!m.folder && typeof m.folder === 'object' && typeof m.folder.named === 'string';
    return {
      id: m.id,
      folder: named ? 'named' : m.folder || 'inbox',
      ...(named ? { box: m.folder.named } : {}),
      ...(sent ? { sent: true, toName: m.to_name || m.to_addr || '', toAddr: m.to_addr || '' } : {}),
      /* draftId (F1): the X-RATA-Draft id of a draft RATA itself saved, so
         finishing it here saves over that copy rather than beside it. */
      ...(draft
        ? { draft: true, toName: m.to_name || m.to_addr || '', toAddr: m.to_addr || '', inReplyTo: m.in_reply_to || '', bcc: m.bcc || [],
            ...(m.draft_id ? { draftId: m.draft_id } : {}) }
        : {}),
      /* Everyone it went to, which Reply all needs (0.1.32) and finishing a
         draft starts from. */
      toAll: m.to_all || [],
      cc: m.cc || [],
      acct: m.acct,
      acctLabel: m.acct_label,
      ch: 'email',
      prov: 'imap',
      fromName: m.from_name,
      fromAddr: m.from_addr,
      subj: m.subject,
      prev: m.preview,
      body: m.body,
      ts: m.ts,
      unread: sent || draft ? false : m.unread,
      starred: m.starred,
      /* The server's own reference, kept so a delete or a mark-read can later
         be done to this exact message in the real mailbox. */
      uid: m.uid,
      uidvalidity: m.uidvalidity,
      /* What a reply needs: the id to thread on, and where the sender asked
         answers to go when that is not the From address. */
      messageId: m.message_id || '',
      replyTo: m.reply_to || '',
      /* What links it into its conversation (H5): the id it answers, and the
         last id of its References, each checked by the engine exactly like
         messageId. Absent when there is none. */
      ...(m.in_reply_to ? { inReplyTo: String(m.in_reply_to) } : {}),
      ...(m.references_last ? { refsLast: String(m.references_last) } : {}),
      /* How to leave the list it came from (List-Unsubscribe): a web address
         Rust has checked as a link, and/or a mailto: it rebuilt. Absent when
         there is none, and on mail stored before it was read. */
      ...(m.unsubscribe && (m.unsubscribe.https || m.unsubscribe.mailto)
        ? { unsub: { ...(m.unsubscribe.https ? { https: String(m.unsubscribe.https) } : {}),
            ...(m.unsubscribe.mailto ? { mailto: String(m.unsubscribe.mailto) } : {}) } }
        : {}),
      /* Decoded from MIME since 0.1.7; anything stored without this was kept
         as it arrived on the wire and is re-read when it can be. */
      bodyV: 2,
      truncated: !!m.truncated,
      atts: (m.attachments || []).map(asAttachment),
      html: !!m.html,
    };
  }

  function body(opts) {
    if (!opts || !opts.body) return {};
    try {
      return JSON.parse(opts.body);
    } catch {
      return {};
    }
  }

  /* An answer the interface treats as "the backend is not there", which is
     exactly right for the things a device cannot do on its own. */
  const cannot = (why) => ({ error: why });

  /* How linking ended, in the interface's words — for a password and for
     Microsoft sign-in alike. */
  function linkedAnswer(res) {
    if (res.outcome === 'ok') {
      const m = res.mailbox;
      return {
        ok: true,
        email: m.email,
        host: m.host,
        port: m.port,
        label: m.label,
        foundBy: m.source,
        auth: m.auth || 'password',
        relinked: false,
      };
    }
    /* Stopped by the customer: nothing to say. */
    if (res.outcome === 'cancelled') return { ok: false, cancelled: true };
    /* "Nothing answered" is the one failure with a next step, and the
       interface has a box for it — so it has to stay distinguishable from
       "your password was wrong". A Microsoft mailbox is the other: its
       password was never sent, and the form switches to Sign in with
       Microsoft (or says why this build cannot). */
    return {
      ok: false,
      error: res.error,
      ...(res.outcome === 'needs-host' ? { needsHost: true } : {}),
      ...(res.outcome === 'microsoft' ? { microsoft: true, configured: !!res.configured } : {}),
    };
  }

  const ROUTES = {
    async '/api/link/mail'(opts) {
      if ((opts?.method || 'GET').toUpperCase() === 'DELETE') {
        const { email } = body(opts);
        /* Answered rather than thrown: the caller keeps the row on screen
           when this fails, and needs the reason to say why. */
        try {
          await invoke('unlink_mailbox', { email });
          return { ok: true, email };
        } catch (e) {
          return { ok: false, error: String(e) };
        }
      }
      const b = body(opts);
      const res = await invoke('link_mailbox', {
        email: b.email,
        password: b.appPassword,
        host: b.host || null,
      });
      return linkedAnswer(res);
    },

    /* Signing in with Microsoft (C2): Rust opens Microsoft's page in the
       browser, waits for it on a one-shot 127.0.0.1 listener and keeps the
       tokens itself. The page names the address and hears how it ended —
       never a token. DELETE stops a sign-in that is waiting; GET says
       whether this build can sign in with Microsoft at all. */
    async '/api/link/microsoft'(opts) {
      const method = (opts?.method || 'GET').toUpperCase();
      if (method === 'DELETE') {
        try {
          return { ok: true, stopped: await invoke('cancel_microsoft') };
        } catch (e) {
          return { ok: false, error: String(e) };
        }
      }
      if (method === 'GET') {
        try {
          return { configured: !!(await invoke('microsoft_ready')) };
        } catch {
          return { configured: false };
        }
      }
      const b = body(opts);
      try {
        return linkedAnswer(await invoke('link_microsoft', { email: String(b.email || '') }));
      } catch (e) {
        return { ok: false, error: String(e) };
      }
    },

    /* What an address is before a password is asked for: its provider, and
       whether it signs in with Microsoft (a company domain at Microsoft 365
       is only known by its DNS). Nothing is dialled. */
    async '/api/link/discover'(opts) {
      const b = body(opts);
      try {
        const f = await invoke('discover_mailbox', { email: String(b.email || '') });
        return { label: f.label || '', microsoft: !!f.microsoft, configured: !!f.configured, note: f.note || '' };
      } catch (e) {
        return { error: String(e) };
      }
    },

    /* `known`: what the interface already holds of each folder, so only
       what is new is downloaded; `flags` comes back for the rest. */
    async '/api/sync/mail'(opts) {
      const b = body(opts);
      /* `only` (0.1.35): the mailboxes to read — the one that said it has new
         mail, or the ones the timer finds due. Absent is all of them. */
      const only = Array.isArray(b.only) && b.only.length ? b.only.map(String) : null;
      const ask = { limit: 15, known: Array.isArray(b.known) ? b.known : null, only };
      /* A command that fails outright (I6) is said as what it is: an error
         the interface shows and leaves its mail alone for. Uncaught, it
         reached the page as a thrown error, which the page read as "this is
         the website" and answered with "download the app". */
      const failed = (e) => ({ error: 'RATA could not refresh your mail: ' + (String((e && typeof e === 'object' ? e.error || e.message : e) || '').replace(/\s+/g, ' ').trim() || 'it gave no reason.') });
      let res, boxes;
      try {
        res = await invoke('refresh_mail', ask);
        /* The licence lapsed while RATA was open (BUG-L): renew it now, and
           read again if that worked, so mail carries on with no relaunch. If
           it did not, settleLicence has put the licence box up. */
        if (res.unlicensed) {
          const after = await settleLicence().catch(() => null);
          if (after && after.licensed) res = await invoke('refresh_mail', ask);
        }
        /* Nothing was read, because this copy is not licensed. Said as an error
           so the interface leaves what it has alone — building an answer from
           the empty lists below would report every mailbox as live and freshly
           synced, with "up to date" on top. */
        if (res.unlicensed) return { error: res.unlicensed, unlicensed: true };
        boxes = await invoke('list_mailboxes');
      } catch (e) {
        return failed(e);
      }
      /* How each mailbox signs in: "oauth" is Microsoft's sign-in (C2), so a
         mailbox that needs it again says "Sign in to Microsoft again", not
         "app password". */
      const authOf = (email) => (boxes.find((x) => x.email === email) || {}).auth || 'password';
      const accounts = [];
      /* "microsoft": Microsoft no longer accepts the sign-in (or it is gone
         from the keychain); only signing in again fixes it. */
      const signIn = (p) => (p.kind === 'microsoft' ? { signIn: 'microsoft' } : {});
      for (const p of res.skipped) {
        accounts.push({ email: p.email, label: p.email, count: 0, error: p.error, needsRelink: true, deferred: false, auth: authOf(p.email), ...signIn(p) });
      }
      for (const p of res.problems) {
        /* "auth": the server refused the password. "missing": the keychain has
           no password for it. Both are fixed by relinking. "keychain": the
           keychain would not open yet — nothing to relink, it clears itself.
           "oauth" and "net" for a Microsoft mailbox are not the customer's to
           fix: a token refused twice, or Microsoft out of reach. */
        const needsRelink = p.kind === 'auth' || p.kind === 'missing' || p.kind === 'microsoft';
        accounts.push({ email: p.email, label: p.email, count: 0, error: p.error, needsRelink, deferred: false, auth: authOf(p.email), ...signIn(p) });
      }
      const counts = {};
      for (const m of res.messages) counts[m.acct] = (counts[m.acct] || 0) + 1;
      for (const box of boxes) {
        if (accounts.some((a) => a.email === box.email)) continue;
        /* A mailbox that was not asked about was not synced, and must not be
           stamped as though it had been. */
        if (only && !only.some((o) => o.toLowerCase() === box.email.toLowerCase())) continue;
        accounts.push({ email: box.email, label: box.label, count: counts[box.email] || 0, error: null, needsRelink: false, deferred: false, auth: box.auth || 'password' });
      }
      const failures = accounts.filter((a) => a.error).map((a) => ({ email: a.email, error: a.error, needsRelink: a.needsRelink, ...(a.signIn ? { signIn: a.signIn } : {}) }));
      /* Never a top-level error, even when every mailbox failed. The
         interface treats `error` as "nothing to absorb" and returns early,
         which skipped the per-account marking — so with one mailbox (the
         usual case) a refused password produced a toast and never a Relink
         button. Per-account failures travel in `accounts` and `partial`,
         where the interface already words each one correctly. */
      return {
        messages: res.messages.map(asMessage),
        flags: res.flags || [],
        gaps: res.gaps || [],
        drafts: res.drafts || [],
        archives: res.archives || [],
        /* What each folder read still holds near its top (BUG-C), so mail
           deleted or moved on another device leaves RATA too. */
        present: res.present || [],
        accounts,
        ...(failures.length ? { partial: failures } : {}),
      };
    },

    /* The mailboxes with a live connection waiting for new mail (0.1.35). */
    async '/api/mail/watching'() {
      try {
        return { emails: await invoke('watching') };
      } catch {
        return { emails: [] };
      }
    },

    /* Do to the real mailbox what was done in RATA. One call per mailbox and
       generation, however many messages — the grouping is the interface's. */
    async '/api/mail/act'(opts) {
      const b = body(opts);
      try {
        return await invoke('change_messages', {
          email: b.email,
          folder: b.folder || 'inbox',
          uids: b.uids,
          uidvalidity: b.uidvalidity,
          action: b.action,
        });
      } catch (e) {
        return { ok: false, kind: 'error', error: String(e) };
      }
    },

    /* One page of history from one mailbox, just older than the oldest
       message RATA already holds for it. */
    async '/api/mail/older'(opts) {
      const b = body(opts);
      try {
        const got = await invoke('older_mail', {
          email: b.email,
          folder: b.folder || 'inbox',
          beforeUid: b.beforeUid,
          uidvalidity: b.uidvalidity,
          limit: b.limit ?? null,
        });
        return { messages: got.map(asMessage) };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    /* Search one mailbox on its server (H6): the newest messages whose
       sender, subject or text has the words in them, and how many matched.
       The inbox only in this version; Rust checks the query and sends it as
       a literal, so the page's words can never change the command. */
    async '/api/mail/search'(opts) {
      const b = body(opts);
      try {
        const got = await invoke('search_mail', {
          email: String(b.email || ''),
          folder: b.folder || 'inbox',
          query: String(b.query || ''),
        });
        return { messages: (got.messages || []).map(asMessage), matched: got.matched || 0 };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    async '/api/mail/open'(opts) {
      const b = body(opts);
      try {
        const got = await invoke('open_message', { email: b.email, folder: b.folder || 'inbox', uid: b.uid, uidvalidity: b.uidvalidity });
        return {
          text: got.text,
          truncated: !!got.truncated,
          atts: (got.attachments || []).map(asAttachment),
          html: got.html || null,
          remoteImages: !!got.remote_images,
        };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    async '/api/mail/attachment'(opts) {
      const b = body(opts);
      try {
        const saved = await invoke('save_attachment', {
          email: b.email,
          folder: b.folder || 'inbox',
          uid: b.uid,
          uidvalidity: b.uidvalidity,
          index: b.index,
          /* The customer's answer to "Save it anyway?"; Rust refuses a
             disguised program without it (kind needs-confirmation). */
          confirmed: b.confirmed === true,
        });
        return { ok: true, path: saved.path, name: saved.name };
      } catch (e) {
        return { ok: false, error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    /* Summaries, translation and task flags, from RATA's AI relay. The
       licence is the credential: the relay only answers a genuine one, and
       only for what its plan includes. The page sends the text it was asked
       about; nothing else goes. */
    async '/api/ai'(opts) {
      const b = body(opts);
      let standing = null;
      try { standing = await invoke('licence_status'); } catch { standing = null; }
      if (!standing || !standing.token) return { error: 'This copy of RATA has no licence to use AI with.' };
      const ask = async (token) => {
        const r = await fetch(AI, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(Object.assign({}, b, { licence: token })),
        });
        return { r, d: await r.json().catch(() => ({})) };
      };
      try {
        let { r, d } = await ask(standing.token);
        /* The relay says the licence has expired (BUG-L): renew it, once,
           and ask again with the new one, rather than showing the customer
           a sentence saying RATA does this by itself. */
        if (d && d.reason === 'expired') {
          const after = await settleLicence(true).catch(() => null);
          if (after && after.licensed && after.token && after.token !== standing.token) ({ r, d } = await ask(after.token));
        }
        if (!r.ok && !d.error) return { error: 'RATA\u2019s AI service could not answer (' + r.status + ').' };
        return d;
      } catch {
        return { error: 'RATA could not reach its AI service. Check the connection and try again.', reason: 'offline' };
      }
    },

    /* A file the interface made, into Downloads (core::save_file). */
    async '/api/file/save'(opts) {
      const b = body(opts);
      try {
        const saved = await invoke('save_file', { name: String(b.name || 'file'), data: String(b.data || '') });
        return { ok: true, path: saved.path, name: saved.name };
      } catch (e) {
        return { ok: false, error: e && e.error ? e.error : String(e) };
      }
    },

    /* Is there a newer RATA (update.rs)? Nothing is downloaded. */
    /* A system notification that new mail arrived. Rust makes the words
       plain before showing them (notify.rs). */
    async '/api/notify'(opts) {
      const b = body(opts);
      try {
        await invoke('notify_mail', { title: String(b.title || ''), body: String(b.body || '') });
        return { ok: true };
      } catch (e) {
        return { ok: false, error: String(e) };
      }
    },

    async '/api/update/check'() {
      try {
        return await invoke('check_update');
      } catch (e) {
        return { error: String(e) };
      }
    },

    /* Download, verify and install it. On success the app restarts and this
       never answers; an answer is always a failure, with why. */
    async '/api/update/install'() {
      try {
        await invoke('install_update');
        return { ok: true };
      } catch (e) {
        return { ok: false, error: e && e.error ? e.error : String(e) };
      }
    },

    /* Settings → Copy diagnostics (H8): one plain-text block for a bug
       report, made in Rust (diagnostics.rs) with no address, password,
       token or key in it. */
    async '/api/diagnostics'() {
      try {
        return { text: String(await invoke('diagnostics')) };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e) };
      }
    },

    /* A web address the customer chose to open. Rust checks it is one. */
    async '/api/link/open'(opts) {
      const b = body(opts);
      try {
        await invoke('open_link', { url: String(b.url || '') });
        return { ok: true };
      } catch (e) {
        return { ok: false, error: e && e.error ? e.error : String(e) };
      }
    },

    /* One attachment's bytes, for the Format Bridge or to show as a picture
       (core::read_attachment). `picture` is what Rust found the bytes to be
       (png, jpeg, gif or webp, and small enough to show), else null: the
       page shows a picture by this alone, never by the name or the type the
       message declared. */
    async '/api/mail/attachment/read'(opts) {
      const b = body(opts);
      try {
        const got = await invoke('read_attachment', { email: b.email, folder: b.folder || 'inbox', uid: b.uid, uidvalidity: b.uidvalidity, index: b.index, confirmed: b.confirmed === true });
        return { ok: true, name: got.name, mime: got.mime, data: got.data, picture: got.picture || null };
      } catch (e) {
        return { ok: false, error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    /* The customer's own folders in one mailbox, and the newest mail of one
       of them — read when the customer opens it, not on every refresh. */
    async '/api/mail/folders'(opts) {
      const b = body(opts);
      try {
        return { folders: await invoke('list_folders', { email: b.email }) };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    async '/api/mail/folder'(opts) {
      const b = body(opts);
      try {
        const got = await invoke('folder_mail', { email: b.email, folder: b.folder, limit: b.limit ?? null });
        return { messages: got.map(asMessage) };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    async '/api/mail/read'(opts) {
      const b = body(opts);
      try {
        const got = await invoke('reread_mail', {
          email: b.email,
          folder: b.folder || 'inbox',
          uids: b.uids || [],
          uidvalidity: b.uidvalidity,
        });
        return { messages: got.map(asMessage) };
      } catch (e) {
        return { error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    async '/api/send/mail'(opts) {
      const b = body(opts);
      try {
        const sent = await invoke('send_mail', {
          draft: {
            from: b.from,
            to: b.to,
            cc: b.cc || '',
            bcc: b.bcc || '',
            subject: b.subject || '',
            body: b.body || '',
            inReplyTo: b.inReplyTo || null,
            attachments: b.attachments || [],
            forward: b.forward || null,
          },
        });
        /* The Message-ID it went with, so the copy the provider files in Sent
           replaces RATA's own record of it rather than doubling it. */
        return { ok: true, via: sent.via, messageId: sent.messageId || '' };
      } catch (e) {
        return { ok: false, error: String(e) };
      }
    },

    /* The composer's draft into the mailbox's Drafts folder (F1), replacing
       RATA's own copy saved before it (`prior`: uid, uidvalidity, draftId),
       which Rust removes only once it has proved that copy carries this
       draft's id. `noPlace`: the mailbox has no Drafts folder. */
    async '/api/mail/draft/save'(opts) {
      const b = body(opts);
      const p = b.prior;
      try {
        const r = await invoke('save_draft', {
          draft: {
            from: b.from,
            to: b.to || '',
            cc: b.cc || '',
            bcc: b.bcc || '',
            subject: b.subject || '',
            body: b.body || '',
            inReplyTo: b.inReplyTo || null,
            attachments: b.attachments || [],
            forward: b.forward || null,
          },
          draftId: String(b.draftId || ''),
          rev: Number(b.rev) || 1,
          prior: p && p.uid && p.uidvalidity ? { uid: p.uid, uidvalidity: p.uidvalidity, draft_id: String(p.draftId || '') } : null,
        });
        if (r.outcome === 'no-place') return { ok: false, noPlace: true, error: r.error };
        return { ok: true, id: r.id, uid: r.uid, uidvalidity: r.uidvalidity, draftId: r.draftId, prior: r.prior };
      } catch (e) {
        return { ok: false, error: e && e.error ? e.error : String(e), kind: e && e.kind };
      }
    },

    async '/api/links'() {
      const s = await invoke('licence_status');
      if (!s.licensed) return cannot(s.message);
      return {
        planLabel: s.plan.label,
        limits: { mail: s.limit === null ? null : s.limit },
        used: { mail: s.used },
        remaining: { mail: s.limit === null ? null : Math.max(0, s.limit - s.used) },
      };
    },

    async '/api/licence'(opts) {
      if ((opts?.method || 'GET').toUpperCase() === 'POST') {
        const b = body(opts);
        return invoke('set_licence', { licence: b.licence ?? null });
      }
      return invoke('licence_status');
    },

    async '/api/account'() {
      return cannot(
        'This copy of RATA keeps everything on this computer, so there is no cloud account to manage here. Remove a mailbox with Unlink, or uninstall the app to remove everything.'
      );
    },
  };

  /* Renewal.

     The licence is good for thirty days with no contact at all, and this is how
     the thirty-first day arrives without anybody noticing. The old token is the
     credential — it is signed with a key only the server holds, so presenting
     one proves where it came from, and an expired one is accepted because
     renewing an expired licence is the entire job.

     Done here rather than in Rust on purpose: the app then needs no HTTP client
     at all, and the one address it may contact is the one line in the content
     security policy that allows it. */
  const RENEW = 'https://mailrata.org/api/licence/renew';
  const AI = 'https://mailrata.org/api/ai';
  /* How often an open RATA looks at its licence (BUG-L). It used to look only
     at launch, so a copy started more than a week before expiry and left
     open past it stopped fetching mail with no word. The harness shortens
     this with window.__RATA_RENEW_EVERY. */
  const RENEW_EVERY = Number(window.__RATA_RENEW_EVERY) > 0 ? Number(window.__RATA_RENEW_EVERY) : 6 * 3600e3;
  /* How long a renewal may take before it counts as unreachable. */
  const RENEW_WAIT = 20e3;

  /* Ask mailrata.org for a fresh licence. Answers the standing after a
     renewal that worked, `{ licensed: false, message }` for a real refusal,
     and null for anything else, which leaves the licence as it was. */
  async function renew(current) {
    if (!current) return null;
    const stop = typeof AbortController === 'function' ? new AbortController() : null;
    const timer = stop ? setTimeout(() => stop.abort(), RENEW_WAIT) : null;
    try {
      const r = await fetch(RENEW, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ licence: current }),
        ...(stop ? { signal: stop.signal } : {}),
      });
      const d = await r.json();
      if (d.licensed && d.licence) {
        /* Rust keeps the new token only if it verifies (set_licence): one
           signed with the wrong key, from a website set up wrongly, is
           refused and the licence already here stays in use. That is a
           server having a bad morning, not an answer. */
        const kept = await invoke('set_licence', { licence: d.licence });
        return kept && kept.licensed && !kept.refused ? kept : null;
      }
      /* A cancelled subscription is a real answer and the app should stop
         asking. So is a licence that expired too long ago to renew itself
         (RENEW_GRACE_DAYS on the website): only signing in gets a new one,
         and the server's sentence says so. A server having a bad morning is
         not — the licence still has days left on it, so nothing is touched
         and it tries again later. */
      if (d.reason === 'no-subscription' || d.reason === 'too-old') return { licensed: false, message: d.message };
      return null;
    } catch {
      /* Offline. Exactly the case the whole design exists for. */
      return null;
    } finally {
      if (timer) clearTimeout(timer);
    }
  }

  /* Check the licence, renew it if it is due, and tell the page: the
     `rata-standing` event, and the licence box when there is still no usable
     licence (or its removal once there is). At launch, every six hours, when
     the connection comes back, when a refresh answers "unlicensed", when the
     AI relay answers "expired" (`force`: renew even if this computer's clock
     thinks the licence is good), and from the box. One at a time: a second
     caller shares the one in flight. Asking first would put a box in front
     of somebody whose licence was about to fix itself.

     Answers the standing, with `renewal`: 'renewed', 'refused' (a real
     answer from mailrata.org), 'unreachable', or 'none' (not due). */
  let SETTLING = null;
  function settleLicence(force) {
    if (SETTLING) return SETTLING;
    SETTLING = (async () => {
      let standing = await invoke('licence_status');
      let renewal = 'none';
      if (standing.token && (force === true || standing.renewSoon || !standing.licensed)) {
        const after = await renew(standing.token);
        renewal = !after ? 'unreachable' : after.licensed ? 'renewed' : 'refused';
        standing = after && after.licensed ? after : await invoke('licence_status');
        /* Still not licensed after a real refusal: the licence box says why in
           the server's words ("Sign in at mailrata.org to get a new one"),
           not only that the licence has expired. */
        if (after && !after.licensed && !standing.licensed && after.message) {
          standing = Object.assign({}, standing, { message: after.message });
        }
      }
      if (!standing.licensed) askForKey(standing);
      else closeBox();
      /* The interface asked at start too; a renewal just now may have changed
         the answer (a plan changed at renewal, say), so it hears the latest. */
      window.dispatchEvent(new CustomEvent('rata-standing', { detail: standing }));
      return Object.assign({}, standing, { renewal });
    })().finally(() => {
      SETTLING = null;
    });
    return SETTLING;
  }

  /* What the box says when mailrata.org could not be reached. */
  const UNREACHABLE = 'RATA could not reach mailrata.org to renew it. It tries again when the connection comes back, or press Try again.';

  function closeBox() {
    const box = document.getElementById('rata-licence');
    if (box) box.remove();
  }

  /* Ask for the key when there is no usable licence, or say the latest in
     the box already showing.

     Deliberately part of the desktop bridge rather than the interface: the
     website has no licence key to type, and giving app.html a field that only
     ever appears in one of the two builds is how one interface becomes two. */
  function askForKey(standing) {
    const shown = document.getElementById('rata-licence');
    if (shown) {
      shown.querySelector('#rata-licence-why').textContent = standing.message;
      shown.querySelector('#rata-licence-retry').hidden = !standing.token;
      return;
    }
    const wrap = document.createElement('div');
    wrap.id = 'rata-licence';
    /* Classes, never a style attribute. Tauri nonces the page's style-src, and
       a nonce in style-src makes CSP ignore 'unsafe-inline' — style attributes
       cannot carry a nonce, so they are dropped. See ui-src/bridge.css. */
    wrap.className = 'rata-lic';
    /* A dialog to a screen reader too (H10): modal, named by its heading,
       its errors announced. app.html keeps Tab inside it. */
    wrap.setAttribute('role', 'dialog');
    wrap.setAttribute('aria-modal', 'true');
    wrap.setAttribute('aria-labelledby', 'rata-licence-title');
    wrap.setAttribute('aria-describedby', 'rata-licence-why');
    wrap.innerHTML = `
      <div class="rata-lic-card">
        <h2 id="rata-licence-title">Your licence key</h2>
        <p id="rata-licence-why"></p>
        <input id="rata-licence-input" aria-label="Licence key" placeholder="v1.…" autocomplete="off" spellcheck="false">
        <p class="rata-lic-error" id="rata-licence-error" role="alert"></p>
        <button id="rata-licence-save">Use this licence</button>
        <button id="rata-licence-retry" class="rata-lic-retry" hidden>Try again</button>
        <p class="rata-lic-note">
          Sign in at <a href="https://mailrata.org/account">mailrata.org</a>
          to find your key. RATA keeps working offline for thirty days at a time.
        </p>
      </div>`;
    document.body.appendChild(wrap);
    wrap.querySelector('#rata-licence-why').textContent = standing.message;

    const input = wrap.querySelector('#rata-licence-input');
    const err = wrap.querySelector('#rata-licence-error');
    const save = wrap.querySelector('#rata-licence-save');
    const retry = wrap.querySelector('#rata-licence-retry');
    /* Try again: renew the licence already here. Only offered when there is
       one to renew. */
    retry.hidden = !standing.token;
    input.focus();

    /* Renew the licence here, and say what came of it. Answers true when
       RATA is licensed afterwards (settleLicence has closed the box). */
    async function renewHere(kept) {
      const after = await settleLicence(true);
      if (after.licensed) return true;
      err.textContent = kept || (after.renewal === 'unreachable' ? UNREACHABLE : '');
      return false;
    }

    async function submit() {
      err.textContent = '';
      save.disabled = true;
      retry.disabled = true;
      /* A key wrapped by a mail client arrives with line breaks inside it;
         white space is never part of one (Rust strips it too). */
      const key = input.value.replace(/\s+/g, '');
      /* set_licence rejects when the key cannot be saved at all. Without the
         catch that rejection skipped both the re-enable and the message,
         leaving a dead button and a silent box. */
      try {
        let next;
        if (key) {
          next = await invoke('set_licence', { licence: key });
          if (next.licensed) {
            wrap.remove();
            location.reload();
            return;
          }
        } else {
          next = await invoke('licence_status');
          if (!next.token) {
            err.textContent = 'Paste the key from your mailrata.org account.';
            return;
          }
        }
        /* The box says RATA renews an expired licence by itself, so it does
           so now, whether the licence is the one just pasted or the one RATA
           kept (Rust never lets a bad paste replace one that can renew; the
           answer's `refused` says so). */
        const kept = next.refused ? next.refused.message : '';
        if (next.reason === 'expired' && next.token) {
          await renewHere(kept);
          return;
        }
        err.textContent = kept || next.message;
      } catch (e) {
        err.textContent = `That licence could not be saved: ${e}`;
      } finally {
        save.disabled = false;
        retry.disabled = false;
      }
    }
    save.addEventListener('click', submit);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') submit();
    });
    retry.addEventListener('click', async () => {
      err.textContent = '';
      retry.disabled = true;
      save.disabled = true;
      try {
        await renewHere('');
      } catch (e) {
        err.textContent = String(e);
      } finally {
        retry.disabled = false;
        save.disabled = false;
      }
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => settleLicence());
  } else {
    settleLicence();
  }
  /* While RATA is open: every six hours, and whenever the connection comes
     back. Each renews only when the licence is due (its last week, or
     expired); otherwise it only re-reads the standing. */
  setInterval(() => settleLicence().catch(() => {}), RENEW_EVERY);
  window.addEventListener('online', () => settleLicence().catch(() => {}));

  /* Every web link on the app's own page — the licence page, checkout, a
     link in the text of a message — opens in the browser. Followed in place,
     it would replace the app with a web page and no way back; links.rs
     refuses that navigation too, and this is the half that still opens the
     page. A handler on the page that claims the click first (preventDefault)
     keeps it. Links inside an HTML message never reach here: that frame is
     another document, and its clicks arrive through Rust. */
  document.addEventListener('click', (e) => {
    if (e.defaultPrevented || e.button !== 0) return;
    const a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (!/^https?:\/\//i.test(href)) return;
    e.preventDefault();
    invoke('open_link', { url: href }).catch((err) => {
      if (typeof window.toast === 'function') window.toast(err && err.error ? err.error : String(err));
    });
  });

  window.__RATA_NATIVE__ = async function (path, opts) {
    const route = ROUTES[path];
    if (route) return route(opts);
    return cannot('This copy of RATA does not have that feature.');
  };
})();
