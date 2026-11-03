/**
 * 列表预览用的隐私遮罩。
 *
 * 只影响**显示**：点「复制」拿到的还是原文，粘出去的也是原文。
 *
 * 为什么要做这件事：剪贴板面板经常在投屏、录屏、旁边有人的场景下打开。
 * 一条含身份证号 / 手机号的历史被整段念出来是真实会发生的尴尬，
 * 而 TieZ 那类工具默认就把预览打码 —— 用户扫一眼就知道"这条是证件号"，
 * 但别人看不到具体数字。
 */

/** 证件/账号类：保留足够辨认的头尾，中间一律打码 */
const RULES: Array<[RegExp, (...m: string[]) => string]> = [
  // 身份证 18 位：保留前 6 后 4（前 6 位是地区码，能认出是哪个省的就够了）
  [/(?<!\d)(\d{6})\d{8}(\d{3}[\dXx])(?!\d)/g, (_m, a, b) => `${a}********${b}`],
  // 手机号 11 位：138****8000
  [/(?<!\d)(1[3-9]\d)\d{4}(\d{4})(?!\d)/g, (_m, a, b) => `${a}****${b}`],
  // 银行卡 16-19 位：6222 **** **** 1234
  [/(?<!\d)(\d{4})\d{8,11}(\d{4})(?!\d)/g, (_m, a, b) => `${a} **** **** ${b}`],
  // 邮箱：abc***@example.com
  [
    /([A-Za-z0-9._%+-]{1,3})[A-Za-z0-9._%+-]*(@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g,
    (_m, a, b) => `${a}***${b}`
  ]
]

/** 只处理前这么多个字符。列表预览本来就截断，没必要对整段长文跑四个正则。 */
const SCAN_LIMIT = 4096

export function maskSensitive(text: string): string {
  if (!text) return text
  const head = text.length > SCAN_LIMIT ? text.slice(0, SCAN_LIMIT) : text
  let out = head
  for (const [re, fn] of RULES) {
    out = out.replace(re, fn as (...args: string[]) => string)
  }
  return head === text ? out : out + text.slice(SCAN_LIMIT)
}

/** 这段文字被遮罩过没有 —— 有的话列表上给个小标记，避免用户以为原文就是这样 */
export function hasSensitive(text: string): boolean {
  return maskSensitive(text) !== text
}
