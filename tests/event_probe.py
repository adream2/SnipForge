"""用最小的 Win32 窗口验证鼠标事件是否真的送到了目标程序。

Win11 的很多右键菜单（桌面、记事本、资源管理器）都是 XAML 弹层，
类名不是传统的 `#32768`，拿它当判据会得出错误结论。
这里自己建一个真·Win32 窗口，直接看它收到了什么消息。
"""
import ctypes
import ctypes.wintypes as wt
import sys
import time

u32 = ctypes.WinDLL("user32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)

WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wt.HWND, ctypes.c_uint, wt.WPARAM, wt.LPARAM)

u32.RegisterClassW.argtypes = [ctypes.c_void_p]
u32.CreateWindowExW.restype = wt.HWND
u32.CreateWindowExW.argtypes = [
    wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD, ctypes.c_int, ctypes.c_int,
    ctypes.c_int, ctypes.c_int, wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p,
]
u32.DefWindowProcW.restype = ctypes.c_ssize_t
u32.DefWindowProcW.argtypes = [wt.HWND, ctypes.c_uint, wt.WPARAM, wt.LPARAM]
u32.PeekMessageW.argtypes = [ctypes.POINTER(wt.MSG), wt.HWND, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
# 64 位下不设 restype/argtypes，指针会被截断成 int —— 消息就分发不出去
u32.DispatchMessageW.argtypes = [ctypes.POINTER(wt.MSG)]
u32.DispatchMessageW.restype = ctypes.c_ssize_t
u32.TranslateMessage.argtypes = [ctypes.POINTER(wt.MSG)]
u32.ShowWindow.argtypes = [wt.HWND, ctypes.c_int]
u32.SetForegroundWindow.argtypes = [wt.HWND]

WM_RBUTTONDOWN = 0x0204
WM_RBUTTONUP = 0x0205
WM_LBUTTONDOWN = 0x0201

got = []


@WNDPROC
def wndproc(hwnd, msg, wparam, lparam):
    if msg == WM_RBUTTONDOWN:
        got.append("r-down")
    elif msg == WM_RBUTTONUP:
        got.append("r-up")
    elif msg == WM_LBUTTONDOWN:
        got.append("l-down")
    return u32.DefWindowProcW(hwnd, msg, wparam, lparam)


class WNDCLASSW(ctypes.Structure):
    _fields_ = [
        ("style", ctypes.c_uint),
        ("lpfnWndProc", WNDPROC),
        ("cbClsExtra", ctypes.c_int),
        ("cbWndExtra", ctypes.c_int),
        ("hInstance", wt.HINSTANCE),
        ("hIcon", wt.HICON),
        ("hCursor", wt.HANDLE),
        ("hbrBackground", wt.HBRUSH),
        ("lpszMenuName", wt.LPCWSTR),
        ("lpszClassName", wt.LPCWSTR),
    ]


def make_window(x=200, y=200, w=700, h=450):
    hinst = k32.GetModuleHandleW(None)
    cls = "SnipForgeEventProbe"
    wc = WNDCLASSW()
    wc.style = 0
    wc.lpfnWndProc = wndproc
    wc.hInstance = hinst
    wc.lpszClassName = cls
    wc.hbrBackground = ctypes.cast(ctypes.c_void_p(6), wt.HBRUSH)  # COLOR_WINDOW
    u32.RegisterClassW(ctypes.byref(wc))
    hwnd = u32.CreateWindowExW(
        0, cls, "probe", 0x00CF0000,  # WS_OVERLAPPEDWINDOW
        x, y, w, h, None, None, hinst, None,
    )
    u32.ShowWindow(hwnd, 5)
    u32.SetForegroundWindow(hwnd)
    return hwnd


def pump(ms):
    """跑一会儿消息泵，把到达的消息派发到 wndproc"""
    msg = wt.MSG()
    end = time.time() + ms / 1000
    while time.time() < end:
        while u32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            u32.TranslateMessage(ctypes.byref(msg))
            u32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.005)


def inject_right_click():
    u32.mouse_event(0x0008, 0, 0, 0, None)   # RIGHTDOWN
    pump(120)
    u32.mouse_event(0x0010, 0, 0, 0, None)   # RIGHTUP
    pump(700)


if __name__ == "__main__":
    hwnd = make_window()
    pump(700)
    u32.SetCursorPos(200 + 350, 200 + 225)
    pump(250)
    inject_right_click()
    print("收到:", got)
