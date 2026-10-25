"""屏幕截图存 PNG（纯 ctypes，不依赖 Pillow / .NET）。

    python tests/shot.py out.png [x y w h]

不给区域就抓主屏全屏。用来人工核对界面渲染。
"""

import ctypes
import ctypes.wintypes as wt
import os
import struct
import sys
import zlib

u32 = ctypes.WinDLL("user32", use_last_error=True)
g32 = ctypes.WinDLL("gdi32", use_last_error=True)

g32.CreateCompatibleDC.argtypes = [wt.HDC]
g32.CreateCompatibleDC.restype = wt.HDC
g32.CreateCompatibleBitmap.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int]
g32.CreateCompatibleBitmap.restype = wt.HBITMAP
g32.SelectObject.argtypes = [wt.HDC, wt.HGDIOBJ]
g32.SelectObject.restype = wt.HGDIOBJ
g32.BitBlt.argtypes = [
    wt.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
    wt.HDC, ctypes.c_int, ctypes.c_int, wt.DWORD,
]
g32.GetDIBits.argtypes = [
    wt.HDC, wt.HBITMAP, wt.UINT, wt.UINT, ctypes.c_void_p,
    ctypes.c_void_p, wt.UINT,
]
g32.DeleteObject.argtypes = [wt.HGDIOBJ]
g32.DeleteDC.argtypes = [wt.HDC]
u32.GetDC.argtypes = [wt.HWND]
u32.GetDC.restype = wt.HDC
u32.ReleaseDC.argtypes = [wt.HWND, wt.HDC]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD),
        ("biWidth", ctypes.c_long),
        ("biHeight", ctypes.c_long),
        ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long),
        ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


def grab(x, y, w, h):
    screen = u32.GetDC(None)
    mem = g32.CreateCompatibleDC(screen)
    bmp = g32.CreateCompatibleBitmap(screen, w, h)
    old = g32.SelectObject(mem, bmp)
    # SRCCOPY | CAPTUREBLT：不加 CAPTUREBLT 时，分层窗口（透明窗口）
    # 会被整体排除 —— 而 SnipForge 的选区/覆盖层/控制条全是透明窗口，
    # 少这个标志截图里就"看不到"它们，排查时会得出完全错误的结论。
    g32.BitBlt(mem, 0, 0, w, h, screen, x, y, 0x00CC0020 | 0x40000000)

    bi = BITMAPINFOHEADER()
    bi.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.biWidth = w
    bi.biHeight = -h  # 负数 = 自上而下
    bi.biPlanes = 1
    bi.biBitCount = 32
    bi.biCompression = 0

    buf = ctypes.create_string_buffer(w * h * 4)
    g32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bi), 0)

    g32.SelectObject(mem, old)
    g32.DeleteObject(bmp)
    g32.DeleteDC(mem)
    u32.ReleaseDC(None, screen)
    return buf.raw, w, h


def write_png(path, raw, w, h):
    rows = bytearray()
    stride = w * 4
    for y in range(h):
        rows.append(0)  # filter: none
        line = raw[y * stride:(y + 1) * stride]
        rgb = bytearray(w * 3)
        # BGRA -> RGB
        rgb[0::3] = line[2::4]
        rgb[1::3] = line[1::4]
        rgb[2::3] = line[0::4]
        rows += rgb

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows), 6))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "shot.png"
    if len(sys.argv) >= 6:
        x, y, w, h = (int(v) for v in sys.argv[2:6])
    else:
        x, y, w, h = 0, 0, u32.GetSystemMetrics(0), u32.GetSystemMetrics(1)
    raw, w, h = grab(x, y, w, h)
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    write_png(out, raw, w, h)
    print(f"{out} {w}x{h}")


if __name__ == "__main__":
    main()
