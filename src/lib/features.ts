import { useSyncExternalStore } from 'react'
import { api } from './api'

/**
 * 功能表 —— "能被手势/托盘触发的功能"的唯一来源。
 *
 * 名字和 id 都定义在后端 `features.rs`，前端只读。以前这里是硬编码的一份
 * `SNIP_IDS`，和后端 `describe`/`run_snip` 各写一套，加功能要改三处，
 * 漏一处就变成"下拉里有、真划没反应"。
 *
 * 用 `useSyncExternalStore` 而不是每页 fetch：这份表全局只拉一次，
 * 组件挂载时同步拿到当前值，拉回来之后自动重渲染。
 */
export interface FeatureInfo {
  id: string
  label: string
  /** 会抓屏幕（截图/录屏）：执行前会先收起手势轨迹 */
  grabsScreen: boolean
}

let cache: FeatureInfo[] = []
let started = false
const subs = new Set<() => void>()

function emit() {
  for (const f of subs) f()
}

function subscribe(cb: () => void) {
  subs.add(cb)
  loadFeatures()
  return () => {
    subs.delete(cb)
  }
}

/** 幂等：拉一次就够，失败（比如页面单独跑在浏览器里）下次再试 */
export function loadFeatures() {
  if (started) return
  started = true
  void api
    .featuresList()
    .then((list) => {
      cache = list
      emit()
    })
    .catch(() => {
      started = false
    })
}

const snapshot = () => cache

export function useFeatures(): FeatureInfo[] {
  loadFeatures()
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}

/** 同步取名字。表还没到就回落到 id —— 比显示"未知操作"好排查 */
export function featureLabel(id: string): string {
  return cache.find((f) => f.id === id)?.label ?? id
}
