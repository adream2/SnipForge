/**
 * 打绿色包（免安装 zip）。
 *
 * 绿色包不能只有一个 exe —— `plugins/*.uploader.json` 是 exe 外部的资源，
 * 打包后由 `app.path().resource_dir()`（Windows 上就是 exe 所在目录）去找。
 * 只拷 exe 出去，插件页里的内置图床会整体消失，而且是静默消失（read_dir 失败
 * 在加载逻辑里是 continue），很难发现。所以这里把 plugins 一起放进去。
 *
 * 用法：
 *   npm run portable            # 用已有的 release exe
 *   npm run portable -- --build # 先完整构建再打（含前端、NSIS 特性）
 */
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '..')
const conf = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const { productName, version } = conf

const exe = path.join(root, 'src-tauri/target/release/snipforge.exe')
const wantBuild = process.argv.includes('--build')

const run = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { stdio: 'inherit', cwd: root, shell: false, ...opts })

if (wantBuild) {
  console.log('== 构建 release（tauri build --no-bundle）==')
  // custom-protocol 不能省：CLI 不会自动打开本 crate 的同名特性，
  // 漏了打出来的 exe 会去连 devUrl（127.0.0.1:5183）。
  run('npx', ['tauri', 'build', '--no-bundle', '--features', 'custom-protocol'])
}

if (!fs.existsSync(exe)) {
  console.error(`找不到 release 产物：${exe}\n先跑一次 \`npx tauri build\`，或 \`npm run portable -- --build\`。`)
  process.exit(1)
}

const outRoot = path.join(root, 'dist-portable')
const dirName = `${productName}-${version}-portable`
const dir = path.join(outRoot, dirName)
const zip = path.join(outRoot, `${productName}_${version}_x64_portable.zip`)

// 每次都从干净目录开始，避免上一次的残留文件混进包里。
//
// 但"递归删目录"在某些环境里会被接管（比如安全软件把 rm 改写成"移到回收站"，
// 而回收站不可用就整个失败）。那种情况下不能直接崩 —— 退一步清空内容，
// 效果一样，目录名也不用改（改了 zip 里就会多一层带时间戳的目录）。
try {
  fs.rmSync(dir, { recursive: true, force: true })
} catch (e) {
  console.warn(`清理 ${dirName} 失败，改为清空内容: ${e.message}`)
  if (fs.existsSync(dir)) {
    for (const name of fs.readdirSync(dir)) {
      try {
        fs.rmSync(path.join(dir, name), { recursive: true, force: true })
      } catch {
        // 单个文件都删不掉就覆盖写，同名文件反正会被 copyFileSync 盖掉
      }
    }
  }
}
fs.mkdirSync(path.join(dir, 'plugins'), { recursive: true })

fs.copyFileSync(exe, path.join(dir, `${productName}.exe`))

// 内置插件：随包分发的声明式图床
const pluginSrc = path.join(root, 'plugins')
let pluginCount = 0
if (fs.existsSync(pluginSrc)) {
  for (const name of fs.readdirSync(pluginSrc)) {
    if (!name.endsWith('.uploader.json') && !name.endsWith('.plugin.json')) continue
    fs.copyFileSync(path.join(pluginSrc, name), path.join(dir, 'plugins', name))
    pluginCount++
  }
}

const exeMb = (fs.statSync(path.join(dir, `${productName}.exe`)).size / 1048576).toFixed(2)
console.log(`== 组装完成 ==`)
console.log(`  ${path.relative(root, dir)}`)
console.log(`  ${productName}.exe  ${exeMb} MB`)
console.log(`  plugins/            ${pluginCount} 个内置插件`)

// 压缩：Windows 上用系统自带的 Compress-Archive，不额外引入依赖
if (fs.existsSync(zip)) {
  try {
    fs.rmSync(zip, { force: true })
  } catch (e) {
    console.warn(`删除旧 zip 失败（会被覆盖）: ${e.message}`)
  }
}
if (process.platform === 'win32') {
  run('powershell', [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    `Compress-Archive -Path '${dir}' -DestinationPath '${zip}' -CompressionLevel Optimal`
  ])
} else {
  run('zip', ['-rq', zip, dirName], { cwd: outRoot })
}

const zipMb = (fs.statSync(zip).size / 1048576).toFixed(2)
console.log(`== 绿色包 ==`)
console.log(`  ${path.relative(root, zip)}  ${zipMb} MB`)
console.log(`\n解压后双击 ${productName}.exe 即可运行；数据仍在 %APPDATA%\\${conf.identifier}。`)
