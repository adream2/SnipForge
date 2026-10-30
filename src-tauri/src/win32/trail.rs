//! 鼠标手势的轨迹层。
//!
//! 一个 GDI 分层窗口，铺满整个虚拟屏（多显示器也算），只负责把用户划过的
//! 轨迹画出来。**不用 webview**：手势每次按下抬起都要闪一下，起一个浏览器内核
//! 就为了画一条线，太重了。
//!
//! 透明靠 `LWA_COLORKEY`：整个窗口底色涂纯黑，黑色被系统当作透明色。
//! 加上 `WS_EX_TRANSPARENT`，鼠标事件直接穿过去，落在下面真正的窗口上。
//!
//! 重绘节流靠 `InvalidateRect`：WM_PAINT 是低优先级消息，系统会把连续的
//! 重绘请求合并成一次，所以我们不需要自己算帧率。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, EndPaint, FillRect, GetDC, InvalidateRect, Polyline, ReleaseDC,
    SelectObject, PAINTSTRUCT, PS_SOLID, SRCCOPY,
};
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, RegisterClassW, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, HWND_TOPMOST, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, SetLayeredWindowAttributes,
    LWA_COLORKEY,
};

static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
static VISIBLE: AtomicBool = AtomicBool::new(false);
static POINTS: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());
/// 线条颜色（COLORREF：0x00BBGGRR）。默认和界面主色一致。
static COLOR: AtomicU32 = AtomicU32::new(0x00E7_5C_6C);

/// "#6C5CE7" → COLORREF。解析失败就用默认紫。
fn parse_color(hex: &str) -> u32 {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return 0x00E7_5C_6C;
    }
    let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(0x6C);
    let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(0x5C);
    let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(0xE7);
    // COLORREF 是 BGR 排列
    (b << 16) | (g << 8) | r
}

/// 设置外观（页面里改颜色/开关轨迹时调）
pub fn set_style(_show: bool, color: &str) {
    COLOR.store(parse_color(color), Ordering::SeqCst);
}

fn hwnd() -> Option<HWND> {
    let raw = HWND_RAW.load(Ordering::SeqCst);
    if raw == 0 {
        None
    } else {
        Some(HWND(raw as *mut core::ffi::c_void))
    }
}

fn screen() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    }
}

/// 启动轨迹窗口线程。幂等。
pub fn ensure() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| unsafe {
        // 和主程序保持一致的 DPI 上下文，否则按缩放比例会画偏
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let (vx, vy, vw, vh) = screen();
        let class_name: Vec<u16> = "SnipForgeGestureTrail\0".encode_utf16().collect();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map(|h| windows::Win32::Foundation::HINSTANCE(h.0))
                .unwrap_or_default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            log::warn!("轨迹窗口类注册失败");
            return;
        }

        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        match CreateWindowExW(
            ex,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            None,
            None,
            Some(wc.hInstance),
            None,
        ) {
            Ok(h) => {
                // 纯黑 = 透明
                let _ = SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_COLORKEY);
                // 别抢焦点、别进 Alt+Tab（WS_EX_NOACTIVATE + TOOLWINDOW 已经处理）
                let _ = HWND_TOPMOST;
                HWND_RAW.store(h.0 as isize, Ordering::SeqCst);
                log::debug!("手势轨迹层已就绪 {vw}x{vh} @ ({vx},{vy})");
            }
            Err(e) => {
                log::warn!("轨迹窗口创建失败: {e}");
                return;
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&mut msg);
        }

        if let Some(h) = hwnd() {
            let _ = DestroyWindow(h);
        }
    });
}

/// 手势开始：按当前颜色清空旧轨迹并显示
pub fn begin_with(color: &str) {
    COLOR.store(parse_color(color), Ordering::SeqCst);
    begin();
}

/// 手势开始：清空旧轨迹并显示
pub fn begin() {
    ensure();
    POINTS.lock().map(|mut p| p.clear()).ok();
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        }
        VISIBLE.store(true, Ordering::SeqCst);
    }
}

/// 追加一个点并请求重绘
pub fn push(x: i32, y: i32) {
    if !VISIBLE.load(Ordering::SeqCst) {
        return;
    }
    if let Ok(mut p) = POINTS.lock() {
        // 同样的点不用记，省一次无意义的重绘
        if p.last() == Some(&(x, y)) {
            return;
        }
        p.push((x, y));
    }
    if let Some(h) = hwnd() {
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
}

/// 手势结束：立刻消失。
///
/// 这里**不等**最后一次重绘：手势已经完成，再留一帧尾巴反而像卡顿。
pub fn end() {
    VISIBLE.store(false, Ordering::SeqCst);
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
    POINTS.lock().map(|mut p| p.clear()).ok();
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // 自己全画，别让系统先擦一遍（否则每帧闪一下黑）
        windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_invalid() {
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    let (vx, vy, vw, vh) = screen();
    let mem = CreateCompatibleDC(Some(hdc));
    if mem.is_invalid() {
        let _ = EndPaint(hwnd, &ps);
        return;
    }
    let bmp = CreateCompatibleBitmap(hdc, vw, vh);
    let old_bmp = SelectObject(mem, bmp.into());

    // 底色纯黑 = 透明色
    let black = CreateSolidBrush(COLORREF(0));
    let rc = RECT {
        left: 0,
        top: 0,
        right: vw,
        bottom: vh,
    };
    FillRect(mem, &rc, black);
    let _ = DeleteObject(black.into());

    let pts: Vec<windows::Win32::Foundation::POINT> = POINTS
        .lock()
        .map(|p| {
            p.iter()
                .map(|(x, y)| windows::Win32::Foundation::POINT {
                    x: x - vx,
                    y: y - vy,
                })
                .collect()
        })
        .unwrap_or_default();

    if pts.len() >= 2 {
        // 先描一圈白：桌面背景可能是浅色，"紫色细线"在浅色壁纸上会糊成一团
        let white = CreatePen(PS_SOLID, 7, COLORREF(0x00FFFFFF));
        let old_pen = SelectObject(mem, white.into());
        let _ = Polyline(mem, &pts);
        let _ = DeleteObject(white.into());

        let pen = CreatePen(PS_SOLID, 4, COLORREF(COLOR.load(Ordering::SeqCst)));
        SelectObject(mem, pen.into());
        let _ = Polyline(mem, &pts);
        let _ = DeleteObject(pen.into());
        SelectObject(mem, old_pen);
    }

    let _ = BitBlt(hdc, 0, 0, vw, vh, Some(mem), 0, 0, SRCCOPY);

    SelectObject(mem, old_bmp);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
    let _ = ReleaseDC(None, hdc);
    let _ = EndPaint(hwnd, &ps);
}
