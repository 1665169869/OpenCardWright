/**
 * 提醒音。
 *
 * 用 Web Audio 现场合成，不引入音频文件（免得到处塞二进制）。
 * 浏览器的自动播放策略要求先有用户手势，所以登录按钮点击时调一次 `unlockSound()`；
 * 没解锁成功时 `playAlert()` 静默返回，提醒仍以弹窗形式出现。
 */

let ctx: AudioContext | null = null

function ensureContext(): AudioContext | null {
  if (ctx) return ctx
  const Ctor: typeof AudioContext | undefined =
    window.AudioContext ??
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
  if (!Ctor) return null
  try {
    ctx = new Ctor()
    return ctx
  } catch {
    return null
  }
}

/** 在用户手势里调用一次，解锁音频上下文。 */
export function unlockSound(): void {
  const c = ensureContext()
  if (c && c.state === 'suspended') {
    void c.resume()
  }
}

/** 两声短促蜂鸣（约 0.4 秒）。 */
export function playAlert(): void {
  const c = ensureContext()
  if (!c) return
  if (c.state === 'suspended') {
    void c.resume()
  }

  const start = c.currentTime
  for (let i = 0; i < 2; i += 1) {
    const at = start + i * 0.22
    const osc = c.createOscillator()
    const gain = c.createGain()
    osc.type = 'sine'
    osc.frequency.setValueAtTime(880, at)
    gain.gain.setValueAtTime(0.0001, at)
    gain.gain.exponentialRampToValueAtTime(0.25, at + 0.02)
    gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.18)
    osc.connect(gain)
    gain.connect(c.destination)
    osc.start(at)
    osc.stop(at + 0.2)
  }
}

/** 仅供设置页「试听」用：强制尝试并返回是否可能发声。 */
export function tryPreview(): boolean {
  unlockSound()
  playAlert()
  return ctx !== null
}
