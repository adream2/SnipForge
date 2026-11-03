# ADR-001：UI 技术栈与内存目标

- 状态：**已决 · 不迁 egui**（2026-11-03 修订，见 §9）
- 日期：2026-11-03
- 决策人：维护者

---

## 1. 结论

**Rust 不变，UI 也不动 —— 继续用 Tauri + WebView2，不迁 egui。**

理由按权重排序：

1. **内存不是瓶颈。** 27 MB 常驻在同类型 Windows 工具里已经偏优（Snipaste ~35 MB、PixPin ~40 MB、ShareX ~80 MB）。为省十几 MB 付 6-8 周重写，还押上截图精度、中文 IME、透明覆盖层这些已经踩平的地基，不划算。
2. **egui 对"手势轨迹 / 全屏覆盖层"是负优化。** 立即模式每帧重建整棵树、整屏重绘；而轨迹层需要的是"只重画实际变化的那几百像素"。现在这套分层窗口只要把重绘范围收窄就能到 60fps，换 egui 是重新面对同一批问题，而且更难收窄。
3. **双栈成本有更便宜的解法。** `src/lib/types/*` 与 `types.rs` 的重复可以用 `ts-rs` 从 Rust 生成 TS 声明消掉，不必换 UI 框架。

真正该投的是**交互体验**，不是渲染后端（见 §9）。

---

## 2. 候选评估

| 选项 | 常驻内存 | 工作量 | 结论 |
|---|---|---|---|
| A. **现状（Tauri + React）** | ~27 MB | 0 | **选用**，只做局部优化 |
| B. Tauri 内部调优 | ~20 MB | 1-2 天 | 可先做，性价比最高 |
| C. 混合：高频窗口原生化，主窗留 WebView | ~15 MB | 1-2 周 | 备选。只在内存真的成为投诉时再上 |
| D. 全 egui | ~12-15 MB | 6-8 周 | **否决**：省十几 MB，赔 6-8 周 + 重踩透明窗口/IME |
| E. Slint | ~20-30 MB | 6-8 周 | 否决，GPL/商业双许可 + 生态小 |
| F. Dioxus / Leptos + Tauri | ~27 MB | — | 否决，仍是 WebView |
| G. Flutter | ~60 MB+ | 全重写 | 否决（Dart VM + Skia） |
| H. Avalonia / Qt / C++ Win32 | — | 全重写 | 否决，后端 23k 行全废 |

否决 D 的关键判断：省下的内存用户感知不到，付出的时间成本是实打实的；而**手势轨迹层这个最需要性能的地方，egui 帮不上忙反而更差**（立即模式整屏重绘）。

---

## 3. 目标架构

```
snipforge/                     cargo workspace
├── crates/
│   ├── sf-core/               配置、路径消毒、版本比较、共享类型（现 core/）
│   ├── sf-platform/           win32/*：唯一 use windows:: 的地方
│   ├── sf-capture/            抓屏、选区几何、长截图拼接
│   ├── sf-clipboard/          采集、存储、检索
│   ├── sf-record/             dxgi / audio / camera / mf / gif / cursor
│   ├── sf-ocr/                引擎抽象 + windows / paddle
│   ├── sf-upload/             Provider trait + 内置 + 声明式插件
│   ├── sf-vault/              Argon2id + AES-256-GCM
│   ├── sf-icons/              PE / LNK / APK / IPA
│   ├── sf-gesture/            钩子 + $P 点云 + 动作分发
│   ├── sf-app/                编排层：Feature 注册表、hooks、AppCtx、事件总线
│   └── sf-ui/                 egui 全部界面（只依赖 sf-app）
└── src-tauri/                 P3 阶段删除
```

依赖方向单一：`sf-ui → sf-app → sf-* 能力层 → sf-platform → windows`。

`sf-ui` **不许**直接依赖能力层 crate —— 这是让 UI 可再替换（以后想换 Slint 只改 sf-ui）的唯一保证。

### 关键契约（迁移前先定，别边写边改）

