#!/usr/bin/env node
/**
 * 把 CHANGELOG.md 渲染进产品页 site/index.html 的「更新日志」区块。
 *
 * 产品页原来只有页脚一个「更新日志」外链 —— 用户在页面上看不到改了什么，
 * 点出去还得在 GitHub 翻。CHANGELOG.md 是唯一事实来源，这里把它渲染成静态
 * HTML 塞进页面的标记区（changelog:start / end），发版后 release.yml 跑一遍
 * 就和仓库同步，不用手工维护两份。
 *
 * 渲染规则：
 *   - 每个版本一张可折叠卡片，最新一版默认展开（<details open>）
 *   - 「预览版 / 正式版」按版本号推导：0.x → 预览版，≥1.0 → 正式版，
 *     和 release.yml 的 prerelease 判定保持一致
 *   - 支持 ### 小节、列表（含嵌套续行）、管道表格、**加粗**、`行内代码`
 *
 * 用法：node scripts/sync-site-changelog.mjs [--check]
 *   --check  只检查是否已同步，不改文件（不一致时退出码 1）
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SITE_FILE = join(ROOT, 'site', 'index.html')
const CHANGELOG_FILE = join(ROOT, 'CHANGELOG.md')
const CHECK_ONLY = process.argv.includes('--check')
const MAX_SECTIONS = 6 // 页面只放最近几版，更早的去 GitHub 看完整文件

const START = '<!-- changelog:start -->'
const END = '<!-- changelog:end -->'

const escapeHtml = (s) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

const mdInline = (s) =>
  escapeHtml(s)
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/`([^`]+)`/g, '<code>$1</code>')

/** 0.x 全部当预览版，和 release.yml 的 prerelease 判定保持一致 */
const stageOf = (v) => (v.startsWith('0.') ? '预览版' : '正式版')

/** 解析 CHANGELOG.md → [{ version, date, lines }] */
function parseChangelog(text) {
  const lines = text.split('\n')
  const heads = []
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^## \[([^\]]+)\](?:\s*-\s*(\d{4}-\d{2}-\d{2}))?/)
    if (m) heads.push({ index: i, version: m[1].trim(), date: m[2] ?? '' })
  }
  return heads.map((h, i) => ({
    version: h.version,
    date: h.date,
    lines: lines.slice(h.index + 1, i + 1 < heads.length ? heads[i + 1].index : lines.length)
  }))
}

/**
 * 正文行 → HTML。状态机：小节标题 / 列表（含缩进续行）/ 管道表格 / 普通段落。
 * 换行续写的列表项直接并进上一条；嵌套子列表扁平化为同级 li（页面上够用）。
 */
function renderBody(lines) {
  const out = []
  let list = null // string[]：当前 <ul> 里的每条 li
  let table = null // string[][]：当前表格的行
  let para = null

  const flush = () => {
    if (list) {
      out.push('<ul>\n' + list.map((li) => `<li>${li}</li>`).join('\n') + '\n</ul>')
      list = null
    }
    if (table) {
      const [head, ...rows] = table.filter((r) => !r.every((c) => /^:?-{2,}:?$/.test(c.trim())))
      out.push(
        '<table>\n<thead><tr>' + head.map((c) => `<th>${mdInline(c)}</th>`).join('') +
        '</tr></thead>\n<tbody>' +
        rows.map((r) => '<tr>' + r.map((c) => `<td>${mdInline(c)}</td>`).join('') + '</tr>').join('\n') +
        '\n</tbody>\n</table>'
      )
      table = null
    }
    if (para) {
      out.push(`<p>${para}</p>`)
      para = null
    }
  }

  const cells = (line) =>
    line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim())

  for (const raw of lines) {
    const line = raw.replace(/\s+$/, '')
    const trimmed = line.trim()

    if (!trimmed) {
      flush()
      continue
    }
    if (trimmed.startsWith('### ')) {
      flush()
      out.push(`<h4>${mdInline(trimmed.slice(4))}</h4>`)
      continue
    }
    if (trimmed.startsWith('|')) {
      para = null // 表格前后不接段落
      if (!table) table = []
      table.push(cells(trimmed))
      continue
    }
    // 列表项：顶格或缩进的 "- " 都算一条 li（嵌套子列表扁平化）
    const li = trimmed.match(/^(?:-\s|\*\s)(.+)$/)
    if (li) {
      para = null
      if (!list) list = []
      list.push(mdInline(li[1]))
      continue
    }
    if (/^\s/.test(raw) && list) {
      // 列表项的换行续写，并进上一条
      list[list.length - 1] += ' ' + mdInline(trimmed)
      continue
    }
    // 普通文本：在列表里就并进上一条，否则是段落（markdown 续行合并）
    if (list) {
      list[list.length - 1] += ' ' + mdInline(trimmed)
    } else if (table) {
      flush()
      para = mdInline(trimmed)
    } else {
      para = para ? `${para} ${mdInline(trimmed)}` : mdInline(trimmed)
    }
  }
  flush()
  return out.join('\n')
}

function renderSection(sec, latest) {
  const badge = sec.version.startsWith('0.') ? 'preview' : 'stable'
  return [
    `  <details class="cl-item"${latest ? ' open' : ''}>`,
    '    <summary>',
    `      <span class="cl-ver">${escapeHtml(sec.version)}</span>`,
    `      <span class="cl-badge ${badge}">${stageOf(sec.version)}</span>`,
    `      <span class="cl-date">${escapeHtml(sec.date)}</span>`,
    '    </summary>',
    '    <div class="cl-body">',
    renderBody(sec.lines),
    '    </div>',
    '  </details>'
  ].join('\n')
}

// ---------- 主流程 ----------
const changelog = readFileSync(CHANGELOG_FILE, 'utf8')
const sections = parseChangelog(changelog).slice(0, MAX_SECTIONS)
if (!sections.length) {
  console.error('CHANGELOG.md 里没解析到任何版本小节（应有 "## [x.y.z] - 日期"）')
  process.exit(1)
}

const html = readFileSync(SITE_FILE, 'utf8')
if (!html.includes(START) || !html.includes(END)) {
  console.error(`site/index.html 缺少更新日志标记（${START} … ${END}），没法替换`)
  process.exit(1)
}

const inner = sections.map((s, i) => renderSection(s, i === 0)).join('\n\n')
const next = html.replace(
  new RegExp(`${START}[\\s\\S]*?${END}`),
  `${START}\n${inner}\n${END}`
)

if (next === html) {
  console.log(`更新日志已是最新（${sections.map((s) => s.version).join('、')}），无需改动`)
  process.exit(0)
}

if (CHECK_ONLY) {
  console.error('站点更新日志与 CHANGELOG.md 不一致，跑一次不带 --check 的即可修复')
  process.exit(1)
}

writeFileSync(SITE_FILE, next, 'utf8')
console.log(`已同步 site/index.html 更新日志 ← ${sections.map((s) => s.version).join('、')}`)
