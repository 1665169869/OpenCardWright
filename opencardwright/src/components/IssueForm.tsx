import { useState } from 'react'
import { Alert, Button, Card, Descriptions, Input, Radio, Select, Space, Tag, Typography } from 'antd'
import {
  api,
  dayOptions,
  describeError,
  formatCardNo,
  formatExpiry,
  formatTime,
  GENDERS,
  hourOptions,
  GUEST_MAX_DAYS,
  GUEST_MAX_HOURS,
  ID_TYPES,
  type GuestInput,
  type IssuedCard,
  type Room,
} from '../api'

const { TextArea } = Input
const { Text } = Typography

type Mode = 'day' | 'hour'

/**
 * 发客卡表单（房间已确定）。
 *
 * 从原来的「发卡」页抽出来，现在直接嵌在房间抽屉里：选时长 → 填客人信息 → 发卡。
 * 幂等键每次点击新生成，请求期间按钮禁用。
 */
export default function IssueForm({
  room,
  onIssued,
  defaultDays = 1,
  renewFrom,
  initialGuest,
  defaultMode = 'day',
  moveNotice = false,
}: {
  room: Room
  onIssued?: () => void
  /** 续住时按剩余天数预填 */
  defaultDays?: number
  /** 续住时的"当前有效期至"，只用于提示 */
  renewFrom?: string | null
  /** 换房带入的客人信息（只存库，不写卡） */
  initialGuest?: GuestInput
  /** 换房带入的时长模式（日租 / 钟点） */
  defaultMode?: Mode
  /** 换房场景：提示旧卡仍能开源房的门，需收回 */
  moveNotice?: boolean
}) {
  const [mode, setMode] = useState<Mode>(defaultMode)
  const [amount, setAmount] = useState<number>(defaultDays)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [result, setResult] = useState<IssuedCard | null>(null)
  const [guest, setGuest] = useState<GuestInput>(initialGuest ?? {})

  const setGuestField = (key: keyof GuestInput, value: string) => {
    setGuest((prev) => ({ ...prev, [key]: value }))
  }

  const guestHasInput = Object.values(guest).some((v) => (v ?? '').trim() !== '')
  const options = mode === 'day' ? dayOptions(GUEST_MAX_DAYS) : hourOptions(GUEST_MAX_HOURS)

  // 预计退房时间：日租按"入住当天 + N 天的中午 12:00"，钟点按"现在 + N 小时"。
  // 这只是估算 —— 卡里只有时长码，锁是从首次刷卡开始计时的。
  const pad = (n: number) => String(n).padStart(2, '0')
  const fmt = (d: Date) =>
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
  const startAt = new Date()
  const checkoutAt = (() => {
    if (mode === 'hour') return new Date(startAt.getTime() + amount * 3600_000)
    const d = new Date(startAt)
    d.setDate(d.getDate() + amount)
    d.setHours(12, 0, 0, 0)
    return d
  })()

  const switchMode = (next: Mode) => {
    setMode(next)
    setAmount(1)
    setResult(null)
  }

  const issue = async () => {
    setLoading(true)
    setError(null)
    try {
      const duration = mode === 'day' ? { days: amount } : { hours: amount }
      const issued = await api.issueGuest(
        room.roomNo,
        duration,
        crypto.randomUUID(),
        guestHasInput ? guest : undefined,
      )
      setResult(issued)
      setGuest({})
      onIssued?.()
    } catch (e) {
      setError(describeError(e))
      setResult(null)
    } finally {
      setLoading(false)
    }
  }

  return (
    <Card
      size="small"
      title="发客卡"
      extra={
        <Button type="primary" loading={loading} onClick={issue} disabled={!room.active}>
          发卡
        </Button>
      }
    >
      <Space direction="vertical" size="small" style={{ width: '100%' }}>
        {moveNotice ? (
          <Alert
            type="warning"
            showIcon
            message="换房：客人信息与时长已从原房间带入"
            description="源房已登记退房。旧卡仍能打开原房间的门，到期前请收回；这张新卡写完后请交给客人。房态不会自动变，写完请按实际改目标房房态。"
          />
        ) : null}
        {renewFrom ? (
          <Alert
            type="warning"
            showIcon
            message={`续住：该房当前${formatExpiry(renewFrom) ?? `有效期至 ${formatTime(renewFrom)}`}`}
            description="写一张天数更长的新卡（卡号比旧卡大，门锁会认）。天数已按「剩余天数 + 1」预填，可以改。"
          />
        ) : null}
        <Space wrap>
          <Radio.Group
            value={mode}
            onChange={(e) => switchMode(e.target.value as Mode)}
            optionType="button"
            buttonStyle="solid"
            options={[
              { label: '日租', value: 'day' },
              { label: '钟点', value: 'hour' },
            ]}
          />
          <Select
            style={{ width: 140 }}
            value={amount}
            onChange={setAmount}
            options={options}
          />
          <Text type="secondary">{mode === 'day' ? '天' : '小时'}</Text>
        </Space>

        <Alert
          type="info"
          showIcon
          message={
            <span>
              预计退房：<b>{fmt(startAt)}</b> - <b>{fmt(checkoutAt)}</b>
            </span>
          }
          description="按发卡时间估算（日租默认中午 12:00 退房）；门锁实际从首次刷卡起算，可能比这里晚。"
        />

        <Space wrap>
          <Input
            placeholder="姓名"
            style={{ width: 130 }}
            value={guest.name ?? ''}
            onChange={(e) => setGuestField('name', e.target.value)}
          />
          <Select
            placeholder="性别"
            allowClear
            style={{ width: 90 }}
            value={guest.gender}
            onChange={(v) => setGuestField('gender', v ?? '')}
            options={GENDERS.map((g) => ({ value: g, label: g }))}
          />
          <Select
            placeholder="证件类型"
            allowClear
            showSearch
            style={{ width: 190 }}
            value={guest.idType}
            onChange={(v) => setGuestField('idType', v ?? '')}
            options={ID_TYPES.map((t) => ({ value: t, label: t }))}
          />
          <Input
            placeholder="证件号码"
            style={{ width: 190 }}
            value={guest.idNumber ?? ''}
            onChange={(e) => setGuestField('idNumber', e.target.value)}
          />
        </Space>
        <Space wrap>
          <Input
            placeholder="民族"
            style={{ width: 100 }}
            value={guest.nation ?? ''}
            onChange={(e) => setGuestField('nation', e.target.value)}
          />
          <Input
            type="date"
            title="出生日期"
            style={{ width: 150 }}
            value={guest.birthDate ?? ''}
            onChange={(e) => setGuestField('birthDate', e.target.value)}
          />
          <Input
            placeholder="住址"
            style={{ width: 300 }}
            value={guest.address ?? ''}
            onChange={(e) => setGuestField('address', e.target.value)}
          />
        </Space>
        <TextArea
          rows={2}
          placeholder="备注"
          value={guest.note ?? ''}
          onChange={(e) => setGuestField('note', e.target.value)}
        />

        {!room.active ? <Alert type="warning" showIcon message="该房间已停用，不能发卡" /> : null}

        {mode === 'day' && amount > GUEST_MAX_DAYS ? (
          <Alert
            type="warning"
            showIcon
            message={`${amount} 天会用月码编码（1 月 ≈ 31 天）`}
            description="锁端按 0xC0|月数 解释有效期；与软件逐字节一致，但实际到期日会是近似值。"
          />
        ) : null}

        {mode === 'day' && amount >= 360 ? (
          <Alert
            type="error"
            showIcon
            message="长有效期卡会顶高该房间的水位"
            description="这张卡刷一次门后，该房间此前的客卡会全部失效，只能用复位卡恢复。"
          />
        ) : null}

        {error ? <Alert type="error" showIcon message="发卡失败" description={error} /> : null}

        {result ? (
          <Card type="inner" size="small" title="发卡结果" extra={<Tag color="green">成功</Tag>}>
            <Descriptions
              column={2}
              size="small"
              items={[
                { key: 'kind', label: '卡型', children: result.card.kindLabel },
                {
                  key: 'duration',
                  label: '时长',
                  children:
                    result.card.days > 0
                      ? `${result.card.days} 天`
                      : result.card.hours > 0
                        ? `${result.card.hours} 小时`
                        : '—',
                },
                {
                  key: 'expiry',
                  label: '到期',
                  children: formatExpiry(result.expiresAt) ?? '—（无时长）',
                },
                { key: 'cardNo', label: '卡号', children: formatCardNo(result.card.cardNo) },
                { key: 'room', label: '房号', children: room.roomNo },
              ]}
            />
            <div style={{ marginTop: 8 }}>
              <Text type="secondary">原始数据：</Text>{' '}
              <Text className="ocw-hex" copyable>
                {result.card.rawHex}
              </Text>
            </div>
          </Card>
        ) : null}
      </Space>
    </Card>
  )
}
