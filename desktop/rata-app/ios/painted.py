"""Whether a Simulator screenshot shows anything drawn below the status bar.

    python3 painted.py SHOT.png     exit 0 when something is drawn there, 1
                                    when that part of the picture is nearly
                                    one colour, 2 when SHOT.png cannot be read
    python3 painted.py --test       its own tests, on pictures it makes

run-smoke.sh judges each run's screenshot with it. The self-test's "page"
check reads the page's DOM, title and script errors, which a page has
whether or not anything reached the screen: on a cold runner WebKit's GPU
process once went unresponsive, the window's first frame came 15 s late,
and the first screen's screenshot was the status bar over plain white
while smoke.json said "Sign in to RATA" (run 41180770621). A window that
never drew shows only its background, so the part of the picture that is
the app's, below the status bar and the Dynamic Island (the top tenth) and
above the home indicator (the last twentieth), is judged: more than
ONE_COLOUR of it the same colour is nothing drawn. The most common
colour covered 100% of that part in that blank screenshot; in the job's
others then and in the run before (41176963356), 77% on RATA's first
screen, 90% on the empty inbox, light and dark, 66% on Settings and 50%
on Safari at example.com.

Standard library only: Python on GitHub's Macs has no imaging package.
"""

import random
import struct
import sys
import zlib
from array import array
from collections import Counter
from itertools import accumulate
from pathlib import Path

ONE_COLOUR = 0.99
TOP = 0.10
BOTTOM = 0.95


class Unreadable(Exception):
    pass


