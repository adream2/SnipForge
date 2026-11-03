# 架构说明

这份文档只讲**结构**和**改动规矩**，不讲功能怎么用（那在 [FEATURES.md](FEATURES.md)，
构建与自检在 [DEVELOPMENT.md](DEVELOPMENT.md)）。

> 技术栈选型（是否继续用 WebView2、要不要迁 egui、内存目标）见
> [ADR-001](ADR-001-ui-stack.md)。那份文档处于**待验证**状态，改动栈之前先读它。

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
                 │ uploader  translate  panel  update  icons  │
                 │ windows                                    │
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
| 纯逻辑 | `core/`（独立 crate） | 配置结构、路径消毒、模板渲染。**不碰 GUI，所以 `cargo test` 真能跑** |
| 能力 | `gesture.rs` `capture/` `clipboard/` `record/` `ocr/` `icons/` `uploader/` `translate.rs` `panel.rs` | 各功能自己的业务逻辑 |

`record/` 内部再分层：`mf.rs` 只负责把帧/采样写进 MP4，`audio.rs` 只管从设备
拿到 16bit 交错立体声（含重采样与混音），`camera.rs` 只管采集和合成，
`dxgi.rs` / `cursor.rs` / `gif.rs` 各自独立，`mod.rs` 是编排（定拍、裁剪、对齐）。
**加一类新的"叠加内容"不该改 `mf.rs`** —— 它只认识 RGBA 帧。
| 能力 | `vault.rs` | 主密码保险箱（Argon2id + AES-256-GCM），敏感字段的唯一落盘出口 |
| 平台 | `win32/` | 鼠标钩子、轨迹窗口、屏幕捕获、窗口枚举、发键 |
| 平台 | `net.rs` | **所有出网请求的唯一出口**：把 Windows 系统代理喂给 reqwest（见坑 13） |

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

### 加一个内置形状（一个字母）

三步，其中两步是自动的：

1. `core/src/shape.rs` 的 `LETTERS` 里加一行：**字符 + 一笔画路径**。
   坐标写成 `[-1,1]²` 里的样子（屏幕坐标系，y 向下），就是它看起来的样子；
2. 跑 `cargo test --manifest-path src-tauri/core/Cargo.toml`。
   `letters_recognize_themselves` 会给每个字母生成 6 个抖动/缩放变体，
   每个都必须认回自己。认不出就是模板不像或阈值太松，改到过为止；
3. 前端**不用改** —— `@` 前缀会自动显示成字母。

规矩：

- 形状字符一律带 `@` 前缀（`@W`），方向串保持 `U/D/L/R`。字母 D/L/R/U 和
  方向同名，不隔离就分不开"向下划"和"画一个 D"。这是 `gesture::normalize`
  的唯一规则，也是 `find_in` 回落逻辑的前提。
- **形状没绑动作时回落方向串**（`find_in` 的第二段）。所以新增字母不会
  吞掉已有的方向串绑定，画 Z 仍然触发 `RLR`。
- 加完跑全量测试：新字母可能把别的字母抢走（V/W、M/N 最容易撞），
  `letters_recognize_themselves` 会一次性告出来。

`shape` 住在 `core` 而不是主 crate，就是因为这条测试必须能真跑 ——
主 crate 的 test harness 在部分机器上加载不起来（见坑 12）。

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
2. **抓屏前先清轨迹**：手势轨迹层淡出要 240ms，截图几十毫秒就完成。
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
7. **音轨不能挂在"有新帧"的分支里**：DXGI 在画面静止时压根不出帧，声音会跟着断。
   音频的读取/写入放在每个节拍都跑的位置。另外 WASAPI **loopback 在没有声音播放时
   不出包**（不是错误），所以必须按视频时间轴补静音 —— 否则音轨是空的、时长也不对。
8. **音频要能"裁"也要能"补"**：只在静音时补零是不够的。编码跟不上时视频会掉帧、
   时间轴变短，而音频按墙钟累积 —— 不裁的话声音比画面晚结束，而且越录偏得越多。
   规则：**视频时间轴是唯一权威**，音频超前就截断、落后就补静音。
