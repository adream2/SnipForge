#!/usr/bin/env node
/**
 * 把产品页推给 IndexNow（Bing / Yandex / Seznam 共用的一份提交接口）。
 *
 * 为什么不只等爬虫自己来：新站点没有外链，Bing 可能几周都不来一次；
 * IndexNow 是「我改了，来抓」的主动通知，提交即生效，不需要账号和 token。
 *
 * 密钥就是 `site/` 下那个 32 位十六进制文件，文件名和内容都等于密钥 ——
 * 这是 IndexNow 的校验方式，公开是正常的。
 *
 * 用法：node scripts/indexnow.mjs            # 部署完之后跑
 *       node scripts/indexnow.mjs --dry      # 只看要发什么，不发请求
 */

import { readdirSync, readFileSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SITE = join(ROOT, 'site')
const SITE_URL = 'https://adream2.github.io/SnipForge/'
const HOST = new URL(SITE_URL).host

const keyFile = readdirSync(SITE).find((f) => /^[0-9a-f]{32}\.txt$/.test(f))
if (!keyFile) {
  console.error('site/ 下没有 IndexNow 密钥文件（32 位十六进制.txt）')
  process.exit(1)
}
const key = readFileSync(join(SITE, keyFile), 'utf8').trim()
if (key !== keyFile.replace(/\.txt$/, '')) {
  console.error(`密钥文件内容与文件名不一致：${keyFile}`)
  process.exit(1)
}

const payload = {
  host: HOST,
  key,
  keyLocation: `${SITE_URL}${keyFile}`,
  urlList: [SITE_URL]
}

if (process.argv.includes('--dry')) {
  console.log(JSON.stringify(payload, null, 2))
  process.exit(0)
}

const res = await fetch('https://api.indexnow.org/indexnow', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json; charset=utf-8' },
  body: JSON.stringify(payload)
})

// 200 = 已接受；202 = 已接受但密钥待校验；422 = 地址和密钥不匹配；429 = 提交太频繁
if (res.status === 200 || res.status === 202) {
  console.log(`IndexNow 已接受（HTTP ${res.status}）：${payload.urlList.join(', ')}`)
} else {
  console.error(`IndexNow 返回 HTTP ${res.status}：${await res.text()}`)
  process.exit(1)
}
