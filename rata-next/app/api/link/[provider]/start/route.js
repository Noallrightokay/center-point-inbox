import { NextResponse } from 'next/server';
import { admin, appUrl, PROVIDERS, LINK_COOKIE, sameState, stateExpired } from '../../../../../lib/server';

export async function GET(req, { params }) {
  const provider = params.provider;
  const state = new URL(req.url).searchParams.get('state') || '';
  const base = appUrl(req);
  const fail = m => NextResponse.redirect(`${base}/app.html?linkerr=${encodeURIComponent(m)}`);
  const sb = admin();
  if (!sb || !PROVIDERS[provider]) return fail('backend not configured');

  /* The cookie proves this is the browser that started the flow, not just
     someone who got hold of the state value. */
  if (!sameState(req.cookies.get(LINK_COOKIE)?.value, state))
    return fail('link session did not match this browser — start again from Settings');

  const { data } = await sb.from('link_states').select('*').eq('id', state).maybeSingle();
  if (!data || stateExpired(data)) return fail('link session expired — try again');

  const redirect = `${base}/api/link/${provider}/callback`;
  if (provider === 'ms') {
    const u = new URL('https://login.microsoftonline.com/common/oauth2/v2.0/authorize');
    u.searchParams.set('client_id', process.env.MS_CLIENT_ID);
    u.searchParams.set('response_type', 'code');
    u.searchParams.set('redirect_uri', redirect);
    u.searchParams.set('response_mode', 'query');
    u.searchParams.set('scope', PROVIDERS.ms.scope);
    u.searchParams.set('state', state);
    return NextResponse.redirect(u.toString());
  }
  if (provider === 'slack') {
    const u = new URL('https://slack.com/oauth/v2/authorize');
    u.searchParams.set('client_id', process.env.SLACK_CLIENT_ID);
    u.searchParams.set('user_scope', PROVIDERS.slack.userScope);
    u.searchParams.set('redirect_uri', redirect);
    u.searchParams.set('state', state);
    return NextResponse.redirect(u.toString());
  }
  return fail('unknown provider');
}
