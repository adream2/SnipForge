# 架构说明

这份文档只讲**结构**和**改动规矩**，不讲功能怎么用（那在 README）。

先说清一个背景：这个项目是从"一个能截图的小工具"长出来的，功能一路加上去，
于是同一批概念被抄了好几遍 —— 同一个功能在浮标、托盘、下拉里各写一份名字，
同一套颜色在每个窗口的 css 里各写一份 `:root`。这里的整理就是为了把这些
"复制出来的事实"收回成一份，并定下以后别再抄回去的规矩。

---

## 1. 分成三层，别越界

```
                 ┌──────────────────────────────────────────┐
   入口层         │ main.rs（命令行自检）  lib.rs::run（装配）  │
                 │ bootstrap.rs（启动顺序）  tray.rs shortcuts  │
                 └────────────────────┬─────────────────────┘
                                      │ 只调用
                 ┌────────────────────▼─────────────────────┐
   IPC 层         │ commands/*（薄壳：校验参数 → 调服务 → 返回）│
                 │ 不写业务逻辑，不直接摸 win32                 │
                 └────────────────────┬─────────────────────┘
                                      │
                 ┌────────────────────▼─────────────────────┐
   能力层         │ features.rs  功能注册表（能被触发的东西）    │
                 │ gesture  capture  clipboard  record  ocr   │
                 │ uploader  translate  panel  windows        │
                 └────────────────────┬─────────────────────┘
                                      │ 唯一允许往下摸的地方
                 ┌────────────────────▼─────────────────────┐
   平台层         │ win32/*（唯一 use windows:: 的地方）        │
                 └──────────────────────────────────────────┘
```

对应的文件：

| 层 | 文件 | 职责 |
|---|---|---|
| 入口 | `lib.rs` | 只做「注册」：插件、setup 回调、窗口事件、命令清单 |
| 入口 | `bootstrap.rs` | 启动顺序：日志 → 配置 → 状态 → 钩子/托盘/快捷键 → 窗口 |
| 入口 | `devtools.rs` | `--smoke` 之类的命令行自检，和应用启动无关 |
| IPC | `commands/*.rs` | 一层薄壳，一个函数对应一个前端 `invoke` |
| 能力 | `features.rs` | **功能注册表**（见第 2 节） |
| 能力 | `gesture.rs` `capture/` `clipboard/` `record/` `ocr/` `uploader/` `translate.rs` `panel.rs` | 各功能自己的业务逻辑 |
| 平台 | `win32/` | 鼠标钩子、轨迹窗口、屏幕捕获、窗口枚举、发键 |

规矩：

1. **`win32/` 是唯一 `use windows::` 的地方。** 句柄、DPI、宽字符这些细节不许
   漏到上层 —— 一旦漏出去，之后每次改动都要重新推理系统调用。
2. **`commands/` 不写业务逻辑。** 要写逻辑就下沉到能力层，命令里只留
   「参数校验 + 调用 + 返回」。
3. **能力层之间不直接调用**，走 `hooks` 或显式传参。
4. **配置只增不删。** 字段一律 `#[serde(default)]`，老配置缺字段自动补默认值。

---

## 2. 功能注册表：`features.rs`

这是这次整理的核心。同一批功能以前被抄了三遍：

- `gesture.rs` 里 `describe()` 的 if/else 名字表
- `gesture.rs` 里 `run_snip()` 的分发 match
- `gesture.rs` 里 `needs_clean_screen()` 的 id 白名单
- 前端 `GesturePage.tsx` 的 `SNIP_IDS`

结果是：加一个功能要改四处，**漏一处就出现"下拉里有、真划没反应"**。

现在只有 `features::FEATURES` 一张表：

```rust
Feature {
    id: "region",             // 配置里存的字符串，不能改
    label: "区域截图",         // 界面上叫什么（浮标/下拉/日志共用）
    grabs_screen: true,       // 会不会抓屏 → 动手前先收手势轨迹
    run: |app| ...,           // 真正干活的那一步
}
```

- 手势浮标文案、执行分发、「要不要先清轨迹」全部查这张表；
- 前端通过 `features_list` 命令拿同一份名单，**不再自己维护一份**。

### 加一个"能被手势触发"的功能

1. `features.rs` 里加一行；
2. 如果有新的 IPC，`commands/` 里加函数并在 `lib.rs` 注册；
3. 前端什么都不用改 —— 下拉自动多一项。

### 加一个内置形状手势（比如 `H`）

比加功能麻烦，因为要动识别：

1. `win32/shape.rs` 的 `builtins()` 里加一个 `Builtin`：**字符 + 规范化方式 + 模板 + 阈值**；
2. 规范化方式要和这个字怎么写保持一致 —— `S` 是斜着写的（弦对齐），`H` 是横平竖直的
   （不转弦，先拉成正方形再采样，否则宽窄会改变采样点分布）；
3. `gesture.rs` 的 `normalize()` 必须放行这个字符，否则用户写的绑定会被过滤成空串；
4. 前端 `gestureShape.ts` 的 `patternToStroke` 加缩略图画法、`GesturePage` 的 `SHAPE` 加映射；
5. 用 `tests/shape_probe.py` 验分离度：正例（大小/抖动/宽窄/反向）都要过，
   负例（直划、Z、L、U、V、圆、C、回笔折线）都要 **None**。

