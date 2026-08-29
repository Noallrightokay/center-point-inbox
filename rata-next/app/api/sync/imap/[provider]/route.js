import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../../lib/server';
import { imapProvider, fetchInbox } from '../../../../../lib/imap';

export const dynamic = 'force-dynamic';

export async function GET(req, { params }) {
  const def = imapProvider(params.provider);
  if (!def) return NextResponse.json({ error: 'Unknown mail provider' }, { status: 400 });

  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const { data: row } = await sb.from('provider_tokens').select('*')
    .eq('user_id', user.id).eq('provider', def.token).maybeSingle();
  if (!row) return NextResponse.json({ error: `${def.label} isn’t linked yet — link it in Settings` });

  const out = await fetchInbox(def, row.label, row.access);
  if (out.error) return NextResponse.json({ error: out.error });
  return NextResponse.json({ label: row.label, messages: out.messages });
}
