import { NextResponse } from 'next/server';
import nodemailer from 'nodemailer';
import { userFromRequest } from '../../../../lib/server';
import { mailKey, smtpCandidates, SMTP_PORTS, isMailKey } from '../../../../lib/mail';
import { decryptSecret } from '../../../../lib/secrets';

export const dynamic = 'force-dynamic';

/* Send from a mailbox the user has linked.

   Reading somebody's mail without being able to answer it is half a product —
   and a message dragged between two inboxes has to leave from the one it
   landed on, or the transfer is theatre. The app password already stored for
   reading is used for submission too, which is how these providers work.

   The message is sent by the user's own mail server, from their own address, so
   it lands in their Sent folder and passes SPF like anything else they send. */
export async function POST(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  let body;
  try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }
  const from = String(body.from || '').trim().toLowerCase();
  const to = String(body.to || '').trim();
  const subject = String(body.subject || '').slice(0, 400);
  const text = String(body.body || '');

  if (!/.+@.+\..+/.test(from)) return NextResponse.json({ error: 'Which account should this come from?' });
  if (!/.+@.+\..+/.test(to)) return NextResponse.json({ error: 'Enter a valid recipient address' });

  const { data: row } = await sb.from('provider_tokens')
    .select('provider,label,access,extra')
    .eq('user_id', user.id).eq('provider', mailKey(from)).maybeSingle();

  if (!row || !isMailKey(row.provider)) {
    return NextResponse.json({ error: `${from} is not linked — add it in Accounts first` });
  }

  const pass = decryptSecret(row.access, user.id, row.provider);
  if (!pass) return NextResponse.json({ error: `${from} could not be unlocked — relink it in Accounts.` });

  const hosts = smtpCandidates(row.extra?.host, from);
  let lastError = '';
  let refused = false;

  outer:
  for (const host of hosts) {
    for (const { port, secure } of SMTP_PORTS) {
      const tx = nodemailer.createTransport({
        host, port, secure,
        auth: { user: from, pass },
        connectionTimeout: 12000, greetingTimeout: 10000, socketTimeout: 20000,
      });
      try {
        const info = await tx.sendMail({ from, to, subject, text });
        return NextResponse.json({ ok: true, id: info.messageId, via: `${host}:${port}` });
      } catch (e) {
        lastError = String(e?.message || 'send failed');
        /* A refusal is an answer: the server exists and said no, so trying
           another port or another name would only repeat it — and repeating a
           bad password is how accounts get locked. */
        if (/auth|credential|password|535|534/i.test(lastError)) { refused = true; break outer; }
      } finally {
        try { tx.close(); } catch {}
      }
    }
  }
  const host = hosts[0];

  return NextResponse.json({
    error: refused
      ? `${from} refused the sign-in for sending. Some providers need a separate app password for mail apps — regenerate it and relink in Accounts.`
      : `Could not reach ${host} to send. Check with your provider that SMTP is enabled for this account.`,
  });
}
