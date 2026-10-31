"""排查：合成鼠标事件到底有没有进到录制框（pointerdown / mousedown 计数）。"""
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
    return m['result']['result'].get('value')


print('行数:', ev("document.querySelectorAll('.ga-row').length"))
if not ev("!!document.querySelector('.gesture-rec-box')"):
    print('点划一个:', ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => x.textContent.includes('划一个'));
      if (!b) return 'no-btn'; b.click(); return 'ok';
    })()"""))
    time.sleep(0.5)

print('装探针:', ev("""(() => {
  const el = document.querySelector('.gesture-rec-box');
  if (!el) return 'no-box';
  window.__p = { pd: 0, pm: 0, pu: 0, md: 0 };
  el.addEventListener('pointerdown', () => window.__p.pd++);
  el.addEventListener('pointermove', () => window.__p.pm++);
  el.addEventListener('pointerup', () => window.__p.pu++);
  el.addEventListener('mousedown', () => window.__p.md++);
  const r = el.getBoundingClientRect();
  return JSON.stringify({ x: r.left, y: r.top, w: r.width, h: r.height });
})()"""))

box = ev("""(() => { const r = document.querySelector('.gesture-rec-box').getBoundingClientRect();
  return JSON.stringify({x: r.left, y: r.top, w: r.width, h: r.height}); })()""")
b = json.loads(box)
x = b['x'] + 40
y = b['y'] + b['h'] / 2

# 先空点一下让窗口拿到焦点：WebView2 的第一下点击会被用来激活窗口，
# 不先吃掉它，真正那一笔的 pointerdown 就丢了
cdp.call(ws, 'Input.dispatchMouseEvent', {'type': 'mousePressed', 'x': 5, 'y': 5, 'button': 'left', 'buttons': 1, 'clickCount': 1})
cdp.call(ws, 'Input.dispatchMouseEvent', {'type': 'mouseReleased', 'x': 5, 'y': 5, 'button': 'left', 'buttons': 0, 'clickCount': 1})
time.sleep(0.2)

cdp.call(ws, 'Input.dispatchMouseEvent', {'type': 'mousePressed', 'x': x, 'y': y, 'button': 'left', 'buttons': 1, 'clickCount': 1})
for i in range(1, 25):
    cdp.call(ws, 'Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': x + i * 4, 'y': y - i * 2, 'button': 'left', 'buttons': 1})
cdp.call(ws, 'Input.dispatchMouseEvent', {'type': 'mouseReleased', 'x': x + 96, 'y': y - 48, 'button': 'left', 'buttons': 0, 'clickCount': 1})
time.sleep(0.8)

print('事件计数:', ev("JSON.stringify(window.__p)"))
print('行数(之后):', ev("document.querySelectorAll('.ga-row').length"))
