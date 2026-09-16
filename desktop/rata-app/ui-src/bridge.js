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
    wrap.setAttribute('style', [
      'position:fixed;inset:0;z-index:99999',
      'background:rgba(15,15,20,.72)',
      'display:flex;align-items:center;justify-content:center',
      'font:15px/1.55 system-ui,-apple-system,Segoe UI,sans-serif',
    ].join(';'));
    wrap.innerHTML = `
      <div style="background:#fff;color:#18181b;max-width:420px;width:calc(100% - 40px);
                  padding:28px;border-radius:14px;box-shadow:0 20px 60px rgba(0,0,0,.3)">
        <h2 style="margin:0 0 10px;font-size:19px">Your licence key</h2>
        <p style="margin:0 0 16px;color:#52525b" id="rata-licence-why"></p>
        <input id="rata-licence-input" placeholder="v1.…" autocomplete="off" spellcheck="false"
               style="width:100%;box-sizing:border-box;padding:11px 12px;border:1px solid #d4d4d8;
                      border-radius:9px;font:13px ui-monospace,SFMono-Regular,Menlo,monospace">
        <p id="rata-licence-error" style="margin:10px 0 0;color:#b91c1c;min-height:20px;font-size:13px"></p>
        <div style="display:flex;gap:10px;margin-top:14px">
          <button id="rata-licence-save"
                  style="flex:1;padding:11px;border:0;border-radius:9px;background:#18181b;color:#fff;
                         font-weight:600;cursor:pointer">Use this licence</button>
        </div>
        <p style="margin:16px 0 0;font-size:13px;color:#71717a">
          Sign in at <a href="https://mailrata.org/account" style="color:#18181b">mailrata.org</a>
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
      const next = await invoke('set_licence', { licence: input.value.trim() });
      save.disabled = false;
      if (next.licensed) {
        wrap.remove();
        location.reload();
        return;
      }
      err.textContent = next.message;
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
