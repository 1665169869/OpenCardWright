/**
 * 系统通知封装（TD-228）。
 *
 * 到期提醒以前只发应用内 antd 通知（`AntdApp.useApp().notification`），窗口最小化或
 * 不在前台时看不到。这里走 `@tauri-apps/plugin-notification` → Windows 原生 toast，
 * 与应用内通知**双管**：系统通知可用就发，被拒 / 不支持时调用方保留应用内通知。
 *
 * 注意：WebView2 **不实现** Web Notifications API，不能用 `new Notification()`。
 */

let grantedCache: boolean | null = null

/**
 * 确保拿到通知权限；返回是否可用。
 *
 * 首次调用会弹系统权限请求（Windows 上通常是"通知和操作"开关），结果缓存起来，
 * 避免每条提醒都弹一次。失败一律返回 `false`，绝不抛给提醒轮询。
 */
export async function ensureNotifyPermission(): Promise<boolean> {
  if (grantedCache !== null) return grantedCache
  try {
    const n = await import('@tauri-apps/plugin-notification')
    let granted = await n.isPermissionGranted()
    if (!granted) {
      const res = await n.requestPermission()
      granted = res === 'granted'
    }
    grantedCache = granted
    return granted
  } catch {
    // 非 Tauri 环境（纯 vite dev）、插件缺失、capability 未授权都会走到这里
    grantedCache = false
    return false
  }
}

/**
 * 发一条系统通知；返回是否真的发出去了。
 *
 * 调用方拿到 `false` 时应保留应用内通知 —— 这是"双管"的降级路径。
 */
export async function systemNotify(title: string, body: string): Promise<boolean> {
  const ok = await ensureNotifyPermission()
  if (!ok) return false
  try {
    const n = await import('@tauri-apps/plugin-notification')
    n.sendNotification({ title, body })
    return true
  } catch {
    return false
  }
}
