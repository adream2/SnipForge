/** 设置：整份配置的结构（也是 settings.json 的形状）。 */

import type { GestureConfig } from './gesture'
/** 翻译服务配置（对应后端 TranslateConfig） */
export interface TranslateConfig {
  /** tencent | google | deepl | custom */
  provider: string
  /** 目标语言规范码，如 zh-CN */
  target: string
  deeplKey: string
  customUrl: string
  customMethod: string
  customHeaders: string
  customBody: string
  customResultPath: string
}

export interface AppSettings {
  version: number
  hotkeys: Record<string, string>
  capture: { after: string }
  save: { dir: string; nameTemplate: string; afterSave: string }
  clipboard: {
    enabled: boolean
    maxItems: number
    pollIntervalMs: number
    ignoreApps: string[]
    skipSensitive: boolean
    retentionDays: number
    captureImages: boolean
    takeoverWinV: boolean
  }
  uploader: {
    enabled: boolean
    activeProvider: string
    copyAsMarkdown: boolean
    /** 上传前压缩（canvas 做，能出有损 WebP） */
    compress: boolean
    /** keep | webp | jpeg | png */
    compressFormat: string
    compressQuality: number
    /** 最长边上限，0 = 不限 */
    compressMaxEdge: number
    providers: Record<string, Record<string, unknown>>
  }
  editor: {
    strokeColor: string
    strokeWidth: number
    fontSize: number
    pinOpacity: number
    watermarkText: string
    watermarkEnabled: boolean
  }
  ocr: {
    engine: string
    paddlePath: string
    timeoutSecs: number
    /** auto | direct | custom */
    downloadMode: string
    /** 自定义加速前缀，如 https://ghfast.top/ */
    mirrorPrefix: string
  }
  record: {
    /** 录系统声音（WASAPI loopback） */
    audio: boolean
    /** 录麦克风，和系统声音混音 */
    mic: boolean
    /** 摄像头画中画 */
    camera: boolean
    /** 摄像头设备名，空 = 用第一个可用设备 */
    cameraDevice: string
    /** 画中画宽度占录制画面的百分比 */
    cameraSize: number
    /** 画中画位置：br / bl / tr / tl */
    cameraCorner: string
    /** 画中画水平镜像 */
    cameraMirror: boolean
    fps: number
    videoBitsPerSecond: number
    countdown: number
    showCursor: boolean
    cursorHighlight: boolean
    magnifier: boolean
    annotate: boolean
    gifFps: number
    /** GIF 输出最大宽度，0 = 原尺寸 */
    gifWidth: number
  }
  general: {
    launchAtLogin: boolean
    enablePlugins: boolean
    closeToTray: boolean
    /** 启动后静默检查一次新版本（关了就不发那次请求） */
    checkUpdates: boolean
  }
  gestures: GestureConfig
  /** 翻译服务（老配置里可能没有，读取时用默认值兜底） */
  translate?: TranslateConfig
  pluginEnabled: Record<string, boolean>
}
