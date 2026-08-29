import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';

export const dynamic = 'force-dynamic';

const slack = (method, token, params = {}) => {
  const u = new URL('https://slack.com/api/' + method);
  Object.entries(params).forEach(([k, v]) => u.searchParams.set(k, v));
  return fetch(u, { headers: { Authorization: 'Bearer ' + token } }).then(r => r.json());
};

export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });
  const { data: row } = await sb.from('provider_tokens').select('*')
    .eq('user_id', user.id).eq('provider', 'slack').maybeSingle();
  if (!row) return NextResponse.json({ error: 'Slack isn\u2019t linked yet — link it in Settings' });
  const token = row.access;
  const me = (row.extra && row.extra.authed_user) || '';

  const convs = await slack('conversations.list', token, { types: 'im,mpim,public_channel,private_channel', limit: '20', exclude_archived: 'true' });
  if (!convs.ok) return NextResponse.json({ error: 'Slack: ' + (convs.error || 'conversations failed') });

  const users = {};
  const uname = async id => {
    if (!id) return 'Unknown';
    if (users[id]) return users[id];
    const u = await slack('users.info', token, { user: id });
    users[id] = (u.ok && (u.user.profile?.display_name || u.user.real_name || u.user.name)) || id;
    return users[id];
  };

  const messages = [];
  for (const c of (convs.channels || []).slice(0, 8)) {
    const hist = await slack('conversations.history', token, { channel: c.id, limit: '4' });
    if (!hist.ok) continue;
    for (const m of hist.messages || []) {
      if (!m.text || m.subtype) continue;
      const sent = me && m.user === me;
      messages.push({
        id: 'sl_' + c.id + '_' + m.ts,
        ch: 'slack', prov: 'slack',
        fromName: sent ? undefined : await uname(m.user),
        handle: c.is_im ? 'DM' : ('#' + (c.name || 'channel')),
        sent,
        subj: '',
        prev: m.text.slice(0, 120),
        body: m.text + '\n\n\u2014 Synced from your real Slack workspace.',
        ts: Math.round(parseFloat(m.ts) * 1000),
        unread: false, starred: false,
      });
    }
  }
  messages.sort((a, b) => b.ts - a.ts);
  return NextResponse.json({ label: row.label, messages: messages.slice(0, 30) });
}
