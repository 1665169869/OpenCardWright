import { Badge, Button, Empty, List, Popover, Space, Tag, Typography } from 'antd'
import { formatCardNo, formatExpiry, type ExpiringCard } from '../api'

const { Text } = Typography

/** 把到期时间换算成"还剩多少 / 已过期多久"。 */
function relative(expiresAt: string, now: number): { text: string; expired: boolean } {
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
 * 顶栏的到期铃铛。
 *
 * 数据由 App 轮询后传进来；这里只负责展示。
 */
export default function ExpiryBell({ items }: { items: ExpiringCard[] }) {
  if (items.length === 0) return null

  const now = Date.now()

  const content = (
    <div style={{ width: 360 }}>
      <List
        size="small"
        dataSource={items}
        locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} /> }}
        renderItem={(c) => {
          const r = relative(c.expiresAt, now)
          return (
            <List.Item>
              <Space direction="vertical" size={2} style={{ width: '100%' }}>
                <Space size="small" wrap>
                  <Text strong>{c.roomNo ?? '未知房号'}</Text>
                  <Tag color="blue">{c.kind}</Tag>
                  <Text type="secondary">卡号 {formatCardNo(c.cardNo)}</Text>
                  <Tag color={r.expired ? 'red' : 'orange'}>{r.text}</Tag>
                </Space>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {formatExpiry(c.expiresAt)}
                </Text>
              </Space>
            </List.Item>
          )
        }}
      />
      <Text type="secondary" style={{ fontSize: 12 }}>
        锁实际是从首次刷卡起算，这里按发卡时间估算，可能偏早。
      </Text>
    </div>
  )

  return (
    <Popover content={content} title="客房卡到期" trigger="click" placement="bottomRight">
      <Badge count={items.length} size="small" offset={[-2, 2]}>
        <Button size="small" danger>
          到期提醒
        </Button>
      </Badge>
    </Popover>
  )
}
