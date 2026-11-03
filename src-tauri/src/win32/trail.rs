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

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, GetTextExtentPoint32W, HBRUSH,
    HDC, HFONT, HGDIOBJ, HPEN, InvalidateRect, Polyline, SelectObject, SetBkMode, SetTextColor,
    TextOutW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FONT_QUALITY,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
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
/// 帧率节流用的定时器（见 `WM_TRAIL_REPAINT`）
const TIMER_THROTTLE: usize = 2;
/// 一帧的最短间隔（约 66fps）。
///
/// 125Hz / 1000Hz 的鼠标每秒能报上百次位移，每次都重画一遍全屏 ——
/// 人眼在 66fps 以上根本看不出差别，多出来的重绘全是在烧 CPU。
const FRAME_MS: i64 = 15;
/// 上一帧真正画完的时刻（`GetTickCount64`，毫秒）
static LAST_PAINT: AtomicU64 = AtomicU64::new(0);

/// 单调毫秒时钟。窗口线程会用 `SetTimer`，所以必须单调 ——
/// `SystemTime` 会被系统对时改掉，改一次就可能导致长时间不再重绘。
fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}
/// 手势结束后把结果多留一会儿，让用户看清识别成了什么。
///
/// 520ms 是上一版的值，实测"粘手"：连续划两个手势时，前一个的浮标还挂在
/// 屏幕上，看起来像没反应过来。240ms 足够读完四个字，又不拖下一个手势。
const FADE_MS: u32 = 240;

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
/// 是否已经有一次重绘在排队。
///
/// 高刷鼠标每秒能吐出上千个 `WM_MOUSEMOVE`，每个都 `PostMessage` 一次，
/// 窗口线程的消息队列会被灌满 —— 表现就是**轨迹明显落后于光标**，
/// 越划越远。这里做合并：队列里已经有一次待处理的重绘就直接返回，
/// 等下一帧画的时候，这期间的点早就都在 `POINTS` 里了。
///
/// 用 `AcqRel` 而不是 Relaxed：置位和清位跨线程（钩子线程 / 窗口线程），
/// 漏掉一次重绘就是"线画到一半停住"，宁可保守。
static REPAINT_PENDING: AtomicBool = AtomicBool::new(false);

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

/// 请求重绘，但**合并**：队列里已经有一次就跳过。
fn post_repaint() {
    if REPAINT_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    post(WM_TRAIL_REPAINT, 0);
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
    let mut changed = false;
    if let Ok(mut p) = POINTS.lock() {
        // 2px 以内的抖动不记。高刷鼠标一个像素一报，逐点存会让点数和
        // 消息量翻好几倍，而屏幕上根本看不出这一像素的差别。
        if let Some(&(lx, ly)) = p.last() {
            if (x - lx).abs() <= 1 && (y - ly).abs() <= 1 {
                return;
            }
        }
        // 上限兜底：手抖得再厉害也不该记几万个点
        if p.len() < 2000 {
            p.push((x, y));
            changed = true;
        }
    }
    if changed {
        post_repaint();
    }
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
    post_repaint();
}

/// 手势结束：先留着让用户看一眼识别结果，几百毫秒后自己消失。
pub fn end() {
    ACTIVE.store(false, Ordering::SeqCst);
    post(WM_TRAIL_FADE, 0);
}

