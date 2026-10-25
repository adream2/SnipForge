//! 把真实鼠标光标画进录制画面。
//!
//! 为什么必须自己做：
//!   DXGI 桌面复制拿到的帧**不包含光标**（它是独立合成层）。如果不补，
//!   录出来的视频里只有一个高亮环，环中间是空的 —— 看起来像 bug。
//!
//! 取位图的办法是「黑底画一次、白底画一次」：
//!   同一个光标分别画到纯黑和纯白的底上，两次结果一减就能反推出 alpha：
//!      白 - 黑 = 255 × (1 - alpha)
//!   然后 颜色 = 黑 / alpha。
//!   这样**不用关心光标是 32 位带 alpha 的、还是单色掩码的**（文本 I 型光标就是单色）。
//!   早期版本靠掩码判断透明度，碰到单色光标会画出一个黑方块。
//!
//! 光标句柄变了才重新取（换形状时才变），逐帧只是查一下句柄，开销可忽略。

use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetObjectW, PatBlt, SelectObject,
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLACKNESS, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ, WHITENESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, HICON, ICONINFO,
};

/// 一次光标采样：屏幕位置 + 热点（热点决定图像左上角往哪偏）
#[derive(Debug, Clone, Copy)]
pub struct CursorPos {
    pub x: i32,
    pub y: i32,
    pub hot_x: i32,
    pub hot_y: i32,
}

/// 一张光标位图（BGRA，自上而下，alpha 有效）
#[derive(Clone)]
struct CursorImage {
    w: i32,
    h: i32,
    px: Vec<u8>,
    hot_x: i32,
    hot_y: i32,
}

pub struct CursorTracker {
    /// 上一次看到的光标句柄，用来判断要不要重读位图
    handle: isize,
    image: Option<CursorImage>,
    pub visible: bool,
}

impl Default for CursorTracker {
    fn default() -> Self {
        Self {
            handle: -1,
            image: None,
            visible: false,
        }
    }
}

impl CursorTracker {
    /// 每帧调一次：更新光标位置/可见性，必要时重读位图。
    pub fn poll(&mut self) -> Option<CursorPos> {
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            if GetCursorInfo(&mut ci).is_err() {
                self.visible = false;
                return None;
            }
        }
        self.visible = ci.flags.0 & CURSOR_SHOWING.0 != 0;
        if !self.visible {
            return None;
        }

        let h = ci.hCursor.0 as isize;
        if h != self.handle {
            self.handle = h;
            self.image = unsafe { read_cursor(ci.hCursor) };
        }

