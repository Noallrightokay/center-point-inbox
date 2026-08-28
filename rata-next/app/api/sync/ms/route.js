import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';

export const dynamic = 'force-dynamic';

async function freshToken(sb, row) {
  if (row.expires_at && Date.now() < new Date(row.expires_at).getTime() - 60000) return row.access;
  if (!row.refresh) return row.access;
  const body = new URLSearchParams({
    client_id: process.env.MS_CLIENT_ID,
    client_secret: process.env.MS_CLIENT_SECRET,
    refresh_token: row.refresh, grant_type: 'refresh_token',
    scope: 'offline_access User.Read Mail.Read',
  });
  const tok = await (await fetch('https://login.microsoftonline.com/common/oauth2/v2.0/token', {
    method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body,
  })).json();
  if (!tok.access_token) return null;
  await sb.from('provider_tokens').update({
    access: tok.access_token, refresh: tok.refresh_token || row.refresh,
    expires_at: new Date(Date.now() + (tok.expires_in || 3600) * 1000).toISOString(),
    updated_at: new Date().toISOString(),
  }).eq('user_id', row.user_id).eq('provider', 'ms');
  return tok.access_token;
}

export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });
  const { data: row } = await sb.from('provider_tokens').select('*')
    .eq('user_id', user.id).eq('provider', 'ms').maybeSingle();
  if (!row) return NextResponse.json({ error: 'Outlook isn\u2019t linked yet — link it in Settings' });
  const token = await freshToken(sb, row);
  if (!token) return NextResponse.json({ error: 'Microsoft session expired — link Outlook again' });

  const r = await (await fetch(
    'https://graph.microsoft.com/v1.0/me/messages?$top=15&$select=id,from,subject,bodyPreview,receivedDateTime,isRead&$orderby=receivedDateTime desc',
    { headers: { Authorization: 'Bearer ' + token } }
  )).json();
  if (r.error) return NextResponse.json({ error: 'Graph: ' + (r.error.message || 'request failed') });

  const messages = (r.value || []).map(m => ({
    id: 'ms_' + m.id,
    ch: 'email', prov: 'outlook',
    fromName: m.from?.emailAddress?.name || m.from?.emailAddress?.address || 'Unknown',
    fromAddr: (m.from?.emailAddress?.address || '').toLowerCase(),
    subj: m.subject || '(no subject)',
    prev: (m.bodyPreview || '').slice(0, 120),
    body: (m.bodyPreview || '') + '\n\n\u2014 Synced from your real Outlook inbox (preview).',
    ts: Date.parse(m.receivedDateTime) || Date.now(),
    unread: !m.isRead, starred: false,
  }));
  return NextResponse.json({ label: row.label, messages });
}
