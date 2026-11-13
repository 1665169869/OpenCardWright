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

/**
 * 音色列表是**异步**加载的：WebView2 刚启动时 `getVoices()` 常返回空数组。
 * 这个标记只用来判断"已经尝试过预热"，避免每次提醒都重复注册监听。
 */
let voicesListenerAttached = false

/** 预热 TTS 音色列表（在用户手势里调用，与 `unlockSound` 一起）。 */
export function primeSpeech(): void {
  try {
    const synth = window.speechSynthesis
    if (!synth) return
    // 先取一次，触发底层加载
    if (synth.getVoices().length > 0) return
    if (voicesListenerAttached) return
    voicesListenerAttached = true
    synth.addEventListener('voiceschanged', () => {
      // 只为触发加载，读取结果不需要处理
      synth.getVoices()
    })
  } catch {
    // 没有 TTS 也不影响提示音
  }
}

/** 在用户手势里调用一次，解锁音频上下文（并预热语音音色）。 */
export function unlockSound(): void {
  const c = ensureContext()
  if (c && c.state === 'suspended') {
    void c.resume()
  }
  primeSpeech()
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
/**
 * 用系统 TTS 播报一句话，返回**是否真的入队**。
 *
 * 依赖 WebView2 的 `speechSynthesis` 与系统中文语音包 —— 音色随系统，不承诺"温柔女声"。
 * 没有 zh 音色、合成失败、API 不存在都返回 false，由调用方决定回退提示音。
 *
 * TD-226：`getVoices()` 是异步加载的，**空数组不代表没有中文音色**（WebView2 刚启动时常见）。
 * 以前这里在 `voices.length === 0` 时直接入队并返回 true，实际念不出声，
 * 又因为"返回了 true"导致调用方不回退 `playAlert()` —— 结果就是完全没声音。
 * 现在空列表一律返回 false，让调用方退回提示音。
 */
export function speak(text: string): boolean {
  try {
    const synth = window.speechSynthesis
    if (!synth || typeof SpeechSynthesisUtterance === 'undefined') return false
    const voices = synth.getVoices()
    // 音色还没加载好 / 没有中文音色：都不能保证念得出来，交给提示音兜底
    if (voices.length === 0) return false
    const zh = voices.find((v) => v.lang && v.lang.toLowerCase().startsWith('zh'))
    if (!zh) return false
    const utter = new SpeechSynthesisUtterance(text)
    utter.voice = zh
    utter.lang = zh.lang
    utter.rate = 0.9
    utter.pitch = 1
    synth.cancel()
    synth.speak(utter)
    return true
  } catch {
    return false
  }
}

/**
 * 试听（TD-226）：与真实提醒**走同一条路径** —— 语音优先、失败 / 不可用退回提示音。
 *
 * 以前试听只播提示音，而真实提醒先试语音，所以会出现「试听能响、到点不响」。
 */
export function tryPreview(voice = true): { spoke: boolean; sounded: boolean } {
  unlockSound()
  const spoke = voice ? speak('提醒测试，语音播报正常。') : false
  if (!spoke) playAlert()
  return { spoke, sounded: spoke || ctx !== null }
}
