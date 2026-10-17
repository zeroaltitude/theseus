#!/usr/bin/env python3
"""Makes the six app emojis the Discord binding uploads (theseus-ss36w).

Standard library only. Run it from anywhere; it writes beside itself:

    theseus_working.gif     a spinning arc (animated)
    theseus_needs_you.gif   a pulsing red dot (animated)
    theseus_done.png        a green disc with a check
    theseus_failed.png      a red disc with a cross
    theseus_sleeping.png    a crescent moon
    theseus_job.png         a gear

Each is 128x128 (Discord's emoji size), well under its 256 KB limit. The
output is committed; this script is how it was made, so a change of art is a
change here and a re-run.
"""
import math
import os
import struct
import zlib

SIZE = 128
SS = 3  # supersampling per axis, for the PNGs' soft edges
HERE = os.path.dirname(os.path.abspath(__file__))

BLUE = (62, 99, 221)
RED = (226, 57, 62)
GREEN = (46, 160, 67)
GOLD = (245, 192, 53)
GREY = (125, 133, 144)
WHITE = (255, 255, 255)


def cover(shade, x, y):
    """Mean colour and alpha of pixel (x, y) under SS x SS samples of `shade`,
    a function (u, v) in [0, SIZE) giving an (r, g, b) colour or None."""
    r = g = b = n = 0
    for i in range(SS):
        for j in range(SS):
            c = shade(x + (i + 0.5) / SS, y + (j + 0.5) / SS)
            if c:
                r, g, b, n = r + c[0], g + c[1], b + c[2], n + 1
    if not n:
        return (0, 0, 0, 0)
    return (r // n, g // n, b // n, 255 * n // (SS * SS))


def png(path, shade):
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)
        for x in range(SIZE):
            raw.extend(cover(shade, x, y))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    data += chunk(b"IEND", b"")
    open(os.path.join(HERE, path), "wb").write(data)


def lzw(indices, min_code):
    """GIF's variable-width LZW."""
    clear, end = 1 << min_code, (1 << min_code) + 1
    out, acc, nbits = bytearray(), 0, 0

    def put(code, width):
        nonlocal acc, nbits
        acc |= code << nbits
        nbits += width
        while nbits >= 8:
            out.append(acc & 255)
            acc >>= 8
            nbits -= 8

    table, width, nxt = {}, min_code + 1, end + 1
    put(clear, width)
    cur = b""
    for ix in indices:
        nxt_seq = cur + bytes([ix])
        if len(nxt_seq) == 1 or nxt_seq in table:
            cur = nxt_seq
            continue
        put(table[cur] if len(cur) > 1 else cur[0], width)
        table[nxt_seq] = nxt
        nxt += 1
        if nxt > (1 << width) and width < 12:
            width += 1
        if nxt >= 4096:
            put(clear, width)
            table, width, nxt = {}, min_code + 1, end + 1
        cur = bytes([ix])
    put(table[cur] if len(cur) > 1 else cur[0], width)
    put(end, width)
    if nbits:
        out.append(acc & 255)
    return bytes(out)


def gif(path, frames, centis):
    """`frames`: shade functions. Colours are the frame's own, quantised to a
    shared palette of the distinct colours; index 0 is transparent."""
    palette, pixels = [(0, 0, 0)], []
    for shade in frames:
        idx = []
        for y in range(SIZE):
            for x in range(SIZE):
                c = shade(x + 0.5, y + 0.5)
                if not c:
                    idx.append(0)
                    continue
                if c not in palette:
                    palette.append(c)
                idx.append(palette.index(c))
        pixels.append(idx)
    assert len(palette) <= 256, len(palette)
    bits = max(1, (len(palette) - 1).bit_length())
    palette += [(0, 0, 0)] * ((1 << bits) - len(palette))
    out = b"GIF89a" + struct.pack("<HHBBB", SIZE, SIZE, 0x80 | (bits - 1), 0, 0)
    out += b"".join(bytes(c) for c in palette)
    out += b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00"  # loop forever
    min_code = max(2, bits)
    for idx in pixels:
        out += b"\x21\xf9\x04" + bytes([0b00001001]) + struct.pack("<H", centis) + b"\x00\x00"
        out += b"\x2c" + struct.pack("<HHHHB", 0, 0, SIZE, SIZE, 0)
        out += bytes([min_code])
        data = lzw(idx, min_code)
        for k in range(0, len(data), 255):
            block = data[k:k + 255]
            out += bytes([len(block)]) + block
        out += b"\x00"
    out += b"\x3b"
    open(os.path.join(HERE, path), "wb").write(out)


C = SIZE / 2


def disc(r, colour):
    return lambda x, y: colour if math.hypot(x - C, y - C) <= r else None


def over(*layers):
    def shade(x, y):
        hit = None
        for layer in layers:
            hit = layer(x, y) or hit
        return hit
    return shade


def seg(x, y, a, b, half):
    """Whether (x, y) lies within `half` of the segment a-b."""
    (ax, ay), (bx, by) = a, b
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(x - (ax + t * dx), y - (ay + t * dy)) <= half


def stroke(points, half, colour):
    return lambda x, y: colour if any(seg(x, y, a, b, half) for a, b in zip(points, points[1:])) else None


def spinner(turn):
    """A ring with a bright arc a third of the way round, at angle `turn`."""
    def shade(x, y):
        r = math.hypot(x - C, y - C)
        if not 38 <= r <= 58:
            return None
        ahead = (turn - math.atan2(y - C, x - C)) % (2 * math.pi)
        if ahead < 2.2:
            return BLUE
        return (178, 190, 238)
    return shade


def pulse(phase):
    r = 34 + 22 * (0.5 + 0.5 * math.sin(phase))
    halo = 60 if r > 52 else 0
    return lambda x, y: (RED if math.hypot(x - C, y - C) <= r else ((250, 190, 192) if halo and math.hypot(x - C, y - C) <= halo else None))


def moon(x, y):
    in_moon = math.hypot(x - 60, y - C) <= 48
    bite = math.hypot(x - 84, y - 50) <= 40
    return GOLD if in_moon and not bite else None


def gear(x, y):
    dx, dy = x - C, y - C
    r, a = math.hypot(dx, dy), math.atan2(dy, dx)
    if r <= 20:
        return None
    tooth = (a * 8 / (2 * math.pi)) % 1.0
    reach = 58 if 0.2 <= tooth <= 0.8 else 46
    return GREY if r <= reach else None


def main():
    gif("theseus_working.gif", [spinner(k * 2 * math.pi / 12) for k in range(12)], 7)
    gif("theseus_needs_you.gif", [pulse(k * 2 * math.pi / 10) for k in range(10)], 8)
    png("theseus_done.png", over(disc(58, GREEN), stroke([(36, 66), (55, 86), (92, 44)], 7, WHITE)))
    png("theseus_failed.png", over(disc(58, RED), stroke([(40, 40), (88, 88)], 7, WHITE), stroke([(88, 40), (40, 88)], 7, WHITE)))
    png("theseus_sleeping.png", moon)
    png("theseus_job.png", gear)


if __name__ == "__main__":
    main()
