"""专用探针：用 Alt+V（配置里的呼出键）走一遍「面板 → 回车 → 粘贴」。

Win+V 的链路 panel_e2e.py 已经覆盖并通过；这里验的是另一条：
全局热键（RegisterHotKey）触发的面板，在 Alt 作为修饰键时，
注入的 Ctrl+V 会不会被残留的 Alt 污染成 Ctrl+Alt+V（那样就粘不出来）。

跑法：先启动 snipforge.exe，再
    python tests/panel_altv_probe.py
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
u32.OpenClipboard.argtypes = [ctypes.c_void_p]
u32.OpenClipboard.restype = ctypes.c_int
u32.GetClipboardData.argtypes = [ctypes.c_uint]
u32.GetClipboardData.restype = ctypes.c_void_p
k32.GlobalLock.argtypes = [ctypes.c_void_p]
k32.GlobalLock.restype = ctypes.c_void_p
k32.GlobalUnlock.argtypes = [ctypes.c_void_p]

VK_ALT, VK_V, VK_RETURN = 0x12, 0x56, 0x0D
KEYEVENTF_KEYUP = 0x0002

results = []


def check(name, ok, detail=""):
    print(f"  [{'ok' if ok else 'FAIL'}] {name}   {detail}", flush=True)
    results.append((name, ok))


def key(vk, up=False):
    u32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def pump(seconds):
    """按键之间必须让系统消息泵跑起来，否则注入的事件会被合并/丢弃。"""
    end = time.time() + seconds
    while time.time() < end:
        u32.PeekMessageW(ctypes.byref(wt.MSG()), None, 0, 0, 1)
        time.sleep(0.005)


def clipboard_text():
    if not u32.OpenClipboard(None):
        return None
    try:
        h = u32.GetClipboardData(13)
        if not h:
            return None
        p = k32.GlobalLock(ctypes.c_void_p(h))
        if not p:
            return None
        try:
            return ctypes.wstring_at(p)
        finally:
            k32.GlobalUnlock(ctypes.c_void_p(h))
    finally:
        u32.CloseClipboard()


def windows(cls=None, pid=None):
    """按类名或进程找顶层窗口。面板窗口的类名是通用的 "Window"，
    所以找面板要按 pid —— 按类名永远找不到。"""
    out = []

    @ENUMPROC
    def cb(h, _):
        if not u32.IsWindowVisible(h):
            return True
        if pid is not None:
            p = wt.DWORD()
            u32.GetWindowThreadProcessId(h, ctypes.byref(p))
            if p.value != pid:
                return True
        if cls is not None:
            cn = ctypes.create_unicode_buffer(128)
            u32.GetClassNameW(h, cn, 128)
            if cn.value != cls:
                return True
        t = ctypes.create_unicode_buffer(256)
        u32.GetWindowTextW(h, t, 256)
        r = wt.RECT()
        u32.GetWindowRect(h, ctypes.byref(r))
        out.append(
            {
                "hwnd": h,
                "title": t.value,
                "x": r.left,
                "y": r.top,
                "w": r.right - r.left,
                "h": r.bottom - r.top,
            }
        )
        return True

    u32.EnumWindows(cb, 0)
    return out


def app_pid():
    for w in windows():
        if w["title"] == "SnipForge":
            p = wt.DWORD()
            u32.GetWindowThreadProcessId(w["hwnd"], ctypes.byref(p))
            return p.value
    return None


def panel_windows(pid):
    return [w for w in windows(pid=pid) if 300 < w["w"] < 700 and w["h"] > 300]


def main():
    print("== 用 Alt+V 呼出面板 ==", flush=True)

    np = subprocess.Popen(["notepad.exe"])
    pump(1.8)
    pads = [w for w in windows("Notepad") if w["w"] > 200]
    if not pads:
        print("没找到记事本窗口")
        return 1
    pad = pads[0]
    u32.SetForegroundWindow(pad["hwnd"])
    pump(0.5)
    check("记事本在前台", u32.GetForegroundWindow() == pad["hwnd"], f"hwnd={pad['hwnd']}")

    # Alt+V（和用户配置一致）
    key(VK_ALT)
    pump(0.05)
    key(VK_V)
    pump(0.06)
    key(VK_V, up=True)
    pump(0.03)
    key(VK_ALT, up=True)
    pump(1.0)

    pid = app_pid()
    panels = panel_windows(pid) if pid else []
    check("Alt+V 弹出面板", len(panels) > 0, panels[0].__str__() if panels else "无面板")
    if not panels:
        np.kill()
        return 1
    check("面板拿到焦点", u32.GetForegroundWindow() == panels[0]["hwnd"])

    # 回车粘贴
    key(VK_RETURN)
    pump(0.08)
    key(VK_RETURN, up=True)
    pump(1.5)

    fg = u32.GetForegroundWindow()
    check("焦点还给记事本", fg == pad["hwnd"], f"fg={fg} expect={pad['hwnd']}")

    # 面板收起
    still = panel_windows(pid) if pid else []
    check("面板已收起", len(still) == 0)

    # 剪贴板是否被写入
    txt = clipboard_text()
    check("剪贴板有内容", bool(txt), repr(txt)[:60])

    # 记事本里到底有没有粘上（读 EDIT 控件）
    edit = ctypes.wintypes.HWND()

    @ENUMPROC
    def find_edit(h, _):
        cn = ctypes.create_unicode_buffer(64)
        u32.GetClassNameW(h, cn, 64)
        if cn.value == "Edit":
            # RichEdit 是 "RICHEDIT50W"
            ctypes.cast(ctypes.pointer(edit), ctypes.POINTER(ctypes.c_void_p))[0] = h
            return False
        return True

    u32.EnumChildWindows(pad["hwnd"], find_edit, 0)
    if edit:
        n = u32.SendMessageW(edit, 0x000E, 0, 0)  # WM_GETTEXTLENGTH
        buf = ctypes.create_unicode_buffer(n + 2)
        u32.SendMessageW(edit, 0x000D, n + 1, ctypes.cast(buf, ctypes.c_ssize_t).value)
        got = buf.value
        check("记事本里粘上了内容", len(got.strip()) > 0, repr(got)[:60])
    else:
        print("  [skip] 读不到记事本编辑框")

    pump(0.3)
    np.kill()
    print(flush=True)
    ok = sum(1 for _, v in results if v)
    print(f"== {ok}/{len(results)} 通过 ==", flush=True)
    return 0 if ok == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