```rust
// sf-app：所有能力对 UI 的唯一入口
pub trait Host {
    fn ctx(&self) -> &AppCtx;
    fn open(&self, win: WindowKind, payload: WindowPayload);
    fn notify(&self, n: Notice);
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static);
}

// sf-app：功能注册表，从 fn(&AppHandle) 改成 fn(&AppCtx)
pub struct Feature {
    pub id: &'static str,
    pub label: &'static str,
    pub grabs_screen: bool,
    pub run: fn(&AppCtx),
}

// sf-app：跨窗口事件，替代 Tauri emit
pub enum AppEvent {
    ClipboardStats(ClipboardStats),
    RecordStop(RecordResult),
    Navigate(&'static str),
    Toast { text: String, level: Level },
    Captured(CaptureResult),
    GestureChanged,
}
```

### 窗口映射

| 窗口 | egui 实现 | 备注 |
|---|---|---|
| main | viewport + 侧栏页面枚举 | 加一页 = 加一个 `impl Page` |
| editor | 独立 viewport，`egui::Painter` 自绘 | 标注对象模型 `editor/model.rs` 直接搬 |
| selector | 独立 viewport，全屏透明置顶 | 逐屏覆盖，物理像素定位（现状逻辑保留） |
| panel | 独立 viewport，`ScrollArea::show_rows` 虚拟列表 | 首帧目标 <50ms |
| pin | 独立 viewport，纯纹理 | 无需任何控件 |
| record bar / overlay | 独立 viewport | overlay 需 `set_ignore_cursor_events` |
| ocr | 独立 viewport，左右分栏 | 文本编辑用 `TextEdit` |

纹理策略：图片一律落盘 + LRU 纹理缓存（上限 64 张 / 256 MB），列表只持有 thumb 句柄。这是 egui 下最容易失控的内存项，必须一开始就做。

---

## 4. 迁移阶段（暂不执行，保留作参考）

| 阶段 | 内容 | 工期 | 验收 |
|---|---|---|---|
| P0 | spike：独立 demo 验 5 个卡关点 | 3 天 | §6 全绿，否则终止 |
| P1 | workspace 拆分，Tauri 外壳保留，UI 仍 React | 2 周 | `cargo test` 覆盖 core + 各能力层；功能零回归 |
| P2 | egui 逐窗口替换，双轨并存 | 6 周 | 每换一个窗口，内存与手感两项对比记录 |
| P3 | 删 Tauri 依赖，收尾打包 | 1 周 | 安装包 + 常驻内存出最终数字 |

P1 单独成阶段的原因：拆 crate 是纯机械重构，风险与收益最不成比例，但它让 P2 能"一次只换一个窗口"而不是"全崩了再修"。

P2 的替换顺序（按 ROI，先换内存收益大且 UI 简单的）：

1. pin（最简单，验证透明窗口）
2. record bar + overlay
3. panel（最高频，价值最大）
4. selector（最复杂，但当前是纯 canvas）
5. main 各页（表单多，工作量最大）
6. editor（最后，1653 行 TSX + 769 行 render.ts，但对 egui 是甜点区）

---

## 5. 明确不做

- 不改语言。理由见 §1。
- 不做"只为省 15MB 的全量重写"以外的重构。录屏/手势/保险箱等模块在 P2 里**一行不动**。
- 不引入 GUI 框架抽象层（如 relm4 / iced 混用）。只选 egui，避免二次迁移。
- 不保留 WebView2 作为可选后端。双后端 = 双倍 UI 代码，是最贵的"保留退路"方式。

---

## 6. Spike 卡关点（暂不执行，留给未来评估）

| # | 验证项 | 通过标准 | 失败即终止的理由 |
|---|---|---|---|
| 1 | 中文 IME | 在 `TextEdit` 内可输入中文，候选框跟随光标位置 | 设置页/Hosts/OCR 全靠文本输入，IME 不可用等于不可用 |
| 2 | 透明 + 点击穿透窗口 | 全屏透明置顶窗口，`ignore_cursor_events` 生效，无白底 | selector / overlay / pin 三窗口的地基 |
| 3 | 多显示器 DPI | 两块不同缩放屏，逐屏覆盖层物理像素对齐无偏移 | 截图精度是本产品核心卖点 |
| 4 | 首帧延迟 | panel `show()` 到可交互 <50ms | 现状 20ms；超过 100ms 就是体验倒退 |
| 5 | 编辑器渲染质量 | 马赛克 / 文字 / 箭头在 200% 缩放下无模糊、无锯齿 | 截图工具的产出物质量即产品 |

附加确认（不阻塞）：托盘（`tray-icon`）、全局快捷键（`global-hotkey`）、单实例（`single-instance`）在纯 winit 下可用 —— 这三个是 Tauri 内部同款 crate，风险低。