def rows_of(data):
    """(width, height, bytes per pixel, rows) of a non-interlaced PNG of 8 or
    16 bits a channel, greyscale or colour, with or without alpha, each row
    its raw pixel bytes, unfiltered."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise Unreadable("not a PNG")
    pos, header, idat = 8, None, []
    while pos + 8 <= len(data):
        (n,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        if kind == b"IHDR":
            header = data[pos + 8 : pos + 8 + 13]
        elif kind == b"IDAT":
            idat.append(data[pos + 8 : pos + 8 + n])
        elif kind == b"IEND":
            break
        pos += 12 + n
    if header is None or len(header) != 13 or not idat:
        raise Unreadable("no picture in it")
    w, h, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", header)
    if interlace or depth not in (8, 16) or colour not in (0, 2, 4, 6):
        raise Unreadable(f"depth {depth}, colour type {colour}, interlace {interlace}: not read here")
    step = {0: 1, 2: 3, 4: 2, 6: 4}[colour] * depth // 8
    stride = w * step
    try:
        raw = zlib.decompress(b"".join(idat))
    except zlib.error as e:
        raise Unreadable(f"its picture data does not inflate: {e}") from None
    if len(raw) < h * (stride + 1):
        raise Unreadable("its picture data is cut short")
    seven = int.from_bytes(b"\x7f" * stride, "little")
    eight = int.from_bytes(b"\x80" * stride, "little")
    keep = (255).__and__
    rows, prev = [], bytes(stride)
    for y in range(h):
        at = y * (stride + 1)
        f = raw[at]
        line = raw[at + 1 : at + 1 + stride]
        if f == 0:
            pass
        elif f == 1:
            # Each byte adds the one a pixel before it: one running sum
            # for each byte of a pixel.
            out = bytearray(line)
            for k in range(step):
                out[k::step] = bytes(map(keep, accumulate(line[k::step])))
            line = bytes(out)
        elif f == 2:
            # Each byte adds the one above it, all at once: the low seven
            # bits of each byte added without a carry leaving the byte,
            # then the top bit put right.
            a = int.from_bytes(line, "little")
            b = int.from_bytes(prev, "little")
            line = (((a & seven) + (b & seven)) ^ ((a ^ b) & eight)).to_bytes(stride, "little")
        elif f in (3, 4):
            out = bytearray(line)
            for i in range(stride):
                left = out[i - step] if i >= step else 0
                up = prev[i]
                if f == 3:
                    out[i] = (out[i] + (left + up) // 2) & 255
                else:
                    corner = prev[i - step] if i >= step else 0
                    p = left + up - corner
                    pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                    out[i] = (out[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else corner)) & 255
            line = bytes(out)
        else:
            raise Unreadable(f"row {y} has filter {f}, which PNG does not have")
        rows.append(line)
        prev = line
    return w, h, step, rows


def judged(data):
    """(the share of the judged part in its most common colour, that colour
    as hex)."""
    w, h, step, rows = rows_of(data)
    first, last = int(h * TOP), int(h * BOTTOM) - 1
    if last < first:
        raise Unreadable(f"{w}x{h} is too small to judge")
    seen = Counter()
    code = {1: "B", 2: "H", 4: "I"}.get(step)
    if code and array(code).itemsize != step:
        code = None
    for line in rows[first : last + 1]:
        if code:
            seen.update(array(code, line))
        else:
            seen.update(line[i : i + step] for i in range(0, len(line), step))
    colour, n = seen.most_common(1)[0]
    if isinstance(colour, int):
        colour = colour.to_bytes(step, sys.byteorder)
    return n / (w * (last + 1 - first)), colour.hex()


def drawn(share):
    """Whether a picture whose judged part has `share` of it in one colour
    shows something drawn."""
    return share <= ONE_COLOUR


def png(rows, colour=6, depth=8, filters=(0,)):
    """A PNG of `rows` (each a list of pixels, each a tuple of channels),
    each row filtered by the next of `filters` in turn: for the tests."""
    step = {0: 1, 2: 3, 4: 2, 6: 4}[colour] * depth // 8
    flat = []
    for row in rows:
        b = bytearray()
        for px in row:
            for v in px:
                b += v.to_bytes(depth // 8, "big")
        flat.append(bytes(b))
    raw, prev = bytearray(), bytes(len(flat[0]))
    for y, line in enumerate(flat):
        f = filters[y % len(filters)]
        raw.append(f)
        for i, x in enumerate(line):
            left = line[i - step] if i >= step else 0
            up = prev[i]
            corner = prev[i - step] if i >= step else 0
            if f == 0:
                pred = 0
            elif f == 1:
                pred = left
            elif f == 2:
                pred = up
            elif f == 3:
                pred = (left + up) // 2
            else:
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                pred = left if pa <= pb and pa <= pc else up if pb <= pc else corner
            raw.append((x - pred) & 255)
        prev = line

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    w, h = len(rows[0]), len(rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, colour, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw)))
        + chunk(b"IEND", b"")
    )


def screen(w, h, page):
    """A phone's screenshot: a status bar (dark text on the top tenth's
    first rows) over `page(x, y)`, RGBA."""
    rows = []
    for y in range(h):
        row = []
        for x in range(w):
            if y < h // 20 and (x // 3) % 2:
                row.append((0, 0, 0, 255))
            else:
                row.append(page(x, y) + (255,))
        rows.append(row)
    return rows


def test():
    failed = 0

    def check(what, ok):
        nonlocal failed
        print(f"{'pass' if ok else 'FAIL'}: {what}")
        failed += not ok

    # The reader gives back every pixel, whatever the filter, depth and
    # colour type, including bytes that wrap past 255.
    rnd = random.Random(166)
    for colour, depth in ((6, 8), (2, 8), (0, 8), (4, 8), (6, 16), (2, 16)):
        n = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
        top = (1 << depth) - 1
        pixels = [[tuple(rnd.randint(0, top) for _ in range(n)) for _ in range(23)] for _ in range(11)]
        for filters in ((0,), (1,), (2,), (3,), (4,), (0, 1, 2, 3, 4)):
            _, _, step, rows = rows_of(png(pixels, colour, depth, filters))
            want = [b"".join(v.to_bytes(depth // 8, "big") for px in row for v in px) for row in pixels]
            check(f"colour type {colour}, {depth} bits, filters {filters}: every pixel read back", rows == want)

    blank = screen(60, 130, lambda x, y: (255, 255, 255))
    for filters in ((0,), (2,), (4,), (1, 2, 4)):
        share, colour = judged(png(blank, filters=filters))
        check(f"a white window under a status bar is one colour (filters {filters})", share == 1 and colour == "ffffffff")

    # A home indicator in the last twentieth is not judged.
    home = screen(60, 130, lambda x, y: (0, 0, 0) if y > 126 and 20 < x < 40 else (255, 255, 255))
    check("a home indicator does not count as drawn", judged(png(home))[0] == 1)

    # A form: a card, a field and a line of words on a grey page.
    def form(x, y):
        if 10 <= x < 50 and 20 <= y < 110:
            if 15 <= x < 45 and 40 <= y < 48:
                return (230, 232, 236)
            if 15 <= x < 45 and 60 <= y < 62 and x % 3:
                return (20, 22, 30)
            return (255, 255, 255)
        return (245, 246, 248)

    share, _ = judged(png(screen(60, 130, form), filters=(0, 1, 2, 3, 4)))
    check(f"a drawn form is not one colour ({share:.3f})", drawn(share))

    # One short line of words on white is drawn; a speck is not enough.
    words = screen(200, 400, lambda x, y: (0, 0, 0) if 150 <= y < 160 and 20 <= x < 180 and x % 4 else (255, 255, 255))
    share, _ = judged(png(words))
    check(f"one line of words counts as drawn ({share:.4f})", drawn(share))
    speck = screen(200, 400, lambda x, y: (0, 0, 0) if 150 <= y < 152 and 20 <= x < 24 else (255, 255, 255))
    share, _ = judged(png(speck))
    check(f"a speck does not ({share:.4f})", not drawn(share))

    for bad, why in ((b"GIF89a", "not a PNG"), (png(blank)[:60], "cut short")):
        try:
            judged(bad)
            check(f"{why}: refused", False)
        except Unreadable:
            check(f"{why}: refused", True)

    print(f"{'all passed' if not failed else f'{failed} failed'}")
    return 1 if failed else 0


def main(args):
    if args == ["--test"]:
        return test()
    if len(args) != 1:
        print(__doc__)
        return 2
    shot = Path(args[0])
    try:
        share, colour = judged(shot.read_bytes())
    except OSError as e:
        print(f"{shot.name}: cannot be read: {e.strerror or e}")
        return 2
    except Unreadable as e:
        print(f"{shot.name}: cannot be judged: {e}")
        return 2
    part = f"{share:.2%} of the picture below the status bar is #{colour}"
    if not drawn(share):
        print(f"{shot.name}: {part}: nothing is drawn there")
        return 1
    print(f"{shot.name}: {part}: something is drawn")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
