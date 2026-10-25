"""放大看燕尾：画一条锥尾箭头 → 悬停滚粗 → 裁尾部特写。

跑法：先启动 snipforge.exe，再
    python tests/arrow_tail.py
"""

import sys
import time

from hover_test import (  # noqa: E402
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


def main():
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

    tap(VK_A)
    time.sleep(0.6)

    tmp = "tests/out/_at_panel.png"
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

    # 一条从左到右的长箭头，尾巴在左边
    tail_x, tail_y = cx - 220, cy
    drag(tail_x, tail_y, cx + 200, cy, steps=10)
    time.sleep(0.5)

    # 悬停到箭身上把它滚粗，燕尾细节才看得清
    u32.SetCursorPos(cx - 60, cy)
    time.sleep(0.5)
    for _ in range(6):
        u32.mouse_event(0x0800, 0, 0, 120, 0)
        time.sleep(0.08)
    time.sleep(0.8)

    shot("tests/out/arrow_big.png", ex, ey, ew, eh)
    # 尾部特写（尾巴在 left+30 附近）
    shot("tests/out/arrow_tail_crop.png", tail_x - 20, tail_y - 60, 220, 120)
    print("saved")
    return 0


if __name__ == "__main__":
    sys.exit(main())
