import { Badge, Button, Empty, List, Popover, Space, Tag, Typography } from 'antd'
import { formatExpiry, type ExpiringCard } from '../api'
import { relative } from '../utils/reminder'

const { Text } = Typography

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
