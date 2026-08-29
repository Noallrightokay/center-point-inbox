import { NextResponse } from 'next/server';
import { userFromRequest, providerReady, PROVIDERS, setLinkCookie } from '../../../../../lib/server';

export async function POST(req, { params }) {
  const provider = params.provider;
  if (!PROVIDERS[provider]) return NextResponse.json({ error: 'Unknown provider' }, { status: 400 });
  if (!providerReady(provider))
    return NextResponse.json({ error: `Backend missing ${PROVIDERS[provider].env.join(' / ')} — see BACKEND-SETUP.md` });
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });
  const { data, error: e2 } = await sb
    .from('link_states')
    .insert({ user_id: user.id, provider })
    .select('id')
    .single();
  if (e2) return NextResponse.json({ error: 'Run the updated database.sql first: ' + e2.message });
  /* Bind the state to this browser as well as this user — see lib/server.js */
  return setLinkCookie(NextResponse.json({ state: data.id }), data.id);
}
