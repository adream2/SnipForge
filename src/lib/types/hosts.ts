/** hosts 编辑：文件内容 + 权限/编码状态 */
export interface HostsInfo {
  path: string
  text: string
  /** 程序本身是否已有管理员权限（没有的话保存会弹 UAC） */
  elevated: boolean
  /** 文件里有非 UTF-8 字节，保存会转成 UTF-8 */
  nonUtf8: boolean
  /** 文件实际的换行风格（保存时统一成 CRLF） */
  eol: 'crlf' | 'lf' | 'cr' | 'mixed'
  /** 现在真的能写吗（试过写句柄，不看 readonly 属性） */
  writable: boolean
}

export interface HostsBackup {
  name: string
  size: number
}

export interface HostsSaveOutcome {
  /** 这次是否走了提权 */
  elevated: boolean
  /** 覆盖前的备份文件名（空 = 文件本来不存在） */
  backup: string
}
