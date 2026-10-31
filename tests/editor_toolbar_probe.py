"""检查截图编辑器工具栏：确认「取字」「钉图」已经不在，其余按钮还在。

跑法（需要调试端口）：
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
  python tests/editor_toolbar_probe.py
"""
import sys
import time

sys.path.insert(0, 'tests')
import cdp


def wait_ws(title, timeout=8.0):
    end = time.time() + timeout
    while time.time() < end:
        ws = cdp.page_ws(title=title)
        if ws:
            return ws
        time.sleep(0.2)
    return None


def main():
    main_ws = cdp.page_ws(title='SnipForge')
    if not main_ws:
        print('没找到主窗口')
        return 1

    # 触发一次区域截图：主窗 → 选区覆盖层
    cdp.evaluate(main_ws, "window.__TAURI_INTERNALS__.invoke('capture_start', { mode: 'region' })")
    time.sleep(1.2)

    sel = wait_ws('SnipForge Selector')
    if not sel:
        print('选区窗口没起来')
        return 1

    # 拖一个选区出来（一开始没有任何选区，回车是空操作）
    def mouse(t, x, y, buttons):
        cdp.call(sel, 'Input.dispatchMouseEvent', {
            'type': t, 'x': x, 'y': y, 'button': 'left', 'buttons': buttons, 'clickCount': 1,
        })

    # 第一下点击会被用来激活窗口，先空点一下吃掉
    mouse('mousePressed', 4, 4, 1)
    mouse('mouseReleased', 4, 4, 0)
    time.sleep(0.2)
    mouse('mousePressed', 120, 120, 1)
    for i in range(1, 8):
        cdp.call(sel, 'Input.dispatchMouseEvent', {
            'type': 'mouseMoved', 'x': 120 + i * 70, 'y': 120 + i * 45,
            'button': 'left', 'buttons': 1,
        })
    mouse('mouseReleased', 620, 440, 0)
    time.sleep(0.4)

    # 回车 = 确认选区，进编辑器。
    # 注意：确认后选区窗口会立刻销毁，CDP 连接会断 —— 这是预期，不是错误。
    for t in ('keyDown', 'keyUp'):
        try:
            cdp.call(sel, 'Input.dispatchKeyEvent', {
                'type': t, 'key': 'Enter', 'code': 'Enter',
                'windowsVirtualKeyCode': 13, 'nativeVirtualKeyCode': 13,
            })
        except RuntimeError:
            break
    time.sleep(2.0)

    ed = wait_ws('SnipForge Editor')
    if not ed:
        print('编辑器没起来')
        return 1

    r = cdp.evaluate(ed, """(() => {
      const bar = document.querySelector('.editor-topbar');
      if (!bar) return JSON.stringify({ error: 'no-topbar' });
      const btns = [...bar.querySelectorAll('button')].map(b => b.textContent.trim()).filter(Boolean);
      return JSON.stringify({ buttons: btns, hasOcr: document.body.innerText.includes('取字'),
                              hasPin: document.body.innerText.includes('钉图') });
    })()""")
    print(r['result']['result'].get('value'))

    cdp.evaluate(ed, "window.__TAURI_INTERNALS__.invoke('capture_close_editor')")
    time.sleep(0.5)
    print('已关闭编辑器')
    return 0


if __name__ == '__main__':
    sys.exit(main())
