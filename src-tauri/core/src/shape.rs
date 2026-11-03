//! 形状手势识别 —— 真正"画出来"的曲线，而不是方向串。
//!
//! 方向串（U/D/L/R）对折线手势够用，但表达不了 "S" 这种东西：用 `DLD` 去近似，
//! 用户得刻意画成一段折线，手感完全不对。
//!
//! # 为什么用点云（$P）而不是逐点比
//!
//! 前两版都是"按弧长重采样成 32 点，然后和第 1 点比第 1 点、第 2 点比第 2 点"。
//! 这条路走不通：**同一个字，画宽一点，32 个点落在哪就全变了**。窄高 H 的
//! 32 点里左腿占 9 个，宽扁 H 里左腿占 13 个 —— 序号对不上，逐点距离白算。
//!
//! 于是只能给每个字补一套"矫正"：S 要把首尾连线转到竖直（手写 S 都斜），
//! H 要先把宽高拉成正方形再采样。补丁越加越多，而且每加一个字都得再想一套。
//!
//! 现在换成 $P（Vatavu 2012, "Gesture Recognition Without Training"）的做法：
//! 点与点之间**没有对应关系**，只问"这两堆点有多远"。宽窄、起笔位置、
//! 正反着画全都不影响，不需要任何按形状定制的归一化。
//!
//! 归一化只剩一步：重采样 → 逐轴缩放到 [-1,1] → 居中。没有旋转
//! （转 90° 的 H 不该还叫 H）。
//!
//! 参考过的实现里，$1 靠"转到 indicative angle + 逐点比"、Easystroke 靠
//! "等比缩放 + 动态规划"、GestureSign / Stroke 靠"角度序列"；它们是各自场景
//! 下的合理选择，但都需要为"形状"再补一步。$P 是唯一一个把这一步彻底消掉的。
//!
//! # 认哪些形状
//!
//! 一笔画能写出来的字母基本都认（A–Z，除去 I / O / Q 这几个一笔画不自然或
//! 和方向串撞的），外加 `S` / `H` 的多种写法模板。
//!
//! **形状和方向串是两个命名空间**：方向用 `U/D/L/R`（`DR`、`DUD`），形状用
//! `@` 前缀（`@W`、`@D`）。字母 D/L/R/U 和方向同名，不隔离就没法既支持
//! "向下划"又支持"画一个 D"。这也是主流工具的做法 —— StrokeIt 内置
//! "画 L 锁定电脑"的同时保留"右→左 = 后退"，因为识别在**形状层面**做，
//! 字符只是给形状起的名字。
//!
//! 匹配顺序：形状优先（阈值严格），形状没绑动作时回落到方向串。
//! 所以画 Z 会被认成 `@Z`，但用户没绑 `@Z` 时会自动落回 `RLR` ——
//! "画 Z 排除当前程序"照常工作。
//!
//! 加一个字母只需要在 `LETTERS` 里加一行，`letters_recognize_themselves`
//! 会验证它认得出自己、也不抢别人。

use std::f32::consts::PI;
use std::sync::OnceLock;

/// 重采样点数。太少糊成一团，太多对抖动敏感。
const SAMPLES: usize = 32;

/// S 的点云距离上限（原始距离，坐标已归一化到 [-1,1]²）。
///
/// 实测（`tests/shape_probe.py`）：正例 ≤0.072（含 12px 抖动、斜写 25°、
/// 宽扁 2.5 / 瘦高 0.3）；最近的负例是"日"字 0.136，Z / C / 圆 / 波浪 ≥0.15。
const CLOUD_S_MAX: f32 = 0.10;

/// H 的点云距离上限。
///
/// 比 S 紧：H 是"两条腿 + 一道中横"，点云上最容易和 U 撞（0.087）。
/// 正例 ≤0.065（最差是 200x520 的极扁 H，典型 ≤0.043）。
const CLOUD_H_MAX: f32 = 0.075;

/// 自定义手势的命中阈值（规范笔画平均点距）。
/// 实测：同一手势重复画（含抖动）<0.12；不同手势 L/V/M 之间 >0.2。
pub const CUSTOM_MATCH: f32 = 0.16;

