import { Alert, Card, Descriptions, Divider, Tag, Typography } from 'antd'
import { formatCardNo, formatExpiry, type CardSnapshot } from '../api'

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
}: {
  card: CardSnapshot
  expiresAt?: string | null
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

  return (
    <Card type="inner" title="卡片数据" extra={<Tag color="blue">{card.kindLabel}</Tag>}>
      <Descriptions
        column={2}
        size="small"
        items={[
          { key: 'kind', label: '卡型', children: card.kindLabel },
          { key: 'room', label: '房号', children: roomText },
          { key: 'duration', label: '有效期', children: expiry ? `${duration} · ${expiry}` : duration },
          { key: 'cardNo', label: '卡号', children: formatCardNo(card.cardNo) },
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
