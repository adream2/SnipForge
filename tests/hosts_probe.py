"""hosts 页 + H 手势的回归探针。

只验证"读"和界面：保存会弹 UAC，脚本里不能点（会把桌面卡住）。

跑法：
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
  python tests/hosts_probe.py
"""
import json
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
        r = cdp.evaluate(ws, expr)
        res = r['result']
        if 'exceptionDetails' in res:
            return 'ERR: ' + json.dumps(res['exceptionDetails'])[:200]
        return res['result'].get('value')

    # 1) 手势列表里有 H
    ev("""(() => {
      const b = [...document.querySelectorAll('button')].find(x => x.textContent.trim() === '鼠标手势');
      b && b.click();
    })()""")
    time.sleep(1.2)
    rows = ev("JSON.stringify([...document.querySelectorAll('.ga-row')].map(r=>r.innerText.replace(/\\s+/g,' ').trim()))")
    print('行数:', len(json.loads(rows)))
    print('含 hosts 的行:', [r for r in json.loads(rows) if 'hosts' in r])

    # 2) 进 hosts 页
    ev("window.__TAURI_INTERNALS__.invoke('app_show_main', { route: 'hosts' })")
    time.sleep(1.2)
    print('路径:', ev("document.querySelector('.hosts-path')?.textContent"))
    print('行数/字符:', ev("document.querySelector('.hosts-foot')?.innerText.replace(/\\s+/g,' ').trim()"))
    print('提示:', ev("JSON.stringify([...document.querySelectorAll('.hosts-note')].map(n=>n.innerText.replace(/\\s+/g,' ').trim()))"))
    print('有 UAC 说明:', ev("document.body.innerText.includes('UAC')"))
    print('内容前 60 字:', ev("document.querySelector('.hosts-editor')?.value.slice(0,60)"))
    print('备份接口:', ev("window.__TAURI_INTERNALS__.invoke('hosts_backups').then(v=>v.length)"))
    return 0


if __name__ == '__main__':
    sys.exit(main())
