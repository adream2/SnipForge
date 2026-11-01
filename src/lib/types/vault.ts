/** 主密码保险箱的状态。 */

export interface VaultStatus {
  /** 设了主密码（磁盘上是密文） */
  enabled: boolean
  /** 当前已解锁（凭据在内存里可用） */
  unlocked: boolean
}
