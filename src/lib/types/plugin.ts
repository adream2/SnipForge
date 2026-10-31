/** 插件：声明式图床的状态展示。 */

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
  dir: string
}

export interface PluginState {
  plugins: PluginInfo[]
  builtinDir: string
  userDir: string
}
