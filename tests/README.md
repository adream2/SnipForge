# tests —— 端到端验证脚本

这些不是单元测试，是**驱动真实窗口**的端到端检查：起鼠标键盘事件、看窗口是否出现、
读回控件文字。目的是回答"这个功能到底能不能用"，而不是"函数返回值对不对"。

全部是纯 ctypes + Win32，不依赖 pytest / selenium / pywinauto，
只要求本机装了 Python 3。

## 前置条件

先启动应用（Debug 或 Release 都行）：

```bash
./src-tauri/target/release/snipforge.exe
```

脚本自己会找 `snipforge.exe` 的进程和窗口，不用传参数。

> 跑的时候**别动鼠标键盘** —— 脚本靠模拟输入驱动界面，人手一碰就串了。

## 脚本

| 脚本 | 验什么 | 预期输出 |
|---|---|---|
| `clipboard_race.py` | **复制后立刻按 Win+V**：面板弹出耗时、是否拿到焦点、粘的是不是刚复制的那条 | `5/5 成功` |
| `clipboard_race.py turbo 25` | 比人手还快：Win+V 后 25ms 就回车，验预热期缓冲 | `8/8 通过` |
| `clipboard_race.py turbo -1` | 极端：Win+V 和回车一次性塞进输入队列，零间隔 | `8/8 通过` |
| `clipboard_race.py plain` | 普通 Ctrl+V 有没有被剪贴板监听拖累（"影响系统"的对照实验） | `15/15 全部命中` |
| `panel_e2e.py` | 剪贴板面板全链路：Win+V 弹面板 → 回车 → 内容落到原窗口 | `8/8 通过` |
| `pin_e2e.py` | 截图 → 编辑器 → F3 钉图 → 贴图窗口位置/尺寸 → Esc 关闭 | `7/7 通过` |
| `hover_test.py` | 悬停图形滚轮调粗细、两种箭头样式 | 截图 `tests/out/h*.png` |
| `arrow_shape.py` | 一次画四条锥尾箭头，肉眼验收形状 | `tests/out/arrow_shape.png` |
| `arrow_tail.py` / `arrow_head.py` | 加粗后裁头部/尾部特写，看燕尾细节 | `tests/out/*_crop.png` |
| `show_panel.py` | 把剪贴板面板叫出来停一会儿，方便手动截图 | — |
| `shot.py` | 纯 ctypes 屏幕截图存 PNG（不依赖 Pillow） | — |

截图产物都落在 `tests/out/`（已 gitignore）。

## 为什么不用现成的 UI 自动化库

- pywinauto / Appium 都要装一堆依赖，还得配驱动
- 这些脚本要的只是"发按键、发鼠标、枚举窗口、读控件文字"，
  四件事 Win32 各有一个 API，加起来不到 200 行
- 出问题时能直接看到每一步的坐标和窗口尺寸，比框架的黑盒报错好查

## 已知限制

- 坐标是按 2560×1440 单屏调的，换分辨率/多屏要改脚本里的常量
- `find_color()` 靠颜色定位按钮，主题色一改就得同步改 RGB
- 依赖"编辑器窗口默认尺寸 1196×809"，改了 `tauri.conf.json` 要跟着改
