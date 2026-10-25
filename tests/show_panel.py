"""临时脚本：把面板叫出来并停一会儿，方便截图确认渲染效果。

    python tests/show_panel.py [停留秒数]
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time

u32 = ctypes.WinDLL("user32", use_last_error=True)

ENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [ENUMPROC, wt.LPARAM]
u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.SetForegroundWindow.argtypes = [wt.HWND]
u32.SetWindowPos.argtypes = [
    wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT
]


def find_notepad():
    found = []

    @ENUMPROC
    def cb(hwnd, _):
        cls = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(hwnd, cls, 256)
        if cls.value.lower() == "notepad" and u32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    u32.EnumWindows(cb, 0)
    return found[0] if found else None


def main():
    hold = float(sys.argv[1]) if len(sys.argv) > 1 else 12.0
    np = None
    win = find_notepad()
    if not win:
        np = subprocess.Popen(["notepad.exe"])
        time.sleep(2.0)
        win = find_notepad()
    if not win:
        print("没有可用的目标窗口")
        return 1

    u32.SetWindowPos(win, None, 120, 120, 760, 460, 0x0004)
    if not u32.SetForegroundWindow(win):
        u32.SetWindowPos(win, wt.HWND(-1), 0, 0, 0, 0, 0x0003)
        u32.SetWindowPos(win, wt.HWND(-2), 0, 0, 0, 0, 0x0003)
        u32.SetForegroundWindow(win)
    time.sleep(0.5)
    u32.SetCursorPos(400, 300)

    u32.keybd_event(0x5B, 0, 0, 0)
    time.sleep(0.03)
    u32.keybd_event(0x56, 0, 0, 0)
    time.sleep(0.03)
    u32.keybd_event(0x56, 0, 2, 0)
    time.sleep(0.03)
    u32.keybd_event(0x5B, 0, 2, 0)

    print(f"面板已呼出，保持 {hold}s")
    time.sleep(hold)
    return 0


if __name__ == "__main__":
    sys.exit(main())
