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


ev("""(() => { const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='鼠标手势'); b&&b.click(); })()""")
time.sleep(1.2)
print('行数:', ev("document.querySelectorAll('.ga-row').length"))
print('同形标记:', ev("document.querySelectorAll('.ga-dup').length"))
print('前 5 行:', ev("JSON.stringify([...document.querySelectorAll('.ga-row')].slice(0,5).map(r=>r.innerText.replace(/\\s+/g,' ').trim()))"))
