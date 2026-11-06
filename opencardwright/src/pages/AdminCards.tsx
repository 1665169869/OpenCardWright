import { useEffect, useState } from 'react'
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Input,
  InputNumber,
  Modal,
  Radio,
  Select,
  Space,
  Tag,
  Typography,
} from 'antd'
import {
  api,
  dayOptions,
  describeError,
  formatCardNo,
  formatExpiry,
  hourOptions,
  LONG_CONFIRM_WORD,
  MASTER_MAX_DAYS,
  MASTER_MAX_HOURS,
  type AdminCardRequest,
  type IssuedCard,
  type Room,
} from '../api'

const { Text } = Typography

type Kind = 'master' | 'reset' | 'clear' | 'install' | 'checkout' | 'timeSync' | 'raw'

const KIND_LABEL: Record<Kind, string> = {
  master: '总控卡',
  reset: '复位卡',
  clear: '清除卡',
  install: '安装卡',
  checkout: '退房钟点卡',
  timeSync: '校时卡',
  raw: '原始直写',
}

/** 每种管理卡的后果说明。这些字是给前台看的，不是给开发看的。 */
const WARNING: Record<Kind, string> = {
  master: '总控卡的时长编码与客卡不同（小时少 1）。发错时长只会影响这张卡自身的有效期。',
  reset:
    '复位卡会清空该房间的卡记录，把"当前有效卡号"归零 —— 之前被顶掉的旧卡会重新能开门。复位卡不受卡号大小限制，请务必确认是目标房间，用完立即收好。',
  clear:
    '清除卡会清除门锁的房间绑定信息，锁可能变成"未安装"状态，需要用安装卡恢复。这一效果尚未在真锁上验证过。',
  install:
    '安装卡只含房间序号，不含房号也不含卡号。拿到锁上刷一次即完成绑定；刷错房间会让锁绑定到错误的房间。',
  checkout: '退房钟点卡只改变锁内记录的退房时间，不改变已发出的卡。',
  timeSync: '校时卡只含当日时/分，不含日期。锁用它建立时间基准。',
  raw: '原始直写会把这 8 个字节原样写进卡里，可能把卡写成任何卡型或废卡。除非你在做逆向验证，否则不要用。',
}

