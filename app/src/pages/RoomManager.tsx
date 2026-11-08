import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  AppstoreOutlined,
  CalendarOutlined,
  CheckSquareOutlined,
  CloseCircleOutlined,
  DeleteOutlined,
  FieldTimeOutlined,
  IdcardOutlined,
  LogoutOutlined,
  PlusOutlined,
  ScanOutlined,
  SwapOutlined,
} from '@ant-design/icons'
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Drawer,
  Dropdown,
  Empty,
  Input,
  InputNumber,
  Modal,
  Segmented,
  Select,
  Space,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  type MenuProps,
} from 'antd'
import {
  api,
  describeError,
  formatExpiry,
  formatTime,
  type CardReadDto,
  type GuestInput,
  type IssuanceRow,
  type Reservation,
  type Room,
  type RoomMovePreview,
  type RoomStatus,
  type RoomType,
} from '../api'
import { roomNoIssue } from '../utils/validate'
import { statusIcon } from '../components/StatusIcon'
import IssueForm from '../components/IssueForm'
import { CheckoutCardModal } from '../components/CheckoutCardModal'
import NoCardCheckoutModal from '../components/NoCardCheckoutModal'
import { CardSnapshotView } from '../components/CardSnapshotView'
import ReservationModal from '../components/ReservationModal'
import RoomCalendar from '../components/RoomCalendar'

const { Text, Title } = Typography

/**
 * 能往房里写卡的房态。
 *
 * 空净房（新客入住）、未满房（加发一张卡）、**预定房**（客人到了直接发卡，TD-107）允许发卡 / 续卡；
 * 空脏房、已满房、维修房、其他房一律默认禁用，免得往脏房或维修房里写卡。
 * 房态 key 见 ocw-store 的 schema.sql（room_status 表）。
 */
const ISSUABLE_STATUS = new Set(['vacant_clean', 'occupied_partial', 'reserved'])

/** 本地今天 / 明天（YYYY-MM-DD）：查「覆盖今天」的预定用
 *  （listReservations(from=今天, to=明天) 即 checkIn < 明天 && checkOut > 今天）。 */
const localDays = (): [string, string] => {
  const pad = (n: number) => String(n).padStart(2, '0')
  const now = new Date()
  const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
  const t = new Date(now.getTime() + 86400000)
  const tomorrow = `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())}`
  return [today, tomorrow]
}

/**
 * 房间管理（格子视图 + 房间抽屉）。
 *
 * 每个格子按**房态**着色（不再按房型），点开抽屉可以：改房态、改房型/上限、
 * 直接发客卡、退房、制作安装卡。
 */
