import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  Button,
  Card,
  DatePicker,
  Input,
  InputNumber,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import {
  api,
  describeError,
  formatTime,
  type AuditRow,
  type IssuanceRow,
} from '../api'

const { Text } = Typography

/** 卡型下拉（与后端 issue 时写入的中文标签一致） */
const KINDS = ['日租客卡', '钟点客卡', '总控卡', '复位卡', '清除卡', '安装卡', '退房钟点设置卡', '校时卡']

/**
 * 记录页：发卡记录 + 操作审计。
 *
 * 前台也要看（"今天发了哪些卡"是日常问题），所以列表走 require_login；
 * 导出含证件号，只给管理员，按钮按角色隐藏。
 */
export default function Logs({ canAdmin }: { canAdmin: boolean }) {
  return (
    <Card title="记录">
      <Tabs
        items={[
          { key: 'issuance', label: '发卡记录', children: <IssuanceTab canAdmin={canAdmin} /> },
          { key: 'audit', label: '操作审计', children: <AuditTab /> },
        ]}
      />
    </Card>
  )
}

/** 本地日期（YYYY-MM-DD）→ 当天 00:00 的 UTC ISO；`end` 为次日 00:00（开区间上界）。 */
const dayToIso = (day: string | null, end = false): string | null => {
  if (!day) return null
  const d = new Date(`${day}T00:00:00`)
  if (Number.isNaN(d.getTime())) return null
  if (end) d.setDate(d.getDate() + 1)
  return d.toISOString().replace(/\.\d{3}Z$/, 'Z')
}

