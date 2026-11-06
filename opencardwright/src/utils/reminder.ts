/**
 * 到期提醒的文案工具。
 *
 * 抽出来是为了让铃铛、通知、语音播报**说同一句话** —— 三处各写一套的话，
 * 迟早会出现"铃铛说还剩 1 小时、语音说已过期"这种自相矛盾。
 */

/** 把到期时间换算成"还剩多少 / 已过期多久"。 */
export function relative(expiresAt: string, now: number): { text: string; expired: boolean } {
  const t = new Date(expiresAt).getTime()
  if (Number.isNaN(t)) return { text: '时间未知', expired: false }
  const diffMin = Math.round((t - now) / 60000)
  if (diffMin >= 0) {
    if (diffMin < 60) return { text: `还剩 ${diffMin} 分钟`, expired: false }
    const h = Math.floor(diffMin / 60)
    const m = diffMin % 60
    return { text: `还剩 ${h} 小时${m ? ` ${m} 分` : ''}`, expired: false }
  }
  const ago = -diffMin
  if (ago < 60) return { text: `已过期 ${ago} 分钟`, expired: true }
  const h = Math.floor(ago / 60)
  return { text: `已过期 ${h} 小时`, expired: true }
}

/**
 * 语音播报文案。
 *
 * 口播比屏幕精简：只报房号 + 剩余时间，卡号和卡型留给屏幕（念出来没人记得住）。
 * 不足 1 小时按分钟说，已过期说"已过期 X 分钟"。
 */
export function expirySpeech(
  card: { roomNo: string | null; expiresAt: string },
  now: number,
): string {
  const room = card.roomNo ?? '未知房号'
  const { text, expired } = relative(card.expiresAt, now)
  if (text === '时间未知') return `${room} 房卡到期时间未知，请检查。`
  return expired ? `${room} 房的卡${text}，请及时处理。` : `${room} 房${text}到期，请提醒客人。`
}
