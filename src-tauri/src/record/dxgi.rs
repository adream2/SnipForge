//! DXGI 桌面复制 —— 自己实现，不用 xcap 的 `video_recorder()`。
//!
//! ## 为什么不用 xcap 的录制器
//!
//! xcap 的 `VideoRecorder::stop()` 只是把取帧线程 **挂起**（`RecorderWaker::sleep`），
//! 而那个线程手里还克隆着一份 `IDXGIOutputDuplication`，永远不会退出。
//! 于是 **同一进程里第二次录制必然失败**：`DuplicateOutput` 返回 0x80070057。
//! 现象就是"录完一段、再录就报'桌面复制被占用'，重启应用才能录"。
//! 这是链路层面的硬伤，外面加重试也救不回来（实测等 40 秒依旧失败）。
//!
//! ## 这里的做法
//!
//! 自己建 D3D11 设备 + `DuplicateOutput`，`Drop` 时确定释放 —— 录多少次都行。
//! 顺带省掉了 xcap 那套「后台线程 + channel」，取帧由录制循环按节拍直接拉，
//! 既少一层拷贝也少一个线程。
//!
//! ## 两个必须记住的细节
//!
//!   1. `AcquireNextFrame` 之后 **必须** `ReleaseFrame`，否则下一次拿不到新帧。
//!      这里用「先拷贝、再释放、最后返回」的顺序，出错也不会漏释放。
//!   2. 桌面复制给的是 **BGRA**，而整条下游链路（MF 写入器 / GIF 编码器 / 光标合成）
//!      都按 RGBA 处理，所以读回时要换一次 R/B 并拍平 alpha。

use crate::error::{AppError, AppResult};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONEAREST};

/// 一个显示器的桌面复制会话。
///
/// `frame` 里始终保存"最近一帧"（RGBA，整块显示器）——
/// 桌面复制只在画面变化时给帧，静止画面靠它重复上一帧。
pub struct Duplicator {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    /// 用来把 GPU 纹理搬到 CPU 的中间纹理，尺寸跟着帧走，跨帧复用
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    /// 显示器在虚拟桌面里的原点（物理像素）
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
    pub frame: Vec<u8>,
    has_frame: bool,
}

impl Duplicator {
    /// 按区域找它所在的显示器并建立复制会话。
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> AppResult<Self> {
        unsafe {
            let probe = RECT {
                left: x,
                top: y,
                right: x + w.max(1) as i32,
                bottom: y + h.max(1) as i32,
            };
            let target = MonitorFromRect(&probe, MONITOR_DEFAULTTONEAREST);
            if target.0.is_null() {
                return Err(AppError::Msg("找不到录制区域所在的显示器".into()));
            }

            let mut device: Option<ID3D11Device> = None;
            let mut ctx: Option<ID3D11DeviceContext> = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .map_err(|e| AppError::Win32(format!("创建 D3D11 设备失败: {e}")))?;
            let device = device.ok_or_else(|| AppError::Msg("D3D11 设备为空".into()))?;
            let ctx = ctx.ok_or_else(|| AppError::Msg("D3D11 上下文为空".into()))?;

            let dxgi_device: IDXGIDevice = device.cast()?;
            let adapter = dxgi_device
                .GetAdapter()
                .map_err(|e| AppError::Win32(format!("取 DXGI 适配器失败: {e}")))?;

            // 按 order 枚举输出，找到 Monitor 句柄一致的那个才 DuplicateOutput。
            // 反例（xcap）：对每个输出都先 Duplicate，找到目标再返回 —— 多余那些
            // 复制会话就永久泄漏了，正好是"第二次录制失败"的元凶之一。
            let mut index = 0u32;
            loop {
                let output = match adapter.EnumOutputs(index) {
                    Ok(o) => o,
                    Err(_) => {
                        return Err(AppError::Msg(
                            "录制区域所在的显示器没有对应的 DXGI 输出".into(),
                        ))
                    }
                };
                index += 1;

                let desc = output
                    .GetDesc()
                    .map_err(|e| AppError::Win32(format!("读取输出描述失败: {e}")))?;
                if desc.Monitor != target {
                    continue;
                }

                let out1: IDXGIOutput1 = output.cast()?;
                let dup = out1.DuplicateOutput(&device).map_err(|e| {
                    AppError::Win32(format!(
                        "启动桌面复制失败（0x{:X}，另一程序可能正在录屏）: {e}",
                        e.code().0
                    ))
                })?;

                let ow = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(1) as u32;
                let oh = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(1) as u32;

                return Ok(Self {
                    device,
                    ctx,
                    dup,
                    staging: None,
                    origin_x: desc.DesktopCoordinates.left,
                    origin_y: desc.DesktopCoordinates.top,
                    width: ow,
                    height: oh,
                    frame: Vec::new(),
                    has_frame: false,
                });
            }
        }
    }

