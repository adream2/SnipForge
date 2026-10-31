//! 鼠标手势的轨迹层。
//!
//! 一个 GDI 分层窗口，铺满整个虚拟屏，负责两件事：画轨迹、显示识别到的动作名。
//! **不用 webview**：每次按下抬起都要闪一下，起个浏览器内核只为画条线，太重。
//!
//! 三条硬要求（都踩过）：
//!   1. **绝不能挡住鼠标**：`WS_EX_TRANSPARENT` + `WM_NCHITTEST` 返回 `HTTRANSPARENT`，
//!      双保险。少一个都可能让整个屏幕点不动。
//!   2. **不能每帧重建位图**：一开始每帧 `CreateCompatibleBitmap(2560x1440)`（14MB），
//!      鼠标一划就卡成幻灯片。内存 DC 和位图缓存起来复用。
//!   3. **钩子里不能同步调窗口**：`ShowWindow` 是跨线程同步的，窗口线程忙的时候
//!      会把钩子卡住 —— 钩子卡住 = 全系统鼠标卡住。所以这里全部走 `PostMessage`。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, GetTextExtentPoint32W, HGDIOBJ,
    InvalidateRect, Polyline, SelectObject, SetBkMode, SetTextColor, TextOutW, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FONT_QUALITY, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetSystemMetrics,
    KillTimer, PostMessageW, RegisterClassW, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT, HWND_TOPMOST, LWA_COLORKEY,
    MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_APP, WM_ERASEBKGND,
    WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const WM_TRAIL_SHOW: u32 = WM_APP + 1;
const WM_TRAIL_REPAINT: u32 = WM_APP + 2;
const WM_TRAIL_FADE: u32 = WM_APP + 3;
const WM_TRAIL_HIDE_NOW: u32 = WM_APP + 4;
const TIMER_FADE: usize = 1;
/// 手势结束后把结果多留一会儿，让用户看清识别成了什么
const FADE_MS: u32 = 520;

static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
/// 手势是否进行中（push 用它做快速过滤，避免无意义的消息洪水）
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POINTS: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());
static LABEL: Mutex<String> = Mutex::new(String::new());
/// 窗口线程"真的已经藏起来了"的计数。截图前要等它 +1，
/// 否则截出来会把刚画的那道 S 一起带进图里。
static HIDE_ACK: AtomicU32 = AtomicU32::new(0);
/// 线条颜色（COLORREF：0x00BBGGRR）
static COLOR: AtomicU32 = AtomicU32::new(0x00E7_5C_6C);

fn hwnd() -> Option<HWND> {
    let raw = HWND_RAW.load(Ordering::SeqCst);
    if raw == 0 {
        None
    } else {
        Some(HWND(raw as *mut core::ffi::c_void))
    }
}

fn post(msg: u32, lparam: isize) {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = PostMessageW(Some(h), msg, WPARAM(0), LPARAM(lparam));
        }
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

fn parse_color(hex: &str) -> u32 {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return 0x00E7_5C_6C;
    }
    let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(0x6C);
    let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(0x5C);
    let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(0xE7);
    (b << 16) | (g << 8) | r
}

/// 页面里改颜色时调
pub fn set_style(_show: bool, color: &str) {
    COLOR.store(parse_color(color), Ordering::SeqCst);
}

/// 启动轨迹窗口。幂等。提前建好，第一次划手势才不会有可见延迟。
pub fn ensure() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let (vx, vy, vw, vh) = screen();
        let class_name: Vec<u16> = "SnipForgeGestureTrail\0".encode_utf16().collect();
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .map(|h| windows::Win32::Foundation::HINSTANCE(h.0))
            .unwrap_or_default();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            log::warn!("轨迹窗口类注册失败");
            return;
        }

        let ex =
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
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
            Some(hinst),
            None,
        ) {
            Ok(h) => {
                // 纯黑 = 透明
                let _ = SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_COLORKEY);
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

/// 手势开始：清空旧轨迹，显示窗口。
///
/// 只投递消息，绝不同步等窗口线程 —— 这个函数是在鼠标钩子里调的。
pub fn begin_with(color: &str) {
    COLOR.store(parse_color(color), Ordering::SeqCst);
    if let Ok(mut p) = POINTS.lock() {
        p.clear();
    }
    if let Ok(mut l) = LABEL.lock() {
        l.clear();
    }
    ACTIVE.store(true, Ordering::SeqCst);
    post(WM_TRAIL_SHOW, 0);
}

/// 追加一个点
pub fn push(x: i32, y: i32) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut p) = POINTS.lock() {
        if p.last() == Some(&(x, y)) {
            return;
        }
        // 上限兜底：手抖得再厉害也不该记几万个点
        if p.len() < 2000 {
            p.push((x, y));
        }
    }
    post(WM_TRAIL_REPAINT, 0);
}

