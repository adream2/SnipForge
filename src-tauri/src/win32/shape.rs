//! 形状手势识别 —— 真正"画出来"的曲线，而不是方向串。
//!
//! 方向串（U/D/L/R）对折线手势足够用，但表达不了"S"这种东西：
//! 用 `DLD` 去近似，用户得刻意画成一段折线，手感完全不对。
//! 这里做的是**形状匹配**：把用户画的那一笔重采样、归一化，再和标准 S 比距离。
//!
//! 做法（就是 $1 unistroke recognizer 的简化版，故意不做旋转不变）：
//!   1. 抖点丢掉，按弧长重采样成固定点数 —— 画快画慢、点密点疏都不影响结果；
//!   2. 每个轴单独缩放到 [-1,1] —— 大小随便，画得扁一点圆一点都认；
//!   3. 和模板逐点比平均距离，正着画反着画都试一遍（S 转 180° 还是 S）；
//!   4. 另外两道闸：首尾太近是画圈、整条笔直是方向手势 —— 都不是 S。
//!
//! 只认 S 一个形状：用户真正需要的是"画个 S 截图"。多加形状（O、C、W）
//! 会和已有的方向串抢手势，误识别一次就够让人不信任整个功能了。

use std::f32::consts::PI;

/// 重采样点数。太少糊成一团，太多对抖动敏感。32 是 $1 的实验结论。
const SAMPLES: usize = 32;

/// 归一化后的平均点距阈值。0 = 完全重合，1 左右 = 毫不相干。
const MATCH_DIST: f32 = 0.30;

/// 单笔最多留这么多点（钩子里兜底，防止手抖记录几万个点）
pub const MAX_POINTS: usize = 1200;

/// 认一下这一笔是不是标准形状。是就返回形状字符（目前只有 `'S'`）。
pub fn recognize(raw: &[(i32, i32)]) -> Option<char> {
    let pts = thin(raw, 2.0);
    if pts.len() < 10 {
        return None;
    }
    let len = length(&pts);
    let (w, h) = extent(&pts);

    // 太小的一抖：不当作画形状（阈值取得很小，96px 的小录制框里也画得下）
    if w < 24.0 || h < 26.0 || len < 70.0 {
        return None;
    }
    // 直接拖一条线：路径长度 ≈ 外框长边。S 的路径长度至少是它的 1.3 倍
    // （窄高的 S、画得很扁的 S 都在这之上），这条闸门就把直线手势挡在外面了。
    if len < 1.15 * w.max(h) {
        return None;
    }
    // 首尾挨着 = 画了个圈，不是 S。拿高度比而不是路径长度比：
    // S 画得再扁再宽，首尾也各自贴着上下两个角（弦长 ≈ 高度），
    // 而圆的弦长永远是 0 —— 用长度比会被宽 S 误伤。
    let start = pts[0];
    let end = *pts.last()?;
    if dist(start, end) < 0.5 * h {
        return None;
    }
    // 弯都不弯的，交给方向串去认
    if max_deviation(&pts) < 0.06 {
        return None;
    }

    let mut path = normalize(&resample(&pts, SAMPLES));
    smooth(&mut path);

    let tpl = template();
    // 正着画、反着画都试一遍：S 转 180° 还是 S，从下往上画的人不少
    let d = mean_dist(&path, &tpl).min(mean_dist(&path, &reversed(&tpl)));
    if d > MATCH_DIST {
        return None;
    }
    Some('S')
}

/// 相邻两个点太近就丢：手抖和鼠标 1000Hz 回报会灌进来一堆重复点
fn thin(raw: &[(i32, i32)], min: f32) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(raw.len().min(MAX_POINTS));
    for &(x, y) in raw {
        let p = (x as f32, y as f32);
        if out.last().map_or(true, |l| dist(*l, p) >= min) {
            out.push(p);
        }
        if out.len() >= MAX_POINTS {
            break;
        }
    }
    out
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn length(p: &[(f32, f32)]) -> f32 {
    p.windows(2).map(|w| dist(w[0], w[1])).sum()
}

