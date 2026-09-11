import { ImapFlow } from 'imapflow';

/* ---------------------------------------------------------------------------
   Mail accounts — any provider, more than one of them.

   RATA reaches mail over IMAP with an app-specific password. That is not a
   fallback for Gmail and iCloud alone: it is the one mechanism every mail host
   on earth supports, needs no per-provider OAuth app, no verification review,
   and no annual CASA assessment. A user brings an address and a revocable
   password and their mail appears — whether that address is at Gmail, Fastmail,
   Zoho, or their accountant's own server.

   Two things this file exists to solve:

   1. Finding the server. People know their email address; almost nobody knows
      their IMAP hostname. Known domains resolve from the table below, and
      anything else is probed at the conventional names before we give up and
      ask. The user can always override.

   2. Holding more than one account. provider_tokens is keyed (user_id,
      provider), so a literal provider name like 'gmail_imap' allows exactly one
      Gmail per person. Each account therefore gets its own synthetic provider
      key — 'mail:<slug of the address>' — which keeps one row per account
      inside the existing primary key. No migration, and the address stays
      readable in the table.
   --------------------------------------------------------------------------- */

export const MAIL_HOSTS = {
  'gmail.com':      { host: 'imap.gmail.com',        label: 'Gmail',        help: 'myaccount.google.com/apppasswords — needs 2-Step Verification on' },
  'googlemail.com': { host: 'imap.gmail.com',        label: 'Gmail',        help: 'myaccount.google.com/apppasswords — needs 2-Step Verification on' },
  'icloud.com':     { host: 'imap.mail.me.com',      label: 'iCloud Mail',  help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  'me.com':         { host: 'imap.mail.me.com',      label: 'iCloud Mail',  help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  'mac.com':        { host: 'imap.mail.me.com',      label: 'iCloud Mail',  help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  'outlook.com':    { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'hotmail.com':    { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'live.com':       { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'msn.com':        { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'yahoo.com':      { host: 'imap.mail.yahoo.com',   label: 'Yahoo Mail',   help: 'login.yahoo.com → Account security → Generate app password' },
  'ymail.com':      { host: 'imap.mail.yahoo.com',   label: 'Yahoo Mail',   help: 'login.yahoo.com → Account security → Generate app password' },
  'aol.com':        { host: 'imap.aol.com',          label: 'AOL Mail',     help: 'login.aol.com → Account security → Generate app password' },
  'zoho.com':       { host: 'imap.zoho.com',         label: 'Zoho Mail',    help: 'accounts.zoho.com → Security → App passwords' },
  'fastmail.com':   { host: 'imap.fastmail.com',     label: 'Fastmail',     help: 'fastmail.com → Settings → Privacy & Security → App passwords' },
  'fastmail.fm':    { host: 'imap.fastmail.com',     label: 'Fastmail',     help: 'fastmail.com → Settings → Privacy & Security → App passwords' },
  'gmx.com':        { host: 'imap.gmx.com',          label: 'GMX',          help: 'Enable IMAP in GMX settings' },
  'gmx.net':        { host: 'imap.gmx.net',          label: 'GMX',          help: 'Enable IMAP in GMX settings' },
  'mail.com':       { host: 'imap.mail.com',         label: 'Mail.com',     help: 'Enable IMAP in your Mail.com settings' },
  'web.de':         { host: 'imap.web.de',           label: 'WEB.DE',       help: 'Enable IMAP in your WEB.DE settings' },
  'yandex.com':     { host: 'imap.yandex.com',       label: 'Yandex Mail',  help: 'id.yandex.com → Security → App passwords' },
  'comcast.net':    { host: 'imap.comcast.net',      label: 'Comcast',      help: 'Use your Xfinity password, with third-party access enabled' },
  'att.net':        { host: 'imap.mail.att.net',     label: 'AT&T Mail',    help: 'currently.att.net → Profile → Sign-in info → Manage secure mail key' },
  'verizon.net':    { host: 'imap.aol.com',          label: 'Verizon Mail', help: 'Verizon mail is served by AOL — generate an AOL app password' },
  'rogers.com':     { host: 'imap.broadband.rogers.com', label: 'Rogers',   help: 'Use your Rogers email password' },
  'shaw.ca':        { host: 'imap.shaw.ca',          label: 'Shaw',         help: 'Use your Shaw email password' },
  'bell.net':       { host: 'imap.bell.net',         label: 'Bell',         help: 'Use your Bell email password' },
  'sympatico.ca':   { host: 'imap.bell.net',         label: 'Bell',         help: 'Use your Bell email password' },
};

/* Proton and Tuta encrypt mail client-side, so there is no IMAP server on the
   internet to reach — only a bridge running on the user's own machine, which a
   web app cannot see. Saying so plainly beats a generic timeout. */
export const NO_IMAP = {
  'proton.me':      'Proton Mail',
  'protonmail.com': 'Proton Mail',
  'pm.me':          'Proton Mail',
  'tuta.com':       'Tuta',
  'tutanota.com':   'Tuta',
};

export const PORT = 993;

/* ---------------------------------------------------------------------------
   Sending.

   Reading mail is IMAP; sending it is SMTP, and the same app password almost
   always works for both. Most providers name their submission host by swapping
   imap for smtp, so that is the rule, with a table for the ones that do not
   follow it. Port 465 is implicit TLS and 587 is STARTTLS; providers differ, so
   both are tried rather than asking the user which their host prefers.
   --------------------------------------------------------------------------- */
export const SMTP_HOSTS = {
  'imap.gmail.com':        'smtp.gmail.com',
  'imap.mail.me.com':      'smtp.mail.me.com',
  'outlook.office365.com': 'smtp-mail.outlook.com',
  'imap.mail.yahoo.com':   'smtp.mail.yahoo.com',
  'imap.aol.com':          'smtp.aol.com',
  'imap.zoho.com':         'smtp.zoho.com',
  'imap.fastmail.com':     'smtp.fastmail.com',
  'imap.gmx.com':          'mail.gmx.com',
  'imap.gmx.net':          'mail.gmx.net',
  'imap.mail.com':         'smtp.mail.com',
  'imap.web.de':           'smtp.web.de',
  'imap.yandex.com':       'smtp.yandex.com',
  'imap.comcast.net':      'smtp.comcast.net',
  'imap.mail.att.net':     'smtp.mail.att.net',
  'imap.shaw.ca':          'smtp.shaw.ca',
  'imap.bell.net':         'smtp.bell.net',
  'imap.broadband.rogers.com': 'smtp.broadband.rogers.com',
};

export function smtpHostFor(imapHost, email) {
  if (imapHost && SMTP_HOSTS[imapHost]) return SMTP_HOSTS[imapHost];
  if (imapHost && imapHost.startsWith('imap.')) return 'smtp.' + imapHost.slice(5);
  if (imapHost) return imapHost;
  return 'smtp.' + domainOf(email);
}

/* 465 first: implicit TLS is encrypted from the first byte, where 587 starts in
   the clear and upgrades. If a host only offers 587 we still get there. */
export const SMTP_PORTS = [
  { port: 465, secure: true },
  { port: 587, secure: false },
];


export function domainOf(email) {
  return String(email || '').trim().toLowerCase().split('@')[1] || '';
}

/* The provider key for one account. Lowercased address, everything outside
   [a-z0-9] folded to '_', so it stays a readable single token in the table. */
export function mailKey(email) {
  return 'mail:' + String(email).trim().toLowerCase().replace(/[^a-z0-9]+/g, '_').slice(0, 180);
}

export function isMailKey(provider) {
  return typeof provider === 'string' && provider.startsWith('mail:');
}

/* What we know about an address before anyone types a password: which server
   to try, and where that provider hides its app-password screen. */
export function describe(email) {
  const domain = domainOf(email);
  if (!domain) return null;
  if (NO_IMAP[domain]) {
    return { domain, unsupported: true, label: NO_IMAP[domain],
      why: `${NO_IMAP[domain]} encrypts mail on your device and offers no IMAP server RATA can reach. Their bridge only runs on your own computer.` };
  }
  const known = MAIL_HOSTS[domain];
  if (known) return { domain, ...known, guessed: false };
  return {
    domain,
    host: 'imap.' + domain,
    label: domain,
    help: 'Your mail provider or IT administrator issues this — look for "app password", "IMAP password", or "mail client access".',
    guessed: true,
  };
}

/* Hosts to try for an unknown domain, in the order they are conventionally
   used. Probing beats making someone find their own IMAP hostname. */
export function candidateHosts(email, override) {
  if (override) return [String(override).trim().toLowerCase()];
  const d = domainOf(email);
  const known = MAIL_HOSTS[d];
  if (known) return [known.host];
  return [`imap.${d}`, `mail.${d}`, d];
}

function client(host, email, pass) {
  return new ImapFlow({
    host, port: PORT, secure: true,
    auth: { user: email, pass },
    logger: false,
    /* A wrong hostname should fail in seconds, not hang the request while we
       try three of them. */
    socketTimeout: 20000,
    greetingTimeout: 10000,
    connectionTimeout: 10000,
  });
}

/* Prove the credentials work — and find the server while we are at it — before
   anything is written. A typo then surfaces as a sign-in error rather than an
   inbox that silently never syncs. */
export async function verifyMail(email, pass, override) {
  const info = describe(email);
  if (info?.unsupported) return { ok: false, error: info.why };

  const hosts = candidateHosts(email, override);
  let refused = false;
  for (const host of hosts) {
    const c = client(host, email, pass);
    try {
      await c.connect();
      await c.logout();
      return { ok: true, host, label: info?.label || host };
    } catch (e) {
      try { await c.logout(); } catch {}
      /* Distinguish "no such server" from "server said no": the first means
         keep looking, the second means the password is wrong and trying other
         hostnames would only lock the account faster. */
      if (/auth|credential|login|password|AUTHENTICATIONFAILED/i.test(String(e?.message || ''))) {
        refused = true;
        break;
      }
    }
  }
  if (refused) {
    return { ok: false, error: `${info?.label || 'The mail server'} rejected the sign-in. Use an app password, not your normal account password${info?.help ? ` — ${info.help}` : ''}.` };
  }
  return { ok: false, error: override
    ? `Could not reach ${override}. Check the server address with your mail provider.`
    : `Could not find a mail server for ${info?.domain || 'that address'}. Enter the IMAP server address below — your provider lists it as "IMAP server" or "incoming mail server".` };
}

const strip = s => String(s || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

export async function fetchInbox(acct, { limit = 15 } = {}) {
  const { host, email, pass, label } = acct;
  const c = client(host, email, pass);
  const messages = [];
  try {
    await c.connect();
    const lock = await c.getMailboxLock('INBOX');
    try {
      const total = c.mailbox.exists || 0;
      if (total > 0) {
        const from = Math.max(1, total - (limit - 1));
        for await (const msg of c.fetch(`${from}:*`, { envelope: true, flags: true, uid: true })) {
          const env = msg.envelope || {};
          const sender = (env.from && env.from[0]) || {};
          let preview = '';
          try {
            const dl = await c.download(msg.uid, 'TEXT', { uid: true, maxBytes: 800 });
            if (dl && dl.content) {
              const chunks = [];
              for await (const chunk of dl.content) {
                chunks.push(chunk);
                if (Buffer.concat(chunks).length > 800) break;
              }
              preview = strip(Buffer.concat(chunks).toString('utf8')).slice(0, 200);
            }
          } catch { /* preview is best-effort; the envelope still lands */ }
          messages.push({
            /* The account is part of the id: the same message uid in two
               different mailboxes must not collide in a combined inbox. */
            id: `${mailKey(email)}_${msg.uid}`,
            acct: email,
            acctLabel: label || email,
            ch: 'email',
            prov: 'imap',
            fromName: sender.name || sender.address || 'Unknown',
            fromAddr: (sender.address || '').toLowerCase(),
            subj: env.subject || '(no subject)',
            prev: (preview || '').slice(0, 120),
            body: (preview || '(preview unavailable)') + `\n\n— Synced from ${email}.`,
            ts: env.date ? new Date(env.date).getTime() : Date.now(),
            unread: !(msg.flags && msg.flags.has('\\Seen')),
            starred: !!(msg.flags && msg.flags.has('\\Flagged')),
          });
        }
      }
    } finally { lock.release(); }
    await c.logout();
  } catch {
    try { await c.logout(); } catch {}
    return { error: `${email} did not sync — the app password may have been revoked. Relink it in Accounts.` };
  }
  messages.sort((a, b) => b.ts - a.ts);
  return { messages };
}
