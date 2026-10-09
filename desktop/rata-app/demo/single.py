#!/usr/bin/env python3
"""Fold the demo folder that build.sh writes into one HTML file.

    python3 single.py <demo-folder> <out-file>     (build.sh --single runs it)

The result opens by double-click from file://, offline, in any modern
browser, so the demo can be sent to someone as one attachment:

- the scripts and stylesheets beside the page go inline, the fonts and the
  pictures as data: URIs, and the service worker is not registered (there
  is no sw.js beside a file someone was sent);
- the vendored libraries the page loads only when needed (Word, Excel, PDF)
  are carried as string literals and handed to the page as blob: URLs, so
  `loadScript` and PDF.js's `import()` load them as they would load the
  files: every 'vendor/<file>.js' literal in the page becomes
  __demoUrl('vendor/<file>.js');
- PDF.js's worker runs as a classic worker made from the same source,
  since Chromium starts no module worker for a page opened from a file;
- storage that throws (some browsers refuse it to a file) is replaced by
  storage in memory, so the demo still runs and forgets on reload.

It refuses rather than guesses: a page that changed shape, an inline script
holding </script or <!--, or a control character or U+FFFD left anywhere
stops the build. The libraries arrive with build.sh's escapes already in
them and are written as pure ASCII; the rest is checked, not rewritten.
"""
import base64
import json
import os
import re
import subprocess
import sys
import tempfile

folder, out = sys.argv[1:]

# Loaded by the page only to sync with an online account, which the demo
# never has (config.js leaves supabaseUrl empty), so it is not carried.
SKIP = ('vendor/supabase-js-',)


def read(rel):
    return open(os.path.join(folder, rel), encoding='utf-8').read()


def data_uri(rel, mime):
    raw = open(os.path.join(folder, rel), 'rb').read()
    return 'data:%s;base64,%s' % (mime, base64.b64encode(raw).decode('ascii'))


s = read('rata-demo.html')


def swap(a, b, count=1):
    global s
    if s.count(a) != count:
        sys.exit('single.py: the page changed: expected %d of %r, found %d'
                 % (count, a[:70], s.count(a)))
    s = s.replace(a, b)


def inline_js(name):
    """A classic script, inline. HTML ends a script at </script and reads
    <!-- inside one specially; neither is in these files today, and
    rewriting code to avoid them is not safe in general, so say so."""
    t = read(name)
    if re.search(r'</script|<!--', t, re.I):
        sys.exit('single.py: %s holds </script or <!--; it cannot go inline as it is' % name)
    return '<script>\n' + t + '\n</script>'


def js_string(t):
    """Source as a JS string literal that is the same text: JSON is JS, every
    character outside ASCII becomes \\uXXXX (so no control character or
    U+FFFD is left raw), and every < becomes \\u003c, so no </script or
    <!-- can occur inside the script element that carries it."""
    return json.dumps(t, ensure_ascii=True).replace('<', '\\u003c')


# Pictures. The touch icon is for a page saved to an iPhone's home screen,
# which a file is not.
swap('<link rel="icon" type="image/png" href="icons/favicon-64.png">',
     '<link rel="icon" type="image/png" href="%s">' % data_uri('icons/favicon-64.png', 'image/png'))
swap('<link rel="apple-touch-icon" href="icons/apple-touch-icon.png">\n', '')
mark = 'src="icons/mark-256.png"'
swap(mark, 'src="%s"' % data_uri('icons/mark-256.png', 'image/png'), s.count(mark) or 1)

# Fonts, from fonts.css, each url() a data: URI.
css = read('fonts/fonts.css')
css, n = re.subn(r'url\(\s*[\'"]?([\w.-]+\.woff2)[\'"]?\s*\)',
                 lambda m: 'url(%s)' % data_uri('fonts/' + m.group(1), 'font/woff2'), css)
if not n or 'url(' in re.sub(r'url\(data:', '', css):
    sys.exit('single.py: fonts.css has a url() that is not a woff2 beside it')
swap('<link rel="stylesheet" href="fonts/fonts.css">', '<style>\n' + css + '</style>')
swap('<link rel="stylesheet" href="bridge.css">', '<style>\n' + read('bridge.css') + '</style>')

# The service worker: build.sh already took its registration out.
if 'sw.js' in s:
    sys.exit('single.py: the page still registers a service worker')

# The vendored libraries.
refs = sorted(set(re.findall(r"'(vendor/[\w.-]+\.js)'", s)))
carried = [r for r in refs if not r.startswith(SKIP)]
on_disk = sorted('vendor/' + f for f in os.listdir(os.path.join(folder, 'vendor'))
                 if f.endswith('.js') and not ('vendor/' + f).startswith(SKIP))
if carried != on_disk:
    sys.exit('single.py: the page asks for %s but the folder holds %s' % (carried, on_disk))
if not any('pdf.worker' in r for r in carried):
    sys.exit('single.py: the page no longer names the PDF.js worker')
for r in carried:
    swap("'%s'" % r, "__demoUrl('%s')" % r, s.count("'%s'" % r))


def node_checks(code, suffix):
    with tempfile.NamedTemporaryFile('w', suffix=suffix, delete=False, encoding='utf-8') as f:
        f.write(code)
    try:
        return subprocess.run(['node', '--check', f.name], capture_output=True).returncode == 0
    finally:
        os.unlink(f.name)


