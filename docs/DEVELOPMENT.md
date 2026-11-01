# 开发与调试

构建、自检、测试脚本和踩过的环境坑都在这里。**用户不需要看这份** ——
只想用的话看 [README](../README.md)，想知道内部怎么分的看 [ARCHITECTURE](ARCHITECTURE.md)。

---

## 开发

前置要求：Rust 1.77+、Node 20+、Windows 10/11。

```bash
# 前端依赖
npm install

# 生成图标（改了图标脚本后跑一次）
npm run icons

# 开发模式（热重载）
npm run app:dev

# 打包安装程序
npm run app:build

# 打绿色包（免安装 zip）
npm run portable
```

产物：
- 安装包 → `src-tauri/target/release/bundle/nsis/SnipForge_<版本>_x64-setup.exe`（NSIS，当前用户级安装，中文+英文）
- 绿色包 → `dist-portable/SnipForge_<版本>_x64_portable.zip`

**绿色包不能只拷 exe。** `plugins/*.uploader.json`（内置声明式图床）是 exe 外部的资源，
打包后由 `app.path().resource_dir()` 定位——Windows 上就是 exe 所在目录。
只拿 exe 出去，插件页里那两个内置图床会**静默消失**（加载逻辑里 `read_dir` 失败是 `continue`，
不报错），所以 `npm run portable` 会把 `plugins/` 一起放进 zip。

绿色版与安装版共用同一份数据目录 `%APPDATA%\fun.lxzt.snipforge`（配置、剪贴板历史、日志），
换形态不用重新配置，卸载也只需删掉程序目录。

### 目录结构补充

`src-tauri/core/` 是**纯逻辑 crate**：只放不碰 GUI 和平台 API 的东西
（`config` 配置结构 + `safe` 路径/文件名消毒）。它的存在只为了一件事 ——
让单元测试真的能执行。**新写的纯逻辑请放这里，别放主 crate**。

### 自检

出问题先跑一遍冒烟测试，它会告诉你哪一环断了：

```bash
snipforge.exe --smoke
```

输出示例（本机实测）：

GIF / 录屏编码有各自的自检：

```bash
snipforge.exe --gif-smoke            # 合成动画 → 写 GIF → 用解码器读回核对帧数/时长/尺寸
snipforge.exe --gif-test 5 900 12    # 真实录主屏 5 秒直出 GIF（宽 900、12fps）
snipforge.exe --gif-inspect a.gif out.png 0   # 抽 GIF 第 0 帧存 PNG，肉眼验颜色
snipforge.exe --record-smoke         # MP4 编码自检
snipforge.exe --record-test 3        # 真实录主屏 3 秒出 MP4
snipforge.exe --icon-test x.exe      # 图标提取：打印每档尺寸/来源，并把前 3 张存成 PNG
snipforge.exe --update-check         # 检查更新：打印当前/最新版本、是否预发布、失败原因
```

**纯逻辑单测**在独立的 `snipforge-core` crate 里（配置结构、路径消毒、模板渲染）。
它不依赖 Tauri，所以 `cargo test` 真能跑 —— 主 crate 的测试 target 在部分机器上
加载不起来（`0xc0000139`），那些断言等于没有：

```bash
cargo test --manifest-path src-tauri/core/Cargo.toml
```

```
== SnipForge 冒烟测试 ==
[ok] 屏幕捕获：1 块显示器
     - #0 P27QBD-RG 2560x1440 @(0,0) scale=1.00 [主屏]
[ok] 预览编码：508 KB
[ok] 窗口枚举：3 个
[ok] OCR 语言：en-US, zh-Hans-CN
[ok] 剪贴板：可读（当前 72 字）
== 完成，0 项失败 ==
```

