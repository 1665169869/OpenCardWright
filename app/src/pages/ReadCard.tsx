import { useState } from 'react'
import { Alert, Button, Card, Space, Typography } from 'antd'
import {
  api,
  describeError,
  formatExpiry,
  formatTime,
  type CardReadDto,
  type RecordLookup,
} from '../api'
import { CardSnapshotView } from '../components/CardSnapshotView'

const { Text } = Typography

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
          <CardSnapshotView
            card={data.card}
            expiresAt={data.lookup.status === 'found' ? data.lookup.latest.expiresAt : null}
            createdAt={data.lookup.status === 'found' ? data.lookup.latest.createdAt : null}
          />
          <RecordBanner lookup={data.lookup} />
        </Space>
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
          message={`已找到发卡记录（该卡共 ${lookup.count} 条记录）`}
          description={
            <>
              <Space size="large" wrap>
                <span>发放时间：{formatTime(lookup.latest.createdAt)}</span>
                {formatExpiry(lookup.latest.expiresAt) ? (
                  <span>{formatExpiry(lookup.latest.expiresAt)}</span>
                ) : null}
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
