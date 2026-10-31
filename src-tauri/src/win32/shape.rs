//! 形状手势识别 —— 真正"画出来"的曲线，而不是方向串。
//!
//! 方向串（U/D/L/R）对折线手势足够用，但表达不了"S"这种东西：
//! 用 `DLD` 去近似，用户得刻意画成一段折线，手感完全不对。
//!
//! 第一版"和标准 S 逐点比距离"在真机上一划就废，两个原因：
//!   1. 手写 S 几乎都斜（右上起笔、左下收笔），不校正斜度，距离凭空大一截；
//!   2. 只放了一个模板，而且那个模板其实是"正弦式"（起笔在正上方），
//!      和真人写的"字体式 S"（起笔在右上角）根本不是一条线。
//!
//! 现在：
//!   1. 抖点丢掉，按弧长重采样、平滑两遍；
//!   2. **把首尾连线转到竖直**再比 —— 斜着写、整体旋转都不影响；
//!   3. **两个模板都放**（字体式 / 正弦式），正反两个方向都比，取最小距离；
//!   4. 几何闸门先挡掉直线、画圈、太小的抖动，距离只负责最后判"像不像 S"。
//!
//! 只认 S 一个形状：用户真正需要的是"画个 S 截图"。多加形状（O、C、W）
//! 会和已有的方向串抢手势，误识别一次就够让人不信任整个功能了。

use std::f32::consts::PI;

/// 重采样点数。太少糊成一团，太多对抖动敏感。
const SAMPLES: usize = 32;

/// 弦对齐后的平均点距上限（0 = 完全重合，1 左右 = 毫不相干）。
/// 实测：手写式 S（含抖动、斜写、不对称）≤0.15；Z / U / V / 圆 / 单段弧 ≥0.72。
/// 折线版的 S 是 0.24 —— 老版本就是用折线画 S 的，认了它正好兼容肌肉记忆。
const MATCH_DIST: f32 = 0.28;

/// 自定义手势的命中阈值（规范笔画平均点距）。
/// 实测：同一手势重复画（含抖动）<0.12；不同手势 L/V/M 之间 >0.2。
pub const CUSTOM_MATCH: f32 = 0.16;

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
    // 首尾挨着 = 画了个圈
    let start = pts[0];
    let end = *pts.last()?;
    if dist(start, end) < 0.5 * h {
        return None;
    }
    let path = smooth(&resample(&pts, SAMPLES));
    let aligned = normalize(&align_down(&path));
    if best_distance(&aligned) > MATCH_DIST {
        return None;
    }
    Some('S')
}

/// 诊断用：把这一笔的各项指标拼成一行，认不出来时写进日志。
///
/// 形状手势没法靠"再试一次"猜 —— 用户每次画的大小、斜度、快慢都不同，
/// 只有把数字记下来才知道是尺寸不够、还是压根不像 S。
pub fn diagnose(raw: &[(i32, i32)]) -> String {
    let pts = thin(raw, 2.0);
    if pts.len() < 3 {
        return format!("点太少（{} 个）", pts.len());
    }
    let len = length(&pts);
    let (w, h) = extent(&pts);
    let chord = dist(pts[0], *pts.last().unwrap());
    let path = smooth(&resample(&pts, SAMPLES));
    let aligned = normalize(&align_down(&path));
    format!(
        "点 {} 长 {len:.0} 外框 {w:.0}x{h:.0} 弦 {chord:.0} 距离 {:.2}",
        pts.len(),
        best_distance(&aligned)
    )
}

/// 相邻两个点太近就丢：手抖和 1000Hz 回报会灌进来一堆重复点
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

/// 三点滑动平均跑两遍。手画的线一定有毛刺，不抹一下距离会被抖动带偏。
fn smooth(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut cur = p.to_vec();
    for _ in 0..2 {
        let src = cur.clone();
        for i in 1..cur.len() - 1 {
            cur[i] = (
                (src[i - 1].0 + src[i].0 + src[i + 1].0) / 3.0,
                (src[i - 1].1 + src[i].1 + src[i + 1].1) / 3.0,
            );
        }
    }
    cur
}

