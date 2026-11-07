//! 轨迹 → 方向串。
//!
//! **不用逐点判方向**：手画的折线在拐角处会来回抖，逐点判定会把一段切成一堆
//! 废 token（用户画一个三段折线，浮标显示 `↙,↓,→,↓,↙` 五个）。
//! 正确做法是先做折线简化（RDP）只留真正的拐点，再对每段算方向 ——
//! 段数就等于"用户画了几笔"。
//!
//! 住在 core 里是为了能跑单测：主 crate 的 test harness 在本机加载不起来，
//! 而"几段变几 token"正是以前反复出错的地方。

/// 位移 → 8 方向之一（屏幕坐标，y 向下）。
///
/// 每 45° 一个扇区，`R` 是 0°、顺时针：R / DR / D / DL / L / UL / U / UR。
///
/// **不要给主方向加权**。试过"一轴投影超过另一轴 1.6 倍就算主方向"（±32°），
/// 结果把 `↙`（约 60°）判成 `↓`、`↙`（约 27°）判成 `←` —— 斜向手势直接消失。
/// 45° 均分 + 匹配层的容错才是对的。
pub fn dir8(dx: i32, dy: i32) -> &'static str {
    let angle = (dy as f32).atan2(dx as f32);
    let idx = ((angle / (std::f32::consts::PI / 4.0)).round() as i32).rem_euclid(8);
    match idx {
        0 => "R",
        1 => "DR",
        2 => "D",
        3 => "DL",
        4 => "L",
        5 => "UL",
        6 => "U",
        _ => "UR",
    }
}

/// 点到线段的垂距（RDP 用）
fn perp(p: (i32, i32), a: (i32, i32), b: (i32, i32)) -> f32 {
    let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
    let len = (dx * dx + dy * dy).sqrt();
    let (px, py) = ((p.0 - a.0) as f32, (p.1 - a.1) as f32);
    if len < 1e-3 {
        return (px * px + py * py).sqrt();
    }
    ((px * dy - py * dx) / len).abs()
}

/// Ramer–Douglas–Peucker 折线简化：只保留真正的拐点。
///
/// 用显式栈而不是递归：长手势有一千多个点，递归会把栈压爆。
fn rdp(pts: &[(i32, i32)], eps: f32) -> Vec<(i32, i32)> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let mut best = (a, 0.0f32);
        for i in a + 1..b {
            let d = perp(pts[i], pts[a], pts[b]);
            if d > best.1 {
                best = (i, d);
            }
        }
        if best.1 > eps {
            keep[best.0] = true;
            stack.push((a, best.0));
            stack.push((best.0, b));
        }
    }
    (0..n).filter(|&i| keep[i]).map(|i| pts[i]).collect()
}

