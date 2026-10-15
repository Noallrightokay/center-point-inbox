import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { IN_APP_PAGES, storeSafe } from '../../../lib/in-app';

/* Made per request, from the page as it is on disk next door, so a copy
   can never say anything its page does not (lib/in-app.js says what a copy
   leaves out and why). */
export const dynamic = 'force-dynamic';

/* /in-app/help, /in-app/privacy and /in-app/terms: the store-safe copies of
   public/help.html, privacy.html and terms.html. The deletion page is a file
   of its own, which proxy.js serves at /in-app/delete-account. */
export async function GET(req, { params }) {
  const { page } = await params;
  if (!Object.hasOwn(IN_APP_PAGES, page)) {
    return new Response('Not found', { status: 404, headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
  }
  const source = await readFile(join(process.cwd(), 'public', IN_APP_PAGES[page]), 'utf8');
  return new Response(storeSafe(page, source), {
    headers: {
      'Content-Type': 'text/html; charset=utf-8',
      /* As Next serves the page it is made from. */
      'Cache-Control': 'public, max-age=0',
    },
  });
}
