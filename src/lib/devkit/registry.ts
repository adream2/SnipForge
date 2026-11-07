/**
 * 工具注册表：id → 实现。
 *
 * 清单 JSON 里写了 id 但这里没有的，页面会显示"未实现"占位而不是崩掉 ——
 * 用户自己改清单加了个不存在的 id 时，得看得见问题在哪。
 */

import type { Tool } from './types'
import { encodeTools } from './tools/encode'
import { formatTools } from './tools/format'
import { generateTools } from './tools/generate'
import { textTools } from './tools/text'

export const ALL_TOOLS: Tool[] = [...encodeTools, ...formatTools, ...generateTools, ...textTools]

const BY_ID = new Map<string, Tool>(ALL_TOOLS.map((t) => [t.id, t]))

export const getTool = (id: string): Tool | undefined => BY_ID.get(id)

/** 按清单顺序取工具，丢掉不认识的 id 由调用方决定怎么提示 */
export function resolve(ids: string[]): Array<Tool | { missing: string }> {
  return ids.map((id) => BY_ID.get(id) ?? { missing: id })
}

export const isMissing = (t: Tool | { missing: string }): t is { missing: string } => 'missing' in t