9. **摄像头是最容易"看着对其实错"的一块**：
   - `MFVideoFormat_RGB32` 在内存里是 **BGRA**，不交换 R/B 人脸会变蓝；
   - `MF_MT_DEFAULT_STRIDE` 可能是**负数**（bottom-up DIB），不按它读会得到斜彩带；
   - 画中画**必须 1:1 贴图**（缩放交给 MF 的视频处理器），所以布局尺寸必须严格
     等于帧尺寸 —— 两处独立算同一个尺寸迟早会差几个像素，然后裁掉一角。
     `--pip-test` 的断言就是为这三条设的（其中"只有上下两侧有描边"这个 bug 被它抓到过）。

10. **图标提取的坑几乎都在"格式细节"上**，每一条都对应一个真实的失败：
   - 资源目录项 `GRPICONDIRENTRY` 是 **14 字节**（比 ICO 少一个 4 字节偏移）。
     位深在 **+6**，+8 是 `dwBytesInRes` —— 写成 +8 会得到 1640、744 这种"位深"；
   - `image` crate 的 ICO 解码器对索引色 DIB（4bpp / 1bpp）会报
     `failed to fill whole buffer`，所以 DIB 必须自己解：`biHeight` 是**图像高度的两倍**
     （下面跟着 AND 掩码）、像素是 **bottom-up**、每行 4 字节对齐、32bpp 的 alpha 常常全 0
     要靠掩码补透明；
   - `LoadLibraryExW` 一定要带 `LOAD_LIBRARY_AS_DATAFILE`：它**不执行 DllMain**，
     只是把文件当资源容器映射进来。普通 `LoadLibrary` 会跑别人的初始化代码；
   - Android 的 `adaptive-icon` 是「背景 + 前景」两层，只取前景会得到透明底图案；
     而且 108dp 画布里只有中间 72dp 可见，裁掉那圈安全边距才和系统显示的一致。

11. **截图只有一个入口，其余都是选区里的选法。**
    `Alt+Q`（或托盘「截图」）打开覆盖层，然后：拖拽 = 区域、单击 = 窗口、
    `Ctrl` = 整个窗口、`Ctrl+A` = 整屏、`S` = 滚动长截图。
    窗口截图从一开始就是这么做的（`FEATURES` 里从来没有 `window` 这一项）；
    全屏和滚动长截图曾经各占一个全局快捷键，但**它们打开的是同一个选区界面**，
    用户还得再按一次键 —— 等于白占两个热键位和两行托盘菜单。2026-11 合并成一个。
    老配置里的 `fullscreen` / `scroll` 手势不会失效：`features::find` 会把它们
    映射到 `region`（哑键比多按一个键难受得多）。

    顺带一个教训：给 `keydown` 的 effect 加新分支时，**依赖数组要跟着改**。
    `Ctrl+A` 一开始完全没反应，因为那个 effect 只依赖 `selection`，
    闭包里捕获的 `display` 永远是首次渲染时的 `null`。

12. **测试要放在不链接 GUI 的 crate 里，否则等于没有。**
    主 crate 的 test harness 在部分机器上加载失败（`0xc0000139`），
    那些 `#[cfg(test)]` 断言一条都不会执行 —— 而**没人会发现**，
    因为 `cargo check --all-targets` 一直是绿的。
    2026-11 把 `config` / `safe` 拆进 `snipforge-core`（只依赖 serde/chrono/log）之后，
    `cargo test` 第一次真正跑起来，**当场就抓到一个行为与注释不符的地方**：
    `safe_join` 收到**双反斜杠开头的 UNC 路径**时会先被 `trim_matches` 裁掉前导斜杠，
    于是 UNC 被静默改写成 `<root>/server/share` —— 没有越界，但用户以为写到了网络共享。
    现在改成显式拒绝 UNC。

    结论：**绿着的 check 不等于跑过的测试**。纯逻辑就该住在能跑测试的 crate 里。