/// 更新浮标文字（"↓→ 关闭标签" / "未绑定"）。方向一变就调一次。
pub fn set_label(text: String) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut l) = LABEL.lock() {
        if *l == text {
            return;
        }
        *l = text;
    }
    post(WM_TRAIL_REPAINT, 0);
}

/// 手势结束：先留着让用户看一眼识别结果，几百毫秒后自己消失。
pub fn end() {
    ACTIVE.store(false, Ordering::SeqCst);
    post(WM_TRAIL_FADE, 0);
}

/// 立刻隐藏，并**等到窗口真的藏起来**再返回。
///
/// 截图/录屏前必须调它：轨迹层的淡出是 520ms，截图只要几十毫秒 ——
/// 不等就会把用户刚画的 S 一起截进图里（这是实测报上来的 bug）。
///
/// 只能在工作线程里调：内部要等窗口线程处理消息，在钩子回调里等会卡住鼠标。
pub fn hide_blocking(timeout_ms: u64) {
    ACTIVE.store(false, Ordering::SeqCst);
    if let Ok(mut p) = POINTS.lock() {
        p.clear();
    }
    if let Ok(mut l) = LABEL.lock() {
        l.clear();
    }
    if hwnd().is_none() {
        return;
    }
    let before = HIDE_ACK.load(Ordering::SeqCst);
    post(WM_TRAIL_HIDE_NOW, 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if HIDE_ACK.load(Ordering::SeqCst) != before {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    log::warn!("轨迹层隐藏超时（{}ms），截图可能会带上轨迹", timeout_ms);
}

/// 立刻隐藏（兜底用：出了问题先让屏幕能点）
pub fn hide_now() {
    ACTIVE.store(false, Ordering::SeqCst);
    if let Ok(mut p) = POINTS.lock() {
        p.clear();
    }
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // 自己全画，别让系统先擦一遍（否则每帧闪黑）
        WM_ERASEBKGND => LRESULT(1),
        // 鼠标事件一律穿透。WS_EX_TRANSPARENT 已经能穿透，这里再兜一层：
        // 有过"整个屏幕点不动"的教训，不敢只靠一个标志位。
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_TRAIL_SHOW => {
            let _ = KillTimer(Some(hwnd), TIMER_FADE);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_TRAIL_REPAINT => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_TRAIL_FADE => {
            // 留一小会儿再收，让用户看清"识别成了什么"
            SetTimer(Some(hwnd), TIMER_FADE, FADE_MS, None);
            LRESULT(0)
        }
        WM_TRAIL_HIDE_NOW => {
            let _ = KillTimer(Some(hwnd), TIMER_FADE);
            let _ = ShowWindow(hwnd, SW_HIDE);
            if let Ok(mut p) = POINTS.lock() {
                p.clear();
            }
            if let Ok(mut l) = LABEL.lock() {
                l.clear();
            }
            HIDE_ACK.fetch_add(1, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_FADE => {
            let _ = KillTimer(Some(hwnd), TIMER_FADE);
            let _ = ShowWindow(hwnd, SW_HIDE);
            if let Ok(mut p) = POINTS.lock() {
                p.clear();
            }
            if let Ok(mut l) = LABEL.lock() {
                l.clear();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 内存 DC + 位图缓存。
///
/// 每帧重建 2560x1440 的兼容位图要十几毫秒、还要分配 14MB，
/// 鼠标划起来直接卡成幻灯片（这是"手势一开鼠标就不听话"的主因之一）。
struct Surface {
    mem: windows::Win32::Graphics::Gdi::HDC,
    bmp: windows::Win32::Graphics::Gdi::HBITMAP,
    /// 创建 DC 时系统给的默认位图，销毁前要选回去，否则 DeleteDC 会失败
    old: HGDIOBJ,
    w: i32,
    h: i32,
}

// HDC/HBITMAP 是裸指针包装，只在窗口线程里用，跨线程移动是安全的
unsafe impl Send for Surface {}

static SURFACE: Mutex<Option<Surface>> = Mutex::new(None);

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_invalid() {
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    let (vx, vy, vw, vh) = screen();

    // 整个绘制期间持有 SURFACE 锁：只有窗口线程会碰它，没有竞争
    let mut guard = match SURFACE.lock() {
        Ok(g) => g,
        Err(_) => {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
    };

    let need_new = match guard.as_ref() {
        Some(s) => s.w != vw || s.h != vh,
        None => true,
    };
    if need_new {
        if let Some(old) = guard.take() {
            SelectObject(old.mem, old.old);
            let _ = DeleteObject(old.bmp.into());
            let _ = DeleteDC(old.mem);
        }
        let mem = CreateCompatibleDC(Some(hdc));
        if mem.is_invalid() {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
        let bmp = CreateCompatibleBitmap(hdc, vw, vh);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        *guard = Some(Surface {
            mem,
            bmp,
            old,
            w: vw,
            h: vh,
        });
    }

    let s = guard.as_ref().unwrap();
    let mem = s.mem;

    let pts: Vec<POINT> = POINTS
        .lock()
        .map(|p| {
            p.iter()
                .map(|(x, y)| POINT {
                    x: x - vx,
                    y: y - vy,
                })
                .collect()
        })
        .unwrap_or_default();
    let label = LABEL.lock().map(|l| l.clone()).unwrap_or_default();

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

    if pts.len() >= 2 {
        // 白描边：浅色壁纸/白底文档上，紫色细线会糊成一团
        let white = CreatePen(PS_SOLID, 7, COLORREF(0x00FFFFFF));
        let old_pen = SelectObject(mem, HGDIOBJ(white.0));
        let _ = Polyline(mem, &pts);
        let _ = DeleteObject(white.into());

        let pen = CreatePen(PS_SOLID, 4, COLORREF(COLOR.load(Ordering::Relaxed)));
        SelectObject(mem, HGDIOBJ(pen.0));
        let _ = Polyline(mem, &pts);
        let _ = DeleteObject(pen.into());
        SelectObject(mem, old_pen);
    }

    // 动作名浮标：跟着轨迹末端走，白底 + 主题色文字
    if !label.is_empty() {
        let anchor = pts.last().copied().unwrap_or(POINT { x: 0, y: 0 });
        let font = CreateFontW(
            -15,
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            FONT_QUALITY(0),
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            windows::core::w!("Microsoft YaHei"),
        );
        let old_font = SelectObject(mem, HGDIOBJ(font.0));
        let _ = SetBkMode(mem, TRANSPARENT);

        let wide: Vec<u16> = label.encode_utf16().collect();
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(mem, &wide, &mut sz);

        let pad = 7;
        let mut bx = anchor.x + 20;
        let mut by = anchor.y + 20;
        // 别让浮标跑出屏幕
        if bx + sz.cx + pad * 2 > vw {
            bx = anchor.x - sz.cx - pad * 2 - 20;
        }
        if by + sz.cy + pad * 2 > vh {
            by = anchor.y - sz.cy - pad * 2 - 20;
        }
        let br = RECT {
            left: bx,
            top: by,
            right: bx + sz.cx + pad * 2,
            bottom: by + sz.cy + pad * 2,
        };
        let bg = CreateSolidBrush(COLORREF(0x00FFFFFF));
        FillRect(mem, &br, bg);
        let _ = DeleteObject(bg.into());

        SetTextColor(mem, COLORREF(COLOR.load(Ordering::Relaxed)));
        let _ = TextOutW(mem, bx + pad, by + pad, &wide);

        SelectObject(mem, old_font);
        let _ = DeleteObject(font.into());
    }

    // 贴回屏幕
    let _ = BitBlt(hdc, 0, 0, vw, vh, Some(mem), 0, 0, SRCCOPY);

    drop(guard);
    let _ = EndPaint(hwnd, &ps);
}