export default function AdminCards({
  roomsVersion = 0,
  onChanged,
}: {
  roomsVersion?: number
  onChanged?: () => void
}) {
  const [kind, setKind] = useState<Kind>('reset')
  const [rooms, setRooms] = useState<Room[]>([])
  const [roomNo, setRoomNo] = useState<string | null>(null)
  const [useHours, setUseHours] = useState(false)
  const [amount, setAmount] = useState<number>(1)
  const [hour, setHour] = useState<number>(12)
  const [minute, setMinute] = useState<number>(0)
  const [now, setNow] = useState(true)
  const [p1, setP1] = useState('')
  const [p2, setP2] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [result, setResult] = useState<IssuedCard | null>(null)

  useEffect(() => {
    api
      .listRooms()
      .then((list) => {
        setRooms(list)
        setRoomNo((prev) => {
          const stillValid = prev !== null && list.some((r) => r.roomNo === prev && r.active)
          if (stillValid) return prev
          return list.find((r) => r.active)?.roomNo ?? null
        })
      })
      .catch((e: unknown) => setError(describeError(e)))
  }, [roomsVersion])

  const buildRequest = (): AdminCardRequest | null => {
    switch (kind) {
      case 'master':
        return useHours ? { kind: 'master', hours: amount } : { kind: 'master', days: amount }
      case 'reset':
        return { kind: 'reset' }
      case 'clear':
        return { kind: 'clear' }
      case 'install':
        return roomNo ? { kind: 'install', roomNo } : null
      case 'checkout':
        return { kind: 'checkout', hour }
      case 'timeSync':
        return now ? { kind: 'timeSync' } : { kind: 'timeSync', hour, minute }
      case 'raw':
        return { kind: 'raw', p1, p2 }
    }
  }

  // 待确认的请求。用受控 Modal 而不是 Modal.confirm：
  // antd v5 的命令式弹窗在 React 19 下会静默失效（ReactDOM.render 已被移除）。
  const [pending, setPending] = useState<AdminCardRequest | null>(null)
  /** 长有效期总控卡（>63 天）的确认词 */
  const [confirmWord, setConfirmWord] = useState('')

  const masterDays = kind === 'master' && !useHours ? amount : 0
  const needsLongConfirm = masterDays > MASTER_MAX_DAYS
  const longConfirmed = !needsLongConfirm || confirmWord.trim() === LONG_CONFIRM_WORD

  const submit = () => {
    const request = buildRequest()
    if (!request) {
      setError('请先补全参数（例如选择房间）')
      return
    }
    setError(null)
    setPending(request)
  }

  const confirmIssue = async () => {
    if (!pending) return
    setLoading(true)
    setError(null)
    try {
      setResult(await api.issueAdminCard(pending, true, needsLongConfirm ? confirmWord.trim() : null))
      onChanged?.()
    } catch (e) {
      setError(describeError(e))
      setResult(null)
    } finally {
      setLoading(false)
      setPending(null)
    }
  }

  return (
    <>
    <Card
      title="管理卡"
      extra={
        <Button type="primary" danger loading={loading} onClick={submit}>
          发卡
        </Button>
      }
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <Alert
          type="warning"
          showIcon
          message="管理卡会改变门锁状态"
          description="复位 / 清除 / 安装 / 校时都可能让已发出的卡失效或让锁变未安装。每次发卡都会二次确认并写入审计日志。"
        />

        <Space wrap size="large">
          <Space>
            <Text>卡型</Text>
            <Select
              style={{ width: 170 }}
              value={kind}
              onChange={(v) => {
                setKind(v as Kind)
                setResult(null)
                setError(null)
              }}
              options={(Object.keys(KIND_LABEL) as Kind[]).map((k) => ({ value: k, label: KIND_LABEL[k] }))}
            />
          </Space>

          {kind === 'master' ? (
            <>
              <Radio.Group
                value={useHours ? 'h' : 'd'}
                onChange={(e) => {
                  setUseHours(e.target.value === 'h')
                  setAmount(1)
                }}
                optionType="button"
                options={[
                  { label: '小时', value: 'h' },
                  { label: '天', value: 'd' },
                ]}
              />
              <Select
                style={{ width: 190 }}
                value={amount}
                onChange={setAmount}
                options={useHours ? hourOptions(MASTER_MAX_HOURS) : dayOptions(MASTER_MAX_DAYS)}
              />
            </>
          ) : null}

          {kind === 'install' ? (
            <Space>
              <Text>房间</Text>
              <Select
                showSearch
                style={{ width: 220 }}
                value={roomNo}
                onChange={setRoomNo}
                optionFilterProp="label"
                // 已逻辑删除的房间不再作为新发卡入口（历史卡仍可在房间页退）
                options={rooms
                  .filter((r) => r.active)
                  .map((r) => ({
                    value: r.roomNo,
                    label: `${r.roomNo}（序号 ${r.roomIndex}）`,
                  }))}
              />
            </Space>
          ) : null}

          {kind === 'checkout' ? (
            <Space>
              <Text>退房钟点</Text>
              <InputNumber min={0} max={23} value={hour} onChange={(v) => setHour(typeof v === 'number' ? v : 12)} />
              <Text type="secondary">点</Text>
            </Space>
          ) : null}

          {kind === 'timeSync' ? (
            <>
              <Radio.Group
                value={now ? 'now' : 'set'}
                onChange={(e) => setNow(e.target.value === 'now')}
                optionType="button"
                options={[
                  { label: '当前时间', value: 'now' },
                  { label: '指定时间', value: 'set' },
                ]}
              />
              {!now ? (
                <Space>
                  <InputNumber min={0} max={23} value={hour} onChange={(v) => setHour(typeof v === 'number' ? v : 0)} />
                  <Text>:</Text>
                  <InputNumber min={0} max={59} value={minute} onChange={(v) => setMinute(typeof v === 'number' ? v : 0)} />
                </Space>
              ) : null}
            </>
          ) : null}

          {kind === 'raw' ? (
            <Space>
              <Input placeholder="页1 8 位 HEX" value={p1} onChange={(e) => setP1(e.target.value)} style={{ width: 160 }} />
              <Input placeholder="页2 8 位 HEX" value={p2} onChange={(e) => setP2(e.target.value)} style={{ width: 160 }} />
            </Space>
          ) : null}
        </Space>

        <Alert type="info" showIcon message={WARNING[kind]} />

        {error ? <Alert type="error" showIcon message="发卡失败" description={error} /> : null}

        {result ? (
          <Card type="inner" title="发卡结果" extra={<Tag color="green">成功</Tag>}>
            <Descriptions
              column={2}
              size="small"
              items={[
                { key: 'kind', label: '卡型', children: result.card.kindLabel },
                {
                  key: 'room',
                  label: '房号',
                  children: result.room ? result.room.roomNo : '—（不适用）',
                },
                { key: 'cardNo', label: '卡号', children: formatCardNo(result.card.cardNo) },
                {
                  key: 'expiry',
                  label: '到期',
                  children: formatExpiry(result.expiresAt) ?? '—（无时长）',
                },
              ]}
            />
            <div style={{ marginTop: 12 }}>
              <Text type="secondary">原始数据：</Text>{' '}
              <Text className="ocw-hex" copyable>
                {result.card.rawHex}
              </Text>
            </div>
          </Card>
        ) : null}
      </Space>

      <Modal
        open={pending !== null}
        title={`确认发出${KIND_LABEL[kind]}？`}
        width={520}
        okText="确认发卡"
        cancelText="取消"
        confirmLoading={loading}
        okButtonProps={{
          danger: kind === 'reset' || kind === 'clear' || kind === 'raw',
          disabled: !longConfirmed,
        }}
        onOk={confirmIssue}
        onCancel={() => setPending(null)}
      >
        <p style={{ color: '#cf1322' }}>{WARNING[kind]}</p>
        {needsLongConfirm ? (
          <Alert
            type="error"
            showIcon
            style={{ marginBottom: 12 }}
            message={`${masterDays} 天总控卡会把全店水位顶到 ${Math.ceil(masterDays / 365)} 年后`}
            description={
              <Space direction="vertical" size={4} style={{ width: '100%' }}>
                <span>之后发的正常卡会被锁当成旧卡拒开，只能给全部门锁刷复位卡恢复。</span>
                <Input
                  placeholder={`原样输入「${LONG_CONFIRM_WORD}」`}
                  value={confirmWord}
                  onChange={(e) => setConfirmWord(e.target.value)}
                  status={longConfirmed ? undefined : 'error'}
                  style={{ maxWidth: 240 }}
                />
              </Space>
            }
          />
        ) : null}
        <p>确认后，本次操作会写入审计日志。</p>
      </Modal>
    </Card>
    </>
  )
}
