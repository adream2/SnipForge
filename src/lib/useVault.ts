import { useEffect, useState } from 'react'
import { api } from './api'
import type { VaultStatus } from './types'

/**
 * 读一次主密码保险箱状态。
 *
 * 加锁后凭据在内存里是空的：图床 token 读不到、收藏列表为空。
 * 相关页面用它显示"去设置里解锁"，否则用户只会看到"东西全没了"。
 */
export function useVaultStatus(): VaultStatus | null {
  const [status, setStatus] = useState<VaultStatus | null>(null)
  useEffect(() => {
    let alive = true
    void api
      .vaultStatus()
      .then((s) => alive && setStatus(s))
      .catch(() => alive && setStatus(null))
    return () => {
      alive = false
    }
  }, [])
  return status
}

/** 设了主密码但没解锁 */
export const isLocked = (v: VaultStatus | null): boolean => !!v?.enabled && !v.unlocked