fn extent(p: &[(f32, f32)]) -> (f32, f32) {
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &(x, y) in p {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    (x1 - x0, y1 - y0)
}

/// 首尾连线的最大偏离（相对弦长）。直线 ≈ 0，S ≈ 0.5。
fn max_deviation(p: &[(f32, f32)]) -> f32 {
    let (x0, y0) = p[0];
    let (x1, y1) = match p.last() {
        Some(&e) => e,
        None => return 0.0,
    };
    let (dx, dy) = (x1 - x0, y1 - y0);
    let n = (dx * dx + dy * dy).sqrt();
    if n < 1e-3 {
        return 1.0;
    }
    let dev = p
        .iter()
        .map(|&(x, y)| ((x - x0) * dy - (y - y0) * dx).abs() / n)
        .fold(0.0f32, f32::max);
    dev / n
}

/// 按弧长等距重采样
fn resample(p: &[(f32, f32)], n: usize) -> Vec<(f32, f32)> {
    let mut cum = Vec::with_capacity(p.len());
    let mut total = 0.0;
    cum.push(0.0);
    for w in p.windows(2) {
        total += dist(w[0], w[1]);
        cum.push(total);
    }
    if total <= 1e-3 || p.len() < 2 {
        return vec![p[0]; n];
    }
    let step = total / (n - 1) as f32;
    let mut out = Vec::with_capacity(n);
    let mut seg = 0usize;
    for i in 0..n {
        let target = (i as f32 * step).min(total);
        while seg + 2 < p.len() && cum[seg + 1] < target {
            seg += 1;
        }
        let (a, b) = (cum[seg], cum[seg + 1]);
        let t = if b > a { ((target - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
        let (x0, y0) = p[seg];
        let (x1, y1) = p[seg + 1];
        out.push((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t));
    }
    out
}

/// 每个轴单独拉伸到 [-1,1]：只比"形状"，不比大小和宽高比
fn normalize(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &(x, y) in p {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (sx, sy) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let sx = if sx.abs() < 1e-3 { 1.0 } else { sx };
    let sy = if sy.abs() < 1e-3 { 1.0 } else { sy };
    p.iter().map(|&(x, y)| ((x - cx) / sx, (y - cy) / sy)).collect()
}

/// 三点滑动平均跑一遍。手画的线一定有毛刺，不抹一下会误判成"尖角"。
fn smooth(p: &mut [(f32, f32)]) {
    if p.len() < 3 {
        return;
    }
    let src = p.to_vec();
    for i in 1..p.len() - 1 {
        p[i] = (
            (src[i - 1].0 + src[i].0 + src[i + 1].0) / 3.0,
            (src[i - 1].1 + src[i].1 + src[i + 1].1) / 3.0,
        );
    }
}

fn mean_dist(a: &[(f32, f32)], b: &[(f32, f32)]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return f32::MAX;
    }
    let sum: f32 = (0..n).map(|i| dist(a[i], b[i])).sum();
    sum / n as f32
}

fn reversed(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    p.iter().rev().copied().collect()
}

/// 标准 S：上下两段半圆，上段往左鼓、下段往右鼓。
///
/// 不用手写坐标表：肉眼调出来的点列换台机器就不像了，
/// 半圆是数学上确定的，改采样点数也不会变形。
fn template() -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let t = i as f32 / (SAMPLES - 1) as f32;
        let p = if t < 0.5 {
            let a = t / 0.5 * PI;
            (-0.5 * a.sin(), 0.5 + 0.5 * a.cos())
        } else {
            let a = (t - 0.5) / 0.5 * PI;
            (0.5 * a.sin(), -0.5 * (1.0 - a.cos()))
        };
        out.push(p);
    }
    normalize(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成一笔：沿 S 走一遍，`size` 是高度，宽高比可调，`noise` 是每点抖动
    pub(super) fn dbg_s(h: f32, ratio: f32, noise: f32, bottom_up: bool) -> Vec<(i32, i32)> {
        draw_s(h, ratio, noise, bottom_up, 80)
    }

    fn draw_s(h: f32, ratio: f32, noise: f32, bottom_up: bool, n: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let t = if bottom_up { 1.0 - t } else { t };
            let (x, y) = if t < 0.5 {
                let a = t / 0.5 * PI;
                (-0.5 * a.sin() * h * ratio, 0.5 + 0.5 * a.cos())
            } else {
                let a = (t - 0.5) / 0.5 * PI;
                (0.5 * a.sin() * h * ratio, -0.5 * (1.0 - a.cos()))
            };
            // 伪随机抖动：不引 rand，用 sin 造
            let jx = ((i * 7919) % 13) as f32 / 13.0 - 0.5;
            let jy = ((i * 104_729) % 11) as f32 / 11.0 - 0.5;
            v.push((
                (500.0 + x + jx * 2.0 * noise) as i32,
                (400.0 + y * h / 2.0 + jy * 2.0 * noise) as i32,
            ));
        }
        v
    }

    fn polyline(pts: &[(f32, f32)], per_seg: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for w in pts.windows(2) {
            for i in 0..per_seg {
                let t = i as f32 / per_seg as f32;
                v.push((
                    (w[0].0 + (w[1].0 - w[0].0) * t) as i32,
                    (w[0].1 + (w[1].1 - w[0].1) * t) as i32,
                ));
            }
        }
        if let Some(&l) = pts.last() {
            v.push((l.0 as i32, l.1 as i32));
        }
        v
    }

    #[test]
    fn recognizes_real_s() {
        assert_eq!(recognize(&draw_s(320.0, 0.6, 0.0, false, 80)), Some('S'));
        assert_eq!(recognize(&draw_s(200.0, 1.0, 0.0, false, 60)), Some('S'));
        assert_eq!(recognize(&draw_s(420.0, 1.4, 0.0, false, 90)), Some('S'));
        // 小一点：页面里 96px 的录制框也画得下
        assert_eq!(recognize(&draw_s(80.0, 0.7, 0.0, false, 60)), Some('S'));
        // 反着画（从下往上）还是同一个 S
        assert_eq!(recognize(&draw_s(300.0, 0.8, 0.0, true, 80)), Some('S'));
        // 手抖
        assert_eq!(recognize(&draw_s(300.0, 0.8, 6.0, false, 80)), Some('S'));
        assert_eq!(recognize(&draw_s(300.0, 0.8, 12.0, false, 80)), Some('S'));
        // 画得又窄又高 / 又扁又宽 / 点很稀（画得快）
        assert_eq!(recognize(&draw_s(360.0, 0.25, 0.0, false, 60)), Some('S'));
        assert_eq!(recognize(&draw_s(300.0, 2.5, 0.0, false, 60)), Some('S'));
        assert_eq!(recognize(&draw_s(320.0, 0.9, 0.0, false, 24)), Some('S'));
    }

    #[test]
    fn rejects_straight_lines() {
        for pts in [
            vec![(0.0, 0.0), (400.0, 0.0)],
            vec![(0.0, 0.0), (0.0, 400.0)],
            vec![(0.0, 0.0), (300.0, 300.0)],
            vec![(0.0, 0.0), (300.0, 60.0)],
        ] {
            assert_eq!(recognize(&polyline(&pts, 40)), None, "{pts:?}");
        }
    }

    #[test]
    fn rejects_polygons() {
        // Z（排除手势）
        let z = [(0.0, 0.0), (300.0, 0.0), (0.0, 300.0), (300.0, 300.0)];
        assert_eq!(recognize(&polyline(&z, 40)), None);
        // L
        let l = [(0.0, 0.0), (0.0, 300.0), (300.0, 300.0)];
        assert_eq!(recognize(&polyline(&l, 40)), None);
        // 折线 S（老版本的 DLD 画法）：像 S，但笔画是折的
        let zig = [(200.0, 0.0), (0.0, 100.0), (200.0, 200.0), (0.0, 300.0)];
        assert_eq!(recognize(&polyline(&zig, 40)), None);
    }

    #[test]
    fn rejects_loops_and_arcs() {
        // 圆
        let circle: Vec<(i32, i32)> = (0..=72)
            .map(|i| {
                let a = i as f32 / 72.0 * std::f32::consts::TAU;
                (
                    (500.0 + 150.0 * a.cos()) as i32,
                    (400.0 + 150.0 * a.sin()) as i32,
                )
            })
            .collect();
        assert_eq!(recognize(&circle), None);

        // C 形（只有一段弧）
        let c: Vec<(i32, i32)> = (0..=40)
            .map(|i| {
                let a = -PI / 2.5 + i as f32 / 40.0 * (PI * 1.25);
                ((500.0 + 150.0 * a.cos()) as i32, (400.0 + 150.0 * a.sin()) as i32)
            })
            .collect();
        assert_eq!(recognize(&c), None);
    }

    #[test]
    fn rejects_tiny_jitter() {
        let jitter = [(0, 0), (6, 3), (2, 9), (11, 6), (4, 2), (9, 8), (3, 5), (7, 1)];
        assert_eq!(recognize(&jitter), None);
    }
}
