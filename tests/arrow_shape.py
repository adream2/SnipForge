"""专门看锥尾箭头长什么样：一次画四条（长/短/斜/细），截图肉眼验收。

跑法：先启动 snipforge.exe，再
    python tests/arrow_shape.py [输出名]
"""

import sys

from hover_test import (  # noqa: E402  复用已有的 Win32 封装
    PID,
    app_pid,
    drag,
    find_color,
    hotkey,
    shot,
    tap,
    u32,
    windows,
    VK_A,
    VK_MENU,
    VK_Q,
    VK_RETURN,
)

import ctypes
import time


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else "tests/out/arrow_shape.png"
    if not app_pid():
        print("SnipForge 没有在运行")
        return 1

    u32.SetCursorPos(600, 500)
    time.sleep(0.2)
    hotkey(VK_MENU, VK_Q)
    time.sleep(1.6)
    drag(560, 420, 1180, 900)
    time.sleep(0.4)
    tap(VK_RETURN)
    time.sleep(2.4)

    ed = windows("编辑器")
    if not ed:
        print("编辑器没打开")
        return 1
    ex, ey, ew, eh = ed[0]
    cx, cy = ex + ew // 2 - 110, ey + eh // 2

    # 切到箭头工具
    tap(VK_A)
    time.sleep(0.6)

    # 找紫色块（当前的「实心箭头」），点右边 = 锥尾
    tmp = "tests/out/_arrow_panel.png"
    shot(tmp, ex, ey, ew, eh)
    found = find_color(tmp, (108, 92, 231), 24, xmin=int(ew * 0.7), ymin=150)
    if found:
        x0, y0, x1, y1 = found
        u32.SetCursorPos(ex + x1 + 2 + (x1 - x0) // 2, ey + (y0 + y1) // 2)
        time.sleep(0.15)
        u32.mouse_event(0x0002, 0, 0, 0, 0)
        time.sleep(0.06)
        u32.mouse_event(0x0004, 0, 0, 0, 0)
        time.sleep(0.5)
    else:
        print("没找到样式按钮")

    # 四条：长横、短横、斜向、陡斜
    drag(cx - 240, cy - 120, cx + 220, cy - 120, steps=12)
    time.sleep(0.3)
    drag(cx - 240, cy - 30, cx - 40, cy - 30, steps=8)
    time.sleep(0.3)
    drag(cx - 240, cy + 60, cx + 200, cy + 190, steps=12)
    time.sleep(0.3)
    drag(cx + 60, cy + 190, cx + 70, cy + 40, steps=8)
    time.sleep(0.6)

    shot(name, ex, ey, ew, eh)
    print("saved", name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
