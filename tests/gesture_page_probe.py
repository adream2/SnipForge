"""手势页冒烟：切到"鼠标手势"，数一数缩略图是否都画出来了。

需要应用带远程调试启动：
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
    start src-tauri/target/release/snipforge.exe
"""
import json
import sys
import time

import cdp

ws = cdp.page_ws()
if not ws:
    print('没找到页面（应用没开调试端口？）')
    sys.exit(1)

click = """(() => {
  const btn = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === '鼠标手势');
  if (!btn) return 'no-nav';
  btn.click();
  return 'clicked';
})()"""
print('导航:', cdp.evaluate(ws, click)['result']['result'].get('value'))
time.sleep(1.5)

probe = """(() => {
  const svgs = document.querySelectorAll('svg.gesture-shape-svg');
  const empty = document.querySelectorAll('.gesture-shape-empty').length;
  const rows = [...document.querySelectorAll('.ga-row')].slice(0, 8)
    .map(r => r.innerText.replace(/\\s+/g, ' ').trim());
  return JSON.stringify({ svgCount: svgs.length, empty, rows });
})()"""
msg = cdp.evaluate(ws, probe)
val = msg['result']['result'].get('value')
print('页面:', val)
if val:
    data = json.loads(val)
    print('缩略图 %d 个，空占位 %d 个' % (data['svgCount'], data['empty']))
    for r in data['rows']:
        print('  ', r)
