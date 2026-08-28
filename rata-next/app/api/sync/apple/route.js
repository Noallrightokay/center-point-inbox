import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';
import { ImapFlow } from 'imapflow';

export const dynamic = 'force-dynamic';

const strip = s => String(s || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });
  const { data: row } = await sb.from('provider_tokens').select('*')
    .eq('user_id', user.id).eq('provider', 'apple').maybeSingle();
  if (!row) return NextResponse.json({ error: 'iCloud Mail isn\u2019t linked yet — link it in Settings' });

  const client = new ImapFlow({
    host: 'imap.mail.me.com', port: 993, secure: true,
    auth: { user: row.label, pass: row.access }, logger: false,
  });
  const messages = [];
  try {
    await client.connect();
    const lock = await client.getMailboxLock('INBOX');
    try {
      const total = client.mailbox.exists || 0;
      if (total > 0) {
        const from = Math.max(1, total - 14);
        for await (const msg of client.fetch(`${from}:*`, { envelope: true, flags: true, uid: true })) {
          const env = msg.envelope || {};
          const sender = (env.from && env.from[0]) || {};
          let preview = '';
          try {
            const dl = await client.download(msg.uid, 'TEXT', { uid: true, maxBytes: 800 });
            if (dl && dl.content) {
              const chunks = [];
              for await (const c of dl.content) { chunks.push(c); if (Buffer.concat(chunks).length > 800) break; }
              preview = strip(Buffer.concat(chunks).toString('utf8')).slice(0, 200);
            }
          } catch {}
          messages.push({
            id: 'ap_' + msg.uid,
            ch: 'email', prov: 'icloud',
            fromName: sender.name || sender.address || 'Unknown',
            fromAddr: (sender.address || '').toLowerCase(),
            subj: env.subject || '(no subject)',
            prev: (preview || '').slice(0, 120),
            body: (preview || '(preview unavailable)') + '\n\n\u2014 Synced from your real iCloud inbox.',
            ts: env.date ? new Date(env.date).getTime() : Date.now(),
            unread: !(msg.flags && msg.flags.has('\\Seen')),
            starred: !!(msg.flags && msg.flags.has('\\Flagged')),
          });
        }
      }
    } finally { lock.release(); }
    await client.logout();
  } catch (e) {
    try { await client.logout(); } catch {}
    return NextResponse.json({ error: 'iCloud sync failed — the app-specific password may have been revoked. Relink in Settings.' });
  }
  messages.sort((a, b) => b.ts - a.ts);
  return NextResponse.json({ label: row.label, messages });
}