/// 把首尾连线（弦）转到竖直向下，中心移到原点。
///
/// 这一步解决"斜着写"：手写 S 几乎都是往一边倾的，不校正的话
/// 逐点距离会凭空大一截（第一版就栽在这里）。
fn align_down(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let (x0, y0) = p[0];
    let (x1, y1) = *p.last().unwrap_or(&p[0]);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    // 屏幕坐标里 +y 是向下，所以"朝下"对应的角度是 +90°
    let a = PI / 2.0 - (y1 - y0).atan2(x1 - x0);
    let (s, c) = (a.sin(), a.cos());
    p.iter()
        .map(|&(x, y)| {
            let (px, py) = (x - cx, y - cy);
            (px * c - py * s, px * s + py * c)
        })
        .collect()
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
    let sx = ((x1 - x0) / 2.0).abs().max(1e-3);
    let sy = ((y1 - y0) / 2.0).abs().max(1e-3);
    p.iter().map(|&(x, y)| ((x - cx) / sx, (y - cy) / sy)).collect()
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

/// 和模板比距离：两个模板、正反两个方向，取最小。
fn best_distance(path: &[(f32, f32)]) -> f32 {
    templates()
        .iter()
        .map(|t| mean_dist(path, t).min(mean_dist(path, &reversed(t))))
        .fold(f32::MAX, f32::min)
}

/// S 的两种写法都得认：
///   - **字体式**：右上角起笔、左下角收笔（和字库里的 S 一样，弦是斜的）；
///   - **正弦式**：正上方起笔，上下拉满，弦是竖直的。
/// 只放一个模板，另一种就永远认不出来 —— 第一版只放了正弦式，真人写的全认不出。
fn templates() -> Vec<Vec<(f32, f32)>> {
    // 不放镜像版（"Ƨ"）：试过，Z 的距离会从 0.79 掉到 0.22 ——
    // 那正好是程序排除手势，为兼容一个没人会写的方向把排除手势抢走不值得。
    vec![letter_s(), wave_s()]
}

/// 字体式 S：一条三次贝塞尔，起笔右上、收笔左下
fn letter_s() -> Vec<(f32, f32)> {
    let (p0, p1) = ((0.6f32, -1.0f32), (-1.0f32, -0.6f32));
    let (p2, p3) = ((1.0f32, 0.6f32), (-0.6f32, 1.0f32));
    let mut out = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let t = i as f32 / (SAMPLES - 1) as f32;
        let u = 1.0 - t;
        let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push((
            b0 * p0.0 + b1 * p1.0 + b2 * p2.0 + b3 * p3.0,
            b0 * p0.1 + b1 * p1.1 + b2 * p2.1 + b3 * p3.1,
        ));
    }
    canonical(&out)
}

/// 正弦式 S：上下两段半圆，起笔收笔都在正中间
fn wave_s() -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let t = i as f32 / (SAMPLES - 1) as f32;
        let p = if t < 0.5 {
            let a = t / 0.5 * PI;
            (-0.5 * a.sin(), -0.5 - 0.5 * a.cos())
        } else {
            let a = (t - 0.5) / 0.5 * PI;
            (0.5 * a.sin(), 0.5 * (1.0 - a.cos()))
        };
        out.push(p);
    }
    canonical(&out)
}