# PDF.js's worker as a classic script. Chromium will not start a module
# worker for a page opened from a file (a classic one it will), and the
# worker is a module only by its last line and by import.meta.url, which
# its picture decoders read and survive without. The page makes the
# classic copy from the module when it is first asked for (the loader's
# EXPORT and the replacement below, the same here as there), so the file
# carries the worker once; here that copy is checked as .cjs, which proves
# no import or export is left.
EXPORT = r'export\s*\{\s*WorkerMessageHandler\s*\}\s*;?\s*$'
worker = next(r for r in carried if 'pdf.worker' in r)
classic, n = re.subn(EXPORT, '\n', read(worker))
if n != 1:
    sys.exit('single.py: %s no longer ends by exporting WorkerMessageHandler' % worker)
classic = classic.replace('import.meta.url', 'self.location.href')
if not node_checks(classic, '.cjs'):
    sys.exit('single.py: %s does not parse as a classic script once its export is gone' % worker)
CLASSIC = worker + '#classic'

# PDF.js is handed that worker as its port, where pdfLib sets the worker.
m = re.search(r'(\w+)\.GlobalWorkerOptions\.workerSrc=[^;\n]+;', s)
at = s.find('async function pdfLib(){')
if (not m or at < 0 or not 0 < m.start() - at < 400
        or len(re.findall(r'GlobalWorkerOptions\.workerSrc=', s)) != 1):
    sys.exit('single.py: the page changed: cannot find where pdfLib sets the PDF.js worker')
s = s[:m.end()] + 'await __demoPdfPort(%s);' % m.group(1) + s[m.end():]

loader = r"""/* RATA demo as one file: what build.sh --single adds (single.py). */
(function () {
  /* Storage some browsers refuse to a page opened from a file. Storage in
     memory instead: the demo works and forgets on reload. */
  function memory() {
    var d = Object.create(null);
    return {
      get length() { return Object.keys(d).length; },
      key: function (i) { var k = Object.keys(d); return i < k.length ? k[i] : null; },
      getItem: function (k) { k = String(k); return k in d ? d[k] : null; },
      setItem: function (k, v) { d[String(k)] = String(v); },
      removeItem: function (k) { delete d[String(k)]; },
      clear: function () { d = Object.create(null); }
    };
  }
  ['localStorage', 'sessionStorage'].forEach(function (n) {
    try { var st = window[n], t = '__rata_demo_probe'; st.setItem(t, t); st.removeItem(t); return; } catch (e) {}
    try { Object.defineProperty(window, n, { configurable: true, value: memory() }); } catch (e) {}
  });
  /* The vendored libraries, loaded from blob: URLs made when first asked. */
  var SRC = {@SRC@}, URLS = {}, CLASSIC = @CLASSIC@, WORKER = @WORKER@;
  window.__demoUrl = function (p) {
    if (p === CLASSIC && !SRC[p])
      SRC[p] = SRC[WORKER].replace(/@EXPORT@/, '\n').split('import.meta.url').join('self.location.href');
    if (!Object.prototype.hasOwnProperty.call(SRC, p)) return p;
    if (!URLS[p]) URLS[p] = URL.createObjectURL(new Blob([SRC[p]], { type: 'text/javascript' }));
    return URLS[p];
  };
  /* PDF.js's worker, started as a classic worker and handed to PDF.js once
     it says it is ready. If it does not start, PDF.js is left to try its
     own module worker and then to read the PDF on the page: slower, same
     result. */
  window.__demoPdfPort = function (m) {
    return new Promise(function (done) {
      var w, t;
      function give(ok) {
        clearTimeout(t);
        w.onmessage = w.onerror = null;
        if (ok) { try { m.GlobalWorkerOptions.workerPort = w; } catch (e) { ok = false; } }
        if (!ok) w.terminate();
        done();
      }
      if (m.GlobalWorkerOptions.workerPort) return done();
      try { w = new Worker(window.__demoUrl(CLASSIC)); } catch (e) { return done(); }
      w.onmessage = function () { give(true); };
      w.onerror = function (e) { e.preventDefault(); give(false); };
      t = setTimeout(function () { give(false); }, 15000);
    });
  };
})();""".replace('@CLASSIC@', json.dumps(CLASSIC)).replace('@WORKER@', json.dumps(worker)).replace(
    '@EXPORT@', EXPORT).replace(
    '@SRC@', ',\n'.join('%s: %s' % (json.dumps(r), js_string(read(r))) for r in carried))

if not node_checks(loader, '.cjs'):
    sys.exit('single.py: the loader does not parse')

# The loader first, then the demo's own scripts inline, in their order.
swap('<script src="demo-backend.js"></script>',
     '<script>\n' + loader + '\n</script>\n' + inline_js('demo-backend.js'))
swap('<script src="config.js"></script>', inline_js('config.js'))
swap('<script src="bridge.js"></script>', inline_js('bridge.js'))

left = re.findall(r'(?:src|href)="(?!data:|https?:)([^"$]+\.(?:js|mjs|css|png|jpe?g|gif|svg|ico|woff2?|json|webmanifest))"', s)
if left:
    sys.exit('single.py: the page still points at files beside it: %s' % sorted(set(left)))
bad = [i for i, c in enumerate(s) if (ord(c) < 32 and c not in '\t\n\r') or c == '\ufffd']
if bad:
    sys.exit('single.py: a control character or U+FFFD at %d' % bad[0])

open(out, 'w', encoding='utf-8').write(s)
