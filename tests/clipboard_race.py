"""剪贴板时序测试 —— 专测「复制完立刻 Win+V」这条最容易出问题的路径。

为什么单独立一个测试：
  剪贴板监听是 500ms 轮询的，面板列表从历史里取。用户「Ctrl+C → 立刻 Win+V」
  的间隔经常只有几十毫秒，如果面板打开时轮询还没跑到，列表里就没有刚复制的内容 ——
  用户看到的是「按了没反应」或「粘出来是上一次的东西」。

每一轮做的事：
  1. 剪贴板设为 OLD{n}，等 1.3s 让监听入库（这是一条"旧"记录）
  2. 记事本里写 T{n}，全选复制（剪贴板变成 T{n}）
  3. 取消选中，**80ms 后**按 Win+V
  4. 等面板出现 → 回车
  5. 用 Ctrl+A/Ctrl+C 把记事本内容读回来（Win11 记事本是 XAML，WM_GETTEXT 读不到）

判定：
  T{n}T{n}    → 正确（粘的是刚复制的那条）
  T{n}OLD{n}  → 粘了旧记录（列表没刷新）
  T{n}        → 什么都没粘进去

跑法：先启动 snipforge.exe（设置里 Win+V 接管要开着），再
    python tests/clipboard_race.py [轮数]
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time

u32 = ctypes.WinDLL("user32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)

ENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
HOOKPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, ctypes.c_int, ctypes.c_size_t, ctypes.c_ssize_t)

u32.EnumWindows.argtypes = [ENUMPROC, wt.LPARAM]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.GetForegroundWindow.restype = wt.HWND
u32.SetForegroundWindow.argtypes = [wt.HWND]
u32.SetWindowPos.argtypes = [
    wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT
]
u32.OpenClipboard.argtypes = [wt.HWND]
u32.EmptyClipboard.argtypes = []
u32.CloseClipboard.argtypes = []
u32.SetClipboardData.restype = wt.HANDLE
u32.SetClipboardData.argtypes = [wt.UINT, wt.HANDLE]
u32.GetClipboardData.restype = wt.HANDLE
u32.GetClipboardData.argtypes = [wt.UINT]
u32.AttachThreadInput.argtypes = [wt.DWORD, wt.DWORD, wt.BOOL]
u32.VkKeyScanW.argtypes = [wt.WCHAR]
u32.VkKeyScanW.restype = ctypes.c_short
u32.SetWindowsHookExW.restype = wt.HHOOK
u32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, wt.HINSTANCE, wt.DWORD]
u32.CallNextHookEx.restype = ctypes.c_ssize_t
u32.CallNextHookEx.argtypes = [wt.HHOOK, ctypes.c_int, ctypes.c_size_t, ctypes.c_ssize_t]

k32.GlobalAlloc.restype = wt.HGLOBAL
k32.GlobalAlloc.argtypes = [wt.UINT, ctypes.c_size_t]
k32.GlobalLock.argtypes = [wt.HANDLE]
k32.GlobalLock.restype = ctypes.c_void_p
k32.GlobalUnlock.argtypes = [wt.HANDLE]
k32.GetCurrentThreadId.restype = wt.DWORD
k32.GetCurrentThreadId.argtypes = []

WH_KEYBOARD_LL = 13
VK_CONTROL, VK_V, VK_LWIN, VK_RETURN = 0x11, 0x56, 0x5B, 0x0D
VK_A, VK_C = 0x41, 0x43
VK_DELETE, VK_RIGHT = 0x2E, 0x27
KEYEVENTF_KEYUP = 0x0002
GMEM_MOVEABLE = 0x0002


def key(vk, up=False):
    u32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def combo(mod, vk):
    key(mod)
    time.sleep(0.02)
    key(vk)
    time.sleep(0.02)
    key(vk, True)
    time.sleep(0.02)
    key(mod, True)
    time.sleep(0.03)


def tap(vk):
    key(vk)
    time.sleep(0.02)
    key(vk, True)
    time.sleep(0.02)


def type_text(s):
    """只支持 ASCII，够测试用了。"""
    for ch in s:
        r = u32.VkKeyScanW(ch)
        if r == -1:
            continue
        vk = r & 0xFF
        shift = bool(r >> 8 & 0x01)
        if shift:
            key(0x10)
            time.sleep(0.01)
        tap(vk)
        if shift:
            key(0x10, True)
            time.sleep(0.01)


def get_clipboard_text():
    if not u32.OpenClipboard(None):
        return ""
    try:
        h = u32.GetClipboardData(13)  # CF_UNICODETEXT
        if not h:
            return ""
        p = k32.GlobalLock(h)
        if not p:
            return ""
        try:
            return ctypes.wstring_at(p)
        finally:
            k32.GlobalUnlock(h)
    finally:
        u32.CloseClipboard()


def set_clipboard_text(s):
    data = (s + "\0").encode("utf-16-le")
    h = k32.GlobalAlloc(GMEM_MOVEABLE, len(data))
    p = k32.GlobalLock(h)
    ctypes.memmove(p, data, len(data))
    k32.GlobalUnlock(h)
    if not u32.OpenClipboard(None):
        return False
    try:
        u32.EmptyClipboard()
        return bool(u32.SetClipboardData(13, h))
    finally:
        u32.CloseClipboard()


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


def find_panel(pid):
    found = []

    @ENUMPROC
    def cb(hwnd, _):
        p = wt.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value != pid or not u32.IsWindowVisible(hwnd):
            return True
        title = ctypes.create_unicode_buffer(256)
        u32.GetWindowTextW(hwnd, title, 256)
        r = wt.RECT()
        u32.GetWindowRect(hwnd, ctypes.byref(r))
        w, h = r.right - r.left, r.bottom - r.top
        # 面板：标题 SnipForge，窄长条（主窗口比它大得多，编辑器标题带「编辑器」）
        if title.value == "SnipForge" and 250 < w < 900 and 150 < h < 900:
            found.append(hwnd)
        return True

    u32.EnumWindows(cb, 0)
    return found[0] if found else None


def get_notepad():
    out = []

    @ENUMPROC
    def cb(hwnd, _):
        if not u32.IsWindowVisible(hwnd):
            return True
        cls = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(hwnd, cls, 256)
        if cls.value == "Notepad":
            out.append(hwnd)
        return True

    u32.EnumWindows(cb, 0)
    return out[0] if out else None


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


def notepad_text(np):
    """用 Ctrl+A / Ctrl+C 把记事本内容读回来（XAML 记事本读不到子控件文本）。"""
    force_foreground(np)
    time.sleep(0.12)
    combo(VK_CONTROL, VK_A)
    combo(VK_CONTROL, VK_C)
    time.sleep(0.25)
    return get_clipboard_text()


def plain_paste_test(trials=15):
    """普通 Ctrl+V 会不会被我们的剪贴板监听拖累。

    「剪贴板功能影响到系统」这句话的另一种可能：应用在监听剪贴板，
    每次变化都去 OpenClipboard 读一遍，如果正好和目标程序的粘贴撞上，
    对方的 OpenClipboard 会失败，粘出来就是空的。

    判据：写剪贴板 → 立刻在记事本里 Ctrl+V，15 次全部命中才算不影响。
    """
    pid = app_pid()
    if not pid:
        print("snipforge.exe 没在运行", flush=True)
        return 2

    subprocess.Popen(["notepad.exe"])
    np = None
    for _ in range(40):
        time.sleep(0.25)
        np = get_notepad()
        if np:
            break
    if not np:
        print("记事本没起来", flush=True)
        return 2

    run = int(time.time()) % 100000
    fails = []
    for i in range(1, trials + 1):
        tag = f"PP{run}x{i}"
        set_clipboard_text(tag)
        # 故意不等：粘贴紧跟在写入之后，最容易撞上监听线程
        force_foreground(np)
        time.sleep(0.05)
        combo(VK_CONTROL, VK_A)
        tap(VK_DELETE)
        combo(VK_CONTROL, VK_V)
        time.sleep(0.18)
        got = notepad_text(np).strip()
        if got != tag:
            fails.append((tag, got))
            print(f"  [{tag}] FAIL 读到 {got!r}", flush=True)
        else:
            print(f"  [{tag}] ok", flush=True)

    print(flush=True)
    if fails:
        print(f"普通粘贴 {trials - len(fails)}/{trials}，失败 {len(fails)} 次 —— 监听干扰了系统剪贴板", flush=True)
        return 1
    print(f"普通粘贴 {trials}/{trials} 全部命中，未干扰系统剪贴板", flush=True)
    return 0


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "plain":
        return plain_paste_test()

    trials = int(sys.argv[1]) if len(sys.argv) > 1 else 4

    pid = app_pid()
    if not pid:
        print("snipforge.exe 没在运行，先启动它", flush=True)
        return 2
    print(f"snipforge pid={pid}", flush=True)

    subprocess.Popen(["notepad.exe"])
    np = None
    for _ in range(40):
        time.sleep(0.25)
        np = get_notepad()
        if np:
            break
    if not np:
        print("记事本没起来", flush=True)
        return 2
    print(f"记事本 hwnd={np}", flush=True)

    stats = {"ok": 0, "stale": 0, "none": 0, "nopanel": 0, "nofocus": 0}
    # 每轮用唯一标记：历史里有去重，重复的标记会被当成"已存在"，
    # 测出来就不是应用的问题而是测试自己的问题
    run = int(time.time()) % 100000

    for i in range(1, trials + 1):
        tag = f"MK{run}x{i}"
        old = f"OL{run}x{i}"

        # 1. 先放一条"旧"内容，等监听入库（这样列表首项是旧内容）
        set_clipboard_text(old)
        time.sleep(1.3)

        # 2. 清空记事本，模拟用户"刚复制了一段文字"
        force_foreground(np)
        time.sleep(0.15)
        combo(VK_CONTROL, VK_A)
        tap(VK_DELETE)
        time.sleep(0.1)

        # 3. 把剪贴板换成新内容（等价于在任意程序里 Ctrl+C），**40ms 后**就按 Win+V
        set_clipboard_text(tag)
        time.sleep(0.04)
        combo(VK_LWIN, VK_V)

        panel = None
        t0 = time.time()
        for _ in range(30):
            time.sleep(0.04)
            panel = find_panel(pid)
            if panel:
                break
        latency = (time.time() - t0) * 1000

        if not panel:
            stats["nopanel"] += 1
            print(f"  [{tag}] FAIL 面板没弹出", flush=True)
            continue

        # 4. 面板必须拿到焦点，否则回车会落到记事本上
        time.sleep(0.12)
        fg = u32.GetForegroundWindow()
        if fg != panel:
            stats["nofocus"] += 1
            print(f"  [{tag}] FAIL 面板弹出了但没拿到焦点（fg={fg} panel={panel}）", flush=True)
            tap(0x1B)  # Esc 收起来
            continue

        # 5. 回车粘贴
        tap(VK_RETURN)
        time.sleep(0.55)

        # 6. 读回结果
        got = notepad_text(np)

        if got.strip() == tag:
            stats["ok"] += 1
            print(f"  [{tag}] ok   {latency:.0f}ms 弹出，粘到的是刚复制的内容", flush=True)
        elif got.strip() == "" or got.strip() == old:
            stats["none"] += 1
            print(f"  [{tag}] FAIL 没粘进去 {got!r}", flush=True)
        else:
            stats["stale"] += 1
            print(f"  [{tag}] FAIL 粘了别的条目 {got[:40]!r}", flush=True)

    print(flush=True)
    print(
        f"结果：成功 {stats['ok']}/{trials}   粘错 {stats['stale']}   "
        f"无粘贴 {stats['none']}  无面板 {stats['nopanel']}  无焦点 {stats['nofocus']}",
        flush=True,
    )
    return 0 if stats["ok"] == trials else 1


if __name__ == "__main__":
    sys.exit(main())
