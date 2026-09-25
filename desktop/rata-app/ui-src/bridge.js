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
      /* Nothing was read, because this copy is not licensed. Said as an error
         so the interface leaves what it has alone — building an answer from
         the empty lists below would report every mailbox as live and freshly
         synced, with "up to date" on top. */
      if (res.unlicensed) return { error: res.unlicensed, unlicensed: true };
      const accounts = [];
      for (const p of res.skipped) {
        accounts.push({ email: p.email, label: p.email, count: 0, error: p.error, needsRelink: true, deferred: false });
      }
      for (const p of res.problems) {
        /* "auth": the server refused the password. "missing": the keychain has
           no password for it. Both are fixed by relinking. "keychain": the
           keychain would not open yet — nothing to relink, it clears itself. */
        const needsRelink = p.kind === 'auth' || p.kind === 'missing';
        accounts.push({ email: p.email, label: p.email, count: 0, error: p.error, needsRelink, deferred: false });
      }
      const counts = {};
      for (const m of res.messages) counts[m.acct] = (counts[m.acct] || 0) + 1;
      for (const box of await invoke('list_mailboxes')) {
        if (accounts.some((a) => a.email === box.email)) continue;
        accounts.push({ email: box.email, label: box.label, count: counts[box.email] || 0, error: null, needsRelink: false, deferred: false });
      }
      const failures = accounts.filter((a) => a.error).map((a) => ({ email: a.email, error: a.error, needsRelink: a.needsRelink }));
      /* Never a top-level error, even when every mailbox failed. The
         interface treats `error` as "nothing to absorb" and returns early,
         which skipped the per-account marking — so with one mailbox (the
         usual case) a refused password produced a toast and never a Relink
         button. Per-account failures travel in `accounts` and `partial`,
         where the interface already words each one correctly. */
      return {
        messages: res.messages.map(asMessage),
        accounts,
        ...(failures.length ? { partial: failures } : {}),
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
      const s = await invoke('licence_status');
      if (!s.licensed) return cannot(s.message);
      return {
        planLabel: s.plan.label,
        limits: { mail: s.limit === null ? null : s.limit, chat: s.plan.chat },
        used: { mail: s.used, chat: 0 },
        remaining: { mail: s.limit === null ? null : Math.max(0, s.limit - s.used), chat: 0 },
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

  /* Signing in with Google, Microsoft or Slack needs a server to receive the
     redirect, and the desktop app is not one. Saying that plainly beats a
     browser window that opens and goes nowhere. */
  const NO_OAUTH =
    'Signing in to Outlook, Slack or Google needs the RATA website. Mailboxes with an app password — which is every IMAP provider, including Gmail and Outlook — link here directly.';

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

  async function renew(current) {
    if (!current) return null;
    try {
      const r = await fetch(RENEW, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ licence: current }),
      });
      const d = await r.json();
      if (d.licensed && d.licence) return invoke('set_licence', { licence: d.licence });
      /* A cancelled subscription is a real answer and the app should stop
         asking. A server having a bad morning is not — the licence still has
         days left on it, so nothing is touched and it tries again tomorrow. */
      if (d.reason === 'no-subscription') return { licensed: false, message: d.message };
      return null;
    } catch {
      /* Offline. Exactly the case the whole design exists for. */
      return null;
    }
  }

  /* Ask for the key, once, when there is no usable licence.

     Deliberately part of the desktop bridge rather than the interface: the
     website has no licence key to type, and giving app.html a field that only
     ever appears in one of the two builds is how one interface becomes two. */
  function askForKey(standing) {
    if (document.getElementById('rata-licence')) return;
    const wrap = document.createElement('div');
    wrap.id = 'rata-licence';
    /* Classes, never a style attribute. Tauri nonces the page's style-src, and
       a nonce in style-src makes CSP ignore 'unsafe-inline' — style attributes
       cannot carry a nonce, so they are dropped. See ui-src/bridge.css. */
    wrap.className = 'rata-lic';
    wrap.innerHTML = `
      <div class="rata-lic-card">
        <h2>Your licence key</h2>
        <p id="rata-licence-why"></p>
        <input id="rata-licence-input" placeholder="v1.…" autocomplete="off" spellcheck="false">
        <p class="rata-lic-error" id="rata-licence-error"></p>
        <button id="rata-licence-save">Use this licence</button>
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
    input.focus();

    async function submit() {
      err.textContent = '';
      save.disabled = true;
      /* set_licence rejects when the key cannot be saved at all. Without the
         catch that rejection skipped both the re-enable and the message,
         leaving a dead button and a silent box. */
      try {
        const next = await invoke('set_licence', { licence: input.value.trim() });
        if (next.licensed) {
          wrap.remove();
          location.reload();
          return;
        }
        err.textContent = next.message;
      } catch (e) {
        err.textContent = `That licence could not be saved: ${e}`;
      } finally {
        save.disabled = false;
      }
    }
    save.addEventListener('click', submit);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') submit();
    });
  }

  /* On launch: check, renew if it is getting on, and ask only if there is
     still no licence after that. Asking first would put a box in front of
     somebody whose licence was about to fix itself. */
  async function settleLicence() {
    let standing = await invoke('licence_status');
    if (standing.token && (standing.renewSoon || !standing.licensed)) {
      const after = await renew(standing.token);
      if (after) standing = after.licensed ? after : await invoke('licence_status');
    }
    if (!standing.licensed) askForKey(standing);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', settleLicence);
  } else {
    settleLicence();
  }

  window.__RATA_NATIVE__ = async function (path, opts) {
    const route = ROUTES[path];
    if (route) return route(opts);
    if (path.startsWith('/api/link/') || path.startsWith('/api/sync/')) return cannot(NO_OAUTH);
    return cannot('This copy of RATA does not have that feature.');
  };
})();
