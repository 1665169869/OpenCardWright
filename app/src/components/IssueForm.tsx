import { useEffect, useState } from 'react'
import { Alert, Button, Card, Checkbox, Descriptions, Radio, Select, Space, Tag, Typography } from 'antd'
import {
  api,
  dayOptions,
  describeError,
  formatExpiry,
  formatTime,
  hourOptions,
  GUEST_MAX_DAYS,
  GUEST_MAX_HOURS,
  type IssuedCard,
  type Room,
} from '../api'

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
  defaultMode = 'day',
  moveNotice = false,
}: {
  room: Room
  onIssued?: () => void
  /** 续住时按剩余天数预填 */
  defaultDays?: number
  /** 续住时的"当前有效期至"，只用于提示 */
  renewFrom?: string | null
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
  /** 挂失卡（TD-150）：页1[2] 低 4 位 = 1，让该房间在用卡失效 */
  const [lost, setLost] = useState(false)
  /** 预计退房时刻（HH:mm）。**只影响展示与估算**，不改变写入卡内的天数。
   *  TD-126：初值读「退房钟点」设置项（默认 12），只读展示，改配置去设置页。 */
  const [checkoutTime, setCheckoutTime] = useState('12:00')
  /** 凌晨房归属时间（TD-225）：早于该时刻开卡，归属日算前一天 */
  const [morningCutoff, setMorningCutoff] = useState('06:00')
  useEffect(() => {
    void api
      .getCheckoutHour()
      .then((h) => setCheckoutTime(`${String(h).padStart(2, '0')}:00`))
      .catch(() => {})
    void api
      .getMorningCutoff()
      .then(setMorningCutoff)
      .catch(() => {})
  }, [])
  const options = mode === 'day' ? dayOptions(GUEST_MAX_DAYS) : hourOptions(GUEST_MAX_HOURS)

  // 预计退房时间：日租按"归属日 + N 天的退房钟点"，钟点按"现在 + N 小时"。
  // 这只是估算 —— 卡里只有时长码，锁是从首次刷卡开始计时的。
  // TD-225：凌晨（早于 morningCutoff）开卡时归属日算前一天，
  // 所以凌晨 2 点开 1 天卡的预计退房是**当天** 12:00，而不是次日。
  const pad = (n: number) => String(n).padStart(2, '0')
  const fmt = (d: Date) =>
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
  const startAt = new Date()
  /** 当前时刻是否落在凌晨房区间（早于归属时间） */
  const isEarlyMorning = (() => {
    const [ch, cm] = morningCutoff.split(':').map((v) => Number(v) || 0)
    return startAt.getHours() * 60 + startAt.getMinutes() < ch * 60 + cm
  })()
  const checkoutAt = (() => {
    if (mode === 'hour') return new Date(startAt.getTime() + amount * 3600_000)
    const d = new Date(startAt)
    // 归属日：凌晨房从「昨天」起算，于是 +N 天落在当天
    d.setDate(d.getDate() + amount + (isEarlyMorning ? -1 : 0))
    const [hh, mm] = checkoutTime.split(':').map((v) => Number(v) || 0)
    d.setHours(hh, mm, 0, 0)
    return d
  })()
  /** 所选时刻在今天已经过了 —— 只是提示，不改天数 */
  const checkoutTimePassed = mode === 'day' && checkoutAt.getTime() <= startAt.getTime()

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
        lost,
      )
      setResult(issued)
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
        <Button
          type="primary"
          loading={loading}
          onClick={issue}
          disabled={!room.active}
        >
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

        {mode === 'day' ? (
          <Space wrap align="center">
            <Text type="secondary">预计退房时刻</Text>
            <Text>{checkoutTime}</Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              钟点在「设置」页配置；这里只是估算，写进卡里的仍然是「{amount} 天」
            </Text>
          </Space>
        ) : null}

        {mode === 'day' && isEarlyMorning ? (
          <Alert
            type="warning"
            showIcon
            message={`凌晨入住（早于 ${morningCutoff}）：按前一天归属，1 天卡到「今天」${checkoutTime} 退房`}
            description="归属时间可在「设置 → 退房钟点 → 凌晨房归属时间」调整。"
          />
        ) : null}

        <Alert
          type="info"
          showIcon
          message={
            <span>
              预计退房：<b>{fmt(startAt)}</b> - <b>{fmt(checkoutAt)}</b>
            </span>
          }
          description={
            checkoutTimePassed
              ? '所选时刻已过，实际按次日估算。时间只影响记录与到期估算，锁按首次刷卡起算。'
              : '按发卡时间估算；时间只影响记录与到期估算，锁按首次刷卡起算，可能比这里晚。'
          }
        />

        <Space wrap align="center">
          <Checkbox checked={lost} onChange={(e) => setLost(e.target.checked)}>
            挂失卡（让本房间在用卡失效）
          </Checkbox>
          <Text type="secondary" style={{ fontSize: 12 }}>
            客人身份请在「入住登记」里登记，发卡不再收集
          </Text>
        </Space>

        {!room.active ? <Alert type="warning" showIcon message="该房间已停用，不能发卡" /> : null}

        {mode === 'day' && amount > GUEST_MAX_DAYS ? (
          <Alert
            type="warning"
            showIcon
            message={`${amount} 天会用月码编码（1 月 ≈ 31 天）`}
            description="锁端按 0xC0|月数 解释有效期；与软件逐字节一致，但实际到期日会是近似值。"
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
                { key: 'room', label: '房号', children: room.roomNo },
              ]}
            />
            <div style={{ marginTop: 8 }}>
              <Text type="secondary">原始数据：</Text>{' '}
              <Text className="ocw-hex" copyable>
                {result.card.rawHex}
              </Text>
            </div>
            {result.invalidated > 0 ? (
              <Alert
                style={{ marginTop: 8 }}
                type="warning"
                showIcon
                message={`已作废 ${result.invalidated} 张同房有效卡`}
                description="旧卡已在本机记录里标记失效：房间有效卡数、续住 / 换房与日历统计都按新口径计算。旧卡在门锁上要靠这张挂失卡才能失效，请务必拿去刷一次门锁。"
              />
            ) : null}
          </Card>
        ) : null}
      </Space>
    </Card>
  )
}
