"""列出某个进程的所有顶层窗口，打印可见性 / 类名 / 标题。

用途：验证「静默启动」—— 进程起来了，但主窗口必须是 hidden。
只读诊断，不碰任何状态。
"""
import ctypes
import subprocess
import sys
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)


def pids(name: str) -> list[int]:
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
        capture_output=True,
    ).stdout.decode("gbk", "replace")
    found = []
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) > 1 and parts[0].lower() == name.lower():
            found.append(int(parts[1]))
    return found


def windows(pid: int):
    rows = []
    proc_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(hwnd, _):
        wpid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
        if wpid.value == pid:
            buf = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, buf, 256)
            cls = ctypes.create_unicode_buffer(256)
            user32.GetClassNameW(hwnd, cls, 256)
            rows.append((int(hwnd), bool(user32.IsWindowVisible(hwnd)), cls.value, buf.value))
        return True

    user32.EnumWindows(proc_type(cb), 0)
    return rows


def main() -> int:
    name = sys.argv[1] if len(sys.argv) > 1 else "SnipForge.exe"
    found = pids(name)
    if not found:
        print(f"{name} 没在运行")
        return 1
    for pid in found:
        print(f"pid {pid}")
        for hwnd, vis, cls, title in windows(pid):
            print(f"  hwnd={hwnd} visible={vis} class={cls} title={title!r}")
        visible = [r for r in windows(pid) if r[1]]
        print(f"  可见顶层窗口数: {len(visible)}  （静默启动应为 0）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