export default function RoomManager({
  onRoomsChanged,
  canAdmin,
  settingsVersion,
}: {
  onRoomsChanged?: () => void
  canAdmin: boolean
  /** 设置页改过房型/房态等配置就 +1，用来触发本页重新拉数据 */
  settingsVersion: number
}) {
  const [rooms, setRooms] = useState<Room[]>([])
  /** 全局默认最大发卡数（TD-161）：卡片上算「有效卡 N/上限」用 */
  const [maxDefault, setMaxDefault] = useState(2)
  const [roomTypes, setRoomTypes] = useState<RoomType[]>([])
  const [statuses, setStatuses] = useState<RoomStatus[]>([])
  const [roomNo, setRoomNo] = useState('')
  const [index, setIndex] = useState<number | null>(null)
  const [newTypeId, setNewTypeId] = useState<number | null>(null)
  const [filterType, setFilterType] = useState<number | 'all' | 'none'>('all')
  const [filterStatus, setFilterStatus] = useState<string | 'all'>('all')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [roomNoErr, setRoomNoErr] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  // 两个二级表单：新增房间、读卡退房（都要先填/先读，再确认，不在工具栏里直接执行）
  const [createOpen, setCreateOpen] = useState(false)
  const [checkoutOpen, setCheckoutOpen] = useState(false)
  const [checkoutTarget, setCheckoutTarget] = useState<Room | null>(null)
  /**
   * 每次打开退房弹窗都换一个 key，强制重新挂载。
   * 只靠 `open` 变化在 effect 里清状态的话，首帧仍会带着上一次读到的卡渲染，
   * 于是"刚点开就冒出上次那张卡 + 强制退房按钮"，甚至误触二次确认。
   */
  const [checkoutSession, setCheckoutSession] = useState(0)

  // 多选 + 批量改房态（多选模式只从房间卡右键菜单进入）
  const [selectMode, setSelectMode] = useState(false)
  const [selected, setSelected] = useState<string[]>([])
  const [batchStatus, setBatchStatus] = useState<string | null>(null)
  const [batchBusy, setBatchBusy] = useState(false)

  // 抽屉里的房间
  const [active, setActiveRoom] = useState<Room | null>(null)
  const [editTypeId, setEditTypeId] = useState<number | null>(null)
  const [savingEdit, setSavingEdit] = useState(false)
  // 改房号（房间显示名称）是独立动作：序号不变，房号不能与其它房间重复
  const [editRoomNo, setEditRoomNo] = useState('')
  /** 房间级最大发卡数（TD-161）：`null` = 跟随全局、`0` = 不限、`n` = 具体张数 */
  const [editMax, setEditMax] = useState<number | null>(null)
  const [maxBusy, setMaxBusy] = useState(false)
  const [renameBusy, setRenameBusy] = useState(false)
  const [renameErr, setRenameErr] = useState<string | null>(null)
  const [renameOk, setRenameOk] = useState<string | null>(null)

  const [installOpen, setInstallOpen] = useState(false)
  /** 无卡退房选卡弹窗（TD-121）：目标房间；null = 关闭 */
  const [noCheckoutRoom, setNoCheckoutRoom] = useState<Room | null>(null)
  // 发卡只从右键菜单进入，抽屉里不再放发卡表单
  const [issueRoom, setIssueRoom] = useState<Room | null>(null)
  const [issueMode, setIssueMode] = useState<'new' | 'renew' | 'move'>('new')
  /** 预定房发卡时预填的客人姓名（TD-107）；拉预定失败或无预定时为 null */
  const [issueGuest, setIssueGuest] = useState<GuestInput | null>(null)
  /** 当天有预定时的发卡确认（TD-113）：先问「是给这位预定客人发卡吗」 */
  const [issueConfirm, setIssueConfirm] = useState<{
    room: Room
    mode: 'new' | 'renew'
    res: Reservation
  } | null>(null)
  /** 改房态的二次确认（TD-119）：目标房当前是预定房且今天有预定时，先问是否取消预定 */
  const [statusConfirm, setStatusConfirm] = useState<{
    text: string
    run: () => Promise<void>
  } | null>(null)

  /**
   * 打开发卡弹窗。该房**覆盖今天**有有效预定时先弹确认（TD-113）：
   * 「是，按预定客人发卡」→ 预填姓名打开；「不是，手动填写」→ 不预填打开；
   * 取消则不开。没有预定直接打开。拉取失败不阻断发卡。换房 move 路径不走这里。
   */
  const openIssue = async (room: Room, mode: 'new' | 'renew') => {
    setIssueMode(mode)
    setIssueGuest(null)
    try {
      const pad = (n: number) => String(n).padStart(2, '0')
      const now = new Date()
      const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
      const tomorrowDate = new Date(now.getTime() + 86400000)
      const tomorrow = `${tomorrowDate.getFullYear()}-${pad(tomorrowDate.getMonth() + 1)}-${pad(tomorrowDate.getDate())}`
      const list = await api.listReservations(today, tomorrow, room.roomNo)
      const hit = list.find((r) => r.status === 'booked')
      if (hit) {
        setIssueConfirm({ room, mode, res: hit })
        return
      }
    } catch {
      // 预定查询失败不影响发卡，直接开表单
    }
    setIssueRoom(room)
  }

  /** 发卡确认框的两个出口（TD-113）：按预定客人（预填姓名）/ 手动填写（不预填）。 */
  const confirmIssue = (useGuest: boolean) => {
    if (!issueConfirm) return
    if (useGuest && issueConfirm.res.guestName) {
      setIssueGuest({ name: issueConfirm.res.guestName })
    } else {
      setIssueGuest(null)
    }
    setIssueMode(issueConfirm.mode)
    setIssueRoom(issueConfirm.room)
    setIssueConfirm(null)
  }
  /**
   * 换房：源房 → 目标房。
   *
   * 流程刻意拆成两步：先看源房能带走什么（几张有效卡 / 客人是谁 / 到期时间），
   * 选好目标房再执行"退旧房"，随后直接打开目标房的发卡表单（旧卡的房间序号是写死的，
   * 不重新写卡客人进不了新房）。`moveFrom` 非空 = 换房弹窗打开。
   */
  const [moveFrom, setMoveFrom] = useState<Room | null>(null)
  const [moveTo, setMoveTo] = useState<string | null>(null)
  const [movePreview, setMovePreview] = useState<RoomMovePreview | null>(null)
  const [moveBusy, setMoveBusy] = useState(false)
  const [moveLoading, setMoveLoading] = useState(false)
  const [moveError, setMoveError] = useState<string | null>(null)
  /**
   * 换房前必须读卡核对：读到的卡号在源房有效卡里才算"原卡"。
   * 不是原卡时弹二次确认（可能是别的房间的卡、原厂卡或空白卡）。
   */
  const [moveRead, setMoveRead] = useState<CardReadDto | null>(null)
  const [moveReading, setMoveReading] = useState(false)
  const [moveReadErr, setMoveReadErr] = useState<string | null>(null)
  const [moveForceOpen, setMoveForceOpen] = useState(false)
  const [moveForceConfirmed, setMoveForceConfirmed] = useState(false)

  /** 读到的卡是不是源房当前的有效卡（读卡记录按房号+卡型+时间戳匹配）。 */
  const moveCardIsOriginal =
    moveRead?.lookup.status === 'found' &&
    moveRead.lookup.latest.roomNo === moveFrom?.roomNo
  // 删除房间 = 逻辑删除（active=0）。二次确认用受控 Modal：antd 命令式弹窗在 React 19 下静默失效
  const [deleteTarget, setDeleteTarget] = useState<Room | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)

  const reload = useCallback(async () => {
    try {
      const [roomList, types, stats] = await Promise.all([
        api.listRooms(),
        api.listRoomTypes(),
        api.listRoomStatuses(),
      ])
      setRooms(roomList)
      setRoomTypes(types)
      setStatuses(stats)
      // 房间被删/被改号后，把选中集合里失效的房号去掉（逻辑删除的也算失效）
      setSelected((prev) => prev.filter((no) => roomList.some((r) => r.roomNo === no && r.active)))
      setActiveRoom((prev) => (prev ? (roomList.find((r) => r.roomNo === prev.roomNo) ?? null) : null))
    } catch (e) {
      setError(describeError(e))
    }
  }, [])

  // 设置页改了房型颜色/房态图标后立刻重画，不用再手动点刷新
  useEffect(() => {
    void reload()
  }, [reload, settingsVersion])

  // 全局默认上限（TD-161）：设置页改完 `settingsVersion` 会变，卡片上的「N/上限」要跟着刷
  useEffect(() => {
    void api
      .getMaxCardsDefault()
      .then(setMaxDefault)
      .catch(() => {})
  }, [settingsVersion])

  const statusOf = (key: string): RoomStatus | undefined => statuses.find((s) => s.key === key)

  /** 房型标签颜色（在设置→房型里自定义；没设就是中性灰）。 */
  const typeColor = (typeId: number | null): string | undefined =>
    typeId === null ? undefined : (roomTypes.find((t) => t.id === typeId)?.color ?? undefined)

  /**
   * 界面上"存在"的房间 = 未删除的房间。
   *
   * `rooms` 仍是全量（含已逻辑删除的）：退房弹窗要靠它按序号反查房号退旧卡，
   * 缺了已删房就退不了那些卡。所以只在这里过滤，不动 state。
   */
  const liveRooms = useMemo(() => rooms.filter((r) => r.active), [rooms])

  const visible = useMemo(
    () =>
      liveRooms.filter(
        (r) =>
          (filterType === 'all' ||
            (filterType === 'none' ? r.roomTypeId === null : r.roomTypeId === filterType)) &&
          (filterStatus === 'all' || r.status === filterStatus),
      ),
    [liveRooms, filterType, filterStatus],
  )

  /**
   * 房型筛选做成标签页（原来在工具栏里是一个 Select）。
   *
   * 旧实现的症状是"选了房型没反应"，而过滤表达式与数据本身都核对过是对的
   * （roomTypeId 与房型 id 都是 number，房间也都带房型），所以怀疑点落在那只
   * Select 对**数字 value** 的处理上。改成 Tabs 后 key 一律是字符串，由
   * onTypeTabChange 自己解析回 id，不再依赖控件对非字符串 value 的行为。
   *
   * 标签上的数字按当前在用的房间现算（不用后端 roomCount），房型增删、房间改房型
   * 或房间被删除后立即跟着变；房态筛选是叠加条件，不改变标签上的数量。
   */
  const typeTabs = useMemo(() => {
    const countOf = (predicate: (r: Room) => boolean) => liveRooms.filter(predicate).length
    const untyped = countOf((r) => r.roomTypeId === null)
    const items = [
      { key: 'all', label: `全部 ${countOf(() => true)}` },
      ...roomTypes.map((t) => ({
        key: String(t.id),
        label: `${t.name} ${countOf((r) => r.roomTypeId === t.id)}`,
      })),
    ]
    if (untyped > 0) items.push({ key: 'none', label: `未分类 ${untyped}` })
    return items
  }, [liveRooms, roomTypes])

  const typeTabKey =
    filterType === 'all' ? 'all' : filterType === 'none' ? 'none' : String(filterType)

  const onTypeTabChange = (key: string) => {
    if (key === 'all' || key === 'none') setFilterType(key)
    else {
      const id = Number(key)
      setFilterType(Number.isFinite(id) ? id : 'all')
    }
  }

  // ---------------------------------------------------------------- 多选 / 批量

  /** 进多选模式：从某房间右键进来时顺手把那间选上 */
  const enterSelectMode = (roomNo?: string) => {
    setSelectMode(true)
    setNotice(null)
    setError(null)
    if (roomNo) setSelected((prev) => (prev.includes(roomNo) ? prev : [...prev, roomNo]))
  }

  const exitSelectMode = () => {
    setSelectMode(false)
    setSelected([])
    setBatchStatus(null)
  }

  const toggleSelect = (roomNo: string) =>
    setSelected((prev) =>
      prev.includes(roomNo) ? prev.filter((n) => n !== roomNo) : [...prev, roomNo],
    )

  const allVisibleSelected = visible.length > 0 && visible.every((r) => selected.includes(r.roomNo))
  const someVisibleSelected = visible.some((r) => selected.includes(r.roomNo))

  /** 全选 / 取消全选只作用于当前筛选结果，不影响已选中的其它房间 */
  const toggleSelectAllVisible = () => {
    if (allVisibleSelected) {
      const drop = new Set(visible.map((r) => r.roomNo))
      setSelected((prev) => prev.filter((n) => !drop.has(n)))
    } else {
      setSelected((prev) => [...new Set([...prev, ...visible.map((r) => r.roomNo)])])
    }
  }

  /** 批量改房态：逐个调用单房命令，失败也继续，最后汇总 */
  const applyBatchStatus = async () => {
    if (!batchStatus || selected.length === 0) return
    const label = statuses.find((s) => s.key === batchStatus)?.label ?? batchStatus

    // 选中的预定房里，今天有预定的要先确认（TD-119）；纯未来预定不弹框
    const reservedNos = selected.filter(
      (no) => rooms.find((r) => r.roomNo === no)?.status === 'reserved',
    )
    if (reservedNos.length > 0) {
      try {
        const [today, tomorrow] = localDays()
        const all = await api.listReservations(today, tomorrow)
        const affected = [
          ...new Set(
            all
              .filter((r) => r.status === 'booked' && reservedNos.includes(r.roomNo))
              .map((r) => r.roomNo),
          ),
        ]
        if (affected.length > 0) {
          setStatusConfirm({
            text: `其中 ${affected.length} 间今天有预定（${affected.join('、')}），改为「${label}」将一并取消这些预定，继续吗？`,
            run: async () => {
              await cancelTodaysReservations(affected)
              await executeBatch()
            },
          })
          return
        }
      } catch {
        // 查询失败按原逻辑继续，不阻断批量改房态
      }
    }
    await executeBatch()
  }

  const executeBatch = async () => {
    if (!batchStatus || selected.length === 0) return
    const label = statuses.find((s) => s.key === batchStatus)?.label ?? batchStatus
    setBatchBusy(true)
    setError(null)
    setNotice(null)
    let ok = 0
    const failed: string[] = []
    for (const roomNo of selected) {
      try {
        await api.setRoomStatus(roomNo, batchStatus)
        ok += 1
      } catch {
        failed.push(roomNo)
      }
    }
    if (failed.length === 0) {
      setNotice(`${ok} 间房间的房态已改为「${label}」`)
    } else {
      setError(
        `${ok} 间已改为「${label}」，${failed.length} 间失败：${failed.join('、')}` +
          (ok === 0 ? '（请确认已登录且有权限）' : ''),
      )
    }
    await reload()
    onRoomsChanged?.()
    exitSelectMode()
    setBatchBusy(false)
  }

  /** 抽屉「有效客卡」列表（TD-121）：打开抽屉时按房实查，关闭清空。
   *  与 room.activeCards 同口径（result='ok'、有卡号、未退房、未过期、日租/钟点，按卡号取最新）。 */
  const [drawerCards, setDrawerCards] = useState<IssuanceRow[]>([])

  const loadDrawerCards = async (roomNo: string) => {
    setDrawerCards([])
    try {
      const p = await api.listIssuances({ roomNo, page: 1, pageSize: 200 })
      const now = Date.now()
      // 有效客卡按「张」逐条：同房多张各占一行，按发卡时间排序
      const valid = p.rows.filter(
        (r) =>
          r.result === 'ok' &&
          r.checkedOutAt === null &&
          !!r.expiresAt &&
          Date.parse(r.expiresAt) > now &&
          (r.kind === '日租客卡' || r.kind === '钟点客卡'),
      )
      setDrawerCards(
        valid.sort((a, b) => a.createdAt.localeCompare(b.createdAt) || a.id - b.id),
      )
    } catch {
      // 拉取失败：区块显示空态，不阻断抽屉其它操作
      setDrawerCards([])
    }
  }

  const openRoom = (room: Room) => {
    setActiveRoom(room)
    setEditTypeId(room.roomTypeId)
    setEditRoomNo(room.roomNo)
    setEditMax(room.maxCards)
    setRenameErr(null)
    setRenameOk(null)
    setError(null)
    setNotice(null)
    void loadDrawerCards(room.roomNo)
  }

  /** 改房号：序号不变，只改显示名称；重名与"场上还有有效卡"都会拦住。 */
  const doRenameRoom = async () => {
    if (!active) return
    const next = editRoomNo.trim()
    const issue = roomNoIssue(next)
    setRenameErr(issue)
    setRenameOk(null)
    if (issue) return
    if (next === active.roomNo) return
    if (rooms.some((r) => r.roomNo === next)) {
      setRenameErr(`房号 ${next} 已被占用，请换一个`)
      return
    }
    setRenameBusy(true)
    try {
      const room = await api.renameRoom(active.roomNo, next)
      setActiveRoom(room)
      setEditRoomNo(room.roomNo)
      setRenameOk(`已改名为 ${room.roomNo}（序号 ${room.roomIndex} 不变）`)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setRenameErr(describeError(e))
    } finally {
      setRenameBusy(false)
    }
  }

  /** 保存房间级最大发卡数（TD-161）：留空跟随全局、`0` = 不限。 */
  const saveMax = async () => {
    if (!active) return
    setMaxBusy(true)
    setError(null)
    try {
      await api.setRoomMaxCards(active.roomNo, editMax)
      setNotice(
        `${active.roomNo} 房最大发卡数已更新：${
          editMax === null ? '跟随全局默认' : editMax === 0 ? '不限' : `${editMax} 张`
        }`,
      )
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setMaxBusy(false)
    }
  }

  const openCreate = () => {
    setRoomNo('')
    setIndex(null)
    setNewTypeId(null)
    setRoomNoErr(null)
    setError(null)
    setNotice(null)
    setCreateOpen(true)
  }

  const addRoom = async () => {
    const issue = roomNoIssue(roomNo)
    setRoomNoErr(issue)
    if (issue) return
    setLoading(true)
    setError(null)
    setNotice(null)
    try {
      const { room, revivedFrom } = await api.createRoom(
        roomNo.trim(),
        index ?? undefined,
        newTypeId ?? undefined,
      )
      setRoomNo('')
      setIndex(null)
      setNewTypeId(null)
      setCreateOpen(false)
      // 序号撞上已删除的房间时后端会复用那条记录（只改房名），提示里要说清楚
      const head = revivedFrom
        ? `已新增 ${room.roomNo}（复用序号 ${room.roomIndex}，原「${revivedFrom}」房的记录已改名）`
        : `已新增 ${room.roomNo}（序号 ${room.roomIndex}）`
      setNotice(`${head}。制作安装卡后到该房门锁刷一次即可。`)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setLoading(false)
    }
  }

  /** 查某房**覆盖今天**的有效预定（TD-119）；查询失败返回 []，不阻断改房态。 */
  const todaysReservationsOf = async (roomNo: string): Promise<Reservation[]> => {
    try {
      const [today, tomorrow] = localDays()
      const list = await api.listReservations(today, tomorrow, roomNo)
      return list.filter((r) => r.status === 'booked')
    } catch {
      return []
    }
  }

  /** 取消这些房**覆盖今天**的全部有效预定（TD-119 确认后的执行）。
   *  listReservations(今天, 明天) 即 checkIn < 明天 && checkOut > 今天 —— 纯未来预定不返回。 */
  const cancelTodaysReservations = async (roomNos: string[]) => {
    const [today, tomorrow] = localDays()
    const all = await api.listReservations(today, tomorrow)
    for (const r of all) {
      if (r.status === 'booked' && roomNos.includes(r.roomNo)) {
        await api.cancelReservation(r.id)
      }
    }
  }

  const changeStatus = async (room: Room, key: string) => {
    // 目标房是预定房且今天有预定 → 先弹确认（TD-119）；纯未来预定不受影响、不弹框
    if (room.status === 'reserved') {
      const todays = await todaysReservationsOf(room.roomNo)
      if (todays.length > 0) {
        const label = statuses.find((s) => s.key === key)?.label ?? key
        const first = todays[0]
        const who = first.guestName ? `（${first.guestName}）` : ''
        setStatusConfirm({
          text: `${room.roomNo} 今天有预定：${first.checkIn} → ${first.checkOut}${who}。改为「${label}」将取消该预定，继续吗？`,
          run: async () => {
            await cancelTodaysReservations([room.roomNo])
            await api.setRoomStatus(room.roomNo, key)
            await reload()
            onRoomsChanged?.()
          },
        })
        return
      }
    }
    try {
      await api.setRoomStatus(room.roomNo, key)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    }
  }

  const saveParams = async () => {
    if (!active) return
    setSavingEdit(true)
    setError(null)
    try {
      if (editTypeId !== active.roomTypeId) {
        await api.setRoomType(active.roomNo, editTypeId)
      }
      setNotice(`${active.roomNo} 的房间参数已保存`)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setSavingEdit(false)
    }
  }

  /**
   * 读卡退房 / 持卡退房：只负责打开二级表单。
   *
   * 真正的"读卡 → 核对 → 退房"两步都在弹窗里完成，工具栏按钮不再一按就读卡，
   * 免得前台还没把卡放上去就先收到一个读卡失败。
   */
  const openCheckout = (target: Room | null) => {
    setCheckoutTarget(target)
    setCheckoutSession((n) => n + 1)
    setCheckoutOpen(true)
  }

  /** 无卡退房（TD-121）：打开选卡弹窗，只退用户勾选的卡（不再直接全退）。 */
  const noCardCheckout = (room: Room) => {
    setNoCheckoutRoom(room)
  }

  /** 预定房间：选日期的弹窗（TD-202，入口是房间卡片按钮 + 日历空白格，不是右键菜单） */
  const [reserveTarget, setReserveTarget] = useState<Room | null>(null)
  const [reserveEditing, setReserveEditing] = useState<Reservation | null>(null)
  /** 日历空白格预填的入住日；卡片按钮打开时为 null（默认今天） */
  const [reserveInitialDate, setReserveInitialDate] = useState<string | null>(null)
  /** 每次打开 +1 当 key，强制重挂载弹窗（HMR 会保留组件 state） */
  const [reserveSession, setReserveSession] = useState(0)
  /** 房间格子视图 / 房态日历（TD-203） */
  const [viewMode, setViewMode] = useState<'rooms' | 'calendar'>('rooms')

  const openReserve = (room: Room, date?: string) => {
    setReserveEditing(null)
    setReserveInitialDate(date ?? null)
    setReserveTarget(room)
    setReserveSession((n) => n + 1)
  }
  const openEditReserve = (r: Reservation) => {
    setReserveEditing(r)
    setReserveTarget(null)
    setReserveInitialDate(null)
    setReserveSession((n) => n + 1)
  }
  const closeReserve = () => {
    setReserveTarget(null)
    setReserveEditing(null)
    setReserveInitialDate(null)
  }

  const doInstall = async () => {
    if (!active) return
    const room = active
    setInstallOpen(false)
    setLoading(true)
    setError(null)
    try {
      const issued = await api.issueInstallCard(room.roomNo)
      setNotice(`已为 ${room.roomNo} 制作安装卡（序号 ${issued.card.roomIndex}），请到该房门锁上刷一次。`)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setLoading(false)
    }
  }

  /** 续住默认天数：剩余天数 + 1，至少 1 天。 */
  const renewDays = (room: Room): number => {
    if (!room.activeUntil) return 1
    const left = new Date(room.activeUntil).getTime() - Date.now()
    return Math.max(1, Math.ceil(left / 86_400_000) + 1)
  }

  /**
   * 换房第一步：打开弹窗并查源房能带走什么（只读）。
   *
   * 没有有效客卡时也允许打开 —— 弹窗里会说明"该房当前没有有效卡，换房只会做退房登记"，
   * 免得前台以为按钮坏了。
   */
  const openMove = async (room: Room) => {
    setMoveFrom(room)
    setMoveTo(null)
    setMovePreview(null)
    setMoveError(null)
    setMoveRead(null)
    setMoveReadErr(null)
    setMoveForceConfirmed(false)
    setMoveForceOpen(false)
    setMoveLoading(true)
    try {
      setMovePreview(await api.roomMovePreview(room.roomNo))
    } catch (e) {
      setMoveError(describeError(e))
    } finally {
      setMoveLoading(false)
    }
  }

  /** 换房前读卡：核对客人手上的卡是不是本房原卡。 */
  const readMoveCard = async () => {
    setMoveReading(true)
    setMoveReadErr(null)
    setMoveForceConfirmed(false)
    try {
      setMoveRead(await api.readCard())
    } catch (e) {
      setMoveReadErr(describeError(e))
      setMoveRead(null)
    } finally {
      setMoveReading(false)
    }
  }

  /** 换房带入到新房的时长：按剩余有效期向上取整，至少 1。 */
  const moveIssueDefaults = (): { mode: 'day' | 'hour'; amount: number } => {
    const latest = movePreview?.latest
    const mode: 'day' | 'hour' = latest?.kind === '钟点客卡' ? 'hour' : 'day'
    if (!movePreview?.activeUntil) return { mode, amount: 1 }
    const leftMs = new Date(movePreview.activeUntil).getTime() - Date.now()
    const amount =
      mode === 'hour'
        ? Math.max(1, Math.ceil(leftMs / 3_600_000))
        : Math.max(1, Math.ceil(leftMs / 86_400_000))
    return { mode, amount }
  }

  /**
   * 换房第二步：退源房 → 打开目标房发卡表单。
   *
   * 不退房直接发新房的话，源房的卡还挂在"有效"里，房态与占用都会乱；
   * 只退房不写新卡的话，客人进不了新房（卡里的序号是旧房的）。
   */
  /** 真正的换房动作：退源房 → 转入目标房发卡表单。 */
  const doMove = async (from: Room, target: Room) => {
    setMoveBusy(true)
    setMoveError(null)
    try {
      const res = await api.moveRoomOut(from.roomNo, target.roomNo)
      setMoveFrom(null)
      setMoveRead(null)
      setMoveForceConfirmed(false)
      setNotice(
        `${from.roomNo} → ${target.roomNo}：已关闭源房 ${res.cardsClosed} 张有效卡，房态改为退房后房态；` +
          '旧卡仍能开源房门，请收回。接着为新房写卡。',
      )
      await reload()
      onRoomsChanged?.()
      // 直接接着写新房卡：客人信息与时长从源房带入
      setIssueMode('move')
      setIssueRoom(target)
    } catch (e) {
      setMoveError(describeError(e))
    } finally {
      setMoveBusy(false)
    }
  }

  const confirmMove = () => {
    if (!moveFrom || !moveTo || !moveRead) return
    const target = rooms.find((r) => r.roomNo === moveTo)
    if (!target) return
    // 不是本房原卡 → 先弹二次确认；确认后由弹窗调用 doMove
    if (!moveCardIsOriginal && !moveForceConfirmed) {
      setMoveForceOpen(true)
      return
    }
    void doMove(moveFrom, target)
  }

  /** 换房带入的客人信息：源房最近一张有效客卡的登记信息（全部为空则不传）。 */
  const guestFromPreview = (): GuestInput | undefined => {
    const l = movePreview?.latest
    if (!l) return undefined
    const g: GuestInput = {
      name: l.guestName ?? undefined,
      gender: l.guestGender ?? undefined,
      idType: l.guestIdType ?? undefined,
      idNumber: l.guestIdNumber ?? undefined,
      nation: l.guestNation ?? undefined,
      address: l.guestAddress ?? undefined,
      birthDate: l.guestBirthDate ?? undefined,
      note: l.guestNote ?? undefined,
    }
    const has = Object.values(g).some((v) => (v ?? '').trim() !== '')
    return has ? g : undefined
  }

  const typeOptions = roomTypes.map((t) => ({ value: t.id, label: t.name }))

  /** 房态下拉的选项（带房态自己的图标与颜色），筛选与批量改房态共用 */
  const statusOptions = statuses.map((s) => ({
    value: s.key,
    label: (
      <Space size={6}>
        <span style={{ color: s.color }}>{statusIcon(s.icon)}</span>
        <span>{s.label}</span>
      </Space>
    ),
  }))

  const activeStatus = active ? statusOf(active.status) : undefined

  /**
   * 房间卡右键菜单。
   *
   * 发卡 / 续卡只有空净房、未满房可点；其它房态置灰，并在菜单里补一行灰字说明原因。
   * 删除房间只给管理员，且是逻辑删除（active=0，序号不释放）。
   */
  const roomMenu = (room: Room): MenuProps['items'] => {
    const st = statusOf(room.status)
    const canIssueCard = ISSUABLE_STATUS.has(room.status)
    return [
      { key: 'multi', icon: <CheckSquareOutlined />, label: '多选房间' },
      { type: 'divider' },
      ...(canIssueCard
        ? []
        : [
            {
              key: 'why',
              disabled: true,
              label: (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  当前「{st?.label ?? '该房态'}」不能发卡 / 续卡
                </Text>
              ),
            },
          ]),
      // 预定入口（TD-108 按用户要求收回右键菜单；日历视图仍可点空白格直接预定）
      {
        key: 'reserve',
        icon: <CalendarOutlined />,
        label: '预定房间',
      },
      { key: 'issue', icon: <IdcardOutlined />, label: '发行房间门卡', disabled: !canIssueCard },
      { key: 'renew', icon: <FieldTimeOutlined />, label: '续住写卡', disabled: !canIssueCard },
      {
        key: 'move',
        icon: <SwapOutlined />,
        label: room.activeCards > 0 ? '换房' : '换房（该房当前无有效卡）',
        disabled: room.activeCards === 0,
      },
      { type: 'divider' },
      { key: 'checkoutCard', icon: <LogoutOutlined />, label: '持卡退房' },
      { key: 'checkoutNoCard', icon: <CloseCircleOutlined />, label: '无卡退房' },
      ...(canAdmin
        ? [
            { type: 'divider' as const },
            { key: 'delete', danger: true, icon: <DeleteOutlined />, label: '删除房间' },
          ]
        : []),
    ]
  }

  /** 逻辑删除房间。有有效卡时按钮是禁用的，这里再兜一次底。 */
  const confirmDelete = async () => {
    if (!deleteTarget || deleteTarget.activeCards > 0) return
    setDeleteBusy(true)
    setError(null)
    try {
      await api.deleteRoom(deleteTarget.roomNo)
      setNotice(
        `已删除 ${deleteTarget.roomNo}（序号 ${deleteTarget.roomIndex}），历史发卡记录保留`,
      )
      setDeleteTarget(null)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setDeleteBusy(false)
    }
  }

  return (
    <>
      <Card title="房间">
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Space wrap>
            {canAdmin ? (
              <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
                新增房间
              </Button>
            ) : null}
            <Button icon={<ScanOutlined />} onClick={() => openCheckout(null)}>
              读卡退房
            </Button>
            <Segmented
              value={viewMode}
              onChange={(v) => setViewMode(v as 'rooms' | 'calendar')}
              options={[
                { label: '房间', value: 'rooms' },
                { label: '日历', value: 'calendar' },
              ]}
            />
          </Space>

          {viewMode === 'calendar' ? (
            <RoomCalendar
              rooms={liveRooms}
              roomTypes={roomTypes}
              statuses={statuses}
              onReserve={(room, date) => openReserve(room, date)}
              onEdit={openEditReserve}
            />
          ) : (
          <>
          <Tabs
            className="ocw-type-tabs"
            activeKey={typeTabKey}
            onChange={onTypeTabChange}
            items={typeTabs}
            tabBarExtraContent={
              <Space size={6}>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  房态
                </Text>
                <Select
                  style={{ width: 152 }}
                  value={filterStatus}
                  onChange={(v) => setFilterStatus(v as string | 'all')}
                  options={[
                    {
                      value: 'all',
                      label: (
                        <Space size={6}>
                          <AppstoreOutlined style={{ color: '#8c8c8c' }} />
                          <span>全部房态</span>
                        </Space>
                      ),
                    },
                    ...statusOptions,
                  ]}
                />
              </Space>
            }
          />

          {/* 多选模式：只从房间卡右键菜单进入，进入后才出现这条操作栏 */}
          {selectMode ? (
            <Space wrap align="center" size={8}>
              <Checkbox
                checked={allVisibleSelected}
                indeterminate={!allVisibleSelected && someVisibleSelected}
                onChange={toggleSelectAllVisible}
              >
                全选当前结果
              </Checkbox>
              <Text type="secondary" style={{ fontSize: 12 }}>
                已选 {selected.length} 间
              </Text>
              <Select
                style={{ width: 170 }}
                placeholder="选择要改成的房态"
                value={batchStatus ?? undefined}
                onChange={(v) => setBatchStatus(typeof v === 'string' ? v : null)}
                options={statusOptions}
                disabled={selected.length === 0}
              />
              <Button
                type="primary"
                loading={batchBusy}
                disabled={selected.length === 0 || batchStatus === null}
                onClick={() => void applyBatchStatus()}
              >
                批量改房态
              </Button>
              {selected.length > 0 ? (
                <Button onClick={() => setSelected([])}>清空选择</Button>
              ) : null}
              <Button onClick={exitSelectMode}>退出多选</Button>
            </Space>
          ) : null}

          <Text type="secondary" style={{ fontSize: 12 }}>
            {selectMode
              ? '多选模式：点房间卡片勾选 / 取消，选好后挑一个房态批量应用。'
              : '点房间卡片可改房态、退房；右键房间卡片可预定（选日期）/ 发卡 / 续卡 / 多选批量改房态 / 删除房间；日历视图点空白格可直接预定。空净房、未满房、预定房能发卡或续卡。'}
          </Text>

          {error ? <Alert type="error" showIcon message={error} /> : null}
          {notice ? <Alert type="success" showIcon message={notice} /> : null}

          {visible.length === 0 ? (
            // 空库和"筛空了"是两件事：空库要给下一步怎么做，不能让前台以为软件坏了
            rooms.length === 0 ? (
              <Empty
                description={
                  <Space direction="vertical" size={4}>
                    <Text>还没有房间。</Text>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {canAdmin
                        ? '到 设置 → 房间 导入房号表（CSV / ini），或点右上角「新增房间」一个个加。'
                        : '请让管理员在 设置 → 房间 导入房号表，或先新增房间。'}
                    </Text>
                  </Space>
                }
              />
            ) : (
              <Empty description="没有符合筛选条件的房间" />
            )
          ) : (
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))',
                gap: 12,
              }}
            >
              {visible.map((room) => {
                const st = statusOf(room.status)
                const color = st?.color ?? '#8c8c8c'
                const isSelected = selected.includes(room.roomNo)
                return (
                  <Dropdown
                    key={room.roomNo}
                    trigger={['contextMenu']}
                    menu={{
                      items: roomMenu(room),
                      onClick: ({ key }) => {
                        if (key === 'multi') enterSelectMode(room.roomNo)
                        else if (key === 'checkoutCard') openCheckout(room)
                        else if (key === 'checkoutNoCard') noCardCheckout(room)
                        else if (key === 'delete') setDeleteTarget(room)
                        else if (key === 'issue') {
                          void openIssue(room, 'new')
                        } else if (key === 'renew') {
                          void openIssue(room, 'renew')
                        } else if (key === 'move') {
                          void openMove(room)
                        } else if (key === 'reserve') {
                          openReserve(room)
                        }
                      },
                    }}
                  >
                  <Card
                    size="small"
                    hoverable
                    // 多选模式下点卡片是勾选，不再打开抽屉
                    onClick={() => (selectMode ? toggleSelect(room.roomNo) : openRoom(room))}
                    style={{
                      borderTop: `3px solid ${color}`,
                      background: `${color}0f`,
                      boxShadow: isSelected ? '0 0 0 2px #1677ff' : undefined,
                    }}
                    styles={{ body: { padding: 12 } }}
                  >
                    <Space direction="vertical" size={6} style={{ width: '100%' }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          {selectMode ? (
                            <Checkbox
                              checked={isSelected}
                              // 勾选不应该顺手把抽屉打开，也不该被卡片点击重复处理一次
                              onClick={(e) => e.stopPropagation()}
                              onChange={() => toggleSelect(room.roomNo)}
                            />
                          ) : null}
                          <Title level={4} style={{ margin: 0, letterSpacing: 1 }}>
                            {room.roomNo}
                          </Title>
                        </div>
                        <Space size={4}>
                          <Tooltip title={st?.label ?? room.status}>
                            <span style={{ color, fontSize: 18 }}>{statusIcon(st?.icon ?? '')}</span>
                          </Tooltip>
                        </Space>
                      </div>

                      <Space size={4} wrap>
                        <Tag color={color} style={{ marginInlineEnd: 0 }}>
                          {st?.label ?? room.status}
                        </Tag>
                        <Tag color={typeColor(room.roomTypeId)} style={{ marginInlineEnd: 0 }}>
                          {room.roomTypeName ?? '未分类'}
                        </Tag>
                      </Space>

                      <Text type="secondary" style={{ fontSize: 12 }}>
                        序号 {room.roomIndex} · 有效卡{' '}
                        <Text style={{ fontSize: 12, color: '#595959' }} strong>
                          {room.activeCards}/{room.maxCards === 0 ? '∞' : (room.maxCards ?? maxDefault)}
                        </Text>
                      </Text>

                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {room.installCardAt ? '已安装' : '未制作安装卡'}
                      </Text>
                    </Space>
                  </Card>
                  </Dropdown>
                )
              })}
            </div>
          )}
          </>
          )}
        </Space>
      </Card>

      <Drawer
        open={active !== null}
        onClose={() => {
          setActiveRoom(null)
          setDrawerCards([])
        }}
        width={560}
        title={
          active ? (
            <Space>
              <span>{active.roomNo} 房</span>
              {activeStatus ? (
                <Tag color={activeStatus.color} icon={statusIcon(activeStatus.icon)}>
                  {activeStatus.label}
                </Tag>
              ) : null}
              <Text type="secondary" style={{ fontSize: 12 }}>
                序号 {active.roomIndex}
              </Text>
            </Space>
          ) : (
            '房间'
          )
        }
      >
        {active ? (
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            {canAdmin ? (
              <div>
                <Text type="secondary">房间名称（房号）</Text>
                <div style={{ display: 'flex', gap: 8, marginTop: 4 }}>
                  <Input
                    value={editRoomNo}
                    onChange={(e) => {
                      setEditRoomNo(e.target.value)
                      setRenameErr(null)
                      setRenameOk(null)
                    }}
                    onPressEnter={() => void doRenameRoom()}
                    status={renameErr ? 'error' : undefined}
                    disabled={renameBusy}
                    style={{ flex: 1 }}
                  />
                  <Button
                    loading={renameBusy}
                    disabled={
                      active.activeCards > 0 ||
                      editRoomNo.trim() === active.roomNo ||
                      editRoomNo.trim() === ''
                    }
                    onClick={() => void doRenameRoom()}
                  >
                    改名
                  </Button>
                </div>
                {renameErr ? (
                  <Text type="danger" style={{ fontSize: 12 }}>
                    {renameErr}
                  </Text>
                ) : renameOk ? (
                  <Text type="success" style={{ fontSize: 12 }}>
                    {renameOk}
                  </Text>
                ) : (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {active.activeCards > 0
                      ? `该房还有 ${active.activeCards} 张有效卡，先退房才能改房号（卡是按房号对账的）`
                      : `序号 ${active.roomIndex} 不变，只改显示名称；不能与其他房间重名。`}
                  </Text>
                )}
              </div>
            ) : null}

            <div>
              <Text type="secondary">最大发卡数（留空 = 跟随全局，0 = 不限）</Text>
              <div style={{ display: 'flex', gap: 8, marginTop: 4 }}>
                <InputNumber
                  min={0}
                  max={99}
                  value={editMax}
                  onChange={(v) => setEditMax(v === null ? null : Number(v))}
                  placeholder="留空 = 跟随全局"
                  disabled={!canAdmin || maxBusy}
                  style={{ flex: 1 }}
                />
                <Button
                  loading={maxBusy}
                  disabled={!canAdmin || (editMax ?? null) === (active.maxCards ?? null)}
                  onClick={() => void saveMax()}
                >
                  保存
                </Button>
              </div>
              <Text type="secondary" style={{ fontSize: 12 }}>
                当前有效卡 {active.activeCards} 张
              </Text>
            </div>

            <div>
              <Text type="secondary">房态</Text>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 6 }}>
                {statuses.map((s) => {
                  const on = s.key === active.status
                  return (
                    <Button
                      key={s.key}
                      size="small"
                      icon={statusIcon(s.icon)}
                      onClick={() => changeStatus(active, s.key)}
                      style={
                        on
                          ? { borderColor: s.color, color: s.color, background: `${s.color}14`, fontWeight: 600 }
                          : { color: '#595959' }
                      }
                    >
                      {s.label}
                    </Button>
                  )
                })}
              </div>
            </div>

            {canAdmin ? (
              <Space wrap align="end">
                <div>
                  <Text type="secondary">房型</Text>
                  <Select
                    style={{ width: 150, display: 'block', marginTop: 4 }}
                    allowClear
                    placeholder="未分类"
                    value={editTypeId ?? undefined}
                    onChange={(v) => setEditTypeId(typeof v === 'number' ? v : null)}
                    options={typeOptions}
                  />
                </div>
                <Button loading={savingEdit} onClick={saveParams}>
                  保存参数
                </Button>
                <Button onClick={() => setInstallOpen(true)}>制作安装卡</Button>
              </Space>
            ) : null}

            {/* 有效客卡（TD-121）：按房实查的纯展示，退房仍走右键菜单流程。
                张数以本列表为准（与 room.activeCards 同口径、独立计算，避免两处不同步时误导） */}
            <div>
              <Text type="secondary">有效客卡（{drawerCards.length} 张）</Text>
              {drawerCards.length === 0 ? (
                <div style={{ marginTop: 4 }}>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    当前没有有效客卡
                  </Text>
                </div>
              ) : (
                <Space
                  direction="vertical"
                  size={4}
                  style={{ marginTop: 4, width: '100%' }}
                >
                  {drawerCards.map((c) => (
                    <Text key={c.id} style={{ fontSize: 12 }}>
                      {c.kind} · {c.days > 0 ? `${c.days} 天` : `${c.hours} 小时`} · 到期{' '}
                      {c.expiresAt ? formatTime(c.expiresAt) : '—'}
                      {c.guestName ? ` · ${c.guestName}` : ''}
                    </Text>
                  ))}
                </Space>
              )}
            </div>

            <Space wrap>
              <Text type="secondary" style={{ fontSize: 12 }}>
                发卡请右键房间卡片选「发行房间门卡」；退房用顶部「读卡退房」或右键菜单。
              </Text>
            </Space>

            {active.installCardAt ? (
              <Text type="secondary" style={{ fontSize: 12 }}>
                上次安装卡：{formatTime(active.installCardAt)}
              </Text>
            ) : null}
          </Space>
        ) : null}
      </Drawer>

      {/* 改房态确认（TD-119）：预定房改房态前先问是否取消今天的预定 */}
      <Modal
        open={statusConfirm !== null}
        title="修改房态确认"
        footer={null}
        onCancel={() => setStatusConfirm(null)}
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Text>{statusConfirm?.text}</Text>
          <Space wrap>
            <Button
              type="primary"
              danger
              onClick={() => {
                const run = statusConfirm?.run
                setStatusConfirm(null)
                if (run) void run()
              }}
            >
              取消预定并改房态
            </Button>
            <Button onClick={() => setStatusConfirm(null)}>返回</Button>
          </Space>
        </Space>
      </Modal>

      {/* 发卡确认（TD-113）：该房覆盖今天有预定时先问「是不是给这位客人发卡」 */}
      <Modal
        open={issueConfirm !== null}
        title={
          issueConfirm ? `该房今天有预定 · ${issueConfirm.room.roomNo}` : '发卡确认'
        }
        footer={null}
        onCancel={() => setIssueConfirm(null)}
      >
        {issueConfirm ? (
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
              <Tag color="blue">
                {issueConfirm.res.checkIn} → {issueConfirm.res.checkOut}
              </Tag>
              {issueConfirm.res.guestName ? (
                <Text strong>{issueConfirm.res.guestName}</Text>
              ) : (
                <Text type="secondary">（预定未填姓名）</Text>
              )}
              {issueConfirm.res.note ? (
                <Text type="secondary">备注：{issueConfirm.res.note}</Text>
              ) : null}
            </div>
            <Text>
              {issueConfirm.res.guestName
                ? '是给这位预定的客人发卡吗？选「是」会把预定姓名带进发卡表单。'
                : '该预定未填姓名，仍可发卡——客人信息在发卡表单里手动填写。'}
            </Text>
            <Space wrap>
              <Button type="primary" onClick={() => confirmIssue(true)}>
                {issueConfirm.res.guestName
                  ? '是，按预定客人发卡'
                  : '是，继续发卡（预定未填姓名）'}
              </Button>
              <Button onClick={() => confirmIssue(false)}>不是，手动填写</Button>
              <Button onClick={() => setIssueConfirm(null)}>取消</Button>
            </Space>
          </Space>
        ) : null}
      </Modal>

      {/* 预定：新建（房间卡片按钮）/ 改期取消（日历色块点开） */}
      <ReservationModal
        key={`reserve-${reserveSession}`}
        open={reserveTarget !== null || reserveEditing !== null}
        room={reserveTarget}
        editing={reserveEditing}
        initialDate={reserveInitialDate}
        onClose={closeReserve}
        onSaved={(msg) => {
          setNotice(msg)
          void reload()
          onRoomsChanged?.()
        }}
      />

      {/* 发行房间门卡 / 续住写卡（只从右键菜单进入） */}
      <Modal
        open={issueRoom !== null}
        title={
          issueRoom
            ? `${
                issueMode === 'renew' ? '续住写卡' : issueMode === 'move' ? '换房写卡' : '发行房间门卡'
              } · ${issueRoom.roomNo}`
            : ''
        }
        footer={null}
        width={640}
        onCancel={() => setIssueRoom(null)}
      >
        {issueRoom ? (
          <IssueForm
            // 换房间 / 换模式时强制重建，否则天数会沿用上一次弹窗的状态
            key={`${issueMode}-${issueRoom.roomNo}`}
            room={issueRoom}
            onIssued={() => {
              void reload()
              // 发卡会推进卡号水位，顶栏必须跟着刷新，否则"发的卡号"和"显示的水位"对不上
              onRoomsChanged?.()
            }}
            defaultDays={
              issueMode === 'renew'
                ? renewDays(issueRoom)
                : issueMode === 'move'
                  ? moveIssueDefaults().amount
                  : 1
            }
            renewFrom={issueMode === 'renew' ? issueRoom.activeUntil : null}
            defaultMode={issueMode === 'move' ? moveIssueDefaults().mode : 'day'}
            initialGuest={
              issueMode === 'move'
                ? guestFromPreview()
                : (issueGuest ?? undefined)
            }
            moveNotice={issueMode === 'move'}
          />
        ) : null}
      </Modal>

      <Modal
        open={installOpen}
        title={active ? `为 ${active.roomNo} 房制作安装卡？` : ''}
        okText="制作"
        cancelText="取消"
        confirmLoading={loading}
        onOk={doInstall}
        onCancel={() => setInstallOpen(false)}
      >
        <p>
          这张卡只包含房间序号 <b>{active?.roomIndex}</b>，不含卡号，也不含房号。
        </p>
        <p>拿到该房间的门锁上刷一次，锁就绑定到这个房间。</p>
        <p style={{ color: '#cf1322' }}>
          如果刷错了房间，需要用清除卡/重新安装来纠正，请先确认卡号与房间。
        </p>
      </Modal>

      {/* 新增房间：二级表单（工具栏只留入口） */}
      <Modal
        open={createOpen}
        title="新增房间"
        okText="新增"
        cancelText="取消"
        confirmLoading={loading}
        onOk={addRoom}
        onCancel={() => setCreateOpen(false)}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <div>
            <Text type="secondary">房号</Text>
            <Input
              placeholder="例如 260"
              value={roomNo}
              onChange={(e) => {
                setRoomNo(e.target.value)
                if (roomNoErr) setRoomNoErr(null)
              }}
              onPressEnter={addRoom}
              style={{ display: 'block', marginTop: 4 }}
              status={roomNoErr ? 'error' : undefined}
              autoFocus
            />
            {roomNoErr ? (
              <Text type="danger" style={{ fontSize: 12 }}>
                {roomNoErr}
              </Text>
            ) : null}
          </div>

          <div>
            <Text type="secondary">锁内序号</Text>
            <InputNumber
              placeholder="留空自动分配（0–255 中最小的空闲值）"
              min={0}
              max={255}
              value={index ?? undefined}
              onChange={(v) => setIndex(typeof v === 'number' ? v : null)}
              style={{ display: 'block', marginTop: 4, width: '100%' }}
            />
          </div>

          <div>
            <Text type="secondary">房型</Text>
            <Select
              allowClear
              placeholder="可留空"
              style={{ display: 'block', marginTop: 4 }}
              value={newTypeId ?? undefined}
              onChange={(v) => setNewTypeId(typeof v === 'number' ? v : null)}
              options={typeOptions}
            />
          </div>

          <Text type="secondary" style={{ fontSize: 12 }}>
            序号是写进卡里的房间标识，同一个序号不能给两个房间用。
            填<Text strong>已删除房间</Text>的序号会直接复用那条记录、只改房名，不会新建。
            新增后需要「制作安装卡」并到该房门锁刷一次，锁才认这个房间。
          </Text>
        </Space>
      </Modal>

      {/* 删除房间：逻辑删除 + 二次确认（受控 Modal；antd 命令式弹窗在 React 19 下失效） */}
      <Modal
        open={deleteTarget !== null}
        title={deleteTarget ? `删除房间 ${deleteTarget.roomNo}？` : ''}
        okText="删除"
        cancelText="取消"
        okButtonProps={{ danger: true, disabled: (deleteTarget?.activeCards ?? 0) > 0 }}
        confirmLoading={deleteBusy}
        onOk={() => void confirmDelete()}
        onCancel={() => setDeleteTarget(null)}
      >
        {deleteTarget ? (
          <Space direction="vertical" size="small" style={{ width: '100%' }}>
            <Text>
              房间会从房间图移除，历史发卡记录保留；
              <Text strong>序号 {deleteTarget.roomIndex} 不会释放</Text>
              （门锁里已经绑定了这个序号）。
            </Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              以后新增房间时填序号 {deleteTarget.roomIndex}，会直接复用这条记录、只改房名，
              等于把这间房重新启用。
            </Text>
            {deleteTarget.activeCards > 0 ? (
              <Alert
                type="warning"
                showIcon
                message={`该房还有 ${deleteTarget.activeCards} 张有效卡`}
                description="先退房（读卡退房 / 无卡退房）再删除，否则这些卡的对账会悬空。"
              />
            ) : null}
          </Space>
        ) : null}
      </Modal>

      {/* 换房：先看源房能带走什么 → 选目标房 → 退旧房 → 转入目标房发卡表单 */}
      <Modal
        open={moveFrom !== null}
        title={moveFrom ? `换房 · ${moveFrom.roomNo}` : ''}
        okText="确认换房并写新卡"
        cancelText="取消"
        width={560}
        confirmLoading={moveBusy}
        okButtonProps={{
          disabled: !moveTo || moveLoading || moveError !== null || !moveRead || moveReading,
        }}
        onOk={() => void confirmMove()}
        onCancel={() => setMoveFrom(null)}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          {moveError ? <Alert type="error" showIcon message="操作失败" description={moveError} /> : null}

          {moveLoading ? (
            <Text type="secondary">正在查询源房的有效卡…</Text>
          ) : (
            <Card type="inner" size="small" title={`源房 ${movePreview?.roomNo ?? moveFrom?.roomNo ?? ''}`}>
              <Space direction="vertical" size={4} style={{ width: '100%' }}>
                <Text>
                  有效客卡：<Text strong>{movePreview?.cards ?? 0} 张</Text>
                  {movePreview?.activeUntil
                    ? `　最晚${formatExpiry(movePreview.activeUntil) ?? `有效期至 ${formatTime(movePreview.activeUntil)}`}`
                    : ''}
                </Text>
                {movePreview?.latest ? (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    客人：
                    {movePreview.latest.guestName ?? '未登记'}
                    {movePreview.latest.guestGender ? ` · ${movePreview.latest.guestGender}` : ''}
                    {movePreview.latest.guestIdType ? ` · ${movePreview.latest.guestIdType}` : ''}
                    {movePreview.latest.guestIdNumber ? ` ${movePreview.latest.guestIdNumber}` : ''}
                    （卡型 {movePreview.latest.kind}
                    {movePreview.latest.days > 0 ? ` · ${movePreview.latest.days} 天` : ''}
                    {movePreview.latest.hours > 0 ? ` · ${movePreview.latest.hours} 小时` : ''}）
                  </Text>
                ) : (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    没有可带入的客人信息（源房没有有效客卡，换房只会做退房登记）。
                  </Text>
                )}
              </Space>
            </Card>
          )}

          {/* 换房前必须读卡核对：读到的卡号必须属于源房当前有效卡 */}
          <Card type="inner" size="small" title="读卡核对">
            <Space direction="vertical" size="small" style={{ width: '100%' }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                把客人手上的卡放到读卡器上，先确认它就是这间房当前在用的卡。
              </Text>
              <Button icon={<ScanOutlined />} loading={moveReading} onClick={() => void readMoveCard()}>
                {moveRead ? '重新读卡' : '读卡'}
              </Button>
              {moveReadErr ? (
                <Alert type="error" showIcon message="读卡失败" description={moveReadErr} />
              ) : null}
              {moveRead ? <CardSnapshotView card={moveRead.card} /> : null}
              {moveRead && moveCardIsOriginal ? (
                <Alert
                  type="success"
                  showIcon
                  message={`核对通过：是 ${moveFrom?.roomNo ?? ''} 房当前有效卡`}
                />
              ) : null}
              {moveRead && !moveCardIsOriginal ? (
                <Alert
                  type="warning"
                  showIcon
                  message="这张卡不是该房当前有效卡"
                  description="可能是别的房间的卡、原厂软件发的卡或空白卡。请先确认客人没拿错卡；拿错卡应先找回原卡，再回来换房。"
                />
              ) : null}
            </Space>
          </Card>

          <div>
            <Text type="secondary">换到</Text>
            <Select
              style={{ width: '100%', marginTop: 4 }}
              placeholder="选择目标房间（只列空净房 / 未满房）"
              value={moveTo ?? undefined}
              onChange={setMoveTo}
              showSearch
              optionFilterProp="label"
              options={rooms
                .filter(
                  (r) =>
                    r.active &&
                    r.roomNo !== moveFrom?.roomNo &&
                    ISSUABLE_STATUS.has(r.status),
                )
                .map((r) => ({
                  value: r.roomNo,
                  label: `${r.roomNo}（序号 ${r.roomIndex} · ${statusOf(r.status)?.label ?? r.status}）`,
                }))}
            />
          </div>

          <Alert
            type="warning"
            showIcon
            message="确认后会发生什么"
            description={
              <ul style={{ margin: '4px 0 0', paddingInlineStart: 20 }}>
                <li>
                  把 {moveFrom?.roomNo ?? '源房'} 的全部有效客卡登记退房，房态改为设置里的「退房后房态」
                </li>
                <li>接着打开目标房的发卡表单：客人信息与剩余时长已带入，写卡仍需读卡器</li>
                <li>目标房房态不会自动变，写完卡请按实际改成「未满房 / 已满房」</li>
                <li>
                  <Text strong>旧卡仍能打开原房间的门</Text>（卡里的房间序号是写死的），到期前请收回
                </li>
                <li>审计日志记一条「换房」，含源房、目标房与关闭张数</li>
              </ul>
            }
          />
        </Space>
      </Modal>

      {/* 非本房原卡：换房前的二次确认（受控 Modal，不用命令式确认框） */}
      <Modal
        open={moveForceOpen}
        title="这张卡不是该房的原卡，仍然换房？"
        okText="仍然换房"
        cancelText="取消"
        okButtonProps={{ danger: true }}
        confirmLoading={moveBusy}
        onOk={() => {
          setMoveForceConfirmed(true)
          setMoveForceOpen(false)
          if (moveFrom && moveTo) {
            const target = rooms.find((r) => r.roomNo === moveTo)
            if (target) void doMove(moveFrom, target)
          }
        }}
        onCancel={() => setMoveForceOpen(false)}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Alert
            type="warning"
            showIcon
            message={`读到的卡不在 ${moveFrom?.roomNo ?? ''} 房的有效卡里`}
            description={
              movePreview?.cards
                ? `${moveFrom?.roomNo} 房当前有 ${movePreview.cards} 张有效卡，请核对客人手中的卡。`
                : `${moveFrom?.roomNo ?? '该房'} 当前没有有效客卡，读到的任何卡都不可能是原卡。`
            }
          />
          <div>
            继续将：
            <ul style={{ margin: '6px 0 0', paddingInlineStart: 20 }}>
              <li>把 {moveFrom?.roomNo ?? '源房'} 的全部有效客卡登记退房（与该卡本身无关）</li>
              <li>房态改为设置里的「退房后房态」，审计标注读到的卡号</li>
              <li>随后打开目标房发卡表单，为客人写一张新房卡</li>
            </ul>
          </div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            请先确认客人是不是拿错了卡（比如拿了旧房卡/别房卡）。拿错卡时应当先找回原卡，否则原房那张卡会一直有效到过期。
          </Text>
        </Space>
      </Modal>

      {/* 读卡退房 / 持卡退房：先读卡核对，再确认退房（每次打开都重新挂载，不带上次的卡） */}
      <CheckoutCardModal
        key={`checkout-${checkoutSession}`}
        open={checkoutOpen}
        target={checkoutTarget}
        rooms={rooms}
        onClose={() => setCheckoutOpen(false)}
        onDone={(message) => {
          setNotice(message)
          void reload()
          onRoomsChanged?.()
        }}
      />

      {/* 无卡退房选卡（TD-121）：只退勾选的卡，0 张时按钮禁用 */}
      <NoCardCheckoutModal
        open={noCheckoutRoom !== null}
        room={noCheckoutRoom}
        onClose={() => setNoCheckoutRoom(null)}
        onDone={(msg) => {
          setNotice(msg)
          void reload()
          onRoomsChanged?.()
        }}
      />
    </>
  )
}
