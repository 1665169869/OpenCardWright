import { useCallback, useEffect, useState } from 'react'
import { ReloadOutlined, UserAddOutlined } from '@ant-design/icons'
import {
  Alert,
  Button,
  Card,
  DatePicker,
  Descriptions,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import CheckinForm from '../components/CheckinForm'
import {
  api,
  describeError,
  formatTime,
  type AuditRow,
  type CheckinDetail,
  type CheckinGuest,
  type CheckinRow,
  type IssuanceRow,
  type Reservation,
} from '../api'

/** 证件有效期：`20231106` → `23年11月06日`（缺值显示 `—`） */
function fmtIdDate(raw: string | null): string {
  if (!raw) return '—'
  const d = raw.replace(/[^0-9]/g, '')
  if (d.length !== 8) return raw
  return `${d.slice(2, 4)}年${d.slice(4, 6)}月${d.slice(6, 8)}日`
}

const { Text } = Typography

/** 卡型下拉（与后端 issue 时写入的中文标签一致） */
const KINDS = ['日租客卡', '钟点客卡', '总控卡', '复位卡', '清除卡', '安装卡', '退房钟点设置卡', '校时卡']

/**
 * 记录页：发卡记录 + 入住记录 + 预定记录 + 操作审计。
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
          { key: 'checkin', label: '入住登记', children: <CheckinTab canAdmin={canAdmin} /> },
          { key: 'reservation', label: '预定记录', children: <ReservationTab canAdmin={canAdmin} /> },
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

/** 预定晚数（退房日不含）；日期都是 YYYY-MM-DD */
const nightsBetween = (checkIn: string, checkOut: string): number => {
  const a = Date.parse(`${checkIn}T00:00:00`)
  const b = Date.parse(`${checkOut}T00:00:00`)
  return Number.isFinite(a) && Number.isFinite(b) && b > a
    ? Math.round((b - a) / 86_400_000)
    : 0
}

function IssuanceTab({ canAdmin }: { canAdmin: boolean }) {
  const [rows, setRows] = useState<IssuanceRow[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  /** 默认每页 20 条（用户设定，TD-128）——三个 tab 一致 */
  const [pageSize, setPageSize] = useState(20)
  const [roomNo, setRoomNo] = useState('')
  const [kind, setKind] = useState<string | null>(null)
  const [fromDay, setFromDay] = useState<string | null>(null)
  const [toDay, setToDay] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)

  // 记录页停留时每 60 秒重渲染一次：到期状态按当前时间计算，跨过到期点要自动变色
  const [, setTick] = useState(0)
  useEffect(() => {
    const timer = window.setInterval(() => setTick((t) => t + 1), 60_000)
    return () => window.clearInterval(timer)
  }, [])

  const filter = {
    roomNo: roomNo.trim() || null,
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
  }, [roomNo, kind, fromDay, toDay, page, pageSize])

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
        <Button icon={<ReloadOutlined />} onClick={() => void load()}>
          刷新
        </Button>
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
          expandedRowRender: (row) => (
            <Space direction="vertical" size={4} style={{ width: '100%' }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                卡内原始数据（28 字节，页1+页2+补零）：
              </Text>
              <Text className="ocw-hex" copyable>
                {row.rawHex ?? '—'}
              </Text>
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
            render: (_: unknown, r: IssuanceRow) => {
              // TD-229：状态由后端算好（active / expired / checked_out / voided / failed），
              // 前端不再自己比较时间，也不再限定卡型 —— 总控卡过期同样要显示「已过期」。
              switch (r.status) {
                case 'failed':
                  return <Tag color="red">失败</Tag>
                case 'checked_out':
                  return <Tag>已退房</Tag>
                case 'voided':
                  return <Tag color="purple">已作废</Tag>
                case 'expired':
                  return <Tag color="red">已过期</Tag>
                default:
                  // 没有到期时间的卡型（安装 / 复位 / 清除 / 退房钟点 / 校时）不参与过期判断
                  return r.expiresAt ? (
                    <Tag color="green">有效</Tag>
                  ) : (
                    <Tag color="green">有效（无期限）</Tag>
                  )
              }
            },
          },
        ]}
      />
    </Space>
  )
}

