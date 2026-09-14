/* Generate the brand glyphs the launcher uses, straight into app.html.

   A letter on a coloured square reads as a placeholder, because that is what
   it is. These are the real marks, drawn as vector so they stay sharp, and
   inlined so they cost no request and work with the network off.

   Provenance matters here and is recorded per icon:

     official  — the mark itself, from simple-icons (CC0). Google's apps,
                 Apple, Dropbox.
     drawn     — authored here, because no redistributable set carries it.
                 Microsoft and Adobe both asked simple-icons to remove their
                 marks, and DocuSign was never in it. These are faithful to
                 the shape and the official colour, but they are recreations,
                 not the vendor's own file.

   Drop an official .svg into public/brand/<key>.svg and it wins over both —
   that is the upgrade path when someone has the real asset to hand.

   Everything is one monochrome path on a 24×24 grid in the brand's colour.
   That is a deliberate choice over reproducing each multicolour icon badly:
   the tiles read as one system, and a silhouette is honest about being a
   silhouette where a bad four-colour copy is not.

   Run:  npm run brand-icons
*/

import * as si from 'simple-icons';
import { readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const appFile = join(here, '..', 'public', 'app.html');
const brandDir = join(here, '..', 'public', 'brand');

/* key -> simple-icons slug */
const OFFICIAL = {
  gdocs:    'siGoogledocs',
  gsheets:  'siGooglesheets',
  gslides:  'siGoogleslides',
  gforms:   'siGoogleforms',
  gdrive:   'siGoogledrive',
  dropbox:  'siDropbox',
  icloud:   'siIcloud',
  apple:    'siApple',
};

/* Authored here. Colours are each vendor's published brand value. */
const DRAWN = {
  word: { hex: '2B579A', title: 'Microsoft Word', path:
    'M6 2h8l4 4v16H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.5V6h2.5L14 3.5zM7.4 9l1.1 6.2L9.7 9h1.4l1.2 6.2L13.4 9h1.5l-1.9 8.6h-1.6l-1.2-6-1.2 6H7.4L5.5 9h1.9z' },
  excel: { hex: '217346', title: 'Microsoft Excel', path:
    'M6 2h8l4 4v16H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.5V6h2.5L14 3.5zM8 9l1.9 3.3L7.8 16h1.9l1.2-2.3 1.2 2.3h2l-2.1-3.7L13.9 9h-1.9l-1.1 2.1L9.9 9H8z' },
  powerpoint: { hex: 'D24726', title: 'Microsoft PowerPoint', path:
    'M6 2h8l4 4v16H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.5V6h2.5L14 3.5zM8.6 9v8.6h1.8v-2.8h1.4c1.8 0 3-1.1 3-2.9S13.6 9 11.8 9H8.6zm1.8 1.5h1.2c.9 0 1.4.5 1.4 1.4s-.5 1.4-1.4 1.4h-1.2v-2.8z' },
  onenote: { hex: '7719AA', title: 'Microsoft OneNote', path:
    'M6 2h8l4 4v16H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.5V6h2.5L14 3.5zM8.4 9v8.6h1.7v-5.4l3.1 5.4h1.8V9h-1.7v5.3L10.2 9H8.4z' },
  onedrive: { hex: '0078D4', title: 'Microsoft OneDrive', path:
    'M10.2 6.3a5.1 5.1 0 0 1 4.6 2.9 3.9 3.9 0 0 1 .7-.1 3.9 3.9 0 0 1 3.8 3.2 3.2 3.2 0 0 1-.7 6.3H6.1A4.1 4.1 0 0 1 5.4 10a5.1 5.1 0 0 1 4.8-3.7z' },
  acrobat: { hex: 'EC1C24', title: 'Adobe Acrobat', path:
    'M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zm4.2 14.6c.9 0 1.7-.8 2.5-2.2a18 18 0 0 1 3.3-.9c.9.7 1.7 1.1 2.4 1.1.8 0 1.3-.4 1.3-1 0-.7-.7-1.1-1.9-1.1-.6 0-1.3.1-2.1.2-.8-.8-1.5-1.8-2-2.9.3-1.1.5-2 .5-2.7 0-1-.4-1.6-1.1-1.6s-1.1.6-1.1 1.5c0 .8.3 1.8.8 2.9-.4 1.2-.9 2.4-1.5 3.6-1.5.6-2.4 1.4-2.4 2.2 0 .6.4.9 1.3.9zm.2-1.1c-.3 0-.4-.1-.4-.3 0-.3.5-.8 1.4-1.2-.4.9-.7 1.5-1 1.5zm2.4-2.5c.4-.8.7-1.6 1-2.4.4.7.9 1.4 1.4 2-.8.1-1.6.2-2.4.4zm.9-4.9c0-.4.1-.6.3-.6s.3.2.3.6c0 .4-.1 1-.3 1.6-.2-.6-.3-1.2-.3-1.6zm5 5.5c.5 0 .8.1.8.4 0 .2-.2.3-.5.3-.4 0-.9-.2-1.5-.6.5-.1.9-.1 1.2-.1z' },
  adobesign: { hex: 'EC1C24', title: 'Adobe Acrobat Sign', path:
    'M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zm2.6 14.9h10.8v1.2H6.6v-1.2zm1.1-2.1 5.9-5.9a.8.8 0 0 1 1.1 0l1.4 1.4a.8.8 0 0 1 0 1.1l-5.9 5.9H7.7v-2.5zm7.5-4.7.9-.9a.5.5 0 0 0 0-.8l-.8-.8a.5.5 0 0 0-.8 0l-.9.9 1.6 1.6z' },
  adobeexpress: { hex: 'FF3366', title: 'Adobe Express', path:
    'M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zm8 4.4-4 9.2h1.9l.8-2h2.6l.8 2H18l-4-9.2h-2zm1 2.6 .9 2.9h-1.8l.9-2.9z' },
  docusign: { hex: 'FFCC22', title: 'DocuSign', path:
    'M5 2h9l5 5v15H5a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zm9 1.8V7h3.2L14 3.8zM8.2 10.4h3c2 0 3.4 1.3 3.4 3.3s-1.4 3.3-3.4 3.3h-3v-6.6zm1.7 1.5v3.6h1.2c1 0 1.7-.7 1.7-1.8s-.7-1.8-1.7-1.8H9.9z' },
  pages: { hex: 'F2870D', title: 'Pages', path:
    'M6 2h8l4 4v16H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.5V6h2.5L14 3.5zM7.5 9.5h9V11h-9V9.5zm0 3h9V14h-9v-1.5zm0 3h6V17h-6v-1.5z' },
  numbers: { hex: '31A952', title: 'Numbers', path:
    'M6 2h8l4 4v16H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.5V6h2.5L14 3.5zM7.5 17V13h1.8v4H7.5zm3.2 0V9.5h1.8V17h-1.8zm3.2 0v-5.6h1.8V17h-1.8z' },
  keynote: { hex: '1C9BF0', title: 'Keynote', path:
    'M4 4h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-6.2l2.4 3.4-1.4 1L12 16.6l-2.8 3.8-1.4-1L10.2 16H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zm1.8 2.6v6.8h12.4V6.6H5.8z' },
};

const parts = [];
for (const [key, slug] of Object.entries(OFFICIAL)) {
  const icon = si[slug];
  if (!icon) throw new Error(`simple-icons no longer carries ${slug} — it was probably removed on request. Move ${key} into DRAWN.`);
  parts.push({ key, hex: icon.hex, title: icon.title, path: icon.path, src: 'official' });
}
for (const [key, d] of Object.entries(DRAWN)) {
  parts.push({ key, hex: d.hex, title: d.title, path: d.path, src: 'drawn' });
}

/* An official file dropped into public/brand wins over everything. */
if (existsSync(brandDir)) {
  for (const f of readdirSync(brandDir).filter(f => f.endsWith('.svg'))) {
    const key = f.replace(/\.svg$/, '');
    const svg = readFileSync(join(brandDir, f), 'utf8');
    const d = (svg.match(/\sd="([^"]+)"/) || [])[1];
    if (!d) { console.warn(`skipped public/brand/${f}: no single <path d="…"> found`); continue; }
    const row = parts.find(p => p.key === key);
    const hex = ((svg.match(/fill="#([0-9a-f]{6})"/i) || [])[1] || row?.hex || '444444').toUpperCase();
    if (row) Object.assign(row, { path: d, hex, src: 'supplied' });
    else parts.push({ key, hex, title: key, path: d, src: 'supplied' });
  }
}

const body = parts
  .sort((a, b) => a.key.localeCompare(b.key))
  .map(p => `  ${p.key}:{h:'#${p.hex}',t:${JSON.stringify(p.title)},s:'${p.src}',d:'${p.path}'},`)
  .join('\n');

const block = `/* BRAND-ICONS:START — generated by scripts/brand-icons.mjs, do not edit by hand.
   s:'official' is the vendor's own mark (simple-icons, CC0); s:'drawn' is a
   recreation, used where no redistributable set carries the mark — Microsoft
   and Adobe both had theirs removed on request. Put an official file in
   public/brand/<key>.svg to replace one. */
const BRAND={
${body}
};
/* The drawn marks put their detail inside the body as extra subpaths, which
   under nonzero winding fills them in and leaves a solid blob. evenodd makes
   the inner shapes cut through. The official paths are built for nonzero and
   are left alone. */
const brandSVG=(k,size)=>{const b=BRAND[k];if(!b)return '';const r=b.s==='drawn'?' fill-rule="evenodd"':'';return \`<svg class="brand-glyph" viewBox="0 0 24 24" width="\${size||20}" height="\${size||20}" role="img" aria-label="\${b.t}" fill="\${b.h}"\${r}><path d="\${b.d}"/></svg>\`};
/* BRAND-ICONS:END */`;

let html = readFileSync(appFile, 'utf8');
const startTag = '/* BRAND-ICONS:START';
const endTag = '/* BRAND-ICONS:END */';
const a = html.indexOf(startTag);
if (a < 0) throw new Error('no BRAND-ICONS:START marker in app.html — add the markers first');
const z = html.indexOf(endTag, a);
if (z < 0) throw new Error('no BRAND-ICONS:END marker in app.html');
html = html.slice(0, a) + block + html.slice(z + endTag.length);
writeFileSync(appFile, html);

const counts = parts.reduce((m, p) => (m[p.src] = (m[p.src] || 0) + 1, m), {});
console.log(`wrote ${parts.length} brand glyphs into public/app.html`);
console.log(Object.entries(counts).map(([k, n]) => `  ${n} ${k}`).join('\n'));