/// 内置字母的统一阈值。
///
/// 字母之间比 S/H 之间更容易撞（V/U、M/W、N/Z 都是"两笔或三笔的对称形状"），
/// 所以取值比 S（0.10）紧一点。`letter_matrix_is_separable` 这个测试守着它：
/// 每个字母的抖动变体都必须认回自己，且和别的字母有明确余量。
const CLOUD_LETTER_MAX: f32 = 0.095;


/// 单笔最多留这么多点（钩子里兜底，防止手抖记录几万个点）
pub const MAX_POINTS: usize = 1200;

/// 认一下这一笔是不是某个内置形状。是就返回形状字符（`'S'` / `'H'`）。
///
/// 每个形状用自己的阈值，先归一化成 `距离 / 阈值` 再比 —— 否则阈值宽的那个
/// 永远赢，会把它自己的"有点像"全抢走。
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
    // 首尾挨着 = 画了个圈。按短边比而不是高度：H 收在右上角时弦 ≈ 宽度。
    //
    // 阈值从 0.5 收到 0.25：P 是"竖 + 半圆回到竖线"，首尾只隔半个高度
    // （实测弦 143 / 短边 296 = 0.48），用 0.5 会把 P 直接判死 ——
    // 它的点云距离只有 0.016，明明认得很准。真正的圈首尾几乎重合
    // （通常 <0.15），0.25 仍然挡得住。
    if dist(pts[0], *pts.last()?) < 0.25 * w.min(h) {
        return None;
    }

    builtins()
        .iter()
        .map(|s| (s.score(&pts), s.ch))
        .filter(|(ratio, _)| *ratio <= 1.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, ch)| ch)
}