/// 立刻隐藏，并**等到窗口真的藏起来**再返回。
///
/// 截图/录屏前必须调它：轨迹层的淡出是 240ms，截图只要几十毫秒 ——
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
            let _ = KillTimer(Some(hwnd), TIMER_THROTTLE);
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
            // 和 REPAINT 分支同样的顺序：先清合并标志，再让窗口失效。
            // 这样"清标志到失效"之间新排的那一帧不会被吞掉。
            REPAINT_PENDING.store(false, Ordering::Release);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_TRAIL_REPAINT => {
            // 先清合并标志，再让它失效：这两步之间新来的点会重新置位，
            // 于是本帧画完之后还会再排一帧 —— 反过来就会丢掉最后一段轨迹。
            REPAINT_PENDING.store(false, Ordering::Release);
            let since = now_ms().saturating_sub(LAST_PAINT.load(Ordering::Relaxed));
            if since >= FRAME_MS as u64 {
                let _ = InvalidateRect(Some(hwnd), None, false);
            } else {
                // 距上一帧不足一帧就先攒着。同一个 id 的 SetTimer 会重置等待，
                // 不会积累出多个定时器。
                SetTimer(Some(hwnd), TIMER_THROTTLE, (FRAME_MS as u64 - since) as u32, None);
            }
            LRESULT(0)
        }
        WM_TRAIL_FADE => {
            // 留一小会儿再收，让用户看清"识别成了什么"
            SetTimer(Some(hwnd), TIMER_FADE, FADE_MS, None);
            LRESULT(0)
        }
        WM_TRAIL_HIDE_NOW => {
            let _ = KillTimer(Some(hwnd), TIMER_FADE);
            // 收起之后还留着一个待触发的重绘，就会在隐藏后多画一帧
            let _ = KillTimer(Some(hwnd), TIMER_THROTTLE);
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
        WM_TIMER if wparam.0 == TIMER_THROTTLE => {
            let _ = KillTimer(Some(hwnd), TIMER_THROTTLE);
            let _ = InvalidateRect(Some(hwnd), None, false);
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

/// 内存 DC + 全部 GDI 资源缓存。
///
/// 缓存的不只是位图，还有画笔/画刷/字体。以前每帧都 `CreatePen` 两次、
/// `CreateFontW` 一次再删掉 —— 字体创建要去查字体缓存，实测几毫秒，
/// 一帧就白扔掉了。鼠标划起来的表现就是"线追不上光标"。
///
/// 只有分辨率变化或轨迹色变化时才重建。
struct Surface {
    mem: HDC,
    bmp: windows::Win32::Graphics::Gdi::HBITMAP,
    /// 创建 DC 时系统给的默认位图，销毁前要选回去，否则 DeleteDC 会失败
    old: HGDIOBJ,
    w: i32,
    h: i32,
    /// 复用点缓冲：每帧 new 一个 Vec<POINT> 是持续堆分配
    pts: Vec<POINT>,
    pen_halo: HPEN,
    pen_main: HPEN,
    /// pen_main 当前颜色。设置页改轨迹色后要重建画笔
    main_color: u32,
    brush_bg: HBRUSH,
    brush_label: HBRUSH,
    font: HFONT,
    /// 上一帧在屏幕上留下的内容范围（轨迹 + 浮标）。
    /// 下一帧必须把它一起擦掉 —— 否则浮标移动后会在原位留个白方块。
    last: Option<RECT>,
}

// HDC/HBITMAP 是裸指针包装，只在窗口线程里用，跨线程移动是安全的
unsafe impl Send for Surface {}

static SURFACE: Mutex<Option<Surface>> = Mutex::new(None);

impl Surface {
    unsafe fn create(hdc: HDC, w: i32, h: i32, color: u32) -> Option<Self> {
        let mem = CreateCompatibleDC(Some(hdc));
        if mem.is_invalid() {
            return None;
        }
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        Some(Self {
            mem,
            bmp,
            old,
            w,
            h,
            pts: Vec::with_capacity(2048),
            // 白描边：浅色壁纸/白底文档上，紫色细线会糊成一团
            pen_halo: CreatePen(PS_SOLID, 7, COLORREF(0x00FFFFFF)),
            pen_main: CreatePen(PS_SOLID, 4, COLORREF(color)),
            main_color: color,
            brush_bg: CreateSolidBrush(COLORREF(0)),
            brush_label: CreateSolidBrush(COLORREF(0x00FFFFFF)),
            font: CreateFontW(
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
            ),
            last: None,
        })
    }

    unsafe fn destroy(self) {
        let _ = DeleteObject(self.pen_halo.into());
        let _ = DeleteObject(self.pen_main.into());
        let _ = DeleteObject(self.brush_bg.into());
        let _ = DeleteObject(self.brush_label.into());
        let _ = DeleteObject(self.font.into());
        SelectObject(self.mem, self.old);
        let _ = DeleteObject(self.bmp.into());
        let _ = DeleteDC(self.mem);
    }
}

fn union_rect(a: RECT, b: RECT) -> RECT {
    RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// 往外扩几个像素并夹到窗口内。
///
/// 描边是 7px 的画笔，`Polyline` 的端点是线的中心，所以点在边界上时
/// 半个线宽会漏到包围盒外面。留 10px 余量，宁可多贴几行像素。
fn inflate_rect(r: RECT, by: i32, vw: i32, vh: i32) -> RECT {
    RECT {
        left: (r.left - by).max(0),
        top: (r.top - by).max(0),
        right: (r.right + by).min(vw),
        bottom: (r.bottom + by).min(vh),
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

    // 整个绘制期间持有 SURFACE 锁：只有窗口线程会碰它，没有竞争
    let mut guard = match SURFACE.lock() {
        Ok(g) => g,
        Err(_) => {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
    };

    let need_new = match guard.as_ref() {
        Some(s) => s.w != vw || s.h != vh || s.main_color != COLOR.load(Ordering::Relaxed),
        None => true,
    };
    if need_new {
        if let Some(old) = guard.take() {
            old.destroy();
        }
        *guard = Surface::create(hdc, vw, vh, COLOR.load(Ordering::Relaxed));
    }

    let Some(s) = guard.as_mut() else {
        let _ = EndPaint(hwnd, &ps);
        return;
    };

    // 复用缓冲：直接往已有 Vec 里填，不再每帧分配
    s.pts.clear();
    if let Ok(p) = POINTS.lock() {
        s.pts.extend(
            p.iter()
                .map(|&(x, y)| POINT {
                    x: x - vx,
                    y: y - vy,
                }),
        );
    }
    let label = LABEL.lock().map(|l| l.clone()).unwrap_or_default();

    let color = s.main_color;

    // ---- 先算出这一帧要画的范围，再决定清哪块、贴哪块 ----
    // 画面上只有两样东西：轨迹（含 7px 白描边）和浮标。它们的并集就是
    // 这一帧的全部内容。4K 双屏下全屏 BitBlt 一次要十几毫秒，
    // 而实际变化的往往只有几百像素见方 —— 只贴那一块就够。
    let mut cur: Option<RECT> = None;

    if !s.pts.is_empty() {
        let mut r = RECT {
            left: i32::MAX,
            top: i32::MAX,
            right: i32::MIN,
            bottom: i32::MIN,
        };
        for p in &s.pts {
            r.left = r.left.min(p.x);
            r.top = r.top.min(p.y);
            r.right = r.right.max(p.x);
            r.bottom = r.bottom.max(p.y);
        }
        cur = Some(r);
    }

    // 浮标位置依赖轨迹末端，所以必须在清屏之前算出来
    let mut label_box: Option<(RECT, Vec<u16>)> = None;
    if !label.is_empty() {
        let anchor = s.pts.last().copied().unwrap_or(POINT { x: 0, y: 0 });
        let old_font = SelectObject(s.mem, HGDIOBJ(s.font.0));
        let wide: Vec<u16> = label.encode_utf16().collect();
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(s.mem, &wide, &mut sz);
        SelectObject(s.mem, old_font);

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
        cur = Some(match cur {
            Some(r) => union_rect(r, br),
            None => br,
        });
        label_box = Some((br, wide));
    }

    // 上一帧画过的区域必须一起擦掉；刚建好的位图内容未定义，铺一次整屏
    let dirty = match (cur, s.last) {
        (Some(_), None) => RECT {
            left: 0,
            top: 0,
            right: vw,
            bottom: vh,
        },
        (Some(c), Some(l)) => union_rect(c, l),
        (None, Some(l)) => l,
        (None, None) => RECT {
            left: 0,
            top: 0,
            right: vw,
            bottom: vh,
        },
    };
    let dirty = inflate_rect(dirty, 10, vw, vh);

    // 底色纯黑 = 透明色。只清脏区，其余区域沿用上一帧内容
    FillRect(s.mem, &dirty, s.brush_bg);

    if s.pts.len() >= 2 {
        let old_pen = SelectObject(s.mem, HGDIOBJ(s.pen_halo.0));
        let _ = Polyline(s.mem, &s.pts);
        SelectObject(s.mem, HGDIOBJ(s.pen_main.0));
        let _ = Polyline(s.mem, &s.pts);
        SelectObject(s.mem, old_pen);
    }

    // 动作名浮标：白底 + 主题色文字
    if let Some((br, wide)) = &label_box {
        FillRect(s.mem, br, s.brush_label);
        let old_font = SelectObject(s.mem, HGDIOBJ(s.font.0));
        let _ = SetBkMode(s.mem, TRANSPARENT);
        SetTextColor(s.mem, COLORREF(color));
        let _ = TextOutW(s.mem, br.left + 7, br.top + 7, wide);
        SelectObject(s.mem, old_font);
    }

    // 只贴脏区回屏幕
    let _ = BitBlt(
        hdc,
        dirty.left,
        dirty.top,
        dirty.right - dirty.left,
        dirty.bottom - dirty.top,
        Some(s.mem),
        dirty.left,
        dirty.top,
        SRCCOPY,
    );
    // 记下这一帧真正画完的时刻，节流按它算间隔
    LAST_PAINT.store(now_ms(), Ordering::Relaxed);

    // 这一帧留下的范围，下一帧要连它一起擦
    s.last = cur;

    drop(guard);
    let _ = EndPaint(hwnd, &ps);
}
