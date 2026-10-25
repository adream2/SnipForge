"""录屏选区"窗口模式"探针。

链路：Ctrl+Alt+R 进录屏选区 → 截图（应看到常驻工具栏）→ 按 W 进窗口模式 →
鼠标移到记事本上（应出现高亮框）→ 点击选中 → 回车开录 → 截图确认控制条出现。

产物存在 tests/out/，肉眼核对高亮框是否贴住窗口边缘。
    python tests/record_window_probe.py
"""

import ctypes
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import drag_probe as dp  # noqa: E402  复用它的按键/前台/拖拽工具
import shot  # noqa: E402

u32 = dp.u32
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")


def snap(name):
    raw, w, h = shot.grab(0, 0, 2560, 1440)
    path = os.path.join(OUT, name)
    shot.write_png(path, raw, w, h)
    print("已存", path)


def main():
    subprocess.Popen(["notepad.exe"])
    time.sleep(1.5)
    pad = dp.wins("Notepad")
    if not pad:
        print("记事本没开起来")
        return 1
    px, py, pw, ph = pad[0][2], pad[0][3], pad[0][4], pad[0][5]
    print(f"记事本窗口：{pw}x{ph} @({px},{py})")

    # 进录屏选区
    dp.key(0x11); dp.key(0x12); time.sleep(0.03)
    dp.key(0x52); time.sleep(0.05)
    dp.key(0x52, True); time.sleep(0.03)
    dp.key(0x12, True); dp.key(0x11, True)
    time.sleep(1.5)
    sel = dp.wins("Record Region")
    if not sel:
        print("录屏选区没出现")
        return 1
    dp.force_foreground(sel[0][0])
    time.sleep(0.6)
    snap("rec_sel_initial.png")

    # 不按 W —— 录屏模式默认就是智能窗口选择：悬停高亮、单击选中
    u32.SetCursorPos(px + pw // 2, py + ph // 2)
    time.sleep(0.5)
    snap("rec_win_hover.png")

    # 点击选中，再回车开录
    u32.mouse_event(0x0002, 0, 0, 0, None); time.sleep(0.06)
    u32.mouse_event(0x0004, 0, 0, 0, None)
    time.sleep(0.5)
    snap("rec_win_selected.png")
    dp.tap(0x0D)  # Enter

    bar = None
    for _ in range(20):
        bar = [w for w in dp.wins("SnipForge Record") if w[1] == "SnipForge Record"]
        if bar:
            break
        time.sleep(0.3)
    if bar:
        print(f"[ok] 控制条出现 {bar[0][4]}x{bar[0][5]}，录制已开始")
    else:
        print("[FAIL] 窗口模式没把录制带起来")
        dp.tap(0x1B)  # Esc 兜底
        return 1

    # 录 2 秒停掉
    time.sleep(2)
    dp.key(0x11); dp.key(0x12); time.sleep(0.03)
    dp.key(0x52); time.sleep(0.05)
    dp.key(0x52, True); time.sleep(0.03)
    dp.key(0x12, True); dp.key(0x11, True)
    time.sleep(2.5)
    print("完成")
    return 0


if __name__ == "__main__":
    sys.exit(main())