    pub fn has_frame(&self) -> bool {
        self.has_frame
    }

    /// 等一帧新的（最多 `timeout_ms`）。返回 false = 超时（画面没动）。
    ///
    /// 成功时整块显示器的 RGBA 已经写进 `self.frame`，跨帧复用同一块缓冲。
    pub fn acquire(&mut self, timeout_ms: u32) -> AppResult<bool> {
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;

            match self.dup.AcquireNextFrame(timeout_ms, &mut info, &mut res) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(false),
                Err(e) => {
                    // 分辨率切换 / 显示模式变化会让复制会话失效，得重建
                    return Err(AppError::Win32(format!(
                        "取帧失败（0x{:X}）: {e}",
                        e.code().0
                    )));
                }
            }

            let copied = match res {
                Some(r) => self.copy_to_frame(r),
                None => Ok(false),
            };
            // 不管拷贝成功与否，都必须释放，否则后续永远拿不到新帧
            let _ = self.dup.ReleaseFrame();
            copied
        }
    }

    unsafe fn copy_to_frame(&mut self, res: IDXGIResource) -> AppResult<bool> {
        let tex: ID3D11Texture2D = res.cast()?;
        let mut src_desc = D3D11_TEXTURE2D_DESC::default();
        tex.GetDesc(&mut src_desc);
        let (w, h) = (src_desc.Width, src_desc.Height);
        if w == 0 || h == 0 {
            return Ok(false);
        }

        // staging 纹理按需建一次，尺寸变了（切分辨率）才重建
        let need_new = match &self.staging {
            Some((_, sw, sh)) => *sw != w || *sh != h,
            None => true,
        };
        if need_new {
            let mut desc = src_desc;
            desc.BindFlags = 0;
            desc.MiscFlags = 0;
            desc.Usage = D3D11_USAGE_STAGING;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MipLevels = 1;
            desc.ArraySize = 1;
            let mut staging: Option<ID3D11Texture2D> = None;
            self.device.CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.ok_or_else(|| AppError::Msg("创建 staging 纹理失败".into()))?;
            self.staging = Some((staging, w, h));
        }

        let staging = &self.staging.as_ref().unwrap().0;
        let ctx = &self.ctx;
        let dst_res: ID3D11Resource = staging.cast()?;
        let src_res: ID3D11Resource = tex.cast()?;
        // CopyResource 不返回 Result（顶点/资源拷贝失败只能靠后续 Map 暴露）
        ctx.CopyResource(&dst_res, &src_res);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&dst_res, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| AppError::Win32(format!("映射帧失败: {e}")))?;

        let out_len = (w as usize) * (h as usize) * 4;
        if self.frame.len() != out_len {
            self.frame = vec![0u8; out_len];
        }
        let pitch = mapped.RowPitch as usize;
        let row_bytes = (w as usize) * 4;
        if !mapped.pData.is_null() {
            for y in 0..h as usize {
                let src = std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * pitch),
                    row_bytes,
                );
                let dst = &mut self.frame[y * row_bytes..(y + 1) * row_bytes];
                // BGRA → RGBA，alpha 拍平：桌面广播里 alpha 经常是 0
                for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                    d[0] = s[2];
                    d[1] = s[1];
                    d[2] = s[0];
                    d[3] = 255;
                }
            }
        }

        ctx.Unmap(&dst_res, 0);
        self.width = w;
        self.height = h;
        self.has_frame = true;
        Ok(true)
    }

}

impl Drop for Duplicator {
    fn drop(&mut self) {
        // 释放复制会话。这一句就是"能连续录制多次"的关键 ——
        // xcap 的录制器把 duplication 克隆留在挂起的线程里，永远不还。
        unsafe {
            let _ = self.dup.ReleaseFrame();
        }
        self.staging = None;
    }
}
