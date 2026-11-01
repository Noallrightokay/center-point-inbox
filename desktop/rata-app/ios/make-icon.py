"""RATA's iOS app icon, made from the desktop app's own artwork.

    python3 make-icon.py            write AppIcon-1024.png beside this script
    python3 make-icon.py --check    the committed AppIcon-1024.png is what this
                                    script makes (by its pixels: zlib builds
                                    differ in the bytes they write)
    python3 make-icon.py --like P   the picture in PNG file P looks like it
                                    (check-app.sh: the icon a build carries)

The source is src-tauri/icons/icon.png, the squirrel the desktop app shows,
512 px. The App Store wants 1024 px with no alpha channel, so it is laid on
its own background colour (it is opaque already; this makes sure) and
doubled with a Catmull-Rom filter, then written as 8-bit RGB. prepare.sh
puts it in the Xcode project as a single-size app icon, from which Xcode
makes every size a phone shows. A sharper icon needs artwork drawn at
1024 px or as vectors (D9).

Standard library only: Python on GitHub's Macs has no imaging package.
"""

import struct
import sys
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE.parent / "src-tauri" / "icons" / "icon.png"
ICON = HERE / "AppIcon-1024.png"
SIZE = 1024


def read_png(path):
    """(width, height, rows of RGB tuples) from an 8- or 16-bit, non-interlaced
    PNG, greyscale or colour, with or without alpha. Alpha is laid over the
    picture's own top-left colour."""
    data = Path(path).read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    pos, chunks, idat = 8, {}, b""
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + n]
        pos += 12 + n
        if kind == b"IDAT":
            idat += body
        else:
            chunks.setdefault(kind, body)
        if kind == b"IEND":
            break
    if b"CgBI" in chunks:
        raise SystemExit(f"{path}: an iPhone-optimised PNG; convert it with sips first")
    w, h, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", chunks[b"IHDR"])
    if interlace or depth not in (8, 16) or colour not in (0, 2, 4, 6):
        raise SystemExit(f"{path}: depth {depth}, colour type {colour}, interlace {interlace}: not read here")
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
    step = channels * depth // 8
    stride = w * step
    raw = zlib.decompress(idat)
    rows, prev = [], bytearray(stride)
    for y in range(h):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - step] if i >= step else 0
            b = prev[i]
            c = prev[i - step] if i >= step else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    out = []
    for line in rows:
        px = []
        for x in range(w):
            v = [line[x * step + k * depth // 8] for k in range(channels)]
            if channels <= 2:
                v = [v[0], v[0], v[0]] + v[1:]
            px.append(tuple(v))
        out.append(px)
    if channels in (2, 4):
        bg = out[0][0][:3]
        out = [
            [tuple((p[k] * p[3] + bg[k] * (255 - p[3]) + 127) // 255 for k in range(3)) for p in row]
            for row in out
        ]
    return w, h, out


def write_png(path, rows):
    """8-bit RGB, no alpha, each row filtered by Sub."""
    h, w = len(rows), len(rows[0])
    raw = bytearray()
    for row in rows:
        raw.append(1)
        last = (0, 0, 0)
        for p in row:
            raw.extend(((p[0] - last[0]) & 255, (p[1] - last[1]) & 255, (p[2] - last[2]) & 255))
            last = p

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    Path(path).write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def catmull_rom(t):
    t = abs(t)
    if t < 1:
        return 1.5 * t**3 - 2.5 * t**2 + 1
    if t < 2:
        return -0.5 * t**3 + 2.5 * t**2 - 4 * t + 2
    return 0.0


def doubled(rows):
    """Twice the size each way. Output pixel i sits at source i/2 - 1/4, so
    there are two sets of four weights, one for even and one for odd."""
    phases = []
    for i in (0, 1):
        at = i / 2 - 0.25
        first = int(at // 1) - 1
        phases.append((first, [catmull_rom(at - (first + k)) for k in range(4)]))

    def line(px):
        n, out = len(px), []
        for i in range(2 * n):
            first, weights = phases[i % 2]
            first += i // 2
            acc = [0.0, 0.0, 0.0]
            for k, wgt in enumerate(weights):
                p = px[min(max(first + k, 0), n - 1)]
                for c in range(3):
                    acc[c] += wgt * p[c]
            out.append(tuple(min(255, max(0, int(v + 0.5))) for v in acc))
        return out

    wide = [line(row) for row in rows]
    cols = [line([row[x] for row in wide]) for x in range(len(wide[0]))]
    return [[cols[x][y] for x in range(len(cols))] for y in range(len(cols[0]))]


def made():
    w, h, rows = read_png(SOURCE)
    if (w, h) != (SIZE // 2, SIZE // 2):
        raise SystemExit(f"{SOURCE} is {w}x{h}; this script doubles a {SIZE // 2} px picture")
    return doubled(rows)


def means(rows, cells=8):
    """The picture as cells x cells average colours."""
    h, w = len(rows), len(rows[0])
    out = []
    for cy in range(cells):
        for cx in range(cells):
            ys = range(cy * h // cells, (cy + 1) * h // cells)
            xs = range(cx * w // cells, (cx + 1) * w // cells)
            n = len(ys) * len(xs)
            out.append(tuple(sum(rows[y][x][c] for y in ys for x in xs) / n for c in range(3)))
    return out


def main(args):
    if not args:
        write_png(ICON, made())
        print(f"wrote {ICON}")
        return 0
    if args == ["--check"]:
        w, h, rows = read_png(ICON)
        data = ICON.read_bytes()
        colour = data[25]
        if (w, h) != (SIZE, SIZE) or colour != 2:
            print(f"{ICON.name}: {w}x{h}, colour type {colour}; it must be {SIZE}x{SIZE} RGB with no alpha")
            return 1
        if rows != made():
            print(f"{ICON.name} is not what make-icon.py makes from {SOURCE.name}: run it again")
            return 1
        print(f"{ICON.name}: {SIZE}x{SIZE}, no alpha, made from {SOURCE.name}")
        return 0
    if len(args) == 2 and args[0] == "--like":
        _, _, ours = read_png(ICON)
        _, _, theirs = read_png(args[1])
        diff = sum(
            abs(a[c] - b[c]) for a, b in zip(means(ours), means(theirs)) for c in range(3)
        ) / (64 * 3)
        print(f"{args[1]}: on average {diff:.1f} of 255 from RATA's icon")
        return 0 if diff < 12 else 1
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