/// 诊断用：把这一笔的各项指标拼成一行，认不出来时写进日志。
///
/// 形状手势没法靠"再试一次"猜 —— 用户每次画的大小、宽窄、快慢都不同，
/// 只有把数字记下来才知道是尺寸不够、还是压根不像。
pub fn diagnose(raw: &[(i32, i32)]) -> String {
    let pts = thin(raw, 2.0);
    if pts.len() < 3 {
        return format!("点太少（{} 个）", pts.len());
    }
    let len = length(&pts);
    let (w, h) = extent(&pts);
    let chord = dist(pts[0], *pts.last().unwrap_or(&pts[0]));
    let scores: Vec<String> = builtins()
        .iter()
        .map(|s| format!("{} {:.3}/{:.3}", s.ch, s.raw_score(&pts), s.max_dist))
        .collect();
    format!(
        "点 {} 长 {len:.0} 外框 {w:.0}x{h:.0} 弦 {chord:.0} | 点云 {}",
        pts.len(),
        scores.join(" ")
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

/* ---------------- 点云匹配（$P） ---------------- */

/// 点云唯一的规范化：重采样 → 平滑 → 逐轴缩放到方 → 居中。
/// 没有旋转，也没有按形状定制的"先拉方"之类的补丁。
fn cloud_canonical(raw: &[(f32, f32)]) -> Vec<(f32, f32)> {
    normalize(&smooth(&resample(raw, SAMPLES)))
}

/// 两个点云之间的最小加权配对距离，已按点数归一化。
///
/// 这就是 $P 的核心：点与点之间**不需要有对应关系**，只问"这堆点到那堆点
/// 有多远"。所以宽窄、起笔位置、正反着画都不影响 —— 那正是之前给 S/H 各写
/// 一套归一化想达到的效果，点云天生就有。
///
/// 和论文一致的三处细节：
///   - 起手点轮换（step = √n）、双向都算，取最小 —— 抗"从哪儿起笔"；
///   - 配对时每次挑最近的未匹配点，权重随序号线性递减 —— 抗"画到一半改道"；
///   - 结果除以点数，得到"平均每点偏移多少"，和形状大小无关。
fn cloud_distance(a: &[(f32, f32)], b: &[(f32, f32)]) -> f32 {
    let n = a.len().min(b.len());
    if n < 3 {
        return f32::MAX;
    }
    let step = (n as f32).sqrt().floor().max(1.0) as usize;
    let mut best = f32::MAX;
    let mut start = 0usize;
    while start < n {
        best = best.min(cloud_from(a, b, start)).min(cloud_from(b, a, start));
        start += step;
    }
    best / n as f32
}

/// 从 `start` 开始的一次贪心配对（返回加权总和，未除以点数）
fn cloud_from(a: &[(f32, f32)], b: &[(f32, f32)], start: usize) -> f32 {
    let n = a.len().min(b.len());
    let mut matched = vec![false; n];
    let mut sum = 0.0;
    let mut i = start;
    let mut taken = 0usize;
    loop {
        let mut best = f32::MAX;
        let mut pick = 0usize;
        for j in 0..n {
            if matched[j] {
                continue;
            }
            let d = dist(a[i], b[j]);
            if d < best {
                best = d;
                pick = j;
            }
        }
        matched[pick] = true;
        // 越往后越"没得挑"，权重从 1 线性降到 1/n
        sum += (1.0 - taken as f32 / n as f32) * best;
        taken += 1;
        i = (i + 1) % n;
        if i == start {
            break;
        }
    }
    sum
}

/* ---------------- 内置形状注册表 ---------------- */

/// 一个内置形状：字符 + 点云模板 + 距离上限。
///
/// 只有这三个字段 —— 换方案之前这里还有 `chord_aligned`、两套模板、
/// 两套规范化，每加一个字都要再想一套规则。
struct Builtin {
    ch: char,
    max_dist: f32,
    templates: Vec<Vec<(f32, f32)>>,
}

impl Builtin {
    /// 到最近模板的原始点云距离（诊断和调阈值看这个）
    fn raw_score(&self, raw: &[(f32, f32)]) -> f32 {
        let p = cloud_canonical(raw);
        self.templates
            .iter()
            .map(|t| cloud_distance(&p, t))
            .fold(f32::MAX, f32::min)
    }

    /// 归一化到阈值：1.0 = 正好卡在线上，越小越像
    fn score(&self, raw: &[(f32, f32)]) -> f32 {
        self.raw_score(raw) / self.max_dist
    }
}

/// 一笔画字母模板。屏幕坐标系（y 向下为正），范围 [-1,1]²。
///
/// # 为什么字母用 `@` 前缀
///
/// 方向串用 `U/D/L/R` 四个字符，而字母里也有 D、L、R、U。同一个字符不能
/// 既表示"向下划"又表示"画一个 D"，所以**形状字符统一加 `@` 前缀**
/// （`@D` / `@W`），方向串保持原样（`DR` / `DUD`）。
///
/// 这样两套体系就能共存 —— 也是主流工具的做法（StrokeIt 内置"画 L 锁定电脑"，
/// 同时保留"右→左 = 后退"）。它们的共同点是：**识别在形状层面做，
/// 字符只是给形状起的名字**，不会因为"名字撞了"就放弃一个形状。
///
/// 匹配顺序：形状先（严格阈值），未命中或形状没绑定动作时回落到方向串。
/// 所以画 Z 仍能触发"排除当前程序"（`RLR`）—— `@Z` 没绑定时自动回落。
///
/// 没有 I 和 O：I 就是一条竖线（等于方向串 U/D），O 是闭合圆 ——
/// `recognize` 里"首尾挨着 = 画了个圈"那条规则专门挡它，避免和别的圈形撞。
const LETTERS: &[(char, &[(f32, f32)])] = &[
    ('A', &[(-1.0, 1.0), (0.0, -1.0), (1.0, 1.0), (-0.5, 0.35), (0.5, 0.35)]),
    // B 是"竖 + 上弧 + 下弧"，真人一笔画会来回提，模板照这个路径走
    ('B', &[
        (-1.0, -1.0), (-1.0, 1.0),
        (-0.3, -0.8), (0.5, -0.45), (0.55, 0.0), (-0.3, 0.4),
        (-1.0, 0.1),
        (0.5, 0.55), (0.6, 0.85), (-1.0, 1.0),
    ]),
    ('C', &[(-0.15, -1.0), (-0.8, -0.65), (-1.0, -0.15), (-0.8, 0.5), (-0.15, 0.95)]),
    // D：竖 + 半圆。和"向下划一条直线"（方向串 D）形状差得远，点云不会混
    ('D', &[
        (-1.0, -1.0), (-1.0, 1.0),
        (0.15, -0.85), (0.7, -0.4), (0.85, 0.0), (0.7, 0.4), (0.15, 0.85),
        (-1.0, 1.0),
    ]),
    ('E', &[
        (-1.0, -1.0), (-1.0, 1.0), (0.85, 1.0),
        (-1.0, 1.0), (-1.0, 0.0), (0.7, 0.0),
        (-1.0, 0.0), (-1.0, -1.0), (0.9, -1.0),
    ]),
    ('F', &[(-1.0, 1.0), (-1.0, -1.0), (0.9, -1.0), (-1.0, -1.0), (-1.0, 0.0), (0.6, 0.0)]),
    ('G', &[
        (-0.1, -1.0), (-0.75, -0.65), (-1.0, 0.0), (-0.75, 0.65), (-0.1, 1.0),
        (0.7, 0.6), (0.7, 0.15), (0.1, 0.15),
    ]),
    ('J', &[(-0.6, -1.0), (0.3, -1.0), (0.3, 0.5), (0.15, 0.85), (-0.35, 0.9), (-0.7, 0.55)]),
    ('K', &[(-1.0, -1.0), (-1.0, 1.0), (-1.0, 0.0), (0.8, -1.0), (-1.0, 0.0), (0.8, 1.0)]),
    ('L', &[(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]),
    ('M', &[(-1.0, 1.0), (-1.0, -1.0), (0.0, 0.5), (1.0, -1.0), (1.0, 1.0)]),
    ('N', &[(-1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (1.0, -1.0)]),
    ('P', &[(-1.0, 1.0), (-1.0, -1.0), (0.5, -0.85), (0.8, -0.4), (0.5, 0.0), (-1.0, 0.0)]),
    // R：P 的弧形收回之后接一条斜腿
    ('R', &[
        (-1.0, 1.0), (-1.0, -1.0),
        (0.5, -0.85), (0.8, -0.4), (0.5, 0.0), (-1.0, 0.0),
        (-1.0, 0.15), (0.8, 1.0),
    ]),
    ('T', &[(-1.0, -1.0), (1.0, -1.0), (0.0, -1.0), (0.0, 1.0)]),
    ('U', &[
        (-1.0, -1.0), (-1.0, 0.45), (-0.55, 0.9), (0.0, 1.0), (0.55, 0.9), (1.0, 0.45), (1.0, -1.0),
    ]),
    ('V', &[(-1.0, -1.0), (0.0, 1.0), (1.0, -1.0)]),
    ('W', &[(-1.0, -1.0), (-0.55, 1.0), (0.0, -0.35), (0.55, 1.0), (1.0, -1.0)]),
    ('X', &[(-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0)]),
    ('Y', &[(-1.0, -1.0), (0.0, 0.0), (1.0, -1.0), (0.0, 0.0), (0.0, 1.0)]),
    ('Z', &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]),
];

fn builtins() -> &'static [Builtin] {
    static CACHE: OnceLock<Vec<Builtin>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut out = vec![
            Builtin {
                ch: 'S',
                max_dist: CLOUD_S_MAX,
                // 两种写法：字体式（右上起笔）和正弦式（正上方起笔）。
                // 真人写的几乎都是前者，但两种都放进模板成本极低。
                // 另加 ±10° 斜写：手写不会摆得笔直，缺这两个模板时
                // 斜 -10° 的距离是 0.119（阈值 0.10），会"画对了却不认"。
                templates: vec![
                    cloud_canonical(&letter_s_curve()),
                    cloud_canonical(&wave_s_curve()),
                    cloud_canonical(&slanted_s_curve(-10.0)),
                    cloud_canonical(&slanted_s_curve(10.0)),
                ],
            },
            Builtin {
                ch: 'H',
                max_dist: CLOUD_H_MAX,
                templates: h_shapes().iter().map(|p| cloud_canonical(p)).collect(),
            },
        ];
        for (ch, path) in LETTERS {
            out.push(Builtin {
                ch: *ch,
                max_dist: CLOUD_LETTER_MAX,
                templates: vec![cloud_canonical(path)],
            });
        }
        out
    })
}

