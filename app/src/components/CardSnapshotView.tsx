import { Alert, Card, Descriptions, Divider, Tag, Typography } from 'antd'
import { formatCardNoText, formatExpiry, type CardSnapshot } from '../api'

const { Text, Paragraph } = Typography

/**
 * 卡片数据展示（卡型 / 房号 / 有效期 / 卡号 + 28 字节原文）。
 *
 * 读卡页与「读卡退房」弹窗共用：两处看到的字段、排序、异常提示必须一致，
 * 否则前台在弹窗里核对过的卡，到读卡页又对不上。
 *
 * `expiresAt` 来自本地发卡记录：有记录时在时长后补上到期时刻，
 * 卡内本身只有时长码、没有绝对到期时间。
 */
export function CardSnapshotView({
  card,
  expiresAt,
  createdAt,
}: {
  card: CardSnapshot
  /** 本地发卡记录里的到期时间；没有记录时由卡内数据推算 */
  expiresAt?: string | null
  /** 本地发卡记录里的发卡时间；没有记录时用卡内的发卡时刻 */
  createdAt?: string | null
}) {
  const duration =
    card.days > 0 ? `${card.days} 天` : card.hours > 0 ? `${card.hours} 小时` : '—'
  const expiry = formatExpiry(expiresAt)

  const roomText =
    card.room !== null
      ? card.room
      : card.roomIndex >= 0
        ? `序号 ${card.roomIndex}（未登记）`
        : '—'

  const abnormal = card.rawHex.length !== 56

  /**
   * 卡内有效期：**完全由卡内数据算**（发卡时刻 + 时长码），与数据库里的 expires_at 无关。
   * 发卡时刻 = 页2[3] 的年月 + 卡号的「日/时」；时长 = 页2[0] 解出的 days / hours。
   * 月数卡（0xC_ 段）的 days 是「月数×30」的近似，这里照此推算并标注。
   */
  const cardValidText = (() => {
    const hi = (card.cardYm >> 4) & 0x0f
    const month = card.cardYm & 0x0f
    if (month < 1 || month > 12) return '—'
    if (card.issueDay < 1 || card.issueHour < 0) return '—'
    if (card.days <= 0 && card.hours <= 0) return '—'
    const nowYear = new Date().getFullYear()
    const base = 2000 + 16 * Math.floor((nowYear - 2000) / 16)
    const start = new Date(base + hi, month - 1, card.issueDay, card.issueHour, 0, 0)
    const end =
      card.hours > 0
        ? new Date(start.getTime() + card.hours * 3600_000)
        : new Date(start.getTime() + card.days * 86400_000)
    const pad = (n: number) => String(n).padStart(2, '0')
    const text = `${end.getFullYear()}-${pad(end.getMonth() + 1)}-${pad(end.getDate())} ${pad(end.getHours())}:${pad(end.getMinutes())}`
    return card.days > 63 ? `${text}（按月数近似）` : text
  })()

  /** 卡内入住时刻：页2[3] 的年月 + 卡号里的日/时，格式 `YYYY-MM-DD HH:00` */
  const cardCheckInText = (() => {
    const hi = (card.cardYm >> 4) & 0x0f
    const month = card.cardYm & 0x0f
    if (month < 1 || month > 12 || card.issueDay < 1 || card.issueHour < 0) return null
    const nowYear = new Date().getFullYear()
    const base = 2000 + 16 * Math.floor((nowYear - 2000) / 16)
    const pad = (n: number) => String(n).padStart(2, '0')
    return `${base + hi}-${pad(month)}-${pad(card.issueDay)} ${pad(card.issueHour)}:00`
  })()

  return (
    <Card type="inner" title="卡片数据" extra={<Tag color="blue">{card.kindLabel}</Tag>}>
      <Descriptions
        column={2}
        size="small"
        items={[
          { key: 'kind', label: '卡型', children: card.kindLabel },
          { key: 'room', label: '房号', children: roomText },
          { key: 'duration', label: '有效期', children: duration },
          // 库里有记录就用库的，没有记录就按卡内数据推算
          { key: 'expire', label: '到期时间', children: expiry ?? cardValidText ?? '—' },
          {
            key: 'checkIn',
            label: '入住时间',
            children: formatExpiry(createdAt) ?? cardCheckInText ?? '—',
          },
          { key: 'cardNo', label: '卡号', children: formatCardNoText(card) },
          {
            key: 'lost',
            label: '挂失',
            children: card.isLost ? <Tag color="red">挂失卡</Tag> : '否',
          },
        ]}
      />

      <Divider orientation="left" plain>
        原始数据（28 字节）
      </Divider>
      <Paragraph copyable={{ text: card.rawHex, tooltips: ['复制', '已复制'] }} style={{ marginBottom: 0 }}>
        <Text className="ocw-hex">{card.rawHex}</Text>
      </Paragraph>

      {abnormal ? (
        <Alert
          type="warning"
          showIcon
          style={{ marginTop: 12 }}
          message="原始数据长度异常"
          description={`应为 56 个十六进制字符，实际 ${card.rawHex.length} 个。卡片数据仍按原样展示，请先确认读卡器是否正常。`}
        />
      ) : null}
    </Card>
  )
}
