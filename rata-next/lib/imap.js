import { ImapFlow } from 'imapflow';

/* ---------------------------------------------------------------------------
   IMAP mail providers.

   This is the fallback path for mail we cannot reach over OAuth. Apple
   publishes no OAuth for Mail at all. Gmail does, but its read scopes are
   "restricted": opening one-click Gmail to the public requires Google
   verification plus an annual third-party CASA security assessment. IMAP with
   an app-specific password needs neither, so it is what lets a real user
   connect their Gmail today.

   Both providers want the same thing — an address and a revocable
   app-specific password — so they share one implementation.
   --------------------------------------------------------------------------- */

export const IMAP_PROVIDERS = {
  apple: {
    token: 'apple',                 // provider_tokens.provider
    prov: 'icloud',                 // message badge in the client
    label: 'iCloud Mail',
    host: 'imap.mail.me.com',
    help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords',
    reject: 'iCloud rejected the sign-in — check the address and app-specific password (not your Apple ID password)',
  },
  gmail: {
    token: 'gmail_imap',            // distinct from the browser-side OAuth path
    prov: 'gmail',
    label: 'Gmail',
    host: 'imap.gmail.com',
    help: 'myaccount.google.com/apppasswords (needs 2-Step Verification on)',
    reject: 'Google rejected the sign-in — use a 16-character App Password, not your Google account password. App Passwords require 2-Step Verification.',
  },
};

const PORT = 993;

export function imapProvider(name) {
  return Object.prototype.hasOwnProperty.call(IMAP_PROVIDERS, name) ? IMAP_PROVIDERS[name] : null;
}

function client(def, email, pass) {
  return new ImapFlow({ host: def.host, port: PORT, secure: true, auth: { user: email, pass }, logger: false });
}

/* Prove the credentials work before storing them, so a typo surfaces as a
   sign-in error rather than an inbox that silently never syncs. */
export async function verifyImap(def, email, pass) {
  const c = client(def, email, pass);
  try {
    await c.connect();
    await c.logout();
    return { ok: true };
  } catch {
    try { await c.logout(); } catch {}
    return { ok: false, error: def.reject };
  }
}

const strip = s => String(s || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

export async function fetchInbox(def, email, pass, { limit = 15 } = {}) {
  const c = client(def, email, pass);
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
            id: `${def.token}_${msg.uid}`,
            ch: 'email', prov: def.prov,
            fromName: sender.name || sender.address || 'Unknown',
            fromAddr: (sender.address || '').toLowerCase(),
            subj: env.subject || '(no subject)',
            prev: (preview || '').slice(0, 120),
            body: (preview || '(preview unavailable)') + `\n\n— Synced from your real ${def.label} inbox.`,
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
    return { error: `${def.label} sync failed — the app-specific password may have been revoked. Relink it in Settings.` };
  }
  messages.sort((a, b) => b.ts - a.ts);
  return { messages };
}