日志在 `%APPDATA%\fun.lxzt.snipforge\logs\`，崩溃记录在 exe 同目录的 `crash.log`。

排查疑难时把日志级别临时调高，不用改代码重新编译 —— 设环境变量 `SNIPFORGE_LOG=debug` 再启动即可。
debug 级别会多出这几条关键信息：

- 面板记住了哪个目标窗口（句柄）
- 面板打开时列表首项是哪一条
- 剪贴板被占用、重试了几次
- 面板有没有抢到焦点

排查「粘错条目 / 按了没反应」基本就靠这几条。

### 环境踩坑记录

- **别再依赖 xcap 的 `video_recorder()`**。它的 `stop()` 只是把取帧线程挂起，
  线程里还克隆着 `IDXGIOutputDuplication` 永不释放 —— 表现为**同一进程第二次录制必然
  报"桌面复制被占用"**（0x80070057），重启应用才能录。现在 `record/dxgi.rs` 自己建
  D3D11 + DuplicateOutput，Drop 即释放，实测连录 4 次都正常。
- **release 二进制必须带 `custom-protocol`**。用 `npx tauri build`（或 `cargo build --release --features custom-protocol`）；
  裸 `cargo build --release` 编出来的 exe 会去连 devUrl（127.0.0.1:5183），
  窗口里全是"无法访问此页面"，看起来像前端坏了，其实是构建姿势不对。
- **必须装完整 Rust 工具链**。只有 `rustc` 没有 `cargo` 是编不了的（有些环境管理工具会装成半套）。
- **必须有 MSVC 链接器 + Windows SDK**。`rustup` 只装编译器本体，链接器要单独装 VS Build Tools 的 C++ 工作负载。装完 Rust 会警告 `installing msvc toolchain without its prerequisites`，别忽略。
- **Git Bash 的 PATH 里有个 coreutils 的 `link.exe`**，会把 rustc 带偏报 `extra operand`。项目的 `.cargo/config.toml` 里已经显式锁死 MSVC 链接器路径。
- **改 `capabilities/default.json` 时注意权限名**。Tauri 版本间会增删权限（比如 `core:window:allow-set-opacity` 在新版就没有了），写错会在 build 阶段直接报"permission not found"。

---

## 网络与代理

所有出网请求（图床上传、OCR 引擎下载、翻译、检查更新）**走系统代理**：

- Windows「Internet 选项」里开着代理（Clash / v2rayN 默认就是这样）→ 自动使用
- 设了 `HTTPS_PROXY` / `ALL_PROXY` 环境变量 → 以环境变量为准
- 都没有 → 直连

为什么值得单独说：reqwest 本身只认环境变量，不读系统代理。不自己接这一层的话，
表现就是"浏览器能打开 GitHub，但这个软件里下载/翻译/更新全部报网络错误"。
`socks` 协议的代理暂不支持（reqwest 没开 socks 特性），只会按直连处理。

---

## 产品主页（`site/`）

产品介绍页是纯静态的，没有构建步骤：

```bash
npm run site:serve   # 本地预览 http://127.0.0.1:4173/
npm run site:check   # 上线前自检（CI 也跑这个）
npm run site:ping    # 部署完成后主动通知 Bing / Yandex 来抓（IndexNow）
```

改完 `site/` 下的文件，推到 `main` 就会自动部署到
<https://adream2.github.io/SnipForge/>（`.github/workflows/pages.yml`，只在 `site/**` 变动时触发）。

仓库第一次部署需要 Pages 已启用：Settings → Pages → Source 选 **GitHub Actions**。
workflow 里 `configure-pages` 带了 `enablement: true`，多数情况下第一次跑会自己开。

站点结构：

```
site/
├── index.html      # 全部内容：功能、截图、对比、快捷键、常见问题、下载
├── assets/         # 截图 + 图标 + og.png（1200×630 分享图）
├── robots.txt      # 全站放行 + 显式放行 AI 抓取器 + 指向 sitemap
├── sitemap.xml
├── llms.txt        # 给 AI 问答引擎的事实卡片
├── <32位十六进制>.txt  # IndexNow 密钥（文件名=内容，公开是正常的）
└── .nojekyll       # 别让 Pages 走 Jekyll 处理
```

换域名 / 改仓库名时，有六处地址要同步改，清单见
[ARCHITECTURE.md · 产品主页](ARCHITECTURE.md#产品主页site--github-pages)。

## 相关文档

| 文档 | 内容 |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 分层、启动顺序、扩展点、踩坑约束、发布流程 |
| [FEATURES.md](FEATURES.md) | 完整功能清单与已知限制 |
| [../CHANGELOG.md](../CHANGELOG.md) | 每个版本改了什么 |