13. **reqwest 不读 Windows 的「Internet 选项」。** 它只认 `HTTP_PROXY` / `ALL_PROXY`
    这些环境变量，而国内代理软件默认开的是**系统代理**（WinINET，写在
    `HKCU\...\Internet Settings` 的 `ProxyEnable` / `ProxyServer` 里）。
    不自己读注册表接一层的话，表现是"浏览器能打开 GitHub，但这个软件里
    下载 OCR、翻译、检查更新全部报网络错误"——用户只会认为网络功能是坏的。

    所以**所有出网请求一律走 `net.rs`**（`with_proxy` / `with_blocking_proxy`），
    不要在别处直接 `reqwest::Client::builder()`。已有的环境变量优先级更高，
    用户显式设了就不再插手。socks 代理暂不支持（没开 `reqwest` 的 socks 特性），
    检测到就按直连处理 —— 半吊子的 socks 支持比没有更难查。

---

## 6. 怎么验证

| 改了什么 | 怎么验 |
|---|---|
| 纯逻辑单测 | `cargo test --manifest-path src-tauri/core/Cargo.toml`（**这条能真跑**，16 个断言：config / safe / version） |
| 后端编译 | `cargo check --all-targets`（主 crate 链接 Tauri，测试跑不起来，只能验编译） |
| 前端类型 | `npm run build`（含 `tsc`） |
| 页面行为 | 带 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动，跑 `tests/*_probe.py`（CDP 直接操作 DOM） |
| 抓屏 / 录屏 | `snipforge.exe --smoke`、`--record-smoke`、`--gif-smoke`、`--record-test N [--with-camera] [--with-mic]` |
| 画中画合成 | `snipforge.exe --pip-test out.png`（无摄像头也能验证位置/圆角/描边/镜像/不越界，8 项断言） |
| 图标提取 | `snipforge.exe --icon-test <文件> [输出目录]`（打印每档尺寸与来源，并把前 3 张存 PNG 供肉眼核对） |
| 主密码 / 加密链路 | `snipforge.exe --vault-selftest`（不依赖 GUI，19 项检查） |
| 版本比较 | `cargo test -p snipforge-core version`（预发布、v 前缀、脏 tag 都有断言） |
| 检查更新 | `snipforge.exe --update-check`（打印当前/最新版本、预发布与否、失败的具体原因） |
| 窗口命中 | `cargo test --lib element_probe -- --ignored --nocapture` |

**注意：`cargo test --lib` 在部分机器上会加载失败**
（`STATUS_ENTRYPOINT_NOT_FOUND 0xc0000139`，测试二进制连 `--list` 都跑不出来）。
原因是测试 target 链接了完整的 Tauri/WinRT 依赖，对运行时 DLL 版本敏感。
它一挂，`#[cfg(test)]` 里那些断言等于零保护，**而且没人会发现**——`cargo check --all-targets` 一直是绿的。

所以纯逻辑一律放 `src-tauri/core/`（独立 crate，只依赖 serde/chrono/log）：
`config`（配置结构与深度合并）、`safe`（路径/文件名消毒）、`version`（版本比较）。
主 crate 只做 `pub use`。**新写的、不碰 GUI 的逻辑请放这里，别放主 crate** ——
放进去就意味着它永远不会有断言。

构建姿势（别用裸 `cargo build --release`）：

```
npm run app:build        # = tauri build --features custom-protocol（出 NSIS 安装包）
npm run portable         # 用刚打好的 release exe 生成绿色 zip
```

**`--features custom-protocol` 不能省。** Tauri v2 的 CLI 只带 `tauri/custom-protocol`
（依赖的特性），不会打开本 crate 的同名特性，`cfg(feature = "custom-protocol")` 仍是假的。
漏了就打出连 devUrl 的 exe，`lib.rs` 的 `compile_error!` 会拦住并给出正确命令。

---

## 7. 扩展点：加功能只动这里

全项目只有**四个**真正的扩展面，其余地方加东西都算改内核，需要同步改文档。

### 7.1 能被触发的东西 → `features.rs`（见 §2）

### 7.2 图床 → `Provider` trait / 声明式 JSON

两条路共用 `ProviderMeta`，前端不区分来源：

- **声明式**：`plugins/xxx.uploader.json`，零代码、零信任风险（推荐给第三方）
- **原生**：实现 `uploader::Provider` trait 并 `registry.register(...)`

