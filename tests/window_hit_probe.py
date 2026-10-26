"""诊断：选区窗口命中测试到底会选中哪个窗口。

对比三种策略的结果：
  A. 当前实现（按面积从大到小，取第一个命中）—— src/win32/window.rs
  B. z 序（EnumWindows 顺序，取第一个命中）
  C. WindowFromPoint（Windows 自己的命中，最接近"用户看到的"）

跑法：把光标移到想测的位置，再执行本脚本。
    python tests/window_hit_probe.py
"""

import ctypes
import ctypes.wintypes as wt
import sys

u32 = ctypes.WinDLL("user32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)

P = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [P, wt.LPARAM]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.IsIconic.argtypes = [wt.HWND]
u32.GetWindowLongW.argtypes = [wt.HWND, ctypes.c_int]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.WindowFromPoint.argtypes = [wt.POINT]
u32.WindowFromPoint.restype = wt.HWND
u32.GetAncestor.argtypes = [wt.HWND, ctypes.c_uint]
u32.GetAncestor.restype = wt.HWND
u32.GetCursorPos.argtypes = [ctypes.POINTER(wt.POINT)]

GWL_EXSTYLE = -20
WS_EX_TOOLWINDOW = 0x00000080
WS_EX_NOACTIVATE = 0x08000000
GA_ROOT = 2


def title(h):
    b = ctypes.create_unicode_buffer(256)
    u32.GetWindowTextW(h, b, 256)
    return b.value


def rect(h):
    r = wt.RECT()
    u32.GetWindowRect(h, ctypes.byref(r))
    return r.left, r.top, r.right - r.left, r.bottom - r.top


def pid(h):
    p = wt.DWORD()
    u32.GetWindowThreadProcessId(h, ctypes.byref(p))
    return p.value


def collect(self_pid):
    out = []

    @P
    def cb(h, _):
        if not u32.IsWindowVisible(h) or u32.IsIconic(h):
            return True
        ex = u32.GetWindowLongW(h, GWL_EXSTYLE) & 0xFFFFFFFF
        if ex & WS_EX_TOOLWINDOW or ex & WS_EX_NOACTIVATE:
            return True
        if pid(h) == self_pid:
            return True
        x, y, w, hh = rect(h)
        if w <= 0 or hh <= 0:
            return True
        out.append((h, title(h), x, y, w, hh))
        return True

    u32.EnumWindows(cb, 0)
    return out


def contains(w, px, py):
    _, _, x, y, ww, hh = w
    return x <= px < x + ww and y <= py < y + hh


def main():
    pt = wt.POINT()
    u32.GetCursorPos(ctypes.byref(pt))
    px, py = pt.x, pt.y
    print(f"光标位置：({px}, {py})")

    # 用 snipforge 的 pid 做排除（找主窗口）
    self_pid = 0
    for h, t, *_ in collect(0):
        if t == "SnipForge":
            self_pid = pid(h)
    print(f"snipforge pid = {self_pid}")

    wins = collect(self_pid)
    hits = [w for w in wins if contains(w, px, py)]
    print(f"\n命中候选共 {len(hits)} 个（z 序，靠前=更上层）")

    a = sorted(hits, key=lambda w: -(w[4] * w[5]))
    print(f"\nA 当前实现（面积最大优先）→ {a[0][1] or '(无标题)'}  {a[0][4]}x{a[0][5]} @({a[0][2]},{a[0][3]})")
    print(f"B z 序（最上层优先）     → {hits[0][1] or '(无标题)'}  {hits[0][4]}x{hits[0][5]} @({hits[0][2]},{hits[0][3]})")

    h = u32.WindowFromPoint(pt)
    root = u32.GetAncestor(h, GA_ROOT) if h else None
    if root:
        x, y, w, hh = rect(root)
        print(f"C WindowFromPoint        → {title(root) or '(无标题)'}  {w}x{hh} @({x},{y})")
    else:
        print("C WindowFromPoint        → 无")

    print("\n全部候选：")
    for w in hits:
        print(f"  {w[4]:5d}x{w[5]:<5d} @({w[2]:5d},{w[3]:<5d})  {w[1][:44]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