/** 入住记录（TD-118）：发卡记录中**填了身份信息**的发卡视图。
 *  未输入身份信息的发卡不进入本表 —— guestOnly 由后端查询层过滤（TRIM 非空），
 *  这里只管展示：谁、哪间房、什么时候发的卡、住到什么时候。 */
function CheckinTab({ canAdmin }: { canAdmin: boolean }) {
  const [rows, setRows] = useState<CheckinRow[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  /** 默认每页 20 条（用户设定，TD-128） */
  const [pageSize, setPageSize] = useState(20)
  const [roomNo, setRoomNo] = useState('')
  const [status, setStatus] = useState<string | null>(null)
  const [fromDay, setFromDay] = useState<string | null>(null)
  const [toDay, setToDay] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  /** 登记单详情（TD-221）：一张单 + 全部住客 */
  const [detail, setDetail] = useState<CheckinDetail | null>(null)
  /** 新增登记（TD-222）：从这里进来的不预填房号 */
  const [createOpen, setCreateOpen] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    setErr(null)
    try {
      const p = await api.listCheckins({
        roomNo: roomNo.trim() || null,
        status,
        from: fromDay,
        to: toDay,
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
  }, [roomNo, status, fromDay, toDay, page, pageSize])

  useEffect(() => {
    void load()
  }, [load])

  const openDetail = async (id: number) => {
    setErr(null)
    try {
      setDetail(await api.getCheckin(id))
    } catch (e) {
      setErr(describeError(e))
    }
  }

  /** 导出当前筛选条件下的登记单（管理员，TD-221） */
  const doExport = async () => {
    setErr(null)
    setMsg(null)
    try {
      const r = await api.exportCheckinsCsv({
        roomNo: roomNo.trim() || null,
        status,
        from: fromDay,
        to: toDay,
      })
      setMsg(`已导出 ${r.rows} 行到 ${r.path}（${bytesText(r.bytes)}），Excel 可直接打开`)
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
        <Select
          placeholder="状态"
          value={status ?? undefined}
          onChange={(v) => {
            setStatus(v ?? null)
            setPage(1)
          }}
          allowClear
          style={{ width: 120 }}
          options={[
            { value: 'open', label: '登记中' },
            { value: 'closed', label: '已关闭' },
          ]}
        />
        <DatePicker
          placeholder="入住起始日"
          onChange={(_, s) => {
            setFromDay(typeof s === 'string' && s ? s : null)
            setPage(1)
          }}
        />
        <DatePicker
          placeholder="入住截止日"
          onChange={(_, s) => {
            setToDay(typeof s === 'string' && s ? s : null)
            setPage(1)
          }}
        />
        <Button icon={<ReloadOutlined />} onClick={() => void load()}>
          刷新
        </Button>
        <Button onClick={() => void load()}>查询</Button>
        <Button icon={<UserAddOutlined />} onClick={() => setCreateOpen(true)}>
          新增登记
        </Button>
        {canAdmin ? (
          <Button type="primary" onClick={() => void doExport()}>
            导出 CSV
          </Button>
        ) : null}
        <Text type="secondary" style={{ fontSize: 12 }}>
          按入住登记单展示，一单可含多位住客
        </Text>
      </Space>

      {/* 新增登记（TD-222）：不预填房号，表单里自己填 */}
      <CheckinForm
        open={createOpen}
        roomNo={null}
        onClose={() => setCreateOpen(false)}
        onSaved={() => void load()}
      />

      {err ? <Alert type="error" showIcon message="操作失败" description={err} /> : null}
      {msg ? <Alert type="success" showIcon message="导出完成" description={msg} /> : null}

      <Table<CheckinRow>
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={rows}
        locale={{ emptyText: '这个条件下没有入住登记' }}
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
          { title: '房号', dataIndex: 'roomNo', width: 90 },
          {
            title: '登记单号',
            dataIndex: 'id',
            width: 100,
            render: (v: number) => `#${v}`,
          },
          {
            title: '房间序号',
            dataIndex: 'roomIndex',
            width: 100,
            render: (v: number | null) => (v === null ? '—' : v),
          },
          {
            title: '首位登记人',
            dataIndex: 'firstGuestName',
            width: 120,
            render: (v: string | null) => v ?? '—',
          },
          { title: '人数', dataIndex: 'guestCount', width: 70 },
          {
            title: '入住时间',
            dataIndex: 'checkInAt',
            width: 170,
            render: (v: string) => formatTime(v),
          },
          {
            title: '退房时间',
            dataIndex: 'checkOutAt',
            width: 170,
            render: (v: string | null) => (v ? formatTime(v) : '—'),
          },
          {
            title: '状态',
            width: 90,
            render: (_: unknown, r: CheckinRow) =>
              r.status === 'open' ? <Tag color="green">登记中</Tag> : <Tag>已关闭</Tag>,
          },
          { title: '备注', dataIndex: 'note', render: (v: string | null) => v ?? '—' },
          {
            title: '详情',
            width: 70,
            render: (_: unknown, r: CheckinRow) => (
              <Button size="small" onClick={() => void openDetail(r.id)}>
                详情
              </Button>
            ),
          },
        ]}
      />

      {/* 登记单详情（TD-221）：一张单 + 全部住客 */}
      <Modal
        open={detail !== null}
        title={`入住登记 · ${detail?.checkin.roomNo ?? ''}`}
        onCancel={() => setDetail(null)}
        footer={null}
        width={900}
      >
        {detail ? (
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Descriptions
              column={2}
              size="small"
              bordered
              items={[
                { key: 'room', label: '房号', children: detail.checkin.roomNo },
                { key: 'no', label: '登记单号', children: `#${detail.checkin.id}` },
                {
                  key: 'index',
                  label: '房间序号',
                  children: detail.roomIndex ?? '—',
                },
                {
                  key: 'status',
                  label: '状态',
                  children: detail.checkin.status === 'open' ? '登记中' : '已关闭',
                },
                {
                  key: 'in',
                  label: '入住时间',
                  children: formatTime(detail.checkin.checkInAt),
                },
                {
                  key: 'out',
                  label: '退房时间',
                  children: detail.checkin.checkOutAt
                    ? formatTime(detail.checkin.checkOutAt)
                    : '—',
                },
                { key: 'count', label: '住客人数', children: detail.guests.length },
                { key: 'note', label: '备注', children: detail.checkin.note ?? '—' },
              ]}
            />
            <Table<CheckinGuest>
              size="small"
              rowKey="id"
              dataSource={detail.guests}
              pagination={false}
              locale={{ emptyText: '这张登记单还没有住客' }}
              columns={[
                { title: '#', dataIndex: 'seq', width: 46 },
                {
                  title: '姓名',
                  dataIndex: 'name',
                  width: 96,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '性别',
                  dataIndex: 'gender',
                  width: 64,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '证件类型',
                  dataIndex: 'idType',
                  width: 96,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '证件号码',
                  dataIndex: 'idNumber',
                  width: 180,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '民族',
                  dataIndex: 'nation',
                  width: 70,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '出生日期',
                  dataIndex: 'birthDate',
                  width: 110,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '证件有效期',
                  width: 180,
                  render: (_: unknown, g: CheckinGuest) =>
                    g.idValidFrom || g.idValidTo
                      ? `${fmtIdDate(g.idValidFrom)} ~ ${fmtIdDate(g.idValidTo)}`
                      : '—',
                },
                {
                  title: '签发机关',
                  dataIndex: 'idAuthority',
                  width: 140,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '住址',
                  dataIndex: 'address',
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '备注',
                  dataIndex: 'note',
                  render: (v: string | null) => v ?? '—',
                },
              ]}
            />
          </Space>
        ) : null}
      </Modal>
    </Space>
  )
}

/** 预定记录（TD-167）：有效 / 已入住 / 已取消都保留，可按房号、姓名、状态、入住日查询。
 *  只读视图；改期 / 取消仍在日历里操作。 */
function ReservationTab({ canAdmin }: { canAdmin: boolean }) {
  const [rows, setRows] = useState<Reservation[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [roomNo, setRoomNo] = useState('')
  const [guestName, setGuestName] = useState('')
  const [status, setStatus] = useState<string | null>(null)
  const [fromDay, setFromDay] = useState<string | null>(null)
  const [toDay, setToDay] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setErr(null)
    try {
      const p = await api.listReservationRecords({
        roomNo: roomNo.trim() || null,
        guestName: guestName.trim() || null,
        status,
        from: fromDay,
        to: toDay,
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
  }, [roomNo, guestName, status, fromDay, toDay, page, pageSize])

  useEffect(() => {
    void load()
  }, [load])

  /** 导出当前筛选条件下的预定记录（TD-205）：只给管理员，范围不受分页限制。 */
  const doExport = async () => {
    setErr(null)
    setMsg(null)
    try {
      const r = await api.exportReservationsCsv({
        roomNo: roomNo.trim() || null,
        guestName: guestName.trim() || null,
        status,
        from: fromDay,
        to: toDay,
      })
      setMsg(`已导出 ${r.rows} 行到 ${r.path}（${bytesText(r.bytes)}），Excel 可直接打开`)
    } catch (e) {
      setErr(describeError(e))
    }
  }

  const statusTag = (s: string) => {
    if (s === 'cancelled') return <Tag>已取消</Tag>
    if (s === 'checked_in') return <Tag color="green">已入住</Tag>
    return <Tag color="blue">有效预定</Tag>
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
        <Input
          placeholder="客人姓名（模糊）"
          value={guestName}
          onChange={(e) => {
            setGuestName(e.target.value)
            setPage(1)
          }}
          style={{ width: 150 }}
          allowClear
        />
        <Select
          placeholder="状态"
          value={status ?? undefined}
          onChange={(v) => {
            setStatus(v ?? null)
            setPage(1)
          }}
          allowClear
          style={{ width: 130 }}
          options={[
            { value: 'booked', label: '有效预定' },
            { value: 'checked_in', label: '已入住' },
            { value: 'cancelled', label: '已取消' },
          ]}
        />
        <DatePicker
          placeholder="入住起始日"
          onChange={(_, s) => {
            setFromDay(typeof s === 'string' && s ? s : null)
            setPage(1)
          }}
        />
        <DatePicker
          placeholder="入住截止日"
          onChange={(_, s) => {
            setToDay(typeof s === 'string' && s ? s : null)
            setPage(1)
          }}
        />
        <Button icon={<ReloadOutlined />} onClick={() => void load()}>
          刷新
        </Button>
        <Button onClick={() => void load()}>查询</Button>
        {canAdmin ? (
          <Button type="primary" onClick={() => void doExport()}>
            导出 CSV
          </Button>
        ) : null}
        <Text type="secondary" style={{ fontSize: 12 }}>
          含已取消 / 已入住；改期与取消仍到日历操作
        </Text>
      </Space>

      {err ? <Alert type="error" showIcon message="操作失败" description={err} /> : null}
      {msg ? <Alert type="success" showIcon message="导出完成" description={msg} /> : null}

      <Table<Reservation>
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={rows}
        locale={{ emptyText: '这个条件下没有预定记录' }}
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
          { title: '房号', dataIndex: 'roomNo', width: 90 },
          {
            title: '客人姓名',
            dataIndex: 'guestName',
            width: 110,
            render: (v: string | null) => v ?? '—',
          },
          {
            title: '入住',
            width: 150,
            render: (_: unknown, r: Reservation) => `${r.checkIn} ${r.checkInTime}`,
          },
          {
            title: '退房',
            width: 150,
            render: (_: unknown, r: Reservation) => `${r.checkOut} ${r.checkOutTime}`,
          },
          {
            title: '晚数',
            width: 70,
            render: (_: unknown, r: Reservation) => nightsBetween(r.checkIn, r.checkOut),
          },
          {
            title: '状态',
            width: 100,
            render: (_: unknown, r: Reservation) => statusTag(r.status),
          },
          { title: '备注', dataIndex: 'note', render: (v: string | null) => v ?? '—' },
        ]}
      />
    </Space>
  )
}

function AuditTab() {
  const [rows, setRows] = useState<AuditRow[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  /** 默认每页 20 条（用户设定，TD-128） */
  const [pageSize, setPageSize] = useState(20)
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
