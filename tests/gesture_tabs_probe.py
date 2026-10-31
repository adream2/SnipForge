"""检查手势页三个标签：通用 / 程序专用 / 设置。需要一个调试端口。"""
import sys
import time

sys.path.insert(0, 'tests')
import cdp

ws = cdp.page_ws()
if not ws:
    print('没找到页面')
    sys.exit(1)


def ev(e):
    return cdp.evaluate(ws, e)['result']['result'].get('value')


ev("""(() => { const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='鼠标手势'); b&&b.click(); return 'ok'; })()""")
time.sleep(1.0)

print('标签:', ev("JSON.stringify([...document.querySelectorAll('.ga-tabs button')].map(b=>b.textContent.trim()))"))
print('通用行数:', ev("document.querySelectorAll('.ga-row').length"))

ev("""(() => { const b=[...document.querySelectorAll('.ga-tabs button')].find(x=>x.textContent.includes('程序专用')); b&&b.click(); })()""")
time.sleep(0.8)
print('专用集:', ev("JSON.stringify([...document.querySelectorAll('.ga-profile-item')].map(x=>x.innerText.replace(/\\s+/g,' ').trim()))"))
print('专用行数:', ev("document.querySelectorAll('.ga-row').length"))

ev("""(() => { const b=[...document.querySelectorAll('.ga-tabs button')].find(x=>x.textContent.includes('手势设置')); b&&b.click(); })()""")
time.sleep(0.5)
print('设置项:', ev("JSON.stringify([...document.querySelectorAll('.setting-row .label strong')].map(x=>x.textContent.trim()))"))