        let (hot_x, hot_y) = self
            .image
            .as_ref()
            .map(|i| (i.hot_x, i.hot_y))
            .unwrap_or((0, 0));
        Some(CursorPos {
            x: ci.ptScreenPos.x,
            y: ci.ptScreenPos.y,
            hot_x,
            hot_y,
        })
    }

    /// 位图统计：不透明像素数、包围盒、alpha 范围。
    /// 「光标没画出来」时靠它分辨是没采到、还是 alpha 全 0。
    pub fn stats(&self) -> Option<(usize, (i32, i32, i32, i32), u8, u8)> {
        let img = self.image.as_ref()?;
        let mut n = 0usize;
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let (mut amin, mut amax) = (255u8, 0u8);
        for y in 0..img.h {
            for x in 0..img.w {
                let a = img.px[((y * img.w + x) * 4 + 3) as usize];
                amin = amin.min(a);
                amax = amax.max(a);
                if a > 0 {
                    n += 1;
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if n == 0 {
            return Some((0, (0, 0, 0, 0), amin, amax));
        }
        Some((n, (x0, y0, x1, y1), amin, amax))
    }

    /// 把当前光标位图导出成 PNG，用来确认系统位图到底读到了什么。
    /// 出问题时就靠它分辨「没读到」和「读到了但合成错了」。
    pub fn dump(&self, path: &std::path::Path) -> crate::error::AppResult<(i32, i32)> {
        let Some(img) = &self.image else {
            return Err(crate::error::AppError::Msg(
                "没读到光标位图（read_cursor 返回 None）".into(),
            ));
        };
        let mut rgba = vec![0u8; img.px.len()];
        for i in (0..img.px.len()).step_by(4) {
            rgba[i] = img.px[i + 2];
            rgba[i + 1] = img.px[i + 1];
            rgba[i + 2] = img.px[i];
            rgba[i + 3] = img.px[i + 3];
        }
        let im = image::RgbaImage::from_raw(img.w as u32, img.h as u32, rgba)
            .ok_or_else(|| crate::error::AppError::Msg("光标尺寸不匹配".into()))?;
        im.save_with_format(path, image::ImageFormat::Png)?;
        Ok((img.w, img.h))
    }

    /// 把光标合进帧。`(x, y)` 是光标图像左上角在帧内的坐标（可为负）。
    pub fn composite(&self, frame: &mut [u8], fw: u32, fh: u32, x: i32, y: i32) {
        let Some(img) = &self.image else { return };
        let (fw, fh) = (fw as i32, fh as i32);

        for cy in 0..img.h {
            let dy = y + cy;
            if dy < 0 || dy >= fh {
                continue;
            }
            for cx in 0..img.w {
                let dx = x + cx;
                if dx < 0 || dx >= fw {
                    continue;
                }

                let si = ((cy * img.w + cx) * 4) as usize;
                let a = img.px[si + 3] as u32;
                if a == 0 {
                    continue;
                }

                let di = ((dy * fw + dx) * 4) as usize;
                // 光标是 BGRA，帧是 RGBA
                let (b, g, r) = (
                    img.px[si] as u32,
                    img.px[si + 1] as u32,
                    img.px[si + 2] as u32,
                );
                let inv = 255 - a;
                frame[di] = ((r * a + frame[di] as u32 * inv) / 255) as u8;
                frame[di + 1] = ((g * a + frame[di + 1] as u32 * inv) / 255) as u8;
                frame[di + 2] = ((b * a + frame[di + 2] as u32 * inv) / 255) as u8;
                frame[di + 3] = 255;
            }
        }
    }
}

/// 位图尺寸。单色光标没有颜色位图，返回的是掩码高度（是实际高度的两倍）。
unsafe fn bitmap_size(bmp: HBITMAP) -> Option<(i32, i32)> {
    if bmp.is_invalid() {
        return None;
    }
    let mut b = BITMAP::default();
    let n = GetObjectW(
        HGDIOBJ(bmp.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut b as *mut _ as *mut _),
    );
    if n == 0 || b.bmWidth <= 0 || b.bmHeight <= 0 {
        return None;
    }
    Some((b.bmWidth, b.bmHeight))
}

unsafe fn read_cursor(hcur: windows::Win32::UI::WindowsAndMessaging::HCURSOR) -> Option<CursorImage> {
    let mut ii = ICONINFO::default();
    if GetIconInfo(HICON(hcur.0), &mut ii).is_err() {
        return None;
    }
    let _icon_guard = IconGuard(ii.hbmColor, ii.hbmMask);

    // 尺寸：优先取颜色位图；单色光标只有掩码，且高度是两倍（AND + XOR）
    let (w, h) = match bitmap_size(ii.hbmColor) {
        Some((w, h)) => (w, h),
        None => {
            let (w, h) = bitmap_size(ii.hbmMask)?;
            (w, h / 2)
        }
    };
    if w <= 0 || h <= 0 || w > 256 || h > 256 {
        return None;
    }

    let hdc = CreateCompatibleDC(None);
    if hdc.is_invalid() {
        return None;
    }
    let _dc_guard = DcGuard(hdc);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            // 负高度 = 自上而下，省得再翻一遍
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    if dib.is_invalid() || bits.is_null() {
        return None;
    }
    let old = SelectObject(hdc, HGDIOBJ(dib.0));
    let _select_guard = SelectGuard(hdc, old);

    let len = (w * h * 4) as usize;
    let mut black = vec![0u8; len];
    let mut white = vec![0u8; len];

    // 黑底画一次
    let _ = PatBlt(hdc, 0, 0, w, h, BLACKNESS);
    let _ = DrawIconEx(hdc, 0, 0, HICON(hcur.0), w, h, 0, None, DI_NORMAL);
    std::ptr::copy_nonoverlapping(bits as *const u8, black.as_mut_ptr(), len);

    // 白底画一次
    let _ = PatBlt(hdc, 0, 0, w, h, WHITENESS);
    let _ = DrawIconEx(hdc, 0, 0, HICON(hcur.0), w, h, 0, None, DI_NORMAL);
    std::ptr::copy_nonoverlapping(bits as *const u8, white.as_mut_ptr(), len);

    let _ = DeleteObject(HGDIOBJ(dib.0));

    // 反推 alpha 与颜色：白 - 黑 = 255×(1-a)，颜色 = 黑/a
    let mut px = vec![0u8; len];
    for i in (0..len).step_by(4) {
        let diff = (white[i] as i32 - black[i] as i32).clamp(0, 255) as u32;
        let a = 255 - diff;
        if a == 0 {
            continue;
        }
        for c in 0..3 {
            px[i + c] = ((black[i + c] as u32 * 255) / a).min(255) as u8;
        }
        px[i + 3] = a as u8;
    }

    Some(CursorImage {
        w,
        h,
        px,
        hot_x: ii.xHotspot as i32,
        hot_y: ii.yHotspot as i32,
    })
}

struct DcGuard(HDC);
impl Drop for DcGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

/// GetIconInfo 会返回两个需要自己释放的位图
struct IconGuard(HBITMAP, HBITMAP);
impl Drop for IconGuard {
    fn drop(&mut self) {
        unsafe {
            for b in [self.0, self.1] {
                if !b.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(b.0));
                }
            }
        }
    }
}

struct SelectGuard(HDC, HGDIOBJ);
impl Drop for SelectGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.0, self.1);
        }
    }
}

/// 检测鼠标按键"刚刚按下"。
///
/// 用 `GetAsyncKeyState` 的低位（0x0001 = 上次调用以来按下过），
/// 而不是自己比对 0x8000 的当前状态 —— 后者按 30Hz 采样会漏掉短促点击，
/// 前者是系统帮我们记住的边沿，一次都不会漏。读完即清。
pub fn poll_clicks() -> (bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    unsafe {
        let l = GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x0001 != 0;
        let r = GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x0001 != 0;
        (l, r)
    }
}