/// 轨迹 → 方向串（`"D,R"`）。
///
/// 这个函数跑在**钩子线程**里：不分配多余 Vec、没有 `unwrap`、没有 panic 路径 ——
/// 钩子回调里 panic 会连带整条输入链。
pub fn path_to_dir(pts: &[(i32, i32)]) -> String {
    if pts.len() < 3 {
        return String::new();
    }
    // 一次遍历算包围盒
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for &(x, y) in pts {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let span = (max_x - min_x).max(max_y - min_y);
    // 容差跟着手势尺度走：画得大就容忍大一点的抖动
    let eps = (span as f32 * 0.055).clamp(6.0, 22.0);
    let key = rdp(pts, eps);

    let mut toks: Vec<&'static str> = Vec::new();
    for pair in key.windows(2) {
        let dx = pair[1].0 - pair[0].0;
        let dy = pair[1].1 - pair[0].1;
        let len = ((dx * dx + dy * dy) as f32).sqrt();
        // 太短的段丢掉：RDP 之后还可能留下五六个像素的碎段
        if len < 22.0 {
            continue;
        }
        let d = dir8(dx, dy);
        if toks.last() != Some(&d) {
            toks.push(d);
        }
    }
    toks.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 沿两点之间走一条直线，`n` 段。
    fn line(a: (i32, i32), b: (i32, i32), n: usize) -> Vec<(i32, i32)> {
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                (
                    (a.0 as f32 + (b.0 - a.0) as f32 * t).round() as i32,
                    (a.1 as f32 + (b.1 - a.1) as f32 * t).round() as i32,
                )
            })
            .collect()
    }

    /// 把几段折线接起来（去掉重复的接点）。
    fn poly(pts: &[(i32, i32)], n: usize) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for w in pts.windows(2) {
            let seg = line(w[0], w[1], n);
            if out.is_empty() {
                out.extend(seg);
            } else {
                out.extend(seg.into_iter().skip(1));
            }
        }
        out
    }

    #[test]
    fn dir8_covers_eight_sectors() {
        assert_eq!(dir8(100, 0), "R");
        assert_eq!(dir8(100, 100), "DR");
        assert_eq!(dir8(0, 100), "D");
        assert_eq!(dir8(-100, 100), "DL");
        assert_eq!(dir8(-100, 0), "L");
        assert_eq!(dir8(-100, -100), "UL");
        assert_eq!(dir8(0, -100), "U");
        assert_eq!(dir8(100, -100), "UR");
    }

    /// 22.5° 是扇区边界：刚好在边界两侧要落到不同方向。
    #[test]
    fn dir8_boundary_is_45_degrees_split() {
        // 22°：还在 R 的扇区里
        assert_eq!(dir8(100, 39), "R");
        // 26°：已经算 DR
        assert_eq!(dir8(100, 47), "DR");
    }

    #[test]
    fn straight_lines_are_one_token() {
        assert_eq!(path_to_dir(&line((0, 0), (200, 0), 40)), "R");
        assert_eq!(path_to_dir(&line((0, 0), (0, 200), 40)), "D");
        assert_eq!(path_to_dir(&line((0, 200), (0, 0), 40)), "U");
        assert_eq!(path_to_dir(&line((200, 0), (0, 0), 40)), "L");
        // 45° 斜线是一条 DR
        assert_eq!(path_to_dir(&line((0, 0), (200, 200), 40)), "DR");
    }

    /// 直角折线：两段就是两个 token。
    #[test]
    fn corner_gives_two_tokens() {
        let p = poly(&[(0, 0), (200, 0), (200, 200)], 30);
        assert_eq!(path_to_dir(&p), "R,D");
        let p = poly(&[(0, 0), (0, 200), (200, 200)], 30);
        assert_eq!(path_to_dir(&p), "D,R");
    }

    /// 截图手势：↙ ↘ ↙ 三段。
    #[test]
    fn three_segment_s_shape() {
        let p = poly(&[(200, 0), (0, 200), (200, 400), (0, 600)], 30);
        assert_eq!(path_to_dir(&p), "DL,DR,DL");
    }

    /// 手抖：一条直线上的锯齿不该切成多个 token。
    #[test]
    fn jitter_on_a_line_stays_one_token() {
        let mut p = line((0, 0), (400, 0), 80);
        // 每隔几个点抖 ±6px（远小于 eps）
        for (i, pt) in p.iter_mut().enumerate() {
            if i % 3 == 0 {
                pt.1 += if i % 2 == 0 { 6 } else { -6 };
            }
        }
        assert_eq!(path_to_dir(&p), "R");
    }

    /// 同名方向相邻要合并：抖出一段又回到原方向，不该变成 R,R。
    #[test]
    fn same_direction_is_merged() {
        let mut p = line((0, 0), (300, 0), 30);
        p.extend(line((300, 30), (600, 30), 30).into_iter().skip(1));
        assert_eq!(path_to_dir(&p), "R");
    }

    /// 太短的轨迹不出 token（交给调用方按"单击"处理）。
    #[test]
    fn tiny_paths_yield_nothing() {
        assert_eq!(path_to_dir(&[]), "");
        assert_eq!(path_to_dir(&[(0, 0)]), "");
        assert_eq!(path_to_dir(&[(0, 0), (5, 0)]), "");
        // 20px 的抖动：RDP 过完之后段长不足 22px，同样不算
        assert_eq!(path_to_dir(&line((0, 0), (20, 0), 10)), "");
    }
}
