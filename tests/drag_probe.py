"""手动探针：触发录屏热键 → 拖框 → 截图（含透明窗口）。

用来定位"回车没反应"这类交互问题：截图里能直接看到选区矩形和工具栏
有没有画出来。跑之前 snipforge.exe 必须在运行。
    python tests/drag_probe.py
"""

import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shot  # noqa: E402

u32 = ctypes.WinDLL("user32")
k32 = ctypes.WinDLL("kernel32")
P = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [P, wt.LPARAM]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.SetForegroundWindow.argtypes = [wt.HWND]
u32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
u32.mouse_event.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_void_p]
k32.GetCurrentThreadId.restype = wt.DWORD

VK_ESCAPE = 0x1B
KEYEVENTF_KEYUP = 0x0002


def key(vk, up=False):
    u32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def tap(vk):
    key(vk)
    time.sleep(0.03)
    key(vk, True)
    time.sleep(0.05)


def wins(title_part):
    out = []

    @P
    def cb(h, _):
        if not u32.IsWindowVisible(h):
            return True
        t = ctypes.create_unicode_buffer(256)
        u32.GetWindowTextW(h, t, 256)
        r = wt.RECT()
        u32.GetWindowRect(h, ctypes.byref(r))
        out.append((h, t.value, r.left, r.top, r.right - r.left, r.bottom - r.top))
        return True

    u32.EnumWindows(cb, 0)
    return [w for w in out if title_part in w[1]]


def force_foreground(hwnd):
    u32.SetWindowPos = u32.SetWindowPos  # noqa: PLW0127
    fg = u32.GetForegroundWindow()
    fg_tid = u32.GetWindowThreadProcessId(fg, None)
    my_tid = k32.GetCurrentThreadId()
    u32.AttachThreadInput(fg_tid, my_tid, True)
    ok = u32.SetForegroundWindow(hwnd)
    u32.AttachThreadInput(fg_tid, my_tid, False)
    return bool(ok)


def drag(x1, y1, x2, y2):
    u32.SetCursorPos(x1, y1)
    time.sleep(0.15)
    u32.mouse_event(0x0002, 0, 0, 0, None)
    time.sleep(0.1)
    for i in range(1, 11):
        u32.SetCursorPos(x1 + (x2 - x1) * i // 10, y1 + (y2 - y1) * i // 10)
        time.sleep(0.02)
    time.sleep(0.08)
    u32.mouse_event(0x0004, 0, 0, 0, None)
    time.sleep(0.4)


def main():
    # Ctrl+Alt+R 进录屏选区
    key(0x11); key(0x12); time.sleep(0.03)
    key(0x52); time.sleep(0.05)
    key(0x52, True); time.sleep(0.03)
    key(0x12, True); key(0x11, True)
    time.sleep(1.5)

    sel = wins("Record Region")
    print("selector:", sel)
    if not sel:
        print("选区窗口没出现")
        return 1
    hwnd = sel[0][0]
    force_foreground(hwnd)
    time.sleep(0.6)

    x1, y1 = sel[0][2] + 120, sel[0][3] + 100
    drag(x1, y1, x1 + 700, y1 + 420)

    raw, w, h = shot.grab(0, 0, 2560, 1440)
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out", "drag_probe.png")
    shot.write_png(out, raw, w, h)
    print("已存", out)

    tap(VK_ESCAPE)  # 收尾，别把选区留在屏幕上
    return 0


if __name__ == "__main__":
    sys.exit(main())
