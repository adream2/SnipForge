"""验证「悬停图形 + 滚轮调参数」和「锥尾箭头」。

流程：Alt+Q → 拖框 → 回车 → 编辑器 → 画矩形 → 悬停到边框 → 滚轮加粗
      → 切箭头工具 → 点「锥尾箭头」→ 画一条 → 截图

跑法：先启动 snipforge.exe，再
    python tests/hover_test.py
"""

import ctypes
import ctypes.wintypes as wt
import struct
import subprocess
import sys
import time
import zlib

PY = "C:/Users/adrea/.workbuddy/binaries/python/versions/3.13.12/python.exe"
u32 = ctypes.WinDLL("user32", use_last_error=True)

ENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [ENUMPROC, wt.LPARAM]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]

KEYEVENTF_KEYUP = 0x0002
VK_MENU, VK_Q, VK_A, VK_RETURN = 0x12, 0x51, 0x41, 0x0D


def app_pid():
    out = subprocess.run(
        ["tasklist", "/FI", "IMAGENAME eq snipforge.exe", "/FO", "CSV", "/NH"],
        capture_output=True, text=True,
    ).stdout
    for line in out.splitlines():
        cells = [c.strip('"') for c in line.split('","')]
        if len(cells) > 1 and cells[1].isdigit():
            return int(cells[1])
    return None


PID = app_pid()


def windows(part=""):
    res = []

    @ENUMPROC
    def cb(h, _):
        p = wt.DWORD()
        u32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if str(p.value) == str(PID) and u32.IsWindowVisible(h):
            t = ctypes.create_unicode_buffer(128)
            u32.GetWindowTextW(h, t, 128)
            if part in t.value:
                r = wt.RECT()
                u32.GetWindowRect(h, ctypes.byref(r))
                res.append((r.left, r.top, r.right - r.left, r.bottom - r.top))
        return True

    u32.EnumWindows(cb, 0)
    return res


def key(vk, up=False):
    u32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def tap(vk):
    key(vk)
    time.sleep(0.04)
    key(vk, up=True)


def hotkey(mod, vk):
    key(mod)
    time.sleep(0.03)
    key(vk)
    time.sleep(0.03)
    key(vk, up=True)
    time.sleep(0.03)
    key(mod, up=True)


def drag(x1, y1, x2, y2, steps=8):
    u32.SetCursorPos(x1, y1)
    time.sleep(0.12)
    u32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.08)
    for i in range(1, steps + 1):
        u32.SetCursorPos(int(x1 + (x2 - x1) * i / steps), int(y1 + (y2 - y1) * i / steps))
        time.sleep(0.03)
    time.sleep(0.05)
    u32.mouse_event(0x0004, 0, 0, 0, 0)


def shot(path, x, y, w, h):
    subprocess.run([PY, "-u", "tests/shot.py", path, str(x), str(y), str(w), str(h)], check=True)


def read_png(path):
    d = open(path, "rb").read()
    i = 8
    w = h = 0
    idat = b""
    while i < len(d):
        ln = struct.unpack(">I", d[i:i + 4])[0]
        tag = d[i + 4:i + 8]
        data = d[i + 8:i + 8 + ln]
        i += 12 + ln
        if tag == b"IHDR":
            w, h = struct.unpack(">II", data[:8])
        elif tag == b"IDAT":
            idat += data
        elif tag == b"IEND":
            break
    raw = zlib.decompress(idat)
    stride = w * 3
    rows = []
    pos = 0
    for _ in range(h):
        pos += 1  # filter byte（shot.py 只用 filter 0）
        rows.append(raw[pos:pos + stride])
        pos += stride
    return w, h, rows


def find_color(path, rgb, tol=20, xmin=0, ymin=0):
    """在截图里找某个颜色的外接框，用来定位按钮。"""
    w, h, rows = read_png(path)
    xs, ys = [], []
    for y in range(ymin, h):
        r = rows[y]
        for x in range(xmin, w):
            if (abs(r[x * 3] - rgb[0]) < tol and abs(r[x * 3 + 1] - rgb[1]) < tol
                    and abs(r[x * 3 + 2] - rgb[2]) < tol):
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    return min(xs), min(ys), max(xs), max(ys)


def main():
    if not PID:
        print("SnipForge 没有在运行")
        return 1

    print("== 截图进编辑器 ==", flush=True)
    u32.SetCursorPos(600, 500)
    time.sleep(0.2)
    hotkey(VK_MENU, VK_Q)
    time.sleep(1.6)
    drag(600, 500, 1000, 820)
    time.sleep(0.4)
    tap(VK_RETURN)
    time.sleep(2.4)

    ed = windows("编辑器")
    if not ed:
        print("编辑器没打开")
        return 1
    ex, ey, ew, eh = ed[0]
    print(f"编辑器 {ex},{ey} {ew}x{eh}", flush=True)

    # 画布位置：编辑器中间那块
    cx, cy = ex + ew // 2 - 100, ey + eh // 2

    print("== 画一个矩形 ==", flush=True)
    drag(cx - 160, cy - 110, cx + 160, cy + 110)
    time.sleep(0.5)
    shot("tests/out/h1_rect.png", ex, ey, ew, eh)

    print("== 悬停到矩形上边框 ==", flush=True)
    u32.SetCursorPos(cx, cy - 110)
    time.sleep(0.7)
    shot("tests/out/h2_hover.png", ex, ey, ew, eh)

    print("== 滚轮加粗 ==", flush=True)
    for _ in range(5):
        u32.mouse_event(0x0800, 0, 0, 120, 0)
        time.sleep(0.09)
    time.sleep(0.7)
    shot("tests/out/h3_thicker.png", ex, ey, ew, eh)

    print("== 切箭头工具 ==", flush=True)
    u32.SetCursorPos(cx, cy)  # 移开，避免仍悬停在矩形上
    time.sleep(0.2)
    tap(VK_A)
    time.sleep(0.8)
    shot("tests/out/h4_arrowpanel.png", ex, ey, ew, eh)

    print("== 先画一条实心箭头 ==", flush=True)
    drag(cx - 150, cy - 40, cx + 150, cy - 100)
    time.sleep(0.4)

    # 「实心箭头」是当前选中项，紫色块。点它右半边 = 锥尾
    # 只在右侧面板、且顶栏以下找紫色（顶栏的「复制」按钮也是紫的，别抓错）
    found = find_color(
        "tests/out/h4_arrowpanel.png", (108, 92, 231), 24,
        xmin=int(ew * 0.7), ymin=150
    )
    if found:
        x0, y0, x1, y1 = found
        # 紫色块是「实心箭头」，右边紧挨着的就是「锥尾箭头」
        chip_cx = ex + x1 + 2 + (x1 - x0) // 2
        chip_cy = ey + (y0 + y1) // 2
        print(f"锥尾按钮 ≈ ({chip_cx},{chip_cy})", flush=True)
        u32.SetCursorPos(chip_cx, chip_cy)
        time.sleep(0.15)
        u32.mouse_event(0x0002, 0, 0, 0, 0)
        time.sleep(0.06)
        u32.mouse_event(0x0004, 0, 0, 0, 0)
        time.sleep(0.6)
    else:
        print("没找到样式按钮，跳过", flush=True)

    print("== 画锥尾箭头 ==", flush=True)
    drag(cx - 150, cy + 60, cx + 150, cy + 10)
    time.sleep(0.6)
    shot("tests/out/h5_arrow.png", ex, ey, ew, eh)

    print("done", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
