import { useEffect, useMemo, useState } from 'react'
import { Button, Descriptions, Empty, Modal, Select, Space, Tag, Typography } from 'antd'
import { LeftOutlined, PlusOutlined, RightOutlined } from '@ant-design/icons'
import dayjs, { type Dayjs } from 'dayjs'
import {
  api,
  describeError,
  type Reservation,
  type Room,
  type RoomStatus,
  type RoomType,
} from '../api'

const { Text, Title } = Typography

/** 列宽 / 行高：30 间房 × 31 天约 930 格，用绝对定位色块，不用每格一个组件。
 *  2026-11-06 按用户反馈加大：44×36 看不清预定信息，改 64×44 + 更高的可视区。 */
const COL_W = 64
const ROW_H = 44
const ROOM_COL_W = 156

const WEEK = ['日', '一', '二', '三', '四', '五', '六']

/** 预定色块调色板：按预定 id 稳定取色（同一条每次渲染同色），
 *  不同预定一眼可分（2026-11-06 用户要求「随机一下颜色」）。过期仍统一灰。 */
const BLOCK_COLORS = [
  '#13c2c2',
  '#1677ff',
  '#722ed1',
  '#eb2f96',
  '#fa8c16',
  '#52c41a',
  '#2f54eb',
  '#d4380d',
]
const blockColor = (id: number): string =>
  BLOCK_COLORS[Math.abs(Math.trunc(id)) % BLOCK_COLORS.length]

/** 每房一段入住信息；同一房间可有多段（含已退房历史）。 */
type GuestSpan = {
  id: number
  start: string
  end: string
  guestName: string | null
  kind: string
  days: number
  hours: number
  plannedEnd: string
  /** 入住状态：active / checked_out / invalidated / expired */
  status: string
  expiresAt: string
}

/** 色块日期区间：同年内 MM-DD → MM-DD，跨年两端带年份（TD-117 完整入退房日期） */
const shortRange = (a: string, b: string): string =>
  a.slice(0, 4) !== b.slice(0, 4) ? `${a} → ${b}` : `${a.slice(5)} → ${b.slice(5)}`

/** 截断渐隐（TD-117）：色块顶到视窗右缘（超出溢出窗口）时右端渐隐，提示还有后续 */
const truncMask = (endPos: number, totalDays: number): React.CSSProperties =>
  endPos >= totalDays
    ? {
        maskImage: 'linear-gradient(to right, #000 calc(100% - 18px), transparent)',
        WebkitMaskImage: 'linear-gradient(to right, #000 calc(100% - 18px), transparent)',
      }
    : {}

/** 入住色块外观（TD-215 补充）：在住 / 已退房 / 已作废 / 已过期 */
const STAY_STYLE: Record<
  string,
  { label: string; bg: string; border: string; color: string; badgeBg?: string }
> = {
  active: {
    label: '入住中',
    bg: 'rgba(82, 196, 26, 0.16)',
    border: '1px solid rgba(82, 196, 26, 0.45)',
    color: '#389e0d',
    badgeBg: 'rgba(82, 196, 26, 0.35)',
  },
  checked_out: {
    label: '已退房',
    bg: 'rgba(0, 0, 0, 0.05)',
    border: '1px dashed rgba(0, 0, 0, 0.25)',
    color: '#8c8c8c',
  },
  invalidated: {
    label: '已作废',
    bg: 'rgba(114, 46, 209, 0.1)',
    border: '1px dashed rgba(114, 46, 209, 0.45)',
    color: '#722ed1',
    badgeBg: 'rgba(114, 46, 209, 0.45)',
  },
  expired: {
    label: '已过期',
    bg: 'rgba(250, 173, 20, 0.14)',
    border: '1px dashed rgba(250, 173, 20, 0.55)',
    color: '#d46b08',
    badgeBg: 'rgba(250, 173, 20, 0.55)',
  },
}
/** 未知状态按「已过期」处理：宁可显示成历史，也不要误标成在住 */
const stayStyle = (status: string) => STAY_STYLE[status] ?? STAY_STYLE.expired

