"""剪贴板面板的管理操作探针：置顶（Ctrl+P）/ 删除（Delete）/ 清空按钮。

验的是"面板里能不能管理历史"，不涉及粘贴链路（那是 panel_e2e 的事）。

跑法：先启动 snipforge.exe，再
    python tests/panel_actions_probe.py
"""

import ctypes
import ctypes.wintypes as wt
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shot  # noqa: E402

u32 = ctypes.WinDLL("user32", use_last_error=True)
ENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [ENUMPROC, wt.LPARAM]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.SetForegroundWindow.argtypes = [wt.HWND]
u32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
u32.mouse_event.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_void_p]

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
HISTORY = os.path.expandvars(r"%APPDATA%\fun.lxzt.snipforge\clipboard\history.json")


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


def windows(pid=None, cls=None):
    out = []

    @ENUMPROC
    def cb(h, _):
        p = wt.DWORD()
        u32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if not u32.IsWindowVisible(h):
            return True
        if pid is not None and p.value != pid:
            return True
        cn = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
        u32.GetClassNameW(h, cn, 256)
        if cls is not None and cn.value != cls:
            return True
        t = ctypes.create_unicode_buffer(256)
        u32.GetWindowTextW(h, t, 256)
        r = wt.RECT()
        u32.GetWindowRect(h, ctypes.byref(r))
        out.append({"hwnd": h, "title": t.value, "cls": cn.value, "x": r.left, "y": r.top,
                    "w": r.right - r.left, "h": r.bottom - r.top})
        return True

    u32.EnumWindows(cb, 0)
    return out


def wait_panel(pid, timeout=6.0):
    """面板是 408 宽的 SnipForge 窗口，和主窗口同名，靠尺寸区分"""
    end = time.time() + timeout
    while time.time() < end:
        for w in windows(pid):
            if w["title"] == "SnipForge" and 380 <= w["w"] <= 460:
                return w
        time.sleep(0.1)
    return None


def key(vk, up=False):
    u32.keybd_event(vk, 0, 0x0002 if up else 0, 0)


def tap(vk):
    key(vk)
    time.sleep(0.03)
    key(vk, True)


def click(x, y):
    u32.SetCursorPos(x, y)
    time.sleep(0.15)
    u32.mouse_event(0x0002, 0, 0, 0, None)
    time.sleep(0.05)
    u32.mouse_event(0x0004, 0, 0, 0, None)


def snap(w, name):
    raw, ww, hh = shot.grab(w["x"], w["y"], w["w"], w["h"])
    path = os.path.join(OUT, name)
    shot.write_png(path, raw, ww, hh)
    print("  截图:", path, flush=True)


def history():
    try:
        d = json.load(open(HISTORY, encoding="utf-8"))
        return d.get("items", [])
    except Exception:
        return []


def main():
    os.makedirs(OUT, exist_ok=True)
    pid = app_pid()
    if not pid:
        print("SnipForge 没在运行")
        return 1

    # 造三条可区分的记录
    for i, tag in enumerate(["ALPHA", "BETA", "GAMMA"]):
        subprocess.run(f'echo probe-{tag}-{i}| clip', shell=True, capture_output=True)
        time.sleep(1.0)

    # 记事本只是当"目标窗口"用，让面板底部显示 → 应用；找不到也不影响结论
    np = subprocess.Popen(["notepad.exe"])
    time.sleep(1.8)
    pad = next((w for w in windows(cls="Notepad")), None)
    if pad:
        u32.SetForegroundWindow(pad["hwnd"])
    time.sleep(0.4)

    print("== 1. Alt+V 呼出面板 ==", flush=True)
    key(0x12); time.sleep(0.03)  # Alt
    key(0x56); time.sleep(0.05)  # V
    key(0x56, True); time.sleep(0.03)
    key(0x12, True)
    panel = wait_panel(pid)
    if not panel:
        print("  面板没出来")
        np.kill()
        return 1
    print(f"  面板 {panel['w']}x{panel['h']} @({panel['x']},{panel['y']})")
    time.sleep(0.6)
    snap(panel, "panel_actions_1.png")

    before = history()
    print(f"  历史 {len(before)} 条", flush=True)

    print("== 2. Ctrl+P 置顶首项 ==", flush=True)
    key(0x11); time.sleep(0.03)
    tap(0x50)  # P
    key(0x11, True)
    time.sleep(0.9)
    snap(panel, "panel_actions_2.png")
    after_pin = history()
    pinned = [i for i in after_pin if i.get("pinned")]
    print(f"  置顶项 {len(pinned)} 条: {[i['text'][:14] for i in pinned]}", flush=True)

    print("== 3. Delete 删掉选中项 ==", flush=True)
    tap(0x2E)  # Delete
    time.sleep(0.9)
    snap(panel, "panel_actions_3.png")
    after_del = history()
    print(f"  历史 {len(after_del)} 条（删除前 {len(after_pin)}）", flush=True)

    print("== 4. Esc 收起 ==", flush=True)
    tap(0x1B)
    time.sleep(0.4)
    np.kill()

    ok = True
    if not any(i.get("pinned") for i in after_pin):
        print("  [FAIL] 置顶没生效")
        ok = False
    if len(after_del) != len(after_pin) - 1:
        print(f"  [FAIL] 删除没生效：{len(after_pin)} → {len(after_del)}")
        ok = False
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
