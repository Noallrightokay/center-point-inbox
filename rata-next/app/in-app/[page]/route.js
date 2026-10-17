import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { IN_APP_PAGES, storeSafe } from '../../../lib/in-app';

/* Made when the site is built, from the page as it is in that build, so a
   copy can never say anything its page does not (lib/in-app.js says what a
   copy leaves out and why), and a page whose copy cannot be made (a
   sentence that changed under its store wording, a word that sells) fails
   `npm run build`: in CI, and on the host, where the site already running
   stays up, rather than answering 500 once deployed. Being in the build, the
   copies need nothing from public/ or the working directory once it runs. */
export const dynamic = 'force-static';
export const dynamicParams = false;

export function generateStaticParams() {
  return Object.keys(IN_APP_PAGES).map(page => ({ page }));
}

/* /in-app/help, /in-app/privacy and /in-app/terms: the store-safe copies of
   public/help.html, privacy.html and terms.html. The deletion page is a file
   of its own, which proxy.js serves at /in-app/delete-account. Any other
   name is Next's own 404 (dynamicParams). */
export async function GET(req, { params }) {
  const { page } = await params;
  if (!Object.hasOwn(IN_APP_PAGES, page)) {
    return new Response('Not found', { status: 404, headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
  }
  const source = await readFile(join(process.cwd(), 'public', IN_APP_PAGES[page]), 'utf8');
  return new Response(storeSafe(page, source), { headers: { 'Content-Type': 'text/html; charset=utf-8' } });
}
