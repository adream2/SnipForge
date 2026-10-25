"""录屏端到端测试。

链路：Alt+R 进选区 → 拖框 → 回车 → 倒计时 → 录制 → Esc 停止 → 校验产物。

验的是四件事：
  1. 选区窗口以"录屏模式"打开（标题不同，框完不是进编辑器而是开录）
  2. 录制控制条出现（说明后端真的起来了）
  3. 停止后产出 mp4，能读出时长（容器没写坏）
  4. 覆盖层被录进了画面（鼠标环），这是整套"所见即所得"的前提

跑法：先启动 snipforge.exe，再
    python tests/record_e2e.py [录制秒数]
"""

import ctypes
import ctypes.wintypes as wt
import glob
import json
import os
import subprocess
import sys
import time

u32 = ctypes.WinDLL("user32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)

P = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumWindows.argtypes = [P, wt.LPARAM]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.GetForegroundWindow.restype = wt.HWND
u32.SetForegroundWindow.argtypes = [wt.HWND]
u32.AttachThreadInput.argtypes = [wt.DWORD, wt.DWORD, wt.BOOL]
u32.SetWindowPos.argtypes = [
    wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT
]
k32.GetCurrentThreadId.restype = wt.DWORD
k32.GetCurrentThreadId.argtypes = []

VK_CONTROL, VK_RETURN, VK_ESCAPE = 0x11, 0x0D, 0x1B
KEYEVENTF_KEYUP = 0x0002

results = []


def check(name, ok, detail=""):
    results.append((name, ok))
    print(("  [ok]   " if ok else "  [FAIL] ") + name + (f"  {detail}" if detail else ""), flush=True)


def key(vk, up=False):
    u32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def tap(vk):
    key(vk)
    time.sleep(0.03)
    key(vk, True)
    time.sleep(0.05)


def record_hotkey():
    """Ctrl+Alt+R —— 默认录屏快捷键。

    不用 Alt+R 是因为它在不少机器上被别的软件占着（本机就是），
    RegisterHotKey 直接失败，测出来像是功能坏了。
    """
    key(0x11)  # Ctrl
    key(0x12)  # Alt
    time.sleep(0.03)
    key(0x52)  # R
    time.sleep(0.05)
    key(0x52, True)
    time.sleep(0.03)
    key(0x12, True)
    key(0x11, True)
    time.sleep(0.2)


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


def windows_of(pid):
    out = []

    @P
    def cb(hwnd, _):
        p = wt.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value != pid or not u32.IsWindowVisible(hwnd):
            return True
        title = ctypes.create_unicode_buffer(256)
        u32.GetWindowTextW(hwnd, title, 256)
        r = wt.RECT()
        u32.GetWindowRect(hwnd, ctypes.byref(r))
        out.append({
            "hwnd": hwnd, "title": title.value,
            "w": r.right - r.left, "h": r.bottom - r.top,
            "x": r.left, "y": r.top,
        })
        return True

    u32.EnumWindows(cb, 0)
    return out


def wait_window(pid, title_part, timeout=6.0, width_min=0):
    end = time.time() + timeout
    while time.time() < end:
        for w in windows_of(pid):
            if title_part in w["title"] and w["w"] >= width_min:
                return w
        time.sleep(0.1)
    return None


def force_foreground(hwnd):
    u32.SetWindowPos(hwnd, wt.HWND(-1), 0, 0, 0, 0, 0x0001 | 0x0002)
    u32.SetWindowPos(hwnd, wt.HWND(-2), 0, 0, 0, 0, 0x0001 | 0x0002)
    if u32.SetForegroundWindow(hwnd):
        return True
    fg = u32.GetForegroundWindow()
    fg_tid = u32.GetWindowThreadProcessId(fg, None)
    my_tid = k32.GetCurrentThreadId()
    u32.AttachThreadInput(fg_tid, my_tid, True)
    ok = u32.SetForegroundWindow(hwnd)
    u32.AttachThreadInput(fg_tid, my_tid, False)
    return bool(ok)


def drag(x1, y1, x2, y2):
    u32.SetCursorPos(x1, y1)
    time.sleep(0.1)
    u32.mouse_event(0x0002, 0, 0, 0, 0)   # LEFT DOWN
    time.sleep(0.08)
    steps = 10
    for i in range(1, steps + 1):
        u32.SetCursorPos(
            x1 + (x2 - x1) * i // steps,
            y1 + (y2 - y1) * i // steps,
        )
        time.sleep(0.02)
    time.sleep(0.06)
    u32.mouse_event(0x0004, 0, 0, 0, 0)   # LEFT UP
    time.sleep(0.2)


def save_dir():
    path = os.path.expandvars(r"%APPDATA%\fun.lxzt.snipforge\settings.json")
    try:
        cfg = json.load(open(path, encoding="utf-8"))
        d = (cfg.get("save") or {}).get("dir") or ""
        if d.strip():
            return d
    except Exception:
        pass
    return os.path.expandvars(r"%USERPROFILE%\Pictures")


def newest_mp4(directory, after):
    files = [f for f in glob.glob(os.path.join(directory, "**", "*.mp4"), recursive=True)
             if os.path.getmtime(f) >= after]
    return max(files, key=os.path.getmtime) if files else None


def main():
    seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 4

    pid = app_pid()
    if not pid:
        print("snipforge.exe 没在运行，先启动它", flush=True)
        return 2
    print(f"snipforge pid={pid}", flush=True)

    started_at = time.time()

    # 记事本当录制内容，别录一片空桌面
    subprocess.Popen(["notepad.exe"])
    time.sleep(1.2)

    print("== 1. Ctrl+Alt+R 进入录制选区 ==", flush=True)
    record_hotkey()
    sel = wait_window(pid, "Record Region", timeout=6)
    check("选区窗口以录屏模式打开", sel is not None,
          f"{sel['title']} {sel['w']}x{sel['h']}" if sel else "没出现")
    if not sel:
        return 1

    print("== 2. 框选区域并按回车 ==", flush=True)
    x1, y1 = sel["x"] + 120, sel["y"] + 100
    x2, y2 = x1 + 700, y1 + 420
    drag(x1, y1, x2, y2)
    tap(VK_RETURN)

    bar = wait_window(pid, "SnipForge Record", timeout=8)
    check("录制控制条出现", bar is not None,
          f"{bar['w']}x{bar['h']}" if bar else "没出现")
    if not bar:
        return 1

    overlay = wait_window(pid, "SnipForge Overlay", timeout=4)
    check("覆盖层与录制区域等大",
          overlay is not None and abs(overlay["w"] - 700) <= 4 and abs(overlay["h"] - 420) <= 4,
          f"{overlay['w']}x{overlay['h']}" if overlay else "没出现")

    print(f"== 3. 等倒计时 + 录 {seconds}s ==", flush=True)
    # 光标停到区域中间，让鼠标环出现在画面里
    u32.SetCursorPos((x1 + x2) // 2, (y1 + y2) // 2)
    time.sleep(4 + seconds)

    print("== 4. 再按一次 Ctrl+Alt+R 停止 ==", flush=True)
    # 控制条故意不抢焦点（不然用户没法边录边操作别的程序），
    # 所以停止走全局快捷键；控制条自己聚焦时按 Esc 也能停。
    record_hotkey()
    time.sleep(2.5)

    mp4 = newest_mp4(save_dir(), started_at)
    check("生成了 mp4", mp4 is not None, mp4 or f"在 {save_dir()} 下没找到")
    if not mp4:
        return 1

    size = os.path.getsize(mp4)
    # 静止画面压缩率极高，几十 KB 也是正常的；这里只挡「空文件」
    check("文件体积合理", size > 15 * 1024, f"{size // 1024} KB")

    exe = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "src-tauri", "target", "release", "snipforge.exe")
    out_png = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out", "record_frame0.png")
    os.makedirs(os.path.dirname(out_png), exist_ok=True)
    r = subprocess.run([exe, "--record-inspect", mp4, out_png],
                       capture_output=True, text=True)
    check("能解出帧（容器完好）", "ok" in r.stdout, r.stdout.strip() or r.stderr.strip())

    print(flush=True)
    ok = sum(1 for _, v in results if v)
    print(f"== {ok}/{len(results)} 通过 ==", flush=True)
    print(f"产物：{mp4}", flush=True)
    return 0 if ok == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