/// 模板也要走和用户轨迹一样的规范化：弦转到竖直 + 逐轴归一化。
/// 两边流程不一致的话，比出来的距离没有意义。
fn canonical(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    normalize(&align_down(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只看距离，绕过前面的几何闸门（调阈值用）
    fn score(raw: &[(i32, i32)]) -> f32 {
        let pts = thin(raw, 2.0);
        let path = smooth(&resample(&pts, SAMPLES));
        best_distance(&normalize(&align_down(&path)))
    }

    /// 字体式 S：右上起笔、左下收笔（真人手写的样子）
    fn letter_s_path(h: f32, ratio: f32, noise: f32, n: i32) -> Vec<(i32, i32)> {
        let (p0, p1) = ((0.6f32, -1.0f32), (-1.0f32, -0.6f32));
        let (p2, p3) = ((1.0f32, 0.6f32), (-0.6f32, 1.0f32));
        let mut v = Vec::new();
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            let x = b0 * p0.0 + b1 * p1.0 + b2 * p2.0 + b3 * p3.0;
            let y = b0 * p0.1 + b1 * p1.1 + b2 * p2.1 + b3 * p3.1;
            let jx = ((i * 7919) % 13) as f32 / 13.0 - 0.5;
            let jy = ((i * 104_729) % 11) as f32 / 11.0 - 0.5;
            v.push((
                (500.0 + x * h * ratio * 0.5 + jx * 2.0 * noise) as i32,
                (400.0 + y * h * 0.5 + jy * 2.0 * noise) as i32,
            ));
        }
        v
    }

    /// 正弦式 S：从正上方起笔（上半往左鼓、下半往右鼓，和字库里的 S 同向）
    fn wave_s_path(h: f32, ratio: f32, noise: f32, bottom_up: bool, n: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let t = if bottom_up { 1.0 - t } else { t };
            let (x, y) = if t < 0.5 {
                let a = t / 0.5 * PI;
                (-0.5 * a.sin() * h * ratio, -0.5 - 0.5 * a.cos())
            } else {
                let a = (t - 0.5) / 0.5 * PI;
                (0.5 * a.sin() * h * ratio, 0.5 * (1.0 - a.cos()))
            };
            let jx = ((i * 7919) % 13) as f32 / 13.0 - 0.5;
            let jy = ((i * 104_729) % 11) as f32 / 11.0 - 0.5;
            v.push((
                (500.0 + x + jx * 2.0 * noise) as i32,
                (400.0 + y * h / 2.0 + jy * 2.0 * noise) as i32,
            ));
        }
        v
    }

    /// 整体旋转（模拟"斜着写"）
    fn rotated(pts: Vec<(i32, i32)>, deg: f32) -> Vec<(i32, i32)> {
        let (s, c) = (deg.to_radians().sin(), deg.to_radians().cos());
        pts.iter()
            .map(|&(x, y)| {
                let (px, py) = (x as f32 - 500.0, y as f32 - 400.0);
                ((500.0 + px * c - py * s) as i32, (400.0 + px * s + py * c) as i32)
            })
            .collect()
    }

    /// 手画的不对称 S：上下两半半径不同、中间被手快画成直线
    fn irregular() -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for i in 0..=60 {
            let t = i as f32 / 60.0;
            let (x, y) = if t < 0.45 {
                let a = t / 0.45 * PI;
                (-0.5 * a.sin() * 1.15, -0.5 - 0.5 * a.cos())
            } else if t < 0.55 {
                let k = (t - 0.45) / 0.1;
                (-0.05 + 0.15 * k, -0.12 + 0.24 * k)
            } else {
                let a = (t - 0.55) / 0.45 * PI;
                (0.5 * a.sin() * 0.85, 0.5 * (1.0 - a.cos()))
            };
            v.push(((400.0 + x * 260.0) as i32, (300.0 + y * 210.0) as i32));
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
    fn recognizes_letter_s() {
        // 真人写的那种：右上起笔
        assert_eq!(recognize(&letter_s_path(300.0, 0.7, 0.0, 60)), Some('S'));
        assert_eq!(recognize(&letter_s_path(180.0, 0.9, 0.0, 40)), Some('S'));
        assert_eq!(recognize(&letter_s_path(420.0, 0.6, 0.0, 80)), Some('S'));
        assert_eq!(recognize(&letter_s_path(90.0, 0.8, 0.0, 40)), Some('S'));
        // 手抖
        assert_eq!(recognize(&letter_s_path(300.0, 0.7, 6.0, 60)), Some('S'));
        assert_eq!(recognize(&letter_s_path(300.0, 0.7, 12.0, 60)), Some('S'));
        // 斜着写（手写 S 基本都斜）
        for deg in [-25.0, -10.0, 10.0, 25.0] {
            let p = rotated(letter_s_path(300.0, 0.7, 4.0, 60), deg);
            assert_eq!(recognize(&p), Some('S'), "斜 {deg}°：{}", diagnose(&p));
        }
        // 上下不对称 + 中间拉直
        let p = irregular();
        assert_eq!(recognize(&p), Some('S'), "{}", diagnose(&p));
    }

    #[test]
    fn recognizes_wave_s() {
        assert_eq!(recognize(&wave_s_path(320.0, 0.6, 0.0, false, 80)), Some('S'));
        assert_eq!(recognize(&wave_s_path(240.0, 1.0, 0.0, false, 60)), Some('S'));
        assert_eq!(recognize(&wave_s_path(300.0, 0.8, 0.0, true, 80)), Some('S'));
        assert_eq!(recognize(&wave_s_path(120.0, 0.7, 0.0, false, 40)), Some('S'));
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
        // Z（程序排除手势）
        let z = [(0.0, 0.0), (300.0, 0.0), (0.0, 300.0), (300.0, 300.0)];
        assert_eq!(recognize(&polyline(&z, 40)), None);
        // L
        let l = [(0.0, 0.0), (0.0, 300.0), (300.0, 300.0)];
        assert_eq!(recognize(&polyline(&l, 40)), None);
        // U
        let u = [(0.0, 0.0), (0.0, 250.0), (250.0, 250.0), (250.0, 0.0)];
        assert_eq!(recognize(&polyline(&u, 40)), None);
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

        // C 形
        let c: Vec<(i32, i32)> = (0..=40)
            .map(|i| {
                let a = -PI / 2.5 + i as f32 / 40.0 * (PI * 1.25);
                ((500.0 + 150.0 * a.cos()) as i32, (400.0 + 150.0 * a.sin()) as i32)
            })
            .collect();
        assert_eq!(recognize(&c), None);

        // 一个周期的波浪（横着划）
        let wave: Vec<(i32, i32)> = (0..=80)
            .map(|i| {
                let t = i as f32 / 80.0;
                (
                    (300.0 + t * 300.0) as i32,
                    (400.0 - (t * std::f32::consts::TAU).sin() * 80.0) as i32,
                )
            })
            .collect();
        assert_eq!(recognize(&wave), None);
    }

    #[test]
    fn rejects_tiny_jitter() {
        let jitter = [(0, 0), (6, 3), (2, 9), (11, 6), (4, 2), (9, 8), (3, 5), (7, 1)];
        assert_eq!(recognize(&jitter), None);
    }

    /// 调阈值用：把各用例的距离打出来看分离度
    #[test]
    fn metrics() {
        let pos = [
            ("letters300", letter_s_path(300.0, 0.7, 0.0, 60)),
            ("letters300n12", letter_s_path(300.0, 0.7, 12.0, 60)),
            ("letters180", letter_s_path(180.0, 0.9, 0.0, 40)),
            ("letters90", letter_s_path(90.0, 0.8, 0.0, 40)),
            ("letters_wide", letter_s_path(260.0, 1.3, 0.0, 60)),
            ("letters_thin", letter_s_path(360.0, 0.3, 0.0, 60)),
            ("letters_slant25", rotated(letter_s_path(300.0, 0.7, 3.0, 60), 25.0)),
            ("line_diag", polyline(&[(0.0, 0.0), (300.0, 300.0)], 40)),
            ("line_wiggle", polyline(&[(0.0, 0.0), (150.0, 8.0), (300.0, 0.0)], 40)),
            ("irregular", irregular()),
            ("wave320", wave_s_path(320.0, 0.6, 0.0, false, 80)),
        ];
        for (name, p) in pos {
            println!("{name}: {:.2} => {}", score(&p), recognize(&p).is_some());
        }
        let neg = [
            ("Z", polyline(&[(0.0, 0.0), (300.0, 0.0), (0.0, 300.0), (300.0, 300.0)], 40)),
            ("L", polyline(&[(0.0, 0.0), (0.0, 300.0), (300.0, 300.0)], 40)),
            ("U", polyline(&[(0.0, 0.0), (0.0, 250.0), (250.0, 250.0), (250.0, 0.0)], 40)),
            ("V", polyline(&[(0.0, 0.0), (150.0, 300.0), (300.0, 0.0)], 40)),
            ("zig", polyline(&[(200.0, 0.0), (0.0, 100.0), (200.0, 200.0), (0.0, 300.0)], 40)),
            (
                "5",
                polyline(
                    &[
                        (300.0, 0.0),
                        (0.0, 0.0),
                        (0.0, 150.0),
                        (260.0, 150.0),
                        (300.0, 300.0),
                        (0.0, 300.0),
                    ],
                    40,
                ),
            ),
        ];
        for (name, p) in neg {
            println!("{name}: {:.2} => {}", score(&p), recognize(&p).is_some());
        }
        let circle: Vec<(i32, i32)> = (0..=72)
            .map(|i| {
                let a = i as f32 / 72.0 * std::f32::consts::TAU;
                (
                    (500.0 + 150.0 * a.cos()) as i32,
                    (400.0 + 150.0 * a.sin()) as i32,
                )
            })
            .collect();
        println!("circle: {:.2} => {}", score(&circle), recognize(&circle).is_some());
    }
}

/* ---------------- 自定义手势（用户自己画一笔当模板） ---------------- */

/// 自定义手势的笔画规范化：等距重采样 → 平移居中 → **等比**缩放到长边为 1。
///
/// 为什么等比而不是逐轴拉满：逐轴拉满会把"竖 + 横"的 L 和"斜 + 斜"的 V
/// 拉得一样，两个手势就分不开了。等比缩放只忽略大小，保留形状比例。
pub fn canonical_stroke(raw: &[(i32, i32)]) -> Vec<[f32; 2]> {
    let pts = thin(raw, 2.0);
    if pts.len() < 3 {
        return Vec::new();
    }
    let rs = resample(&pts, SAMPLES);
    let (w, h) = extent(&rs);
    let scale = w.max(h).max(1e-3);
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &(x, y) in &rs {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    rs.iter()
        .map(|&(x, y)| [(x - cx) / scale, (y - cy) / scale])
        .collect()
}

/// 两条规范笔画的平均点距（0 = 完全重合）。点数不同返回最大值。
pub fn stroke_distance(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::MAX;
    }
    let sum: f32 = a
        .iter()
        .zip(b.iter())
        .map(|(p, q)| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt())
        .sum();
    sum / a.len() as f32
}

#[cfg(test)]
mod stroke_tests {
    use super::*;

    fn line(a: (f32, f32), b: (f32, f32), n: i32, jitter: f32) -> Vec<(i32, i32)> {
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let j = if jitter > 0.0 {
                    ((i * 7919) % 13) as f32 / 13.0 - 0.5
                } else {
                    0.0
                };
                (
                    (a.0 + (b.0 - a.0) * t + j * jitter) as i32,
                    (a.1 + (b.1 - a.1) * t + j * jitter) as i32,
                )
            })
            .collect()
    }

    fn path(pts: &[(f32, f32)], per: i32, jitter: f32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for w in pts.windows(2) {
            v.extend(line(w[0], w[1], per, jitter));
        }
        v
    }

    #[test]
    fn same_shape_matches() {
        let a = canonical_stroke(&path(&[(0.0, 0.0), (0.0, 200.0), (200.0, 200.0)], 20, 0.0));
        // 同一个 L，画大一倍、换个位置
        let b = canonical_stroke(&path(&[(300.0, 100.0), (300.0, 500.0), (700.0, 500.0)], 20, 0.0));
        assert!(stroke_distance(&a, &b) < 0.02, "{}", stroke_distance(&a, &b));
    }

    #[test]
    fn jitter_still_matches() {
        let a = canonical_stroke(&path(&[(0.0, 0.0), (0.0, 200.0), (200.0, 200.0)], 20, 0.0));
        let b = canonical_stroke(&path(&[(0.0, 0.0), (0.0, 200.0), (200.0, 200.0)], 20, 12.0));
        assert!(stroke_distance(&a, &b) < 0.12, "{}", stroke_distance(&a, &b));
    }

    #[test]
    fn different_shapes_differ() {
        let l = canonical_stroke(&path(&[(0.0, 0.0), (0.0, 200.0), (200.0, 200.0)], 20, 0.0));
        let v = canonical_stroke(&path(&[(0.0, 0.0), (100.0, 200.0), (200.0, 0.0)], 20, 0.0));
        let m = canonical_stroke(&path(
            &[(0.0, 200.0), (0.0, 0.0), (100.0, 150.0), (200.0, 0.0), (200.0, 200.0)],
            20,
            0.0,
        ));
        println!("L-V {:.3} L-M {:.3}", stroke_distance(&l, &v), stroke_distance(&l, &m));
        assert!(stroke_distance(&l, &v) > 0.2);
        assert!(stroke_distance(&l, &m) > 0.2);
    }
}