const bytesText = (n: number) =>
  n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(2)} MB` : `${(n / 1024).toFixed(1)} KB`

function IssuanceTab({ canAdmin }: { canAdmin: boolean }) {
  const [rows, setRows] = useState<IssuanceRow[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(50)
  const [roomNo, setRoomNo] = useState('')
  const [cardNo, setCardNo] = useState<number | null>(null)
  const [kind, setKind] = useState<string | null>(null)
  const [fromDay, setFromDay] = useState<string | null>(null)
  const [toDay, setToDay] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  const [history, setHistory] = useState<Record<number, IssuanceRow[]>>({})

  const filter = {
    roomNo: roomNo.trim() || null,
    cardNo,
    kind,
    from: dayToIso(fromDay),
    to: dayToIso(toDay, true),
  }

  const load = useCallback(async () => {
    setLoading(true)
    setErr(null)
    try {
      const p = await api.listIssuances({ ...filter, page, pageSize })
      setRows(p.rows)
      setTotal(p.total)
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roomNo, cardNo, kind, fromDay, toDay, page, pageSize])

  useEffect(() => {
    void load()
  }, [load])

  const doExport = async () => {
    setErr(null)
    setMsg(null)
    try {
      const r = await api.exportIssuancesCsv(filter)
      setMsg(`已导出 ${r.rows} 行到 ${r.path}（${bytesText(r.bytes)}），Excel 可直接打开`)
    } catch (e) {
      setErr(describeError(e))
    }
  }

  /** 展开一行时按卡号拉「同一张卡的全部写入」，解释这张卡被改写过几次。 */
  const loadHistory = async (row: IssuanceRow) => {
    if (row.cardNo === null || history[row.id]) return
    try {
      const list = await api.cardHistory(row.cardNo)
      setHistory((prev) => ({ ...prev, [row.id]: list }))
    } catch (e) {
      setErr(describeError(e))
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space wrap>
        <Input
          placeholder="房号（前缀匹配）"
          value={roomNo}
          onChange={(e) => {
            setRoomNo(e.target.value)
            setPage(1)
          }}
          style={{ width: 150 }}
          allowClear
        />
        <InputNumber
          placeholder="卡号（十进制）"
          value={cardNo ?? undefined}
          min={0}
          max={65535}
          onChange={(v) => {
            setCardNo(v === null ? null : Number(v))
            setPage(1)
          }}
          style={{ width: 150 }}
        />
        <Select
          placeholder="卡型"
          value={kind ?? undefined}
          onChange={(v) => {
            setKind(v ?? null)
            setPage(1)
          }}
          allowClear
          style={{ width: 150 }}
          options={KINDS.map((k) => ({ value: k, label: k }))}
        />
        <DatePicker
          placeholder="起始日"
          onChange={(_, s) => {
            setFromDay(typeof s === 'string' && s ? s : null)
            setPage(1)
          }}
        />
        <DatePicker
          placeholder="截止日"
          onChange={(_, s) => {
            setToDay(typeof s === 'string' && s ? s : null)
            setPage(1)
          }}
        />
        <Button onClick={() => void load()}>查询</Button>
        {canAdmin ? (
          <Button type="primary" onClick={() => void doExport()}>
            导出 CSV
          </Button>
        ) : null}
      </Space>

      {err ? <Alert type="error" showIcon message="操作失败" description={err} /> : null}
      {msg ? <Alert type="success" showIcon message="导出完成" description={msg} /> : null}

      <Table<IssuanceRow>
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={rows}
        locale={{ emptyText: '这个条件下没有记录' }}
        pagination={{
          current: page,
          pageSize,
          total,
          showSizeChanger: true,
          pageSizeOptions: [20, 50, 100, 200],
          showTotal: (t) => `共 ${t} 条`,
          onChange: (p, s) => {
            setPage(p)
            setPageSize(s)
          },
        }}
        expandable={{
          onExpand: (expanded, row) => {
            if (expanded) void loadHistory(row)
          },
          expandedRowRender: (row) => (
            <Space direction="vertical" size={4} style={{ width: '100%' }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                卡内原始数据（28 字节，页1+页2+补零）：
              </Text>
              <Text className="ocw-hex" copyable>
                {row.rawHex ?? '—'}
              </Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                同一张卡的写入历史：
              </Text>
              {row.cardNo === null ? (
                <Text type="secondary">该卡没有卡号（安装卡等）</Text>
              ) : history[row.id] ? (
                <Space direction="vertical" size={2}>
                  {history[row.id].map((h) => (
                    <Text key={h.id} style={{ fontSize: 12 }}>
                      {formatTime(h.createdAt)} · {h.kind} · {h.roomNo ?? '—'} ·{' '}
                      {h.days > 0 ? `${h.days} 天` : `${h.hours} 小时`} · {h.result}
                    </Text>
                  ))}
                </Space>
              ) : (
                <Text type="secondary">加载中…</Text>
              )}
            </Space>
          ),
        }}
        columns={[
          {
            title: '发卡时间',
            dataIndex: 'createdAt',
            width: 170,
            render: (v: string) => formatTime(v),
          },
          { title: '房号', dataIndex: 'roomNo', width: 90, render: (v: string | null) => v ?? '—' },
          { title: '卡型', dataIndex: 'kind', width: 130 },
          {
            title: '时长',
            width: 100,
            render: (_: unknown, r: IssuanceRow) =>
              r.days > 0 ? `${r.days} 天` : r.hours > 0 ? `${r.hours} 小时` : '—',
          },
          {
            title: '卡号',
            dataIndex: 'cardNo',
            width: 100,
            render: (v: number | null) => (v === null ? '—' : `0x${v.toString(16).toUpperCase().padStart(4, '0')}`),
          },
          { title: '客人', dataIndex: 'guestName', width: 100, render: (v: string | null) => v ?? '—' },
          { title: '操作员', dataIndex: 'operatorName', width: 100, render: (v: string | null) => v ?? '—' },
          {
            title: '到期',
            dataIndex: 'expiresAt',
            width: 170,
            render: (v: string | null) => (v ? formatTime(v) : '—'),
          },
          {
            title: '状态',
            width: 100,
            render: (_: unknown, r: IssuanceRow) =>
              r.checkedOutAt ? <Tag>已退房</Tag> : <Tag color="green">有效</Tag>,
          },
        ]}
      />
    </Space>
  )
}

function AuditTab() {
  const [rows, setRows] = useState<AuditRow[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(50)
  const [action, setAction] = useState('')
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setErr(null)
    try {
      const p = await api.listAudits({
        action: action.trim() || null,
        page,
        pageSize,
      })
      setRows(p.rows)
      setTotal(p.total)
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setLoading(false)
    }
  }, [action, page, pageSize])

  useEffect(() => {
    void load()
  }, [load])

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space wrap>
        <Input
          placeholder="动作前缀，例如 issue / checkout / restore"
          value={action}
          onChange={(e) => {
            setAction(e.target.value)
            setPage(1)
          }}
          style={{ width: 320 }}
          allowClear
        />
        <Button onClick={() => void load()}>查询</Button>
        <Text type="secondary" style={{ fontSize: 12 }}>
          审计只记"谁在什么时候做了什么"，不改也不删。
        </Text>
      </Space>

      {err ? <Alert type="error" showIcon message="查询失败" description={err} /> : null}

      <Table<AuditRow>
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={rows}
        locale={{ emptyText: '没有审计记录' }}
        pagination={{
          current: page,
          pageSize,
          total,
          showSizeChanger: true,
          pageSizeOptions: [20, 50, 100, 200],
          showTotal: (t) => `共 ${t} 条`,
          onChange: (p, s) => {
            setPage(p)
            setPageSize(s)
          },
        }}
        columns={[
          { title: '时间', dataIndex: 'at', width: 170, render: (v: string) => formatTime(v) },
          { title: '动作', dataIndex: 'action', width: 180 },
          { title: '对象', dataIndex: 'target', width: 120, render: (v: string | null) => v ?? '—' },
          { title: '详情', dataIndex: 'detail', ellipsis: true, render: (v: string | null) => v ?? '—' },
          { title: '操作员', dataIndex: 'operatorName', width: 110, render: (v: string | null) => v ?? '—' },
          {
            title: '结果',
            dataIndex: 'result',
            width: 90,
            render: (v: string) => (v === 'ok' ? <Tag color="green">成功</Tag> : <Tag color="red">失败</Tag>),
          },
        ]}
      />
    </Space>
  )
}
