"""形状识别的数值探针（不需要跑 Rust 单测）。

后端 `cargo test` 的测试 exe 在这台机器上一启动就 0xC0000139，所以这里
直接调 IPC，用合成轨迹量分离度。

用法：
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
  python tests/shape_probe.py          # 只看判定
  python tests/shape_probe.py -v       # 连诊断行（逐点/点云两套距离）一起打
"""
import json
import math
import sys

sys.path.insert(0, 'tests')
import cdp

VERBOSE = '-v' in sys.argv or '--verbose' in sys.argv


def polyline(v, per=20):
    out = []
    for a, b in zip(v, v[1:]):
        for i in range(per):
            t = i / per
            out.append((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
    out.append(v[-1])
    return out


def one_stroke_h(cx, cy, h, w, up_first=True, jitter=0.0, per=14, bar=0.0):
    x0, x1 = cx - w / 2, cx + w / 2
    y0, y1 = cy - h / 2, cy + h / 2
    ym = cy + bar * h / 2
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


def cases():
    out = []
    # 正例：H
    for name, pts in [
        ('H 标准', one_stroke_h(600, 400, 300, 220)),
        ('H 收右上', one_stroke_h(600, 400, 300, 220, up_first=False)),
        ('H 小 140x100', one_stroke_h(600, 400, 140, 100, per=8)),
        ('H 抖 6px', one_stroke_h(600, 400, 300, 220, jitter=6)),
        ('H 宽扁 300x380', one_stroke_h(600, 400, 300, 380)),
        ('H 极扁 200x520', one_stroke_h(600, 400, 200, 520)),
        ('H 瘦高 380x150', one_stroke_h(600, 400, 380, 150)),
        ('H 横偏上', one_stroke_h(600, 400, 300, 220, bar=0.35)),
        ('H 横偏下', one_stroke_h(600, 400, 300, 220, bar=-0.35)),
        ('H 反向', list(reversed(one_stroke_h(600, 400, 300, 220)))),
    ]:
        out.append(('H', name, pts))
    # 正例：S
    for name, pts in [
        ('S 字体式 300', letter_s(600, 400, 300)),
        ('S 斜写 25°', letter_s(600, 400, 300, jitter=3)),
        ('S 抖 12px', letter_s(600, 400, 300, jitter=12)),
        ('S 宽扁 2.5', letter_s(600, 400, 300, ratio=2.5)),
        ('S 瘦高 0.3', letter_s(600, 400, 360, ratio=0.3)),
        ('S 反向', list(reversed(letter_s(600, 400, 300)))),
    ]:
        out.append(('S', name, pts))
    # 反例：都不能被认成形状（否则会抢掉方向手势）
    negs = [
        ('竖划', polyline([(600, 200), (600, 600)], 40)),
        ('横划', polyline([(300, 400), (800, 400)], 40)),
        ('斜划', polyline([(300, 200), (700, 600)], 40)),
        ('Z 300', polyline([(400, 200), (700, 200), (400, 600), (700, 600)], 30)),
        ('Z 大 500', polyline([(300, 100), (800, 100), (300, 700), (800, 700)], 40)),
        ('Z 扁', polyline([(300, 300), (800, 300), (300, 500), (800, 500)], 40)),
        ('L', polyline([(400, 200), (400, 600), (700, 600)], 30)),
        ('U', polyline([(400, 200), (400, 600), (700, 600), (700, 200)], 30)),
        ('V', polyline([(400, 200), (600, 600), (800, 200)], 30)),
        ('圆', circle(600, 400, 150)),
        ('C', [(600 + 150 * math.cos(a), 400 + 150 * math.sin(a))
               for a in [(-2.5 + i / 40 * 3.9) for i in range(41)]]),
        ('工字', polyline([(400, 200), (800, 200), (800, 400), (400, 400), (400, 600), (800, 600)], 20)),
        ('日字', polyline([(400, 200), (700, 200), (700, 600), (400, 600), (400, 200),
                           (400, 400), (700, 400)], 20)),
        ('回笔竖', polyline([(600, 200), (600, 600), (600, 300)], 30)),
        ('回笔横', polyline([(400, 400), (800, 400), (550, 400)], 30)),
        # 下面几个是点云匹配最容易被"拉近"的形状：点分布像，但没有 S/H 的走向。
        # 切点云之后必须仍然全部拒绝，否则就成了"点云什么都像"。
        ('波浪', [(300 + i * 12, 400 + 120 * math.sin(i / 8 * math.pi))
                  for i in range(41)]),
        ('三角', polyline([(500, 200), (700, 600), (300, 600), (500, 200)], 25)),
        ('J 勾', [(500, 200), (500, 500)] + [
            (500 + 130 * math.sin(i / 30 * math.pi / 2),
             500 + 130 * (1 - math.cos(i / 30 * math.pi / 2))) for i in range(31)]),
        ('心形', [(600 + 150 * math.sin(t) ** 3,
                   400 - 120 * (0.8125 * math.cos(t) - 0.3125 * math.cos(2 * t)
                                - 0.125 * math.cos(3 * t) - 0.0625 * math.cos(4 * t)))
                  for t in [i / 40 * math.tau for i in range(41)]]),
        # 手画的"拖一下"：点云天生对直线宽容，这类必须全部拒绝。
        # 宽度是重点：窄于 24px 已经被 bbox 闸门挡了，这里专门测"宽到能过闸门
        # 但其实还是直线"的那一段。
        ('竖拖 26x400', polyline([(600, 200), (626, 600)], 40)),
        ('竖拖 32x400', polyline([(600, 200), (632, 600)], 40)),
        ('竖拖 60x400', polyline([(600, 200), (660, 600)], 40)),
        ('竖划微斜 40x400', polyline([(600, 200), (640, 600)], 40)),
        ('手抖竖划 8px', [
            (600 + (((i * 7919) % 13) / 13 - 0.5) * 16, 200 + i * 10) for i in range(41)]),
        ('手抖竖划 3px', [
            (600 + (((i * 7919) % 13) / 13 - 0.5) * 6, 200 + i * 10) for i in range(41)]),
        ('略弯的拖线', [(600 + 40 * math.sin(i / 40 * math.pi), 200 + i * 10) for i in range(41)]),
        ('慢慢拖的短笔', polyline([(600, 300), (620, 420)], 12)),
        ('横拖 400x40', polyline([(400, 400), (800, 440)], 40)),
        ('斜拖 45°', polyline([(400, 200), (700, 500)], 40)),
    ]
    for name, pts in negs:
        out.append(('无', name, pts))
    return out


def main():
    ws = cdp.page_ws(title='SnipForge')
    if not ws:
        print('没找到主窗口（确认带了 --remote-debugging-port=9222）')
        return 1

    def invoke(cmd, pts):
        arr = [[int(round(x)), int(round(y))] for x, y in pts]
        expr = (f"window.__TAURI_INTERNALS__.invoke('{cmd}', "
                "{ points: %s })" % json.dumps(arr))
        r = cdp.evaluate(ws, expr)['result']
        if 'exceptionDetails' in r:
            return 'ERR'
        return r['result'].get('value')

    bad = 0
    for want, name, pts in cases():
        got = invoke('gesture_recognize', pts) or 'None'
        ok = (got == want) if want != '无' else (got == 'None')
        if not ok:
            bad += 1
        line = f"{'ok ' if ok else 'BAD'} {name:16s} 期望 {want:4s} 实际 {got}"
        if VERBOSE:
            line += '   | ' + str(invoke('gesture_diagnose', pts))
        print(line)

    total = len(cases())
    print(f"\n{total - bad}/{total} 通过")
    return 0 if bad == 0 else 1


if __name__ == '__main__':
    sys.exit(main())