规矩：
1. 图床只能通过 `UploaderRegistry` 拿配置，别自己读 `settings`。
2. 落盘类图床（如本地目录）**必须**走 `uploader::safe_join` / `safe_file_name`：
   `dir` 和 `file_name` 都是前端给的字符串，直接 `Path::join` 就是任意位置写文件。
3. 声明式 JSON **不支持钩子**（没有脚本能力）。`hooks` 字段已经从结构体里删掉，
   写进 JSON 不会生效 —— 与其让人以为能用，不如根本不声明。

### 7.3 钩子 → `hooks.rs`（原生插件专用）

```
capture:before / capture:after     截图前后（前可取消，后可改 dataUrl）
clipboard:captured                 剪贴板收录后（只能通知，不能撤销）
image:beforeUpload / afterUpload   上传前后（前可取消）
```

规矩：

1. **只允许出现真正被调用的钩子点。** 以前写了 8 个常量，其中 4 个从没被调用 ——
   插件挂上去永远不触发，比没有更坑。加新点必须同时接调用处。
2. 清单由 `HookRegistry::points()` 提供给插件页，**前端不硬编码**，
   加一个点页面自动多一行。
3. 载荷是 `serde_json::Value` 而不是具体类型：钩子签名不能随业务字段演进反复改。

### 7.4 设置项 → `config.rs` + `apply_runtime`

加一个设置字段的标准动作：

1. `config.rs` 里加字段，**必须带 `#[serde(default)]`**（老配置缺字段要能补上）；
2. 在**真正用它的地方**读（能力层每轮现读，别在启动时拷一份 bool 进闭包 ——
   "改了要重启"就是这么来的）；
3. 如果是"要真的去注册/写注册表/重装钩子"的东西，副作用写在
   `commands/settings.rs::apply_runtime` 里，`settings_patch` 和 `settings_reset` 共用它
   （以前 reset 漏了手势/托盘/自启三样，用户点完"恢复默认"行为不变）；
4. 前端 `lib/types/settings.ts` 同步加字段。

**没有读取点的字段不要加。** 项目里曾经有 `general.language` / `theme` /
`minimize_to_tray` / `show_notifications` 四个纯装饰字段，连 UI 都没有，
却每次写进 settings.json，看配置的人会以为功能存在。同理，
界面上能点的开关必须有后端实现（`skip_sensitive` 的形参以前叫 `_skip_sensitive`，
打开什么也不做 —— 这是隐私承诺和实现不符）。

### 7.5 敏感字段 → `vault.rs`

**判断标准：这条数据被别人看到会不会出事？** 会，就走保险箱。

| 数据 | 位置 | 槽 |
|---|---|---|
| 收藏的密码/密钥 | `secrets.json`（明文模式） | `secrets` |
| 图床凭据 | `settings.json → uploader.providers` | `providers` |
| DeepL key / 自定义请求头 | `settings.json → translate` | `translate` |

规矩：

1. **敏感字段只允许通过 `AppState::persist_settings` 落盘。** 它会自动把
   providers / 翻译凭据搬进保险箱，并从 settings.json 里摘掉。
   自己 `fs::write(settings_file)` 就等于绕过加密 —— 这是最容易犯的错。
2. **锁定时写入要报错，不要静默丢。** 锁着的时候内存里没有凭据，
   写进去会被落盘逻辑剥掉；`upload_provider_save` / `settings_patch` 都先检查
   `is_enabled() && !is_unlocked()` 并直接拒绝。
3. **加解密只在 `vault.rs` 里。** 上层只认 `read_slot` / `write_slots`，
   谁都不许自己摆弄 nonce 或密钥派生参数。
4. **改密码要重新生成盐**（`change_password`），不能拿旧盐换 key —— 否则
   拿到旧库的人可以离线对比两次派生的开销。
5. 新增敏感字段时，同时更新 `--vault-selftest` 的检查项，否则"加密了没"
   只能靠读代码确认。

---

## 8. 发布

发布是**打 tag 触发**的，不手工传文件：

```bash
# 1. 改版本号（三处必须一致，CI 会校验）
#    src-tauri/tauri.conf.json / package.json / CHANGELOG.md
# 2. 提交后打 tag
git tag v0.1.0 && git push origin v0.1.0
```

