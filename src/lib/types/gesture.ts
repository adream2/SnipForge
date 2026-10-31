/** 手势：绑定、程序专用集、功能表。 */

/** 手势能做的事。和 Rust 的 `GestureAction` 一一对应（serde tag = type）。 */
export type GestureAction =
  /** 发送按键，如 "Ctrl+Shift+T"；多组用逗号分隔 */
  | { type: 'keys'; keys: string }
  /** 当前窗口操作 */
  | { type: 'window'; op: string }
  /** 打开程序 / 文件 / 文件夹 */
  | { type: 'app'; path: string }
  /** 打开网址 */
  | { type: 'url'; url: string }
  /** 运行命令 */
  | { type: 'cmd'; cmd: string }
  /** 媒体与系统键 */
  | { type: 'media'; key: string }
  /** 输入文字 */
  | { type: 'text'; text: string }
  /** 滚动 */
  | { type: 'scroll'; dir: string; amount: number }
  /** 本工具的功能 */
  | { type: 'snip'; id: string }

export interface GestureBinding {
  /** 方向串（U/D/L/R，如 "DR"）、形状字符（"S"）；自定义手势是空串 */
  pattern: string
  /**
   * 用户自己画的手势模板（32 点，等比归一化）。
   * 非空就是"自定义手势"，按形状匹配，不看方向串。
   */
  points?: number[][]
  action: GestureAction
  enabled: boolean
}

/**
 * 程序专用手势集：进程名命中时先用这里的绑定，没写到的回落通用。
 */

export interface GestureProfile {
  id: string
  name: string
  /** 进程名（小写，带不带 .exe 都认） */
  exes: string[]
  bindings: GestureBinding[]
}

export interface GestureConfig {
  enabled: boolean
  /** right / middle / x1 */
  trigger: string
  /** 移动超过多少像素才算手势 */
  threshold: number
  /** 判一次转向所需位移 */
  step: number
  showTrail: boolean
  /** 轨迹颜色 #RRGGBB */
  trailColor: string
  /** 前台窗口全屏时自动暂停 */
  pauseFullscreen: boolean
  /** 把触发键的按下也吞掉（默认开；关掉退回只吞抬起） */
  swallowDown: boolean
  /** 不响应手势的程序（exe 文件名，小写） */
  excluded: string[]
  /** 在这个程序里划一下就能加进排除列表（默认 RLR = Z 形） */
  excludePattern: string
  bindings: GestureBinding[]
  /** 程序专用手势集（命中进程时优先，其余回落通用） */
  profiles?: GestureProfile[]
}

/**
 * 功能表条目（后端 `features.rs`）。
 *
 * 手势动作下拉里能选的东西就来自这里 —— 名字和 id 都由后端给，
 * 前端不再维护第二份名单。
 */
export interface FeatureInfo {
  id: string
  label: string
  /** 会抓屏幕（截图/录屏）：执行前会先收起手势轨迹 */
  grabsScreen: boolean
}

/** 出厂的默认手势（"恢复默认"用，由后端提供） */
export interface GestureDefaults {
  bindings: GestureBinding[]
  profiles: GestureProfile[]
}