---

## 7. 内存目标

| 场景 | 现状 | 目标 | 说明 |
|---|---|---|---|
| 冷启动常驻（托盘待命） | ~27 MB | ≤ 15 MB | 单进程，无 WebView |
| 打开主窗口 | ~27 MB | ≤ 22 MB | 多一屏控件 + 字体图集 |
| 打开编辑器 | ~45 MB | ≤ 60 MB | 图片纹理是大头，可接受 |
| 安装包 | 2.9 MB | ≤ 3.5 MB | 去掉 WebView2 bootstrapper，加 egui |

如果 P3 实测常驻 >20 MB，说明纹理或字体图集管理有问题，先查这两个再谈回滚。

---

## 8. 保留的扩展面

迁移不改变加功能的方式，四个扩展点全部保留：

| 扩展点 | 迁移后的位置 | 加一个要改几处 |
|---|---|---|
| 能被触发的东西 | `sf-app/features.rs` | 1 行 |
| 图床 | `sf-upload` Provider trait / 声明式 JSON | 0 行（放 JSON）或 1 个 impl |
| 钩子 | `sf-app/hooks.rs` | 加常量 + 加调用处 |
| 设置项 | `sf-core/config.rs` + `sf-app/apply_runtime` + `sf-ui` 表单 | 3 处 |

UI 侧新增页面：`sf-ui/pages/` 加一个文件 + 在 `Page` 枚举加一项，不需要改能力层。

---

## 9. 修订记录：为什么翻掉"迁 egui"

初版（同日早些时候）的结论是"迁 egui"。翻案触发于一条实际反馈：

> 鼠标手势并不流畅灵活，与其它鼠标手势工具相比差距极大。

查下来问题全在**渲染实现**，与 UI 框架无关。这恰好构成"不该迁 egui"最硬的证据。

### 9.1 定位到的四个缺陷

| # | 位置 | 问题 | 后果 |
|---|---|---|---|
| 1 | `trail.rs::paint` | 每帧 `FillRect` + `BitBlt` 整个虚拟屏 | 4K 双屏每帧拷 8M 像素，十几毫秒 |
| 2 | `trail.rs::paint` | 每帧 `CreateFontW` / `CreatePen`×2 / `CreateSolidBrush`×2 | 字体创建要查字体面，每帧白扔几毫秒 |
| 3 | `trail.rs::push` | 每个 `WM_MOUSEMOVE` 都 `PostMessage` 一次重绘 | 1000Hz 鼠标灌满消息队列，轨迹越划越落后 |
| 4 | 默认参数 | `threshold 30` / `step 55` / 淡出 `520ms` | 划一小段没反应、短促手势划不出方向、浮标粘手 |

第 2/3/4 条跟"用哪套 UI 框架"完全无关；第 1 条恰恰是 egui 更严重的地方 ——
立即模式每帧从头画整个 viewport，比"按需重绘"的全屏 GDI 更费。

### 9.2 已做的修复

- 重绘合并（`REPAINT_PENDING`）：队列里已有一帧就跳过
- 帧率上限 66fps（`FRAME_MS = 15`）：高刷鼠标不再烧 CPU
- GDI 资源缓存：画笔/画刷/字体只在窗口重建时创建
- 脏区绘制：只清、只贴"轨迹 bbox ∪ 浮标 bbox ∪ 上一帧范围"
- 点采样阈值 2px
- 默认值 30/55 → 16/38、淡出 520 → 240ms，带 v3 配置迁移

### 9.3 仍未做（按优先级）

1. `LWA_COLORKEY` 分层窗口是**软件合成**路径，GPU 不参与。彻底解决要
   `WS_EX_NOREDIRECTIONBITMAP` + DirectComposition（约 200-300 行 COM/D3D11）。
2. 形状识别仍在钩子线程同步跑。点数上千后单次 `$P` 匹配开始有感，
   应移到独立线程，钩子只累积点。
3. `Polyline` 每帧重画全部点，划得越长越慢。理想做法是增量追加线段。

### 9.4 什么时候重新考虑换 UI 栈

只有这三种情况之一出现，才值得重开这个 ADR：

- 常驻内存成为可量化的投诉（比如 >50 MB）
- WebView2 在目标用户机器上大面积缺失或异常
- 需要跨平台（Linux / macOS）—— 那时 `win32/` 本来就要重写，顺带换 UI 才划算