/// 字体式 S：一条三次贝塞尔，起笔右上、收笔左下（和字库里的 S 一样）
fn letter_s_curve() -> Vec<(f32, f32)> {
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
    out
}

/// 整体旋转过的 S。
///
/// 点云对旋转不免疫（转 90° 的 H 不该还叫 H），但真人写字很少摆得笔直 ——
/// 实测斜 -10° 时到"笔直模板"的距离是 0.119，已经超过阈值，表现就是
/// "明明画对了却不认"。加两个斜写模板比放宽阈值安全：放宽会让 Z、C 更容易撞。
fn slanted_s_curve(deg: f32) -> Vec<(f32, f32)> {
    let r = deg.to_radians();
    let (s, c) = (r.sin(), r.cos());
    letter_s_curve()
        .into_iter()
        .map(|(x, y)| (x * c - y * s, x * s + y * c))
        .collect()
}

/// 正弦式 S：上下两段半圆，起笔收笔都在正中间
fn wave_s_curve() -> Vec<(f32, f32)> {
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
    out
}

/// 一笔能画出来的 H：左腿下 → 回到中线 → 中横 → 右腿上下收笔。
///
/// 两条腿的收笔方向、中横的高低都有人这么画，模板各放一份
/// （实测中横偏 0.4 时距离就到 0.05，不覆盖会"画得没问题却不认"）。
#[allow(clippy::type_complexity)]
fn h_shapes() -> [&'static [(f32, f32)]; 4] {
    [
        // 收在右下
        &[
            (-1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, 0.0),
            (1.0, 0.0),
            (1.0, -1.0),
            (1.0, 1.0),
        ],
        // 收在右上
        &[
            (-1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (1.0, -1.0),
        ],
        // 中横偏下
        &[
            (-1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, 0.35),
            (1.0, 0.35),
            (1.0, -1.0),
            (1.0, 1.0),
        ],
        // 中横偏上
        &[
            (-1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -0.35),
            (1.0, -0.35),
            (1.0, -1.0),
            (1.0, 1.0),
        ],
    ]
}

/* ---------------- 自定义手势（用户自己画一笔当模板） ---------------- */

/// 自定义手势的笔画规范化：等距重采样 → 平移居中 → **等比**缩放到长边为 1。
///
/// 为什么等比而不是逐轴拉满：逐轴拉满会把"竖 + 横"的 L 和"斜 + 斜"的 V
/// 拉得一样，两个手势就分不开了。等比缩放只忽略大小，保留形状比例。
///
/// 这条路和内置形状不共用实现，是刻意的：内置形状要"宽窄都能认"
/// （点云），而用户自绘要"守住自己的比例"（多点 M 和 W 本来就像）。
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
mod tests {
    use super::*;

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

    /// 正弦式 S：从正上方起笔（上半往左鼓、下半往右鼓）
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

    /// 一笔画出来的 H：左腿下、回中线、中横、右腿上下收笔
    fn h_path(h: f32, w: f32, jitter: f32, up_first: bool, n: i32) -> Vec<(i32, i32)> {
        let (x0, x1) = (500.0 - w / 2.0, 500.0 + w / 2.0);
        let (y0, y1) = (400.0 - h / 2.0, 400.0 + h / 2.0);
        let ym = (y0 + y1) / 2.0;
        let mut v = vec![(x0, y0), (x0, y1), (x0, ym), (x1, ym)];
        if up_first {
            v.push((x1, y0));
            v.push((x1, y1));
        } else {
            v.push((x1, y1));
            v.push((x1, y0));
        }
        let mut raw = Vec::new();
        for seg in v.windows(2) {
            for i in 0..n {
                let t = i as f32 / n as f32;
                let jx = ((i * 7919) % 13) as f32 / 13.0 - 0.5;
                let jy = ((i * 104_729) % 11) as f32 / 11.0 - 0.5;
                raw.push((
                    (seg[0].0 + (seg[1].0 - seg[0].0) * t + jx * 2.0 * jitter) as i32,
                    (seg[0].1 + (seg[1].1 - seg[0].1) * t + jy * 2.0 * jitter) as i32,
                ));
            }
        }
        raw
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
    fn recognizes_s() {
        // 真人写的那种：右上起笔
        for (h, ratio, n) in [(300.0, 0.7, 60), (180.0, 0.9, 40), (90.0, 0.8, 40)] {
            assert_eq!(recognize(&letter_s_path(h, ratio, 0.0, n)), Some('S'));
        }
        // 手抖 / 斜写 / 宽扁 / 瘦高
        assert_eq!(recognize(&letter_s_path(300.0, 0.7, 12.0, 60)), Some('S'));
        for deg in [-25.0, -10.0, 10.0, 25.0] {
            let p = rotated(letter_s_path(300.0, 0.7, 4.0, 60), deg);
            assert_eq!(recognize(&p), Some('S'), "斜 {deg}°：{}", diagnose(&p));
        }
        assert_eq!(recognize(&letter_s_path(300.0, 2.5, 0.0, 60)), Some('S'));
        assert_eq!(recognize(&letter_s_path(360.0, 0.3, 0.0, 60)), Some('S'));
        // 正弦式、反向画
        assert_eq!(recognize(&wave_s_path(320.0, 0.6, 0.0, false, 80)), Some('S'));
        assert_eq!(recognize(&wave_s_path(300.0, 0.8, 0.0, true, 80)), Some('S'));
        let mut rev = letter_s_path(300.0, 0.7, 0.0, 60);
        rev.reverse();
        assert_eq!(recognize(&rev), Some('S'));
    }

    #[test]
    fn recognizes_h() {
        assert_eq!(recognize(&h_path(300.0, 220.0, 0.0, true, 16)), Some('H'));
        assert_eq!(recognize(&h_path(300.0, 220.0, 0.0, false, 16)), Some('H'));
        // 画大画小、手抖、宽扁、瘦高
        assert_eq!(recognize(&h_path(160.0, 130.0, 0.0, true, 12)), Some('H'));
        assert_eq!(recognize(&h_path(320.0, 260.0, 5.0, false, 16)), Some('H'));
        assert_eq!(recognize(&h_path(300.0, 380.0, 0.0, true, 16)), Some('H'));
        assert_eq!(recognize(&h_path(380.0, 150.0, 0.0, true, 16)), Some('H'));
        // 整笔反向
        let mut rev = h_path(300.0, 220.0, 0.0, true, 16);
        rev.reverse();
        assert_eq!(recognize(&rev), Some('H'));
    }

    #[test]
    fn s_and_h_dont_steal_each_other() {
        assert_eq!(recognize(&letter_s_path(300.0, 0.7, 0.0, 60)), Some('S'));
        assert_eq!(recognize(&h_path(300.0, 220.0, 0.0, true, 16)), Some('H'));
    }

    #[test]
    fn rejects_straight_and_tiny() {
        for pts in [
            vec![(0.0, 0.0), (400.0, 0.0)],
            vec![(0.0, 0.0), (0.0, 400.0)],
            vec![(0.0, 0.0), (300.0, 300.0)],
            vec![(0.0, 0.0), (300.0, 60.0)],
            // 回笔竖：竖直划下去再回来，没有 S/H 的走向
            vec![(0.0, 0.0), (0.0, 300.0), (0.0, 100.0)],
        ] {
            assert_eq!(recognize(&polyline(&pts, 40)), None, "{pts:?}");
        }
        let jitter = [(0, 0), (6, 3), (2, 9), (11, 6), (4, 2), (9, 8), (3, 5), (7, 1)];
        assert_eq!(recognize(&jitter), None);
    }

    #[test]
    fn straight_strokes_are_not_shapes() {
        // 方向手势（一划到底的直线）必须留给方向串，不能被字母模板吃掉。
        // L / U / V / Z 这类折线**会**被认成字母，这是设计内的 ——
        // 形状没绑动作时 `gesture::match_action` 会回落到方向串，
        // 所以"划 L 形"仍然能触发 `DR` 绑定。
        for pts in [
            vec![(0.0, 0.0), (400.0, 0.0)],               // 右
            vec![(0.0, 0.0), (0.0, 400.0)],               // 下
            vec![(400.0, 0.0), (0.0, 0.0)],               // 左
            vec![(0.0, 400.0), (0.0, 0.0)],               // 上
            vec![(0.0, 0.0), (300.0, 300.0)],             // 斜
            vec![(0.0, 0.0), (0.0, 300.0), (0.0, 100.0)], // 回笔竖
        ] {
            assert_eq!(recognize(&polyline(&pts, 40)), None, "{pts:?}");
        }
    }

    #[test]
    fn rejects_loops_and_arcs() {
        let circle: Vec<(i32, i32)> = (0..=72)
            .map(|i| {
                let a = i as f32 / 72.0 * std::f32::consts::TAU;
                ((500.0 + 150.0 * a.cos()) as i32, (400.0 + 150.0 * a.sin()) as i32)
            })
            .collect();
        assert_eq!(recognize(&circle), None);

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

    /// 调阈值用：把各用例的点云距离打出来看分离度
    /// （等价于 `python tests/shape_probe.py -v`，没装 node/没开调试端口时用）
    #[test]
    fn metrics() {
        for (name, p) in [
            ("s300", letter_s_path(300.0, 0.7, 0.0, 60)),
            ("s300n12", letter_s_path(300.0, 0.7, 12.0, 60)),
            ("s_wide", letter_s_path(300.0, 2.5, 0.0, 60)),
            ("s_thin", letter_s_path(360.0, 0.3, 0.0, 60)),
            ("s_slant25", rotated(letter_s_path(300.0, 0.7, 3.0, 60), 25.0)),
            ("h300", h_path(300.0, 220.0, 0.0, true, 16)),
            ("h_flat", h_path(200.0, 520.0, 0.0, true, 16)),
            ("h_tall", h_path(380.0, 150.0, 0.0, true, 16)),
            ("Z", polyline(&[(0.0, 0.0), (300.0, 0.0), (0.0, 300.0), (300.0, 300.0)], 40)),
            ("L", polyline(&[(0.0, 0.0), (0.0, 300.0), (300.0, 300.0)], 40)),
            ("U", polyline(&[(0.0, 0.0), (0.0, 250.0), (250.0, 250.0), (250.0, 0.0)], 40)),
            ("V", polyline(&[(0.0, 0.0), (150.0, 300.0), (300.0, 0.0)], 40)),
        ] {
            let pts = thin(&p, 2.0);
            let s: String = builtins()
                .iter()
                .map(|b| format!("{}({:.3}/{:.3}) ", b.ch, b.raw_score(&pts), b.max_dist))
                .collect();
            println!("{name:10} {s} => {:?}", recognize(&p));
        }
    }
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

#[cfg(test)]
mod letter_tests {
    use super::*;

    struct Rng(u32);

    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) as f32 / 16_777_216.0
        }
        /// [-1,1]
        fn sym(&mut self) -> f32 {
            self.next() * 2.0 - 1.0
        }
    }

    /// 把模板变成"手画的样子"：密集采样 → 加抖动 → 非等比缩放 → 转像素坐标。
    fn draw(path: &[(f32, f32)], seed: u32, amp: f32, sx: f32, sy: f32) -> Vec<(i32, i32)> {
        let dense = resample(path, 96);
        let mut rng = Rng(seed);
        dense
            .into_iter()
            .map(|(x, y)| {
                let nx = x * sx + rng.sym() * amp;
                let ny = y * sy + rng.sym() * amp;
                ((400.0 + nx * 150.0) as i32, (400.0 + ny * 150.0) as i32)
            })
            .collect()
    }

    /// 每个字母的抖动变体都必须认回自己。
    ///
    /// 这是加字母的门槛：过不了说明阈值太松（会误识别）或模板不像（会漏识别），
    /// 两种都比"不支持这个字母"更糟 —— 用户宁可没有 W，也不想画 W 得到 M。
    #[test]
    fn letters_recognize_themselves() {
        let mut bad = Vec::new();
        for (ch, path) in LETTERS {
            for seed in 0..6u32 {
                let amp = 0.03 + seed as f32 * 0.010;
                let sx = 0.78 + seed as f32 * 0.10;
                let sy = 1.22 - seed as f32 * 0.08;
                let stroke = draw(path, seed * 977 + 13, amp, sx, sy);
                let got = recognize(&stroke);
                if got != Some(*ch) {
                    bad.push(format!("{ch}#{seed} → {got:?} [{}]", diagnose(&stroke)));
                }
            }
        }
        assert!(bad.is_empty(), "认不出的变体: {}", bad.join(", "));
    }
}
