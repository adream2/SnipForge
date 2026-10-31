// 后端能力的唯一入口。**组件里不直接写 invoke 字符串**，一律走这里。
// 按领域拆成多个文件，这里只做拼装 —— 调用方仍然只 `import { api } from "../lib/api"`。

import { settingsApi } from './settings'
import { captureApi } from './capture'
import { imageApi } from './image'
import { clipboardApi } from './clipboard'
import { recordApi } from './record'
import { uploadApi } from './upload'
import { pluginApi } from './plugin'

export const api = {
  ...settingsApi,
  ...captureApi,
  ...imageApi,
  ...clipboardApi,
  ...recordApi,
  ...uploadApi,
  ...pluginApi,
}

export { events } from './events'
