"""自定义手势端到端（JS 合成指针事件版）。

CDP 的 Input.dispatchMouseEvent 在 WebView2 上按下事件会被窗口激活吃掉，
所以这里直接用 DOM 事件走 React 的处理链 —— 验证的是"录制 → 规范化 → 建条目"
这条业务链路，而不是浏览器输入层。
"""
import json
import sys
import time

import cdp

ws = cdp.page_ws()
if not ws:
    print('没找到页面')
    sys.exit(1)


def ev(expr):
    m = cdp.evaluate(ws, expr)
    r = m['result']['result']
    return r.get('value')


print('行数:', ev("document.querySelectorAll('.ga-row').length"))
print(
    '点划一个:',
    ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => x.textContent.includes('划一个'));
      if (!b) return 'no-btn'; b.click(); return 'ok';
    })()"""),
)
time.sleep(0.5)

draw = """(async () => {
  const box = document.querySelector('.gesture-rec-box');
  if (!box) return 'no-box';
  const r = box.getBoundingClientRect();
  const x0 = r.left + 30, y0 = r.top + r.height * 0.78;
  const w = Math.min(r.width - 60, 150), h = r.height * 0.55;
  const pts = [
    [x0, y0],
    [x0 + w * 0.15, y0 - h],
    [x0 + w * 0.35, y0 - h * 0.35],
    [x0 + w * 0.55, y0 - h],
    [x0 + w * 0.75, y0]
  ];
  const fire = (type, x, y, buttons) => {
    box.dispatchEvent(new PointerEvent(type, {
      bubbles: true, cancelable: true, clientX: x, clientY: y,
      button: 0, buttons, pointerId: 1, pointerType: 'mouse', isPrimary: true
    }));
  };
  fire('pointerdown', pts[0][0], pts[0][1], 1);
  for (let i = 1; i < pts.length; i++) {
    for (let k = 1; k <= 12; k++) {
      const t = k / 12;
      fire('pointermove',
        pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * t,
        pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * t, 1);
      await new Promise(r => setTimeout(r, 8));
    }
  }
  fire('pointerup', pts[pts.length - 1][0], pts[pts.length - 1][1], 0);
  await new Promise(r => setTimeout(r, 1200));
  const rows = [...document.querySelectorAll('.ga-row')];
  const last = rows[rows.length - 1];
  const svg = last?.querySelector('svg.gesture-shape-svg');
  return JSON.stringify({
    count: rows.length,
    text: last ? last.innerText.replace(/\\s+/g, ' ').trim() : '',
    hasSvg: !!svg,
    pts: svg ? svg.querySelector('polyline').getAttribute('points').split(' ').length : 0
  });
})()"""

print('画 M:', ev(draw))
