/**
 * 从 CHANGELOG.md 里取当前版本的说明，写到 `.release-notes.md`。
 *
 * 为什么不直接用 GitHub 的 `generate_release_notes`：那玩意儿是按 PR/commit
 * 自动拼接的，标题就是 commit message。我们的 commit 是给人看的工程记录
 * （"收拢同一个事实写好几遍的地方"），不适合当发布说明。发布说明要写
 * **用户能感知的变化**，所以以手写的 CHANGELOG 为准。
 *
 * 用法：node scripts/release-notes.mjs [版本号]
 *   不给版本号时取 tauri.conf.json 里的 version。
 */
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '..')
const conf = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const version = process.argv[2] || conf.version

const changelog = fs.readFileSync(path.join(root, 'CHANGELOG.md'), 'utf8')
const lines = changelog.split('\n')

// 找到 "## [x.y.z]" 这一节，取到下一个 "## [" 之前
const start = lines.findIndex((l) => l.startsWith(`## [${version}]`))
if (start === -1) {
  console.error(`CHANGELOG.md 里没有 ${version} 这一节，发布说明不能凭空生成`)
  process.exit(1)
}
let end = lines.length
for (let i = start + 1; i < lines.length; i++) {
  if (lines[i].startsWith('## [')) {
    end = i
    break
  }
}

const section = lines.slice(start, end).join('\n').trim()
const out = path.join(root, '.release-notes.md')
fs.writeFileSync(out, section + '\n', 'utf8')
console.log(`== 发布说明（${version}，${section.length} 字符）==`)
console.log(section.split('\n').slice(0, 12).join('\n'))
console.log(`== 已写入 ${path.relative(root, out)} ==`)
