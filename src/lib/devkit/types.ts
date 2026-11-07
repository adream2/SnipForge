/**
 * 开发者工具箱 · 工具契约。
 *
 * 为什么算法写在 TS 而不是像图床那样纯 JSON：图床是"拼一个 HTTP 请求"，
 * JSON 能表达；正则测试、行级 diff、cron 求下次触发时间表达不了，
 * 硬造 DSL 只会得到一个谁都写不对的半门语言。
 *
 * 所以分工是：**清单 JSON 决定上架哪些工具、怎么分组**（`plugins/devkit.plugin.json`），
 * **算法留在前端注册表**。用户放一份自己的同名清单就能调整上架范围，
 * 但改不了算法 —— 这是有意的，插件目录里的 JSON 不该有执行能力。
 */

import type { ComponentType } from 'react'

export interface OptionDef {
  key: string
  label: string
  kind: 'select' | 'switch' | 'number'
  /** kind=select 时必填 */
  options?: Array<{ label: string; value: string }>
  /** 默认值：select 用字符串，switch 用布尔，number 用数字 */
  default: string | boolean | number
  min?: number
  max?: number
  hint?: string
}

export type OptionValues = Record<string, string | boolean | number>

/** 单输入单输出的转换型工具，页面给一套通用双栏 UI */
export interface TransformTool {
  kind: 'transform'
  id: string
  name: string
  desc: string
  /** 有双向时显示"编码 / 解码"切换，run 的 dir 参数才有意义 */
  dirs?: { encode: string; decode: string }
  options?: OptionDef[]
  placeholder?: string
  /** 输出框初始行数 */
  rows?: number
  /** 抛异常 = 这次转换失败，页面把它当错误提示显示，不打断其它工具 */
  run: (input: string, opts: OptionValues, dir: 'encode' | 'decode') => string
}

/** 交互复杂的工具（正则、diff、时间戳…）自己画 UI */
export interface PanelTool {
  kind: 'panel'
  id: string
  name: string
  desc: string
  Panel: ComponentType
}

export type Tool = TransformTool | PanelTool

export const optStr = (o: OptionValues, k: string): string => String(o[k] ?? '')
export const optBool = (o: OptionValues, k: string): boolean => Boolean(o[k])
export const optNum = (o: OptionValues, k: string): number => Number(o[k] ?? 0)

/** 按选项定义生成默认值表 */
export function defaultsOf(tool: Tool): OptionValues {
  const out: OptionValues = {}
  if (tool.kind === 'transform') {
    for (const o of tool.options ?? []) out[o.key] = o.default
  }
  return out
}
