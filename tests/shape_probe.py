"""形状识别的数值探针（不需要跑 Rust 单测）。

后端 `cargo test` 的测试 exe 在这台机器上一启动就 0xC0000139，所以这里
直接调 `gesture_recognize` 命令，用合成轨迹验证 S / H 的分离度。

跑法：
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
  python tests/shape_probe.py
"""
import json
import math
import sys

sys.path.insert(0, 'tests')
import cdp


def polyline(v, per=20):
    out = []
    for a, b in zip(v, v[1:]):
        for i in range(per):
            t = i / per
            out.append((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
    out.append(v[-1])
    return out


def one_stroke_h(cx, cy, h, w, up_first=True, jitter=0.0, per=14):
    x0, x1 = cx - w / 2, cx + w / 2
    y0, y1 = cy - h / 2, cy + h / 2
    ym = cy
    v = [(x0, y0), (x0, y1), (x0, ym), (x1, ym)]
    v += [(x1, y0), (x1, y1)] if up_first else [(x1, y1), (x1, y0)]
    pts = polyline(v, per)
    if jitter:
        pts = [
            (x + (((i * 7919) % 13) / 13 - 0.5) * 2 * jitter,
             y + (((i * 104729) % 11) / 11 - 0.5) * 2 * jitter)
            for i, (x, y) in enumerate(pts)
        ]
    return pts


def letter_s(cx, cy, h, ratio=0.7, jitter=0.0, n=60):
    p0, p1, p2, p3 = (0.6, -1), (-1, -0.6), (1, 0.6), (-0.6, 1)
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        b0, b1, b2, b3 = u**3, 3 * u * u * t, 3 * u * t * t, t**3
        x = b0 * p0[0] + b1 * p1[0] + b2 * p2[0] + b3 * p3[0]
        y = b0 * p0[1] + b1 * p1[1] + b2 * p2[1] + b3 * p3[1]
        jx = ((i * 7919) % 13) / 13 - 0.5
        jy = ((i * 104729) % 11) / 11 - 0.5
        out.append((cx + x * h * ratio * 0.5 + jx * 2 * jitter,
                    cy + y * h * 0.5 + jy * 2 * jitter))
    return out


def circle(cx, cy, r, n=72):
    return [(cx + r * math.cos(i / n * math.tau), cy + r * math.sin(i / n * math.tau))
            for i in range(n + 1)]


def main():
    ws = cdp.page_ws(title='SnipForge')
    if not ws:
        print('没找到主窗口（确认带了 --remote-debugging-port=9222）')
        return 1

    def recognize(pts):
        arr = [[int(round(x)), int(round(y))] for x, y in pts]
        expr = (
            "window.__TAURI_INTERNALS__.invoke('gesture_recognize', "
            "{ points: %s })" % json.dumps(arr)
        )
        r = cdp.evaluate(ws, expr)
        res = r['result']
        if 'exceptionDetails' in res:
            return 'ERR:' + json.dumps(res['exceptionDetails'])[:120]
        return res['result'].get('value')

    cases = []
    # 正例：H
    for name, pts in [
        ('H 标准', one_stroke_h(600, 400, 300, 220)),
        ('H 收右上', one_stroke_h(600, 400, 300, 220, up_first=False)),
        ('H 小 140x100', one_stroke_h(600, 400, 140, 100, per=8)),
        ('H 抖 6px', one_stroke_h(600, 400, 300, 220, jitter=6)),
        ('H 宽扁 300x380', one_stroke_h(600, 400, 300, 380)),
        ('H 瘦高 380x150', one_stroke_h(600, 400, 380, 150)),
        ('H 反向', list(reversed(one_stroke_h(600, 400, 300, 220)))),
    ]:
        cases.append(('H', name, pts))
    # 正例：S（不能被 H 抢走）
    for name, pts in [
        ('S 字体式 300', letter_s(600, 400, 300)),
        ('S 斜 25°', letter_s(600, 400, 300, jitter=3)),
        ('S 抖 12px', letter_s(600, 400, 300, jitter=12)),
    ]:
        cases.append(('S', name, pts))
    # 反例：不该被认成任何形状
    negs = [
        ('竖划', polyline([(600, 200), (600, 600)], 40)),
        ('横划', polyline([(300, 400), (800, 400)], 40)),
        ('Z', polyline([(400, 200), (700, 200), (400, 600), (700, 600)], 30)),
        ('L', polyline([(400, 200), (400, 600), (700, 600)], 30)),
        ('U', polyline([(400, 200), (400, 600), (700, 600), (700, 200)], 30)),
        ('V', polyline([(400, 200), (600, 600), (800, 200)], 30)),
        ('圆', circle(600, 400, 150)),
        ('C', [(600 + 150 * math.cos(a), 400 + 150 * math.sin(a))
               for a in [(-2.5 + i / 40 * 3.9) for i in range(41)]]),
    ]
    for name, pts in negs:
        cases.append(('无', name, pts))

    def diagnose(pts):
        arr = [[int(round(x)), int(round(y))] for x, y in pts]
        expr = ("window.__TAURI_INTERNALS__.invoke('gesture_diagnose', "
                "{ points: %s })" % json.dumps(arr))
        r = cdp.evaluate(ws, expr)['result']
        if 'exceptionDetails' in r:
            return 'ERR'
        return r['result'].get('value')

    bad = 0
    for want, name, pts in cases:
        got = recognize(pts) or 'None'
        ok = (got == want) if want != '无' else (got == 'None')
        if not ok:
            bad += 1
        extra = '' if ok else '   | ' + diagnose(pts)
        print(f"{'ok ' if ok else 'BAD'} {name:16s} 期望 {want:4s} 实际 {got}{extra}")

    print(f"\n{len(cases) - bad}/{len(cases)} 通过")
    return 0 if bad == 0 else 1


if __name__ == '__main__':
    sys.exit(main())
