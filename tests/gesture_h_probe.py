"""端到端：在「试划」框里画一个 H，应该提示「编辑 hosts」。

走 DOM 合成指针事件（CDP 的鼠标事件会被窗口激活吃掉），验证的是
"录制 → 形状识别 → 绑定匹配 → 文案"这条链路。

跑法：
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
  python tests/gesture_h_probe.py
"""
import sys
import time

sys.path.insert(0, 'tests')
import cdp


def main():
    ws = cdp.page_ws(title='SnipForge')
    if not ws:
        print('没找到主窗口')
        return 1

    def ev(expr):
        r = cdp.evaluate(ws, expr)['result']
        if 'exceptionDetails' in r:
            return 'ERR: ' + str(r['exceptionDetails'])[:160]
        return r['result'].get('value')

    # 进手势页
    ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => x.textContent.trim() === '鼠标手势');
      b && b.click();
    })()""")
    time.sleep(1.0)

    # 点「试划」
    print('试划:', ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => x.textContent.includes('试划'));
      if (!b) return 'no-btn'; b.click(); return 'ok';
    })()"""))
    time.sleep(0.5)

    # 画一个 H：左腿下 → 回到中线 → 中横 → 右腿上 → 右腿下
    draw = """(() => {
      const box = document.querySelector('.gesture-rec-box');
      if (!box) return 'no-box';
      const r = box.getBoundingClientRect();
      const h = Math.min(r.height - 30, 62);
      const w = h * 0.8;
      const x0 = r.left + r.width * 0.3;
      const y0 = r.top + (r.height - h) / 2;
      const ym = y0 + h / 2;
      const pts = [[x0, y0], [x0, y0 + h], [x0, ym], [x0 + w, ym], [x0 + w, y0], [x0 + w, y0 + h]];
      const fire = (type, x, y, buttons) => box.dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, clientX: x, clientY: y,
        button: 0, buttons, pointerId: 1, pointerType: 'mouse', isPrimary: true
      }));
      fire('pointerdown', pts[0][0], pts[0][1], 1);
      for (let i = 1; i < pts.length; i++) {
        for (let k = 1; k <= 10; k++) {
          const t = k / 10;
          fire('pointermove',
            pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * t,
            pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * t, 1);
        }
      }
      fire('pointerup', pts[pts.length - 1][0], pts[pts.length - 1][1], 0);
      return 'drawn';
    })()"""
    print('画图:', ev(draw))
    time.sleep(1.2)
    print('提示:', ev("JSON.stringify([...document.querySelectorAll('.toast')].map(t=>t.innerText.trim()))"))
    return 0


if __name__ == '__main__':
    sys.exit(main())