/**
 * 房态房间日历图（TD-203）：行 = 房间，列 = 日期，默认当月、可切月。
 *
 * 交互（2026-11-06 用户确认）：
 * * 空白格点击 → 预定弹窗，预填该房 + 该日期；
 * * 已有预定色块点击 → 改期 / 取消弹窗；
 * * 入住—退房区间，**退房日不含**（色块铺到退房日前一天）。
 *
 * 数据一次拉 `list_reservations(当月首日, 次月首日)`；跨月预定与视窗相交即返回，
 * 色块按与视窗的交集截断，翻到下个月能看到同一条的后半段。
 */
export default function RoomCalendar({
  rooms,
  roomTypes,
  statuses,
  onReserve,
  onNewReservation,
  onEdit,
}: {
  /** 全量在用房间；日历用自己的房型筛选，不跟随房间页（TD-204） */
  rooms: Room[]
  /** 房型定义：日历头部的独立筛选 */
  roomTypes: RoomType[]
  /** 房态定义（label / 颜色），房号列直接显示当前房态 tag */
  statuses: RoomStatus[]
  /** 顶部「新增预定」：不依赖具体房间，弹窗内选择房间 */
  onNewReservation: () => void
  /** 空白格点击：预定该房该天 */
  onReserve: (room: Room, date: string) => void
  /** 色块点击：改期 / 取消 */
  onEdit: (r: Reservation) => void
}) {
  const [month, setMonth] = useState<Dayjs>(() => dayjs().startOf('month'))
  /** 日历自己的房型筛选（TD-204）：与房间页的筛选互不影响 */
  const [typeFilter, setTypeFilter] = useState<number | 'all' | 'none'>('all')
  const untypedCount = rooms.filter((r) => r.roomTypeId === null).length
  const rows = useMemo(
    () =>
      rooms.filter(
        (r) =>
          typeFilter === 'all' ||
          (typeFilter === 'none' ? r.roomTypeId === null : r.roomTypeId === typeFilter),
      ),
    [rooms, typeFilter],
  )
  const [list, setList] = useState<Reservation[]>([])
  const [err, setErr] = useState<string | null>(null)
  const [reloadToken, setReloadToken] = useState(0)
  /** 入住时段（TD-116）：每房最新一张有效客卡的 [发卡日本地, 到期日本地] + 详情字段（TD-120）。
   *  数据来自 list_issuances（与记录页同一口径），按房取 createdAt 最新一条。
   *  声明提前到 days 之前：溢出天数计算要用它。 */
  const [guestSpans, setGuestSpans] = useState<Record<string, GuestSpan[]>>({})
  /** 入住详情（TD-120）：点入住色块弹出；纯展示无操作 */
  const [stayDetail, setStayDetail] = useState<{ roomNo: string; span: GuestSpan } | null>(null)

  const from = month.format('YYYY-MM-DD')
  /** 视窗左界（TD-123）：含前导列（上月末日），跨月色块能画进前导格 */
  const leadDay = month.subtract(1, 'day').format('YYYY-MM-DD')
  const today = dayjs().format('YYYY-MM-DD')
  /** 拉取范围固定「当月 + 下月 7 天」：一次拉够所有可能的溢出显示，
   *  避免「视窗依赖溢出天数、溢出天数依赖拉取结果」的循环依赖。 */
  const queryTo = month.date(month.daysInMonth() + 8).format('YYYY-MM-DD')

  /** 溢出预览天数（TD-117/123）：当月数据（预定退房日、客卡到期日）超出月末的
   *  最远天数，0–7 天封顶；**尾部至少 1 格**（下月 1 号，TD-123 用户要求）。
   *  后缘按「0-based 格 index + 1」算，含前导列偏移 +1。 */
  const overflowDays = (() => {
    const n = month.daysInMonth()
    let farthest = 0
    const scan = (endDay: string) => {
      // endDay 所在格的后缘（0-based index + 1；days[0] = 上月末日，故 +2）
      const edge = dayjs(endDay).diff(month, 'day') + 2
      if (edge > farthest) farthest = edge
    }
    for (const r of list) if (r.status === 'booked') scan(r.checkOut)
    for (const spans of Object.values(guestSpans)) for (const v of spans) scan(v.end)
    // 溢出 = 超出「前导 1 格 + 当月 n 格」的部分
    const spill = Math.max(0, farthest - (n + 1))
    return Math.max(1, Math.min(7, spill))
  })()

  /** 视窗上界（开区间）：月末 + 溢出天数的次日；spanOf / guestSpanOf 按它截断，
   *  所以 11-30 → 12-01 的色块能在 11 月视图里完整画到 12-01。 */
  const to = month.date(month.daysInMonth() + overflowDays + 1).format('YYYY-MM-DD')

  const days = useMemo(() => {
    const n = month.daysInMonth()
    // 前导列（TD-123）：固定显示上月最后一天（如 11 月视图左侧的 10-31）
    const lead = [month.subtract(1, 'day')]
    const base = Array.from({ length: n }, (_, i) => month.date(i + 1))
    // dayjs 的 date() 越界自动进位到下月：date(31) 在 11 月 = 12-01
    const over = Array.from({ length: overflowDays }, (_, i) => month.date(n + 1 + i))
    return [...lead, ...base, ...over]
  }, [month, overflowDays])

  useEffect(() => {
    let alive = true
    api
      .listReservations(leadDay, queryTo)
      .then((r) => {
        if (alive) setList(r)
      })
      .catch((e) => {
        if (alive) setErr(describeError(e))
      })
    return () => {
      alive = false
    }
  }, [from, queryTo, reloadToken])

  // 房间页数据变化（预定增删会改房态/房间列表）时重拉；父组件用 key 变化触发
  useEffect(() => {
    setReloadToken((t) => t + 1)
  }, [rooms])

  useEffect(() => {
    let alive = true
    api
      .listGuestStays(leadDay, queryTo)
      .then((rows) => {
        if (!alive) return
        const grouped: Record<string, GuestSpan[]> = {}
        for (const s of rows) {
          ;(grouped[s.roomNo] ??= []).push({
            id: s.id,
            start: s.start,
            end: s.end,
            guestName: s.guestName,
            kind: s.kind,
            days: s.days,
            hours: s.hours,
            plannedEnd: s.plannedEnd,
            status: s.status,
            expiresAt: s.plannedEnd,
          })
        }
        setGuestSpans(grouped)
      })
      .catch(() => {
        // 拉取失败不挡日历：只是没有入住色块
      })
    return () => {
      alive = false
    }
  }, [leadDay, queryTo, reloadToken])

  const resOf = (roomNo: string) =>
    list.filter((r) => r.roomNo === roomNo)

  /** 该房**今天**是否有有效预定（TD-114 状态 tag）：
   *  用日历已拉的当月 list 判定，不新增查询。 */
  /** 间夜列 D = [D 12:00, D+1 12:00)；所有预定归属/色块共用。 */
  const nightStart = (d: dayjs.Dayjs) => d.hour(12).minute(0).second(0).millisecond(0)
  const nightOverlap = (start: dayjs.Dayjs, end: dayjs.Dayjs, col: dayjs.Dayjs) =>
    start.isBefore(nightStart(col).add(1, 'day')) && end.isAfter(nightStart(col))

  const bookedToday = (room: Room) => {
    return list.some((r) => {
      if (r.roomNo !== room.roomNo || r.status !== 'booked') return false
      const start = dayjs(`${r.checkIn}T${r.checkInTime}`)
      const end = dayjs(`${r.checkOut}T${r.checkOutTime}`)
      return nightOverlap(start, end, dayjs(today))
    })
  }

  /** 半格占用框：入住日从右半格起，退房日到左半格止。
   *  [s, e] 是占用的连续列区间，e 为开区间终点；
   *  5→6 一格显示为 5 右半 + 6 左半，5→7 为 5 右半 + 6 整格 + 7 左半。 */
  const occupancyBox = (s: number, e: number, sameDay = false) =>
    sameDay
      ? { left: s * COL_W + 2, width: COL_W / 2 - 4 }
      : {
          left: s * COL_W + COL_W / 2 + 2,
          width: (e - s) * COL_W - 4,
        }

  /** 色块在视窗内的连续坐标（单位=格，可带 0.5 半格）：
   *  终点 = 退房日格子的**中点**（TD-110 半格错位：张三 6→7 只画到 7 号格左半），
   *  起点 = 入住日格子左缘（前序退房日衔接时改中点，见下方 startHalf）。
   *  2026-11-06 用户要求退房日当天也显示，但相邻预定不能在同一列叠放。 */
  const spanOf = (r: Reservation): [number, number] | null => {
    // 间夜列定义：日期 D 的列 = [D 12:00, D+1 12:00)；入住/退房时间都不能省略。
    const start = dayjs(`${r.checkIn}T${r.checkInTime}`)
    const end = dayjs(`${r.checkOut}T${r.checkOutTime}`)
    if (!end.isAfter(start)) return null
    let startPos = -1
    let endPos = -1
    days.forEach((d, idx) => {
      if (nightOverlap(start, end, d)) {
        if (startPos < 0) startPos = idx
        endPos = idx + 1
      }
    })
    return startPos >= 0 && startPos < endPos ? [startPos, endPos] : null
  }

  /** 入住色块坐标（TD-116/123）：区间 [归属日, 到期日]（含到期日当天，锁按首刷起算），
   *  与视窗 [leadDay, to) 相交才画，跨月截断到视窗边缘。
   *  列 index = diff(month) + 1（前导列偏移，TD-123）；start 早于视窗（含归属日前移到
   *  上月末日）从左缘起画。 */
  const guestSpanOf = (span: { start: string; end: string }): [number, number] | null => {
    const endNext = dayjs(span.end).add(1, 'day').format('YYYY-MM-DD')
    if (span.start >= to || endNext <= leadDay) return null
    const startPos = span.start >= from ? dayjs(span.start).diff(month, 'day') + 1 : 0
    // 退房/到期日也画左半格：终点用退房日，而不是退房日的次日。
    const endPos = span.end >= to ? days.length : dayjs(span.end).diff(month, 'day') + 1
    if (startPos < endPos) return [startPos, endPos]
    // 同一天入住/到期：只占当天一格，避免首尾两个半格叠在一起。
    return startPos < days.length ? [startPos, startPos + 1] : null
  }

  const gridWidth = days.length * COL_W

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space wrap>
        <Button icon={<LeftOutlined />} onClick={() => setMonth((m) => m.subtract(1, 'month'))}>
          上月
        </Button>
        <Title level={5} style={{ margin: 0, minWidth: 130, textAlign: 'center' }}>
          {month.format('YYYY 年 M 月')}
        </Title>
        <Button icon={<RightOutlined />} onClick={() => setMonth((m) => m.add(1, 'month'))}>
          下月
        </Button>
        <Button onClick={() => setMonth(dayjs().startOf('month'))}>回到今天</Button>
        <Button type="primary" icon={<PlusOutlined />} onClick={onNewReservation}>
          新增预定
        </Button>
        <Select
          value={typeFilter === 'all' ? 'all' : typeFilter === 'none' ? 'none' : String(typeFilter)}
          onChange={(v) => setTypeFilter(v === 'all' || v === 'none' ? v : Number(v))}
          style={{ width: 150 }}
          options={[
            { value: 'all', label: `全部房型 ${rooms.length}` },
            ...roomTypes.map((t) => ({
              value: String(t.id),
              label: `${t.name} ${rooms.filter((r) => r.roomTypeId === t.id).length}`,
            })),
            ...(untypedCount > 0 ? [{ value: 'none', label: `未分类 ${untypedCount}` }] : []),
          ]}
        />
        <Text type="secondary">
          {list.length > 0 ? `本月 ${list.length} 条预定` : '本月暂无预定'} · 点空白格预定，
          点色块改期 / 取消
        </Text>
      </Space>

      {err ? <Text type="danger">{err}</Text> : null}

      {rows.length === 0 ? (
        <Empty description={rooms.length === 0 ? '还没有可显示的房间' : '该房型下没有房间'} />
      ) : (
        <div style={{ overflow: 'auto', maxHeight: '78vh', border: '1px solid #f0f0f0' }}>
          <div style={{ minWidth: ROOM_COL_W + gridWidth, position: 'relative' }}>
            {/* 表头：左上角 sticky，日期列 sticky top */}
            <div
              style={{
                display: 'flex',
                position: 'sticky',
                top: 0,
                zIndex: 3,
                background: '#fff',
                borderBottom: '1px solid #f0f0f0',
              }}
            >
              <div
                style={{
                  position: 'sticky',
                  left: 0,
                  zIndex: 4,
                  width: ROOM_COL_W,
                  flex: `0 0 ${ROOM_COL_W}px`,
                  background: '#fafafa',
                  borderRight: '1px solid #f0f0f0',
                  padding: '4px 8px',
                  display: 'flex',
                  alignItems: 'center',
                }}
              >
                <Text type="secondary">房间 / 日期</Text>
              </div>
              {days.map((d) => {
                const ds = d.format('YYYY-MM-DD')
                const isToday = ds === today
                const weekend = d.day() === 0 || d.day() === 6
                // 溢出预览列（TD-117）：不属于当前月的下月列，浅灰 + M/D 标注归属
                const isOverflow = d.month() !== month.month()
                return (
                  <div
                    key={ds}
                    style={{
                      width: COL_W,
                      flex: `0 0 ${COL_W}px`,
                      textAlign: 'center',
                      padding: '2px 0',
                      background: isToday
                        ? '#e6f4ff'
                        : weekend
                          ? '#fafafa'
                          : isOverflow
                            ? '#f5f5f5'
                            : '#fff',
                      borderRight: '1px solid #f0f0f0',
                    }}
                  >
                    <div style={{ fontSize: 11, color: '#8c8c8c' }}>{WEEK[d.day()]}</div>
                    <div
                      style={{
                        fontSize: 13,
                        fontWeight: isToday ? 700 : 500,
                        color: isToday ? '#1677ff' : isOverflow ? '#8c8c8c' : '#262626',
                      }}
                    >
                      {isOverflow ? `${d.month() + 1}/${d.date()}` : d.date()}
                    </div>
                  </div>
                )
              })}
            </div>

            {rows.map((room) => {
              // 房态定义来自 RoomManager 传入的 statuses（与房间格子同一份数据）
              const st = statuses.find((s) => s.key === room.status)
              const stColor = st?.color
              return (
                <div key={room.roomNo} style={{ display: 'flex', borderBottom: '1px solid #f0f0f0' }}>
                  {/* 房号列 sticky left：横向滚动时仍看得见是谁 + 当前房态 + 入住/预定 */}
                  <div
                    style={{
                      position: 'sticky',
                      left: 0,
                      zIndex: 2,
                      width: ROOM_COL_W,
                      flex: `0 0 ${ROOM_COL_W}px`,
                      background: stColor ? `${stColor}14` : '#fff',
                      borderRight: '1px solid #f0f0f0',
                      height: ROW_H,
                      padding: '2px 8px',
                      display: 'flex',
                      flexDirection: 'column',
                      justifyContent: 'center',
                      gap: 2,
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4, minWidth: 0 }}>
                      <span
                        style={{
                          width: 6,
                          height: 6,
                          borderRadius: '50%',
                          background: stColor ?? '#d9d9d9',
                          flex: '0 0 6px',
                        }}
                      />
                      <Text style={{ fontSize: 13, fontWeight: 600, flex: '0 0 auto' }}>{room.roomNo}</Text>
                      {/* 房型：弹性项，空间不够就省略（title 里看全称） */}
                      {room.roomTypeName ? (
                        <Text
                          type="secondary"
                          title={room.roomTypeName}
                          style={{
                            fontSize: 11,
                            flex: '1 1 auto',
                            minWidth: 0,
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                          }}
                        >
                          {room.roomTypeName}
                        </Text>
                      ) : null}
                      {st ? (
                        <Tag
                          color={st.color}
                          style={{
                            marginInlineEnd: 0,
                            fontSize: 11,
                            lineHeight: '16px',
                            padding: '0 4px',
                            flex: '0 0 auto',
                          }}
                        >
                          {st.label}
                        </Tag>
                      ) : null}
                    </div>
                    {/* 状态行（TD-114）：发卡即入住（activeCards>0），否则看当天有没有预定 */}
                    {room.activeCards > 0 ? (
                      <Tag
                        color="green"
                        style={{ marginInlineEnd: 0, fontSize: 11, lineHeight: '16px', padding: '0 4px', width: 'fit-content' }}
                      >
                        入住
                      </Tag>
                    ) : bookedToday(room) ? (
                      <Tag
                        color="blue"
                        style={{ marginInlineEnd: 0, fontSize: 11, lineHeight: '16px', padding: '0 4px', width: 'fit-content' }}
                      >
                        预定
                      </Tag>
                    ) : null}
                  </div>

                  <div style={{ position: 'relative', width: gridWidth, height: ROW_H }}>
                    {/* 空白格：点击预定该房该天 */}
                    <div style={{ display: 'flex', position: 'absolute', inset: 0 }}>
                      {days.map((d) => {
                        const ds = d.format('YYYY-MM-DD')
                        const weekend = d.day() === 0 || d.day() === 6
                        // 禁止修改昨天往前的数据（TD-112）：过去日期不能新建预定
                        const locked = ds < today
                        // 溢出预览列（TD-117）：下月列浅灰，与 header 对齐
                        const isOverflow = d.month() !== month.month()
                        return (
                          <div
                            key={ds}
                            onClick={locked ? undefined : () => onReserve(room, ds)}
                            title={
                              locked
                                ? `${room.roomNo} · ${ds} · 已过去，不可预定`
                                : `${room.roomNo} · ${ds} · 点击预定`
                            }
                            style={{
                              width: COL_W,
                              flex: `0 0 ${COL_W}px`,
                              height: '100%',
                              cursor: locked ? 'not-allowed' : 'pointer',
                              background:
                                ds === today
                                  ? '#e6f4ff'
                                  : weekend
                                    ? '#fafafa'
                                    : isOverflow
                                      ? '#f5f5f5'
                                      : 'transparent',
                              borderRight: '1px solid #f0f0f0',
                              opacity: locked ? 0.45 : 1,
                            }}
                          />
                        )
                      })}
                    </div>

                    {/* 入住色块（TD-116/120）：有效客卡的发卡日 → 到期日，浅绿垫底；
                        点击弹入住详情；预定色块 zIndex 更高在上层，两者互不干扰 */}
                    {(guestSpans[room.roomNo] ?? []).map((g) => {
                      const span = guestSpanOf(g)
                      if (!span) return null
                      const [s, e] = span
                      const st = stayStyle(g.status)
                      return (
                        <div
                          key={g.id}
                          title={`${room.roomNo} ${st.label} · ${g.start} → ${g.end}（点击查看入住详情）`}
                          onClick={(ev) => {
                            ev.stopPropagation()
                            setStayDetail({ roomNo: room.roomNo, span: g })
                          }}
                          style={{
                            position: 'absolute',
                            ...occupancyBox(s, e, g.start === g.end),
                            top: 4,
                            height: ROW_H - 8,
                            borderRadius: 4,
                            background: st.bg,
                            border: st.border,
                            cursor: 'pointer',
                            zIndex: 0,
                            overflow: 'hidden',
                            whiteSpace: 'nowrap',
                            fontSize: 13,
                            lineHeight: `${ROW_H - 8}px`,
                            padding: '0 48px 0 8px',
                            color: st.color,
                            ...truncMask(e, days.length),
                          }}
                        >
                          {st.badgeBg ? (
                            <span
                              style={{
                                position: 'absolute',
                                top: 2,
                                right: 2,
                                fontSize: 10,
                                lineHeight: '14px',
                                padding: '0 4px',
                                borderRadius: 3,
                                background: st.badgeBg,
                                color: '#fff',
                              }}
                            >
                              {st.label}
                            </span>
                          ) : null}
                          {g.guestName ? g.guestName : `入住 ${shortRange(g.start, g.end)}`}
                        </div>
                      )
                    })}

                    {/* 预定色块：绝对定位跨格，点击改期 / 取消 */}
                    {resOf(room.roomNo).map((r) => {
                      const span = spanOf(r)
                      if (!span) return null
                      const [s, e] = span
                      const past = r.checkOut <= today
                      // 已结束（退房日在昨天或更早）的预定禁止改期 / 取消（TD-112）；
                      // 今天退房的仍可调整（客人上午才走）
                      const locked = r.checkOut < today
                      // 完整入退房日期（TD-117）：跨年带年份，同年内 MM-DD
                      const range = shortRange(r.checkIn, r.checkOut)
                      return (
                        <div
                          key={r.id}
                          onClick={
                            locked
                              ? undefined
                              : (ev) => {
                                  ev.stopPropagation()
                                  onEdit(r)
                                }
                          }
                          title={`${room.roomNo} · ${r.checkIn} ${r.checkInTime} → ${r.checkOut} ${r.checkOutTime}${
                            r.guestName ? ` · ${r.guestName}` : ''
                          }${r.note ? ` · 备注：${r.note}` : ''}${
                            locked ? '（已结束，不可修改）' : '（点击改期 / 取消）'
                          }`}
                          style={{
                            position: 'absolute',
                            ...occupancyBox(s, e, r.checkIn === r.checkOut),
                            top: 4,
                            height: ROW_H - 8,
                            borderRadius: 4,
                            background: past ? '#d9d9d9' : blockColor(r.id),
                            color: past ? '#8c8c8c' : '#fff',
                            fontSize: 13,
                            lineHeight: `${ROW_H - 8}px`,
                            padding: '0 44px 0 8px',
                            overflow: 'hidden',
                            whiteSpace: 'nowrap',
                            textOverflow: 'ellipsis',
                            cursor: 'pointer',
                            boxShadow: '0 1px 2px rgba(0,0,0,0.12)',
                            zIndex: 1,
                            ...truncMask(e, days.length),
                          }}
                        >
                          {/* 角标（TD-117）：右上角小号「预定」 */}
                          <span
                            style={{
                              position: 'absolute',
                              top: 2,
                              right: 2,
                              fontSize: 10,
                              lineHeight: '14px',
                              padding: '0 4px',
                              borderRadius: 3,
                              background: past ? 'rgba(0,0,0,0.15)' : 'rgba(255,255,255,0.3)',
                              color: past ? '#8c8c8c' : '#fff',
                            }}
                          >
                            预定
                          </span>
                          {/* 有客人姓名就只显示姓名（用户要求）；否则退到备注，再退到日期区间 */}
                          {r.guestName ? r.guestName : r.note ? r.note : range}
                        </div>
                      )
                    })}
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      )}

      {/* 入住详情（TD-120）：点入住色块弹出；纯展示，只有关闭 */}
      <Modal
        open={stayDetail !== null}
        title={`入住详情 · ${stayDetail?.roomNo ?? ''}`}
        footer={null}
        onCancel={() => setStayDetail(null)}
      >
        {stayDetail ? (
          <Descriptions bordered size="small" column={1}>
            <Descriptions.Item label="客人">
              {stayDetail.span.guestName ?? '（未填写）'}
            </Descriptions.Item>
            <Descriptions.Item label="房号">{stayDetail.roomNo}</Descriptions.Item>
            <Descriptions.Item label="卡型">{stayDetail.span.kind}</Descriptions.Item>
            <Descriptions.Item label="入住 → 到期">
              {stayDetail.span.start} → {stayDetail.span.end}
            </Descriptions.Item>
            <Descriptions.Item label="时长">
              {stayDetail.span.days > 0
                ? `${stayDetail.span.days} 天`
                : `${stayDetail.span.hours} 小时`}
            </Descriptions.Item>
            <Descriptions.Item label="状态">
              {(() => {
                const span = stayDetail.span
                if (span.status === 'checked_out') return <Tag>已退房</Tag>
                if (span.status === 'invalidated') return <Tag color="purple">已作废</Tag>
                if (span.status === 'expired') return <Tag color="orange">已过期</Tag>
                const left = Date.parse(span.expiresAt) - Date.now()
                return left <= 24 * 3600 * 1000 ? (
                  <Tag color="orange">即将到期</Tag>
                ) : (
                  <Tag color="green">有效</Tag>
                )
              })()}
            </Descriptions.Item>
            <Descriptions.Item label="说明">按发卡时间估算，锁从首次刷卡起算</Descriptions.Item>
          </Descriptions>
        ) : null}
      </Modal>
    </Space>
  )
}
