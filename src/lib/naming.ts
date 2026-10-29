/**
 * 给"从剪贴板存为收藏"自动取个名字。
 *
 * 快速收藏的诉求是"一键存下来"，不该逼用户先想名字 —— 名字之后可以改。
 * 规则：取第一行非空内容，去掉多余空白，太长就截断。
 */
export function autoName(text: string, max = 40): string {
  const line = text
    .split(/\r?\n/)
    .map((s) => s.trim())
    .find(Boolean)

  if (!line) return '未命名'

  const compact = line.replace(/\s+/g, ' ')
  return compact.length > max ? `${compact.slice(0, max)}…` : compact
}
