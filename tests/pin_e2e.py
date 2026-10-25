"""钉图 + 编辑器 端到端测试。

链路：Alt+Q 区域截图 → 拖框 → 编辑器打开 → F3 钉图 → 屏幕最上层出现贴图窗口。

跑法：先启动 snipforge.exe，再
    python tests/pin_e2e.py
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time

u32 = ctypes.WinDLL("user32", use_last_error=True)

ENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [ENUMPROC, wt.LPARAM]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]

VK_MENU, VK_Q, VK_F3, VK_ESCAPE = 0x12, 0x51, 0x72, 0x1B
KEYEVENTF_KEYUP = 0x0002

results = []


def check(name, ok, detail=""):
    results.append((name, ok, detail))
    print(("  [ok]   " if ok else "  [FAIL] ") + name + (f"  {detail}" if detail else ""), flush=True)


def windows():
    out = []

    @ENUMPROC
    def cb(hwnd, _):
        pid = wt.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        r = wt.RECT()
        u32.GetWindowRect(hwnd, ctypes.byref(r))
        cls = ctypes.create_unicode_buffer(128)
        u32.GetClassNameW(hwnd, cls, 128)
        out.append({
            "hwnd": hwnd, "pid": pid.value, "cls": cls.value,
            "visible": bool(u32.IsWindowVisible(hwnd)),
            "x": r.left, "y": r.top, "w": r.right - r.left, "h": r.bottom - r.top,
        })
        return True

    u32.EnumWindows(cb, 0)
    return out


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


def key(vk, up=False):
    u32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def hotkey(mod, vk):
    key(mod)
    time.sleep(0.03)
    key(vk)
    time.sleep(0.03)
    key(vk, up=True)
    time.sleep(0.03)
    key(mod, up=True)


def mouse(ev, x=None, y=None):
    if x is not None:
        u32.SetCursorPos(x, y)
    u32.mouse_event(ev, 0, 0, 0, 0)


def main():
    pid = app_pid()
    if not pid:
        print("SnipForge 没有在运行", flush=True)
        return 1

    screen_w = u32.GetSystemMetrics(0)
    screen_h = u32.GetSystemMetrics(1)

    # 记下按下之前光标位置，结束时还原
    pt = wt.POINT()
    u32.GetCursorPos(ctypes.byref(pt))

    print("== 1. Alt+Q 区域截图 ==", flush=True)
    u32.SetCursorPos(500, 420)
    time.sleep(0.2)
    hotkey(VK_MENU, VK_Q)
    time.sleep(1.4)

    sel = next(
        (w for w in windows()
         if w["pid"] == pid and w["visible"] and w["w"] >= screen_w - 2 and w["h"] >= screen_h - 2),
        None,
    )
    check("选区覆盖层出现", sel is not None, f"{sel['w']}x{sel['h']}" if sel else "")

    if sel:
        print("== 2. 拖出选区 ==", flush=True)
        mouse(0x0002, 500, 420)  # LEFTDOWN
        for i in range(1, 9):
            mouse(0x0001, 500 + i * 60, 420 + i * 45)  # MOUSEMOVE
            time.sleep(0.03)
        mouse(0x0004, 980, 780)  # LEFTUP
        time.sleep(0.4)

        # 截图工具的惯例：框完不自动提交，要按回车或点确认。
        # 这一步也顺带验证了确认键在覆盖层里是通的。
        print("== 2b. 回车确认选区 ==", flush=True)
        key(0x0D)
        time.sleep(0.05)
        key(0x0D, up=True)
        time.sleep(2.2)

        editor = next(
            (w for w in windows()
             if w["pid"] == pid and w["visible"] and 600 < w["w"] < 1600 and 400 < w["h"] < 1000),
            None,
        )
        check("编辑器打开", editor is not None, f"{editor['w']}x{editor['h']}" if editor else "")

        if editor:
            print("== 3. F3 钉图 ==", flush=True)
            key(VK_F3)
            time.sleep(0.05)
            key(VK_F3, up=True)
            time.sleep(2.0)

            after = windows()
            # 贴图窗口：我们自己进程、可见、尺寸不超过屏宽 75%
            pins = [
                w for w in after
                if w["pid"] == pid and w["visible"]
                and w["w"] <= screen_w * 0.75 and w["h"] <= screen_h * 0.75
                and w["w"] > 40 and w["h"] > 30
                and not (w["w"] >= screen_w - 2 and w["h"] >= screen_h - 2)
            ]
            check("出现贴图窗口", len(pins) > 0, f"{len(pins)} 个：{[ (p['w'], p['h']) for p in pins ]}")

            if pins:
                p = pins[-1]
                check("贴图靠近光标", abs(p["x"] + p["w"] // 2 - 980) < screen_w * 0.4,
                      f"中心 x={p['x'] + p['w'] // 2} (光标 980)")
                check("贴图不超出屏幕",
                      p["x"] >= 0 and p["y"] >= 0
                      and p["x"] + p["w"] <= screen_w and p["y"] + p["h"] <= screen_h,
                      f"({p['x']},{p['y']}) {p['w']}x{p['h']}")

                still_editor = any(w["hwnd"] == editor["hwnd"] and w["visible"] for w in after)
                check("钉完编辑器收起", not still_editor)

                # 收尾：Esc 关掉贴图
                u32.SetForegroundWindow(p["hwnd"])
                time.sleep(0.3)
                key(VK_ESCAPE)
                time.sleep(0.05)
                key(VK_ESCAPE, up=True)
                time.sleep(0.8)
                check("Esc 关闭贴图",
                      not any(w["hwnd"] == p["hwnd"] and w["visible"] for w in windows()))

    u32.SetCursorPos(pt.x, pt.y)

    failed = [r for r in results if not r[1]]
    print(f"== {len(results) - len(failed)}/{len(results)} 通过 ==", flush=True)
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
