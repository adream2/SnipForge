/** 录屏：状态、结果、录制过程中的事件。 */

/** 录制格式：跟着录制动作走，不是全局设置 */
export type RecordFormat = 'mp4' | 'gif'

export interface RecordStatus {
  running: boolean
  paused: boolean
  /** 倒计时结束后才为 true */
  started: boolean
  lens: boolean
  elapsedMs: number
  frames: number
  fps: number
  path: string
  width: number
  height: number
  /** 这一路是否在同时写 GIF */
  gif: boolean
  /** 是否在写 MP4 */
  mp4: boolean
}

export interface RecordResult {
  /** 主产物：有 MP4 就是 MP4，只出 GIF 时是 GIF */
  path: string
  durationMs: number
  frames: number
  bytes: number
  width: number
  height: number
  /** 同时产出的 GIF，没开就是 null */
  gifPath: string | null
  gifBytes: number
  gifFrames: number
  gifWidth: number
  gifHeight: number
}

export interface RecordCursorEvt {
  /** 区域相对坐标（物理像素） */
  x: number
  y: number
  recording: boolean
}

export interface RecordClickEvt {
  x: number
  y: number
  button: string
}

export interface RecordLensEvt {
  url: string
  x: number
  y: number
}

/** 一个已录好的文件 */
export interface RecordFile {
  name: string
  path: string
  bytes: number
  /** 毫秒时间戳 */
  createdAt: number
  /** mp4 | gif */
  format: string
}
