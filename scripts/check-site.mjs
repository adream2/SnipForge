#!/usr/bin/env node
/**
 * 站点自检：上线前把「会 404」「SEO 标签写漏」这两类低级问题挡在 CI 里。
 *
 * 检查项：
 *   1. 必备文件在不在（robots / sitemap / llms.txt / og 图 / .nojekyll）
 *   2. HTML 里引用的本地资源是否都存在（改名忘了改引用 → 这里报错）
 *   3. JSON-LD 能不能解析、有没有 canonical / og:image / twitter:card
 *   4. 每张 <img> 都有 alt（无障碍与图片 SEO）
 *   5. 没把 localhost 之类的调试地址带上线
 *
 * 用法：node scripts/check-site.mjs
 */

import { readFileSync, existsSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SITE = join(ROOT, 'site')
const SITE_URL = 'https://adream2.github.io/SnipForge/'

const errors = []
const fail = (msg) => errors.push(msg)

// ---------- 1. 必备文件 ----------
const required = [
  'index.html',
  'robots.txt',
  'sitemap.xml',
  'llms.txt',
  '.nojekyll',
  'assets/og.png',
  'assets/icon.png'
]
for (const f of required) {
  if (!existsSync(join(SITE, f))) fail(`缺少文件 site/${f}`)
}

const html = readFileSync(join(SITE, 'index.html'), 'utf8')

// ---------- 2. 本地资源引用 ----------
const refs = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1])
const local = refs.filter(
  (r) => !/^(https?:|mailto:|#|data:|\/\/)/.test(r) && !r.startsWith('//')
)
for (const r of new Set(local)) {
  const clean = r.split(/[?#]/)[0]
  if (!clean) continue
  if (!existsSync(join(SITE, clean))) fail(`引用了不存在的资源: ${r}`)
}

// ---------- 3. 关键 meta 与结构化数据 ----------
const must = [
  ['canonical', /<link rel="canonical" href="https:\/\/[^"]+">/],
  ['og:title', /<meta property="og:title" content="[^"]+">/],
  ['og:description', /<meta property="og:description" content="[^"]+">/],
  ['og:image（绝对地址）', /<meta property="og:image" content="https:\/\/[^"]+">/],
  ['twitter:card', /<meta name="twitter:card" content="summary_large_image">/],
  ['viewport', /<meta name="viewport" content="[^"]+">/],
  ['description', /<meta name="description" content="[^"]{40,}">/]
]
for (const [name, re] of must) {
  if (!re.test(html)) fail(`缺少或格式不对：${name}`)
}

const canonical = html.match(/<link rel="canonical" href="([^"]+)">/)?.[1]
if (canonical && canonical !== SITE_URL) fail(`canonical 应为 ${SITE_URL}，实际 ${canonical}`)

const blocks = [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)]
if (blocks.length === 0) fail('没有 JSON-LD 结构化数据')
const types = []
for (const [, body] of blocks) {
  try {
    const data = JSON.parse(body)
    const nodes = data['@graph'] ?? [data]
    for (const n of nodes) types.push(n['@type'])
  } catch (e) {
    fail(`JSON-LD 解析失败: ${e.message}`)
  }
}
for (const t of ['SoftwareApplication', 'FAQPage', 'WebSite']) {
  if (!types.includes(t)) fail(`JSON-LD 缺少 @type=${t}`)
}

// ---------- 4. 图片 alt ----------
const imgs = [...html.matchAll(/<img\b[^>]*>/g)].map((m) => m[0])
for (const tag of imgs) {
  if (!/\balt="/.test(tag)) fail(`<img> 缺 alt: ${tag.slice(0, 80)}`)
}

// ---------- 5. 调试残留 ----------
for (const bad of ['localhost', '127.0.0.1', 'file://', 'TODO']) {
  if (html.includes(bad)) fail(`页面里残留了 ${bad}`)
}

// ---------- 6. sitemap / robots 一致性 ----------
const sitemap = existsSync(join(SITE, 'sitemap.xml'))
  ? readFileSync(join(SITE, 'sitemap.xml'), 'utf8')
  : ''
if (sitemap && !sitemap.includes(SITE_URL)) fail(`sitemap.xml 里没有 ${SITE_URL}`)
const robots = existsSync(join(SITE, 'robots.txt'))
  ? readFileSync(join(SITE, 'robots.txt'), 'utf8')
  : ''
if (robots && !robots.includes(`${SITE_URL}sitemap.xml`)) fail('robots.txt 没指向 sitemap')

// ---------- 结果 ----------
if (errors.length) {
  console.error(`站点自检未通过（${errors.length} 项）：`)
  for (const e of errors) console.error(`  ✗ ${e}`)
  process.exit(1)
}
console.log(
  `站点自检通过：${required.length} 个必备文件、${new Set(local).size} 个本地引用、` +
    `${imgs.length} 张图、结构化数据 ${types.join(' / ')}`
)
