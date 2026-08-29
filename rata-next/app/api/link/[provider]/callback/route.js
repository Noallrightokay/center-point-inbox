import { NextResponse } from 'next/server';
import { admin, appUrl, LINK_COOKIE, clearLinkCookie, sameState, stateExpired } from '../../../../../lib/server';

export async function GET(req, { params }) {
  const provider = params.provider;
  const url = new URL(req.url);
  const code = url.searchParams.get('code');
  const state = url.searchParams.get('state') || '';
  const base = appUrl(req);
  /* The flow ends here either way, so every exit clears the binding cookie. */
  const fail = m => clearLinkCookie(NextResponse.redirect(`${base}/app.html?linkerr=${encodeURIComponent(m)}`));
  const ok = label => clearLinkCookie(NextResponse.redirect(`${base}/app.html?linked=${provider}:${encodeURIComponent(label)}`));

  const sb = admin();
  if (!sb) return fail('backend not configured');

  /* Check the browser binding before touching the row, so a stolen state value
     can't be used to delete someone else's in-flight link session. */
  if (!sameState(req.cookies.get(LINK_COOKIE)?.value, state))
    return fail('link session did not match this browser — start again from Settings');

  const { data: st } = await sb.from('link_states').select('*').eq('id', state).maybeSingle();
  await sb.from('link_states').delete().eq('id', state);
  if (!st) return fail('link session expired — try again');
  /* /start enforces the TTL too, but the row is redeemed here — check it where
     it is spent, not only where it is presented. */
  if (stateExpired(st)) return fail('link session expired — try again');
  if (!code) return fail(url.searchParams.get('error_description') || 'authorization was cancelled');
  const redirect = `${base}/api/link/${provider}/callback`;

  try {
    if (provider === 'ms') {
      const body = new URLSearchParams({
        client_id: process.env.MS_CLIENT_ID,
        client_secret: process.env.MS_CLIENT_SECRET,
        code, redirect_uri: redirect, grant_type: 'authorization_code',
        scope: 'offline_access User.Read Mail.Read',
      });
      const tok = await (await fetch('https://login.microsoftonline.com/common/oauth2/v2.0/token', {
        method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body,
      })).json();
      if (!tok.access_token) return fail(tok.error_description || 'Microsoft token exchange failed');
      const me = await (await fetch('https://graph.microsoft.com/v1.0/me', {
        headers: { Authorization: 'Bearer ' + tok.access_token },
      })).json();
      const label = (me.mail || me.userPrincipalName || 'Microsoft account').toLowerCase();
      await sb.from('provider_tokens').upsert({
        user_id: st.user_id, provider: 'ms', label,
        access: tok.access_token, refresh: tok.refresh_token || null,
        expires_at: new Date(Date.now() + (tok.expires_in || 3600) * 1000).toISOString(),
        updated_at: new Date().toISOString(),
      });
      return ok(label);
    }

    if (provider === 'slack') {
      const body = new URLSearchParams({
        client_id: process.env.SLACK_CLIENT_ID,
        client_secret: process.env.SLACK_CLIENT_SECRET,
        code, redirect_uri: redirect,
      });
      const tok = await (await fetch('https://slack.com/api/oauth.v2.access', {
        method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body,
      })).json();
      if (!tok.ok || !tok.authed_user?.access_token) return fail(tok.error || 'Slack authorization failed');
      const label = ((tok.team && tok.team.name) ? tok.team.name + ' (Slack)' : 'Slack workspace');
      await sb.from('provider_tokens').upsert({
        user_id: st.user_id, provider: 'slack', label,
        access: tok.authed_user.access_token, refresh: null,
        extra: { authed_user: tok.authed_user.id, team: tok.team?.id || null },
        expires_at: null, updated_at: new Date().toISOString(),
      });
      return ok(label);
    }
  } catch (e) {
    return fail('linking failed — ' + (e.message || 'unknown error'));
  }
  return fail('unknown provider');
}
