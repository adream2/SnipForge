/** 翻译相关的前端常量：设置页和取字窗口共用一份，避免两边语言列表对不上。 */

export const LANGUAGES: Array<{ id: string; label: string }> = [
  { id: 'zh-CN', label: '简体中文' },
  { id: 'zh-TW', label: '繁體中文' },
  { id: 'en', label: 'English' },
  { id: 'ja', label: '日本語' },
  { id: 'ko', label: '한국어' },
  { id: 'fr', label: 'Français' },
  { id: 'de', label: 'Deutsch' },
  { id: 'es', label: 'Español' },
  { id: 'ru', label: 'Русский' },
  { id: 'pt', label: 'Português' },
  { id: 'it', label: 'Italiano' },
  { id: 'ar', label: 'العربية' },
  { id: 'th', label: 'ไทย' },
  { id: 'vi', label: 'Tiếng Việt' }
]

export const PROVIDERS: Array<{ id: string; label: string; hint: string }> = [
  {
    id: 'tencent',
    label: '腾讯（免 key）',
    hint: '国内直连，自带语言识别，默认就是它'
  },
  {
    id: 'google',
    label: '谷歌（免 key）',
    hint: '国内基本连不上，需要代理才能用'
  },
  { id: 'deepl', label: 'DeepL（需 key）', hint: '质量好，key 以 :fx 结尾的是免费版' },
  { id: 'custom', label: '自定义 HTTP', hint: '自己填 URL / 请求体 / 结果路径' }
]

export function providerLabel(id: string | undefined): string {
  return PROVIDERS.find((p) => p.id === id)?.label ?? '腾讯（免 key）'
}

export function languageLabel(id: string): string {
  return LANGUAGES.find((l) => l.id === id)?.label ?? id
}
