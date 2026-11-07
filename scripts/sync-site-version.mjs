#!/usr/bin/env node
/**
 * 把工程版本号同步进产品介绍页（`site/index.html`）。
 *
 * 为什么需要这个：站点是纯静态页面，版本号、安装包大小、发布日期都是写死的。
 * 发版只改 tag 不会动它，于是页面上永远挂着上个版本 —— 用户点了下载还以为
 * 没更新。以前靠手工改，改漏一处就出现「页头写 0.2.0、下载区写 0.1.0」。
 *
 * 由 `release.yml` 在发布成功后调用，改完自动提交回 main，触发 Pages 重新部署。
 *
 * 顺带修一个死链：`/releases/latest` 只指向**正式版**，而 0.x 全部标了
 * prerelease，所以那个地址在 0.x 阶段永远指不到东西。统一改成发布列表页。
 *
 * 用法：node scripts/sync-site-version.mjs [--check]
 *   --check  只检查是否已同步，不改文件（不一致时退出码 1）
 */

import { readFileSync, writeFileSync, existsSync, statSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SITE_FILE = join(ROOT, 'site', 'index.html')
const CHECK_ONLY = process.argv.includes('--check')

const version = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')).version
const today = new Date().toISOString().slice(0, 10)

/** 发布阶段随版本号走：0.x → 预览版，≥1.0 → 正式版（和 release.yml 的 prerelease 一致） */
const stage = version.startsWith('0.') ? '预览版' : '正式版'
const dlNote =
  stage === '预览版' ? '当前为预览版，接口与配置项可能变动。' : '当前为正式版，可放心日常使用。'

/** 产物大小（MB，两位小数）。没构建过就返回 null，对应条目跳过不动。 */
function sizeMB(relPath) {
  const full = join(ROOT, relPath)
  return existsSync(full) ? (statSync(full).size / 1024 / 1024).toFixed(2) : null
}

const setupMB = sizeMB(`src-tauri/target/release/bundle/nsis/SnipForge_${version}_x64-setup.exe`)
const zipMB = sizeMB(`dist-portable/SnipForge_${version}_x64_portable.zip`)

/** [名称, 正则, 替换, 未命中是否算失败] */
const rules = [
  // 页头徽标：`<b>0.2.0 预览版</b>`
  ['页头版本徽标', /(<span class="eyebrow">Windows 10\/11 · <b>)[^<]*(<\/b>)/, `$1${version} ${stage}$2`, true],
  // 参数表：`<dt>当前版本</dt><dd>0.2.0 预览版</dd>`
  ['参数表-当前版本', /(<dt>当前版本<\/dt><dd>)[^<]*(<\/dd>)/, `$1${version} ${stage}$2`, true],
  // 下载区标题：`<h3>SnipForge 0.2.0 <span ...>预览版</span></h3>`
  ['下载区标题', /(<h3>SnipForge )\d+(?:\.\d+)*( <span)/, `$1${version}$2`, true],
  // 下载区说明：`当前为预览版，接口与配置项可能变动。` 随阶段切换
  ['下载区版本说明', /(<p>Windows 10 \/ 11 · x64 · )[^<]*(<\/p>)/, `$1${dlNote}$2`, true],
  // 页脚：`最后更新 2026-11-03`
  ['页脚更新日期', /(最后更新 )\d{4}-\d{2}-\d{2}/, `$1${today}`, false],
  // `/releases/latest` 在 0.x 阶段永远是空的，换发布列表页
  [
    '下载链接（latest → 列表）',
    /https:\/\/github\.com\/adream2\/SnipForge\/releases\/latest/g,
    'https://github.com/adream2/SnipForge/releases',
    false
  ],
  // JSON-LD 结构化数据
  ['JSON-LD-版本号', /("softwareVersion":\s*")[^"]*(")/, `$1${version}$2`, true],
  ['JSON-LD-修改日期', /("dateModified":\s*")[^"]*(")/, `$1${today}$2`, false]
]

if (setupMB) {
  rules.push(['JSON-LD-安装包大小', /("fileSize":\s*")[^"]*(")/, `$1${setupMB}MB$2`, false])
  rules.push([
    '参数表-安装包大小',
    /(<dt>安装包<\/dt><dd>)[^<]*(<\/dd>)/,
    `$1${setupMB} MB · 绿色版 ${zipMB ?? '0.00'} MB$2`,
    false
  ])
  rules.push([
    '下载卡-安装版',
    /(<span class="size">)[0-9.]+ MB(<\/span>\s*<strong>安装版)/,
    `$1${setupMB} MB$2`,
    false
  ])
}
if (zipMB) {
  rules.push([
    '下载卡-绿色版',
    /(<span class="size">)[0-9.]+ MB(<\/span>\s*<strong>绿色版)/,
    `$1${zipMB} MB$2`,
    false
  ])
}

let html = readFileSync(SITE_FILE, 'utf8')
const original = html
const failures = []
const warnings = []

for (const [name, re, to, critical] of rules) {
  // 命中判断用「结构是否还在」，不能用「替换后是否变化」——
  // 页面已经是最新版本时，替换结果自然和原文相同，那样会把
  // 「幂等重跑」误判成「正则失效」，导致 CI 直接失败。
  // （0.2.0 重发时就踩了这个：站点早已同步、脚本却报 4 条规则未命中。）
  const probe = new RegExp(re.source, re.flags.replace('g', ''))
  if (!probe.test(html)) {
    ;(critical ? failures : warnings).push(name)
    continue
  }
  html = html.replace(re, to)
}

if (html === original) {
  console.log(`站点已是 ${version}，无需改动`)
  process.exit(0)
}

if (failures.length) {
  console.error('以下必须替换的项没命中，页面结构可能改过了：')
  for (const f of failures) console.error(`  - ${f}`)
  process.exit(1)
}

if (warnings.length) {
  console.warn('以下项没命中（通常无害，页面结构变动时会出现）：')
  for (const w of warnings) console.warn(`  - ${w}`)
}

if (CHECK_ONLY) {
  console.error(`站点版本与工程不一致（工程 ${version}），跑一次不带 --check 的即可修复`)
  process.exit(1)
}

writeFileSync(SITE_FILE, html)
console.log(`已同步 site/index.html → ${version}（${stage}）`)
if (setupMB) console.log(`  安装包 ${setupMB} MB · 绿色版 ${zipMB ?? '未知'} MB`)

// llms.txt 也写死了版本号（"当前版本：0.1.0（预览版）"），一并同步，
// 不然 AI 引擎读到的还是两个版本前的信息
const LLM_FILE = join(ROOT, 'site', 'llms.txt')
if (existsSync(LLM_FILE)) {
  const llms = readFileSync(LLM_FILE, 'utf8')
  const llmsNext = llms
    .replace(/(当前版本：)\d+(?:\.\d+)*（[^）]*）/g, `$1${version}（${stage}）`)
    .replace(/(注明版本 )\d+(?:\.\d+)*( 与日期 )\d{4}-\d{2}-\d{2}/g, `$1${version}$2${today}`)
  if (llmsNext !== llms) {
    writeFileSync(LLM_FILE, llmsNext, 'utf8')
    console.log(`已同步 site/llms.txt → ${version}（${stage}）`)
  }
}
