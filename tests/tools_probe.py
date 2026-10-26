"""图片工具探针：加载图片 → 右键菜单。

验两件事：
  1. 走系统文件对话框选中一张图，页面能正常加载并给出转换结果
     （之前用 convertFileSrc + fetch，被 CSP 拦死，选了图毫无反应）
  2. 右键能唤出快捷菜单（复制 / 另存为 / 钉图 / 取字 / 上传 / 打开目录）

跑法：先启动 snipforge.exe，再
    python tests/tools_probe.py [图片路径]
"""

import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import drag_probe as dp  # noqa: E402
import shot  # noqa: E402

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_IMG = os.path.join(HERE, "out", "record_frame0.png")


def snap(x, y, w, h, name):
    raw, W, H = shot.grab(x, y, w, h)
    path = os.path.join(OUT, name)
    shot.write_png(path, raw, W, H)
    print("  截图:", path, flush=True)


def click(x, y):
    dp.u32.SetCursorPos(x, y)
    time.sleep(0.25)
    dp.u32.mouse_event(0x0002, 0, 0, 0, None)
    time.sleep(0.06)
    dp.u32.mouse_event(0x0004, 0, 0, 0, None)


def right_click(x, y):
    dp.u32.SetCursorPos(x, y)
    time.sleep(0.25)
    dp.u32.mouse_event(0x0008, 0, 0, 0, None)
    time.sleep(0.08)
    dp.u32.mouse_event(0x0010, 0, 0, 0, None)


def main():
    img = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_IMG
    if not os.path.exists(img):
        print("找不到测试图：", img)
        return 2

    w = [x for x in dp.wins("SnipForge") if x[1] == "SnipForge"]
    if not w:
        print("SnipForge 主窗口没开")
        return 2
    hwnd, _t, x, y, ww, hh = w[0]

    # 先把路径放进剪贴板，等下在文件对话框里 Ctrl+V
    subprocess.run(f'echo {img}| clip', shell=True, capture_output=True)
    time.sleep(0.4)

    print("== 1. 切到图片工具 ==", flush=True)
    click(x + ww // 2, y + 18)          # 点标题栏拿焦点
    time.sleep(0.5)
    click(x + 40, y + 44 + 38 * 2 + 18)  # 第 3 个导航项
    time.sleep(1.0)

    print("== 2. 点拖放区 → 文件对话框选图 ==", flush=True)
    click(x + 615, y + 205)
    time.sleep(2.0)
    dp.key(0x11); time.sleep(0.05)
    dp.tap(0x56)                         # Ctrl+V 粘路径
    dp.key(0x11, True); time.sleep(0.5)
    dp.tap(0x0D)                         # 回车确认
    time.sleep(2.5)
    snap(x, y, ww, hh, "tools_loaded.png")

    print("== 3. 右键唤出快捷菜单 ==", flush=True)
    right_click(x + 470, y + 200)
    time.sleep(0.8)
    snap(x, y, ww, hh, "tools_menu.png")
    dp.tap(0x1B)  # Esc 收起
    time.sleep(0.3)
    print("完成", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
