/** 插件：声明式图床的状态展示。 */

/** 工具型插件（devkit）声明的工具分组 —— 清单 JSON 是唯一事实源 */
export interface PluginGroup {
  id: string
  name: string
  tools: string[]
}

export interface PluginInfo {
  id: string
  name: string
  version: string
  description?: string
  author?: string
  origin: 'builtin' | 'user'
  enabled: boolean
  loaded: boolean
  error?: string
  hooks: string[]
  providers: string[]
  /** devkit 这类工具型插件的分组；图床插件为空数组 */
  groups: PluginGroup[]
  /** 全部工具 id 平铺 */
  tools: string[]
  dir: string
}

export interface PluginState {
  plugins: PluginInfo[]
  builtinDir: string
  userDir: string
}

/**
 * 一个钩子点（插件可以介入的时机）。
 *
 * 清单由后端 `hooks::names::ALL` 提供 —— 前端不再自己抄一份，
 * 加一个钩子点页面自动多一行，不会出现"文档里有、代码里没有"。
 */
export interface HookPoint {
  name: string
  handlers: number
}
