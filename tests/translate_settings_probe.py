"""设置页「翻译服务」冒烟：卡片在不在、点"测试"能不能拿到译文。"""
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


# 打开设置 → 输出与识别 tab
ev("""(() => { const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='设置'); b&&b.click(); })()""")
time.sleep(0.8)
ev("""(() => { const b=[...document.querySelectorAll('button')].find(x=>x.textContent.includes('输出与识别')); b&&b.click(); })()""")
time.sleep(0.6)

print(
    '翻译卡片:',
    ev("""(() => {
      const cards=[...document.querySelectorAll('.setting-card')];
      const c=cards.find(x=>x.querySelector('header')?.textContent.trim()==='翻译服务');
      if(!c) return 'missing';
      return JSON.stringify({
        rows:[...c.querySelectorAll('.setting-row .label strong')].map(x=>x.textContent.trim()),
        provider:c.querySelector('select')?.value
      });
    })()"""),
)

# 点"测试"
print(
    '点测试:',
    ev("""(() => {
      const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='测试');
      if(!b) return 'no-btn'; b.click(); return 'ok';
    })()"""),
)
time.sleep(3)
print(
    '结果:',
    ev("""(() => {
      const cards=[...document.querySelectorAll('.setting-card')];
      const c=cards.find(x=>x.querySelector('header')?.textContent.trim()==='翻译服务');
      const rows=[...c.querySelectorAll('.setting-row')];
      const last=rows[rows.length-1];
      return last ? last.innerText.replace(/\s+/g,' ').trim() : 'none';
    })()"""),
)
