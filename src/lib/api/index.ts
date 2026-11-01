// 后端能力的唯一入口。**组件里不直接写 invoke 字符串**，一律走这里。
// 按领域拆成多个文件，这里只做拼装 —— 调用方仍然只 `import { api } from "../lib/api"`。

import { settingsApi } from './settings'
import { captureApi } from './capture'
import { imageApi } from './image'
import { clipboardApi } from './clipboard'
import { hostsApi } from './hosts'
import { recordApi } from './record'
import { uploadApi } from './upload'
import { pluginApi } from './plugin'
import { vaultApi } from './vault'
import { iconApi } from './icon'
import { updateApi } from './update'
import { aboutApi } from './about'

export const api = {
  ...settingsApi,
  ...captureApi,
  ...imageApi,
  ...clipboardApi,
  ...hostsApi,
  ...recordApi,
  ...uploadApi,
  ...pluginApi,
  ...vaultApi,
  ...iconApi,
  ...updateApi,
  ...aboutApi,
}

export { events } from './events'
export type { AboutTarget } from './about'
