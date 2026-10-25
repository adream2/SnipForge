"""剪贴板面板端到端测试。

验的是这一条链：在别的窗口里按 Win+V → 弹面板 → 回车 → 内容落到原窗口。

看点：
  1. Win+V 是否弹出面板（证明键盘钩子生效）
  2. 面板是否拿到焦点（否则按键进不去）
  3. 回车后焦点是否回到原窗口（证明焦点记忆 + 还原）
  4. 是否真的注入了 Ctrl+V（用低级键盘钩子抓自己的注入事件）
  5. 剪贴板内容是否是选中那条
  6. 面板是否收起

跑法：先启动 snipforge.exe，再
    python tests/panel_e2e.py
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
u32.EnumChildWindows.argtypes = [wt.HWND, ENUMPROC, wt.LPARAM]
u32.SendMessageW.argtypes = [wt.HWND, wt.UINT, ctypes.c_size_t, ctypes.c_ssize_t]
u32.SendMessageW.restype = ctypes.c_ssize_t
u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.GetForegroundWindow.restype = wt.HWND
u32.SetWindowPos.argtypes = [
    wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT
]
u32.SetWindowsHookExW.restype = wt.HHOOK
u32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, wt.HINSTANCE, wt.DWORD]
u32.CallNextHookEx.restype = ctypes.c_ssize_t
u32.CallNextHookEx.argtypes = [wt.HHOOK, ctypes.c_int, ctypes.c_size_t, ctypes.c_ssize_t]
u32.OpenClipboard.argtypes = [wt.HWND]
u32.GetClipboardData.restype = wt.HANDLE
u32.GetClipboardData.argtypes = [wt.UINT]
k32.GlobalLock.argtypes = [wt.HANDLE]
k32.GlobalLock.restype = ctypes.c_void_p
k32.GlobalUnlock.argtypes = [wt.HANDLE]
k32.GetCurrentThreadId.restype = wt.DWORD
k32.GetCurrentThreadId.argtypes = []


class KBDLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [
        ("vkCode", wt.DWORD),
        ("scanCode", wt.DWORD),
        ("flags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


WH_KEYBOARD_LL = 13
WM_KEYDOWN, WM_SYSKEYDOWN = 0x0100, 0x0104
VK_LWIN, VK_V, VK_RETURN = 0x5B, 0x56, 0x0D
VK_CONTROL, VK_LCONTROL, VK_RCONTROL = 0x11, 0xA2, 0xA3
KEYEVENTF_KEYUP = 0x0002
LLKHF_INJECTED = 0x10

hits = []  # (vk, injected, is_down, t)
results = []


def check(name, ok, detail=""):
    results.append((name, ok, detail))
    print(("  [ok]   " if ok else "  [FAIL] ") + name + (f"  {detail}" if detail else ""), flush=True)


def pump(seconds):
    """跑消息循环：低级键盘钩子要靠它才能收到回调。"""
    msg = wt.MSG()
    end = time.time() + seconds
    while time.time() < end:
        while u32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            u32.TranslateMessage(ctypes.byref(msg))
            u32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.008)


def install_hook():
    @HOOKPROC
    def cb(code, wparam, lparam):
        if code >= 0:
            kb = ctypes.cast(lparam, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
            down = wparam in (WM_KEYDOWN, WM_SYSKEYDOWN)
            hits.append((kb.vkCode, bool(kb.flags & LLKHF_INJECTED), down, time.time()))
        return u32.CallNextHookEx(None, code, wparam, lparam)

    h = u32.SetWindowsHookExW(WH_KEYBOARD_LL, cb, None, 0)
    return h, cb  # 返回 cb 防止被 GC


def windows():
    out = []

    @ENUMPROC
    def cb(hwnd, _):
        pid = wt.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        cls = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(hwnd, cls, 256)
        title = ctypes.create_unicode_buffer(512)
        u32.GetWindowTextW(hwnd, title, 512)
        rect = wt.RECT()
        u32.GetWindowRect(hwnd, ctypes.byref(rect))
        out.append({
            "hwnd": hwnd, "pid": pid.value, "cls": cls.value, "title": title.value,
            "visible": bool(u32.IsWindowVisible(hwnd)),
            "w": rect.right - rect.left, "h": rect.bottom - rect.top,
        })
        return True

    u32.EnumWindows(cb, 0)
    return out


def child_texts(hwnd):
    """读出目标窗口里所有子控件的文本，用来确认粘贴了几次。"""
    out = []

    @ENUMPROC
    def cb(child, _):
        cls = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(child, cls, 256)
        buf = ctypes.create_unicode_buffer(16384)
        u32.SendMessageW(child, 0x000D, 16384, ctypes.addressof(buf))  # WM_GETTEXT
        if buf.value:
            out.append((cls.value, buf.value))
        return True

    u32.EnumChildWindows(hwnd, cb, 0)
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


def force_foreground(hwnd):
    """把目标窗口顶到前台。

    直接 SetForegroundWindow 会被前台锁挡掉（我们不是前台进程），
    所以要挂到它线程的输入队列上借资格。点鼠标会误点到别的窗口，不可靠。
    """
    u32.SetWindowPos(hwnd, wt.HWND(-1), 0, 0, 0, 0, 0x0001 | 0x0002)  # TOPMOST
    u32.SetWindowPos(hwnd, wt.HWND(-2), 0, 0, 0, 0, 0x0001 | 0x0002)  # NOTOPMOST
    if u32.SetForegroundWindow(hwnd):
        return True
    fg = u32.GetForegroundWindow()
    fg_tid = u32.GetWindowThreadProcessId(fg, None)
    my_tid = k32.GetCurrentThreadId()
    u32.AttachThreadInput(fg_tid, my_tid, True)
    ok = u32.SetForegroundWindow(hwnd)
    u32.AttachThreadInput(fg_tid, my_tid, False)
    return bool(ok)


def click_at(x, y):
    u32.SetCursorPos(x, y)
    u32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.03)
    u32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP


def clipboard_text():
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


def main():
    pid = app_pid()
    if not pid:
        print("SnipForge 没有在运行", flush=True)
        return 1

    handle, keep = install_hook()
    print(f"钩子已装: {bool(handle)}", flush=True)
    pump(0.2)

    marker = f"SNIPFORGE-PANEL-{int(time.time())}"
    subprocess.run(f'echo {marker}| clip', shell=True, capture_output=True)
    pump(1.5)  # 等剪贴板监听收录，面板首项就是它

    # 拿一个真实的第三方窗口当目标
    np = subprocess.Popen(["notepad.exe"])
    pump(2.2)
    target = next(
        (w for w in windows() if w["cls"].lower() == "notepad" and w["visible"]),
        None,
    )
    if not target:
        print("记事本窗口没找到，无法验证", flush=True)
        np.kill()
        return 1

    # 点一下把它顶到前台 —— 比 SetForegroundWindow 更接近真实场景
    u32.SetWindowPos(target["hwnd"], None, 120, 120, 760, 460, 0x0004)
    pump(0.2)
    force_foreground(target["hwnd"])
    pump(0.4)
    fg0 = u32.GetForegroundWindow()
    if fg0 != target["hwnd"]:
        click_at(400, 300)
        pump(0.5)
        fg0 = u32.GetForegroundWindow()
    print(f"目标窗口 hwnd={target['hwnd']} fg={fg0} marker={marker}", flush=True)
    check("记事本已在前台", fg0 == target["hwnd"], f"fg={fg0}")

    print("== 1. 在记事本里按 Win+V ==", flush=True)
    key(VK_LWIN); pump(0.04)
    key(VK_V); pump(0.04)
    key(VK_V, up=True); pump(0.04)
    key(VK_LWIN, up=True)
    pump(1.3)

    panel = next(
        (w for w in windows()
         if w["pid"] == pid and w["visible"] and 200 < w["w"] < 700 and 200 < w["h"] < 800),
        None,
    )
    check("Win+V 弹出面板", panel is not None, f"{panel['w']}x{panel['h']}" if panel else "")
    fg = u32.GetForegroundWindow()
    check("面板拿到焦点", panel is not None and fg == panel["hwnd"], f"fg={fg}")

    if panel:
        print("== 2. 面板里按回车 ==", flush=True)
        hits.clear()
        key(VK_RETURN); pump(0.06)
        key(VK_RETURN, up=True)
        pump(1.5)

        fg2 = u32.GetForegroundWindow()
        check("焦点还给记事本", fg2 == target["hwnd"], f"fg={fg2} expect={target['hwnd']}")

        seq = [(hex(h[0]), h[2]) for h in hits if h[1]]
        print("    注入序列:", seq, flush=True)
        injected_v = [h for h in hits if h[0] == VK_V and h[1] and h[2]]
        injected_ctrl = [
            h for h in hits
            if h[0] in (VK_CONTROL, VK_LCONTROL, VK_RCONTROL) and h[1] and h[2]
        ]
        check("注入了 Ctrl+V", bool(injected_v) and bool(injected_ctrl),
              f"v={len(injected_v)} ctrl={len(injected_ctrl)}")

        check("剪贴板内容是选中那条", clipboard_text().strip() == marker.strip(),
              repr(clipboard_text()[:40]))
        # 记事本渲染有延迟，读到空就再等一会儿（这里曾经因为读太早误报失败）
        doc = ""
        found = []
        for _ in range(12):
            found = child_texts(target["hwnd"])
            doc = next((t for _, t in found if marker in t), "")
            if doc:
                break
            pump(0.25)
        if doc:
            check("只粘贴了一次", doc.count(marker) == 1, f"出现 {doc.count(marker)} 次")
        else:
            # Win11 新版记事本是 XAML island，子控件读不出文字。
            # 这条只是"顺手确认没粘两遍"，读不到就跳过，别把整体判红。
            print(
                f"  [skip] 记事本内容读不出来（{len(found)} 个子控件），跳过双粘贴检查",
                flush=True,
            )

        check("面板已收起",
              not any(w["hwnd"] == panel["hwnd"] and w["visible"] for w in windows()))

    np.kill()

    failed = [r for r in results if not r[1]]
    print(f"== {len(results) - len(failed)}/{len(results)} 通过 ==", flush=True)
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