**别贪多。** 每多一个形状，就多一分和方向手势抢的风险；只有"确实有人会画"
的字才值得内置，其余让用户自绘（`canonical_stroke`）。

---

## 3. 前端：多窗口 + 共享 lib

应用是 8 个独立窗口（主窗、选区、编辑器、贴图、取字、面板、录制条、录制覆盖层），
每个窗口是独立 HTML + 独立 React 树。所以"共享"只能靠 `src/lib/` 和 `src/styles/`。

```
src/
├── main.tsx  App.tsx         主窗口
├── selector.tsx editor.tsx   选区 / 编辑器
├── ocr.tsx pin.tsx panel.tsx 取字 / 贴图 / 剪贴板面板
├── record.tsx
├── pages/                    主窗口各页面
├── components/               跨页面复用的组件
├── lib/
│   ├── api/                  后端能力，按领域拆开，index.ts 拼成 api
│   │   ├── settings.ts capture.ts image.ts clipboard.ts
│   │   ├── record.ts upload.ts plugin.ts events.ts
│   │   └── index.ts          export const api = { ...各域 }
│   ├── types/                类型定义，同样按领域拆，index.ts 汇总
│   ├── features.ts           功能表（useSyncExternalStore 缓存一次）
│   └── gestureShape.ts       手势笔画 → 缩略图/判重
└── styles/
    ├── tokens.css            颜色/圆角/阴影/字体 —— 所有窗口共用
    └── global.css editor.css ocr.css panel.css ...
```

规矩：

1. **组件里不写 `invoke('xxx')` 字符串**，一律走 `api.xxx()`。命令改名时只改一处。
2. **颜色只用 `tokens.css` 的变量。** 不要在某个窗口的 css 里再写 `:root`：
   以前三个窗口各有一套灰阶，同一个产品看着像三个软件。
3. **功能的 id / 名字来自后端**，前端不硬编码（见上一节）。

---

## 4. 启动顺序（`bootstrap.rs`）

顺序是有原因的，动之前先看一眼：

```
Paths → 目录 → 日志（必须最先，后面出错才有记录）
      → 读配置 + 迁移（老版本手势表、缺失字段补写）
      → 剪贴板服务 / 图床注册表 / AppState
      → 开机自启校正（绿色版挪位置后自启会失效）
      → DPI 感知 → 插件运行时（快捷键 + 托盘 + 剪贴板监听）
      → 键盘钩子（Win+V）→ 鼠标手势钩子
      → 后台预建剪贴板面板（400ms 后，避免和主窗抢启动）
      → 显示主窗口
```

坑：`panel::prewarm` 必须放后台线程 —— 建窗口要把请求投递给主线程再等结果，
在 `setup`（本身就在主线程）里直接调会自锁。

---

## 5. 几条踩过坑的约束

1. **release 必须带 `custom-protocol`**，否则打出来的 exe 会去连 devUrl
   （127.0.0.1:5183），用户看到"拒绝连接"。`lib.rs` 里有 `compile_error!` 守着。
2. **抓屏前先清轨迹**：手势轨迹层淡出要 520ms，截图几十毫秒就完成。
   `trail::hide_blocking()` 会等窗口线程确认真的藏起来了再返回。
   哪些功能算"抓屏"写在 `features.rs` 的 `grabs_screen`。
3. **手势识别只有一份实现**：钩子实时识别、页面"试划"、录制框都走后端
   `shape` 模块。前端自己再算一遍，迟早出现"试得出来、真划没反应"。
4. **手势匹配只有一个入口**：`gesture::match_action`（真手势）与
   `match_scoped`（页面查重）共用 `find_in`。执行和"试划"必须同一套顺序。
5. **写系统文件（hosts）必须提权**：普通权限写不进 `System32`。做法是
   备份 → 写临时文件 → `ShellExecuteEx("runas")` 复制 → **回读校验**。
   用户取消 UAC（错误码 1223）必须报"没保存"，不能静默失败 —— 改 hosts 的人
   最怕"以为改了，其实没改"。
6. **不是所有窗口都能显示自己**：选区覆盖层是全屏置顶窗口，所以命中测试
   不能用 `WindowFromPoint`（永远返回它自己），必须自己按 z 序枚举。

---

## 6. 怎么验证

| 改了什么 | 怎么验 |
|---|---|
| 后端逻辑 | `cargo check --lib` / `cargo test --lib`（单测在 `#[cfg(test)] mod tests`） |
| 前端类型 | `npm run build`（含 `tsc`） |
| 页面行为 | 带 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动，跑 `tests/*_probe.py`（CDP 直接操作 DOM） |
| 抓屏 / 录屏 | `snipforge.exe --smoke`、`--record-smoke`、`--gif-smoke` |
| 窗口命中 | `cargo test --lib element_probe -- --ignored --nocapture` |

构建姿势（别用裸 `cargo build --release`）：

```
npm run build                       # 前端产物进 dist/
cargo build --release --features custom-protocol
```
