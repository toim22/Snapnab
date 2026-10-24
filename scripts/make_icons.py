# Gera src/resources/Snapnab.ico e SnapnabEditor.ico (PNG embutido em ICO) sem dependências externas.
#   python scripts/make_icons.py
import zlib, struct, math, os

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'src', 'resources')

def png(w, h, px):
    raw = b''.join(b'\x00' + bytes(px[y*w*4:(y+1)*w*4]) for y in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')

def render(s, editor):
    ss = 4  # supersampling
    px = [0] * (s * s * 4)
    c1 = (255, 122, 60) if editor else (30, 140, 255)
    c2 = (214, 40, 90) if editor else (20, 70, 200)
    r = s * 0.22
    for y in range(s):
        for x in range(s):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(ss):
                for sx in range(ss):
                    fx = x + (sx + .5) / ss; fy = y + (sy + .5) / ss
                    # quadrado arredondado
                    dx = max(r - fx, 0, fx - (s - r)); dy = max(r - fy, 0, fy - (s - r))
                    if dx * dx + dy * dy > r * r: continue
                    t = (fx + fy) / (2 * s)
                    col = [c1[i] + (c2[i] - c1[i]) * t for i in range(3)]
                    u, v = fx / s, fy / s
                    white = False
                    if editor:
                        # quadro + lápis diagonal
                        if (0.18 < u < 0.70 and 0.20 < v < 0.72) and not (0.25 < u < 0.63 and 0.27 < v < 0.65): white = True
                        d = abs((u - 0.42) - (0.75 - v) * 1.0) / math.sqrt(2)
                        if d < 0.06 and 0.38 < u < 0.86: white = True
                    else:
                        cx, cy = 0.5, 0.5
                        rr = math.hypot(u - cx, v - cy)
                        if 0.15 < rr < 0.22: white = True
                        if abs(u - cx) < 0.035 and (0.10 < v < 0.33 or 0.67 < v < 0.90): white = True
                        if abs(v - cy) < 0.035 and (0.10 < u < 0.33 or 0.67 < u < 0.90): white = True
                    if white: col = [255, 255, 255]
                    for i in range(3): acc[i] += col[i]
                    acc[3] += 255
            n = ss * ss
            a = acc[3] / n
            i = (y * s + x) * 4
            if a > 0:
                px[i] = int(acc[0] / (acc[3] / 255)); px[i+1] = int(acc[1] / (acc[3] / 255)); px[i+2] = int(acc[2] / (acc[3] / 255))
            px[i+3] = int(a)
    return [min(255, max(0, v)) for v in px]

def ico(path, editor):
    sizes = [16, 24, 32, 48, 64, 256]
    imgs = [png(s, s, render(s, editor)) for s in sizes]
    out = struct.pack('<HHH', 0, 1, len(sizes))
    off = 6 + 16 * len(sizes)
    for s, d in zip(sizes, imgs):
        out += struct.pack('<BBBBHHII', s % 256, s % 256, 0, 0, 1, 32, len(d), off)
        off += len(d)
    out += b''.join(imgs)
    open(path, 'wb').write(out)

ico(os.path.join(OUT, 'Snapnab.ico'), False)
ico(os.path.join(OUT, 'SnapnabEditor.ico'), True)
print('ok')
