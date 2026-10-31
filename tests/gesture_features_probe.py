"""回归探针：手势页的功能下拉是否真的来自后端注册表。

跑法（需要带调试端口启动）：
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
  start src-tauri/target/release/snipforge.exe
  python tests/gesture_features_probe.py
"""
import json
import sys
import time

sys.path.insert(0, 'tests')
import cdp


def main():
    ws = cdp.page_ws()
    if not ws:
        print('没找到页面（确认带了 --remote-debugging-port=9222）')
        return 1

    def ev(expr):
        r = cdp.evaluate(ws, expr)
        return r['result']['result'].get('value')

    ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => x.textContent.trim() === '鼠标手势');
      b && b.click();
    })()""")
    time.sleep(1.2)

    print('标签:', ev("JSON.stringify([...document.querySelectorAll('.ga-tabs button')].map(b=>b.textContent.trim()))"))
    print('行数:', ev("document.querySelectorAll('.ga-row').length"))
    print('缩略图:', ev("document.querySelectorAll('svg.gesture-shape-svg').length"))

    # 展开第一条 snip 动作，看下拉里的选项
    ev("""(() => {
      const rows = [...document.querySelectorAll('.ga-row')];
      const r = rows.find(x => x.innerText.includes('截图'));
      r && r.click();
    })()""")
    time.sleep(0.6)
    print('动作下拉:', ev("""JSON.stringify([...document.querySelectorAll('.ga-editor select')]
        .map(s => [...s.options].map(o => o.textContent)))"""))

    # 暂停/恢复：确认托盘勾选状态跟着变（配置落盘 + 菜单重建不报错）
    print('暂停按钮:', ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => ['暂停','启用'].includes(x.textContent.trim()));
      if (!b) return 'none';
      const label = b.textContent.trim();
      b.click();
      return label;
    })()"""))
    time.sleep(1.2)
    print('状态徽标:', ev("document.querySelector('.hotkey-chip')?.textContent.trim()"))
    # 再点回来，别把用户的手势停着
    ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => ['暂停','启用'].includes(x.textContent.trim()));
      b && b.click();
    })()""")
    time.sleep(0.8)
    print('恢复后:', ev("document.querySelector('.hotkey-chip')?.textContent.trim()"))
    return 0


if __name__ == '__main__':
    sys.exit(main())
