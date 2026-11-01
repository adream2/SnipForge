/** 版本检查结果。字段和后端 `update::UpdateInfo` 一一对应。 */
export interface UpdateInfo {
  /** 当前运行版本 */
  current: string
  /** 远端最新版本（已去掉 v 前缀） */
  latest: string
  hasUpdate: boolean
  /** release 标题，如 "SnipForge 0.1.0 预览版" */
  title: string
  /** 发布说明（Markdown 原文，已截断） */
  notes: string
  publishedAt: string
  /** 是不是预发布版 */
  prerelease: boolean
  /** 发布页地址 */
  url: string
}
