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
   expects. Where the desktop genuinely cannot do something — OAuth sign-in
   needs a server to receive the callback — it says so in a sentence rather
   than failing in a way that reads as a bug. */

(function () {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return; // Running on the web; leave apiFetch alone.

  /* The interface's own vocabulary for a message. The Rust side uses its own
     names, and translating here keeps the mail layer from being shaped by one
     particular screen's field names. */
  function asMessage(m) {
    return {
      id: m.id,
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
      unread: m.unread,
      starred: m.starred,
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

  const ROUTES = {
    async '/api/link/mail'(opts) {
      if ((opts?.method || 'GET').toUpperCase() === 'DELETE') {
        const { email } = body(opts);
        await invoke('unlink_mailbox', { email });
        return { ok: true, email };
      }
      const b = body(opts);
      const res = await invoke('link_mailbox', {
        email: b.email,
        password: b.appPassword,
        host: b.host || null,
      });
      if (res.outcome === 'ok') {
        const m = res.mailbox;
        return {
          ok: true,
          email: m.email,
          host: m.host,
          port: m.port,
          label: m.label,
          foundBy: m.source,
          relinked: false,
        };
      }
      /* "Nothing answered" is the one failure with a next step, and the
         interface has a box for it — so it has to stay distinguishable from
         "your password was wrong". */
      return {
        ok: false,
        error: res.error,
        ...(res.outcome === 'needs-host' ? { needsHost: true } : {}),
      };
    },

    async '/api/sync/mail'() {
      const res = await invoke('refresh_mail', { limit: 15 });
      const accounts = [];
      for (const p of res.skipped) {
        accounts.push({ email: p.email, label: p.email, count: 0, error: p.error, needsRelink: true, deferred: false });
      }
      for (const p of res.problems) {
        accounts.push({ email: p.email, label: p.email, count: 0, error: p.error, needsRelink: p.kind === 'auth', deferred: false });
      }
      const counts = {};
      for (const m of res.messages) counts[m.acct] = (counts[m.acct] || 0) + 1;
      for (const box of await invoke('list_mailboxes')) {
        if (accounts.some((a) => a.email === box.email)) continue;
        accounts.push({ email: box.email, label: box.label, count: counts[box.email] || 0, error: null, needsRelink: false, deferred: false });
      }
      const failures = accounts.filter((a) => a.error).map((a) => ({ email: a.email, error: a.error, needsRelink: a.needsRelink }));
      return {
        messages: res.messages.map(asMessage),
        accounts,
        ...(failures.length ? { partial: failures } : {}),
        /* Only a refresh where every mailbox failed is an error. One that lost
           a single account still delivered the rest. */
        ...(failures.length && failures.length === accounts.length ? { error: failures[0].error } : {}),
      };
    },

    async '/api/send/mail'(opts) {
      const b = body(opts);
      try {
        const via = await invoke('send_mail', {
          from: b.from,
          to: b.to,
          subject: b.subject || '',
          body: b.body || '',
          inReplyTo: b.inReplyTo || null,
        });
        return { ok: true, via };
      } catch (e) {
        return { ok: false, error: String(e) };
      }
    },

    async '/api/links'() {
      /* Honest rather than convenient. Plan limits are enforced by whoever
         issues the licence, and the device does not check one yet — so this
         says it cannot answer instead of reporting "no limit", which would be
         a claim rather than a fact. */
      return cannot('Plan limits are not checked on this device yet.');
    },

    async '/api/account'() {
      return cannot(
        'This copy of RATA keeps everything on this computer, so there is no cloud account to manage here. Remove a mailbox with Unlink, or uninstall the app to remove everything.'
      );
    },
  };

  /* Signing in with Google, Microsoft or Slack needs a server to receive the
     redirect, and the desktop app is not one. Saying that plainly beats a
     browser window that opens and goes nowhere. */
  const NO_OAUTH =
    'Signing in to Outlook, Slack or Google needs the RATA website. Mailboxes with an app password — which is every IMAP provider, including Gmail and Outlook — link here directly.';

  window.__RATA_NATIVE__ = async function (path, opts) {
    const route = ROUTES[path];
    if (route) return route(opts);
    if (path.startsWith('/api/link/') || path.startsWith('/api/sync/')) return cannot(NO_OAUTH);
    return cannot('This copy of RATA does not have that feature.');
  };
})();