`.github/workflows/release.yml` 会：校验版本号 → 跑 core 单测 → 打 NSIS 安装包 →
组装绿色包 → 从 `CHANGELOG.md` 取本次说明 → 建 Release（`0.x` 自动标 Pre-release）。

几条规矩：

1. **版本号三方一致**（tag / tauri.conf / package.json）。只改 tag 不改版本，
   装出来的程序会显示旧版本号，检查更新会一直提示"有新版本"——CI 第一步就拦这个。
2. **发布说明取自 `CHANGELOG.md`，不是 commit 历史。** commit message 是给维护者看的
   工程记录，用户要看的是"我能用到什么新东西"。`scripts/release-notes.mjs` 负责取对应小节。
3. **`0.x` 不做 `-preview.1` 这样的后缀叠加。** 预览性质由 GitHub 的 Pre-release 标记表达。
   后缀会让版本比较复杂（`0.1.0-preview.1 < 0.1.0`，用户装了预览版反而"没有更新"），
   而且每加一个后缀就得在 NSIS 版本号里做一次兼容。
4. **产物只有两个**：安装包（`setup.exe`）和绿色包（`portable.zip`）。
   绿色包必须含 `plugins/`，否则内置图床会静默消失。

### 产品主页（`site/` → GitHub Pages）

产品页是**纯静态、零构建**的：`site/` 目录原样作为 Pages artifact 上传，不跑 Jekyll、不跑打包器。
改完 HTML 直接推送就生效，本地预览用 `npm run site:serve`（127.0.0.1:4173）。

`.github/workflows/pages.yml` 只在 `site/**` 变动时触发，先跑 `scripts/check-site.mjs` 自检
（资源引用是否存在、JSON-LD 能否解析、每张图有没有 alt、有没有把 localhost 带上线），
再部署。检查失败就不会覆盖线上版本 —— 比部署完发现 404 再回滚便宜。

站点里跟 SEO / GEO 有关的文件各管一段，别互相抄：

| 文件 | 管什么 |
|---|---|
| `index.html` 的 `<head>` | title / description / canonical / OG / Twitter Card |
| `index.html` 的 JSON-LD | `SoftwareApplication`（软件属性、功能列表）、`FAQPage`、`WebSite`、`Organization` |
| `sitemap.xml` | 页面与图片的地址、lastmod |
| `robots.txt` | 全站放行，并显式列出 GPTBot / PerplexityBot / ClaudeBot 等 AI 抓取器 |
| `llms.txt` | 给 AI 问答引擎的"事实卡片"：版本、体积、许可、数据目录、与同类工具的差异 |
| `assets/og.png` | 1200×630 分享图（分享卡片、部分 AI 引用会带上） |
| `site/<key>.txt` | IndexNow 密钥文件，`npm run site:ping` 用它通知 Bing / Yandex 重抓 |

**改地址是这里最容易漏的一步。** 一旦换域名或改仓库名，下面六处必须一起改，否则 canonical、
sitemap 和实际地址对不上，收录会分裂：

1. `scripts/check-site.mjs` 的 `SITE_URL`
2. `site/index.html` 的 `canonical` 与 `og:url`
3. `site/sitemap.xml` 的 `<loc>`
4. `site/robots.txt` 的 `Sitemap:`
5. `site/llms.txt` 里的链接
6. `src-tauri/src/commands/about.rs` 的 `SITE`（应用内"打开官网"点这里）

### 检查更新（`update.rs`）

`GET /repos/{repo}/releases` → 挑版本号最大的已发布项（跳过 draft）→ 和 `CARGO_PKG_VERSION` 比。

- **失败一律静默**（只写 debug 日志）：公司网络出不去是常态，每次启动弹"检查失败"比不检查更烦
- **预发布版也算数**：自己就是 0.x 预览版，只看正式版永远收不到提示
- **解析不出来的 tag 不提示**（`version::is_newer` 返回 false）：宁可漏一次，也不要因为怪 tag 天天弹
- **不自动下载/安装**：那需要签名与增量更新基础设施，预览阶段不该假装有
- 打开链接只放行 `https://github.com/` 开头（`html_url` 终究是网络数据）
