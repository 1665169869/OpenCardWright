import { useState } from 'react'
import { Alert, Button, Card, Descriptions, Divider, Space, Tag, Typography } from 'antd'
import {
  api,
  describeError,
  formatCardNo,
  formatTime,
  type CardReadDto,
  type CardSnapshot,
  type RecordLookup,
} from '../api'

const { Text, Paragraph } = Typography

/**
 * 读卡页：A 区（卡片数据）永远展示，B 区（发卡记录）只影响提示条。
 * 需求原文：数据库没有该卡片数据只做"未查到"的提示，但继续显示卡片数据信息。
 */
export default function ReadCard() {
  const [loading, setLoading] = useState(false)
  const [data, setData] = useState<CardReadDto | null>(null)
  const [error, setError] = useState<string | null>(null)

  const read = async () => {
    setLoading(true)
    setError(null)
    try {
      setData(await api.readCard())
    } catch (e) {
      setError(describeError(e))
      setData(null)
    } finally {
      setLoading(false)
    }
  }

  return (
    <Card
      title="读卡"
      extra={
        <Button type="primary" onClick={read} loading={loading}>
          读卡
        </Button>
      }
    >
      {error ? (
        <Alert type="error" showIcon message="读卡失败" description={error} style={{ marginBottom: 16 }} />
      ) : null}

      {!data && !error ? (
        <Text type="secondary">把卡放到读卡器上，然后点右上角「读卡」。</Text>
      ) : null}

      {data ? (
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <CardSnapshotView card={data.card} />
          <RecordBanner lookup={data.lookup} />
        </Space>
      ) : null}
    </Card>
  )
}

function CardSnapshotView({ card }: { card: CardSnapshot }) {
  const duration =
    card.days > 0 ? `${card.days} 天` : card.hours > 0 ? `${card.hours} 小时` : '—'

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
          { key: 'duration', label: '有效期', children: duration },
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

function RecordBanner({ lookup }: { lookup: RecordLookup }) {
  switch (lookup.status) {
    case 'found':
      return (
        <Alert
          type="success"
          showIcon
          message={`已找到发卡记录（该卡号共 ${lookup.count} 条）`}
          description={
            <>
              <Space size="large" wrap>
                <span>发放时间：{formatTime(lookup.latest.createdAt)}</span>
                <span>房号：{lookup.latest.roomNo ?? '—'}</span>
                <span>卡型：{lookup.latest.kind}</span>
                <span>结果：{lookup.latest.result}</span>
              </Space>
              {lookup.latest.guestName ? (
                <div style={{ marginTop: 6 }}>
                  客人：{lookup.latest.guestName}
                  {lookup.latest.guestGender ? ` · ${lookup.latest.guestGender}` : ''}
                  {lookup.latest.guestIdType ? ` · ${lookup.latest.guestIdType}` : ''}
                  {lookup.latest.guestIdNumber ? ` ${lookup.latest.guestIdNumber}` : ''}
                </div>
              ) : null}
            </>
          }
        />
      )

    case 'notFound':
      return (
        <Alert
          type="warning"
          showIcon
          message="未查到该卡的发卡记录"
          description="该卡可能由原厂软件或其它机器发放。卡片数据仍照常显示，不影响对卡本身的判断。"
        />
      )

    case 'notApplicable':
      return <Alert type="info" showIcon message={lookup.reason} />

    case 'failed':
      return (
        <Alert
          type="error"
          showIcon
          message="记录查询失败"
          description={`${lookup.message} —— 这不是"未查到"，本地数据库可能出了问题，请检查后重试。`}
        />
      )
  }
}
