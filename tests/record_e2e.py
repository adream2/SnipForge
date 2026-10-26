"""录屏端到端测试。

链路：Alt+R 进选区 → 拖框 → 回车 → 倒计时 → 录制 → Esc 停止 → 校验产物。

验的是四件事：
  1. 选区窗口以"录屏模式"打开（标题不同，框完不是进编辑器而是开录）
  2. 录制控制条出现（说明后端真的起来了）
  3. 停止后产出文件，能读出时长 / 解出帧（容器没写坏）
  4. 覆盖层与录制区域等大，这是整套"所见即所得"的前提

格式不再是全局设置，而是跟着"动作"走：Ctrl+Alt+R = 录视频，Ctrl+Alt+G = 录 GIF。
所以这里也用第二个参数指定要测哪种：

跑法：先启动 snipforge.exe，再
    python tests/record_e2e.py [录制秒数] [mp4|gif]
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


def record_hotkey(fmt="mp4"):
    """Ctrl+Alt+R = 录视频，Ctrl+Alt+G = 录 GIF。

    不用 Alt+R 是因为它在不少机器上被别的软件占着（本机就是），
    RegisterHotKey 直接失败，测出来像是功能坏了。
    """
    letter = 0x47 if fmt == "gif" else 0x52  # G / R
    key(0x11)  # Ctrl
    key(0x12)  # Alt
    time.sleep(0.03)
    key(letter)
    time.sleep(0.05)
    key(letter, True)
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


def wait_window(pid, title_part, timeout=6.0, width_min=0, exact=False):
    end = time.time() + timeout
    while time.time() < end:
        for w in windows_of(pid):
            hit = w["title"] == title_part if exact else title_part in w["title"]
            if hit and w["w"] >= width_min:
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


def read_cfg():
    path = os.path.expandvars(r"%APPDATA%\fun.lxzt.snipforge\settings.json")
    try:
        return json.load(open(path, encoding="utf-8"))
    except Exception:
        return {}


def save_dir(cfg):
    d = (cfg.get("save") or {}).get("dir") or ""
    if d.strip():
        return d
    return os.path.expandvars(r"%USERPROFILE%\Pictures")


def newest(directory, ext, after):
    files = [f for f in glob.glob(os.path.join(directory, "**", "*." + ext), recursive=True)
             if os.path.getmtime(f) >= after]
    return max(files, key=os.path.getmtime) if files else None


def mp4_gif_pair(directory, after):
    """找最近一组 mp4 / gif（both 模式用来核对同名）"""
    a = newest(directory, "mp4", after)
    b = newest(directory, "gif", after)
    return (a, b) if a and b else None


def main():
    seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 4

    pid = app_pid()
    if not pid:
        print("snipforge.exe 没在运行，先启动它", flush=True)
        return 2
    print(f"snipforge pid={pid}", flush=True)

    cfg = read_cfg()
    fmt = (sys.argv[2] if len(sys.argv) > 2 else "mp4").lower()
    if fmt not in ("mp4", "gif"):
        print("格式只能是 mp4 或 gif")
        return 2
    print(f"本次录制格式：{fmt}", flush=True)

    started_at = time.time()

    # 记事本当录制内容，别录一片空桌面
    subprocess.Popen(["notepad.exe"])
    time.sleep(1.2)

    print(f"== 1. {'Ctrl+Alt+G' if fmt == 'gif' else 'Ctrl+Alt+R'} 进入录制选区 ==", flush=True)
    record_hotkey(fmt)
    sel = wait_window(pid, "Record Region", timeout=6)
    check("选区窗口以录屏模式打开", sel is not None,
          f"{sel['title']} {sel['w']}x{sel['h']}" if sel else "没出现")
    if not sel:
        return 1

    print("== 2. 框选区域并按回车 ==", flush=True)
    # 先把选区窗口抢到前台：它刚创建时可能还在 WebView 初始化，
    # 拖拽和回车落到别的窗口上就会表现成"回车没反应"。
    force_foreground(sel["hwnd"])
    time.sleep(0.5)
    x1, y1 = sel["x"] + 120, sel["y"] + 100
    x2, y2 = x1 + 700, y1 + 420
    drag(x1, y1, x2, y2)
    tap(VK_RETURN)

    # 控制条标题正好是 "SnipForge Record"，用精确匹配 —— 否则会先命中
    # "SnipForge Record Region"（选区）或 "SnipForge Record Overlay"。
    bar = wait_window(pid, "SnipForge Record", timeout=8, exact=True)
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

    print("== 4. 再按一次快捷键停止 ==", flush=True)
    # 控制条故意不抢焦点（不然用户没法边录边操作别的程序），
    # 所以停止走全局快捷键；控制条自己聚焦时按 Esc 也能停。
    record_hotkey(fmt)
    time.sleep(2.5)

    outdir = save_dir(cfg)
    exe = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "src-tauri", "target", "release", "snipforge.exe")
    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
    os.makedirs(out_dir, exist_ok=True)
    produced = []

    if fmt == "mp4":
        mp4 = newest(outdir, "mp4", started_at)
        check("生成了 mp4", mp4 is not None, mp4 or f"在 {outdir} 下没找到")
        if mp4:
            produced.append(mp4)
            size = os.path.getsize(mp4)
            # 静止画面压缩率极高，几十 KB 也是正常的；这里只挡「空文件」
            check("mp4 体积合理", size > 15 * 1024, f"{size // 1024} KB")
            png = os.path.join(out_dir, "record_frame0.png")
            r = subprocess.run([exe, "--record-inspect", mp4, png],
                               capture_output=True, text=True)
            check("mp4 能解出帧（容器完好）", "ok" in r.stdout,
                  r.stdout.strip() or r.stderr.strip())

    else:
        gif = newest(outdir, "gif", started_at)
        check("生成了 gif", gif is not None, gif or f"在 {outdir} 下没找到")
        if gif:
            produced.append(gif)
            size = os.path.getsize(gif)
            check("gif 体积合理", size > 5 * 1024, f"{size // 1024} KB")
            png = os.path.join(out_dir, "record_gif_frame.png")
            r = subprocess.run([exe, "--gif-inspect", gif, png, "0"],
                               capture_output=True, text=True)
            check("gif 能解出帧（编码完好）", "ok" in r.stdout,
                  r.stdout.strip() or r.stderr.strip())
            # 只出 GIF 时不应该再多出一个 mp4
            stray = newest(outdir, "mp4", started_at)
            check("没有多余的同名 mp4", stray is None, stray or "干净")

    print(flush=True)
    ok = sum(1 for _, v in results if v)
    print(f"== {ok}/{len(results)} 通过 ==", flush=True)
    for f in produced:
        print(f"产物：{f}", flush=True)
    return 0 if ok == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
