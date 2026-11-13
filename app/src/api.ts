import { invoke } from '@tauri-apps/api/core'

/** 与 Rust 端 `CardKind` 的 serde 输出一一对应。 */
export type CardKind =
  | 'guestDay'
  | 'guestHour'
  | 'master'
  | 'reset'
  | 'clear'
  | 'install'
  | 'checkoutHour'
  | 'timeSync'
  | 'blank'
  | 'unknown'

export interface CardSnapshot {
  kind: CardKind
  kindLabel: string
  room: string | null
  roomIndex: number
  days: number
  hours: number
  cardNo: number | null
  /** 卡内「年+月」字节（页2[3]）：高 4 位 = (年−2000)&0x0F、低 4 位 = 月；0 = 无 */
  cardYm: number
  /** 挂失卡（页1[2] 低 4 位 = 1；TD-150） */
  isLost: boolean
  /** 发卡日 / 小时 / 小时内序号（从卡号解码，-1 = 无卡号） */
  issueDay: number
  issueHour: number
  issueSeq: number
  rawHex: string
}

export interface IssuanceRecord {
  id: number
  uuid: string
  roomNo: string | null
  kind: string
  days: number
  hours: number
  cardNo: number | null
  rawHex: string | null
  operatorId: number | null
  result: string
  errorCode: number | null
  createdAt: string
  /** 到期时间（UTC ISO8601）。凡带时长的卡型都有，无时长卡型为 null。 */
  expiresAt: string | null
  // 客人身份信息（只存库，不写卡）
  guestName: string | null
  guestGender: string | null
  guestIdType: string | null
  guestIdNumber: string | null
  guestNation: string | null
  guestAddress: string | null
  guestBirthDate: string | null
  guestNote: string | null
  /** 身份证扩展字段（只来自读卡器；头像不入库） */
  idValidFrom: string | null
  idValidTo: string | null
  idAuthority: string | null
  idDeviceModel: string | null
  idDeviceSerial: string | null
}

/** 发卡时录入的客人信息，全部可选。 */
export interface GuestInput {
  name?: string
  gender?: string
  idType?: string
  idNumber?: string
  nation?: string
  address?: string
  birthDate?: string
  note?: string
  /** 身份证扩展字段（只来自读卡器，头像不入库） */
  idValidFrom?: string
  idValidTo?: string
  idAuthority?: string
  deviceModel?: string
  deviceSerial?: string
}

/**
 * 读到的身份证信息（`read_id_card` 的返回）。
 *
 * 字段与 Rust 端 `IdCardDto` 一一对应；解析厂商协议、连 9001 服务都在 Rust 侧，
 * 前端拿到的是已经归一好的字段，不需要知道读卡器型号或报文格式。
 */
export interface IdCardInfo {
  name: string | null
  gender: string | null
  nation: string | null
  /** 出生日期，`YYYY-MM-DD` */
  birthDate: string | null
  address: string | null
  idNumber: string | null
  /** 证件有效期起 / 止，厂商原样格式（多为 `YYYYMMDD`） */
  idValidFrom: string | null
  idValidTo: string | null
  idAuthority: string | null
  deviceModel: string | null
  deviceSerial: string | null
}

/** 入住登记单（TD-220）：一间房同一时刻最多一张 open。 */
export interface Checkin {
  id: number
  roomNo: string
  checkInAt: string
  checkOutAt: string | null
  status: string
  operatorId: number | null
  note: string | null
  createdAt: string
}

/** 同住的客人（`seq` 决定「首个登记人」）。 */
export interface CheckinGuest {
  id: number
  checkinId: number
  seq: number
  name: string | null
  gender: string | null
  idType: string | null
  idNumber: string | null
  nation: string | null
  address: string | null
  birthDate: string | null
  note: string | null
  idValidFrom: string | null
  idValidTo: string | null
  idAuthority: string | null
  deviceModel: string | null
  deviceSerial: string | null
}

/** 登记单关联的一张卡（登记 → 卡的反向可见）。 */
export interface CheckinCard {
  id: number
  /** 16 位卡号位域（`DDHHII`），只做内部核对，界面不显示十六进制 */
  cardNo: number | null
  /** 10 位作业流水号 `YYMMDDHHII`，界面用这个；不占号的卡型为 null */
  workNo: string | null
  kind: string
  rawHex: string | null
  expiresAt: string | null
  result: string
  checkedOutAt: string | null
  invalidatedAt: string | null
}

/** 追加住客的结果：`reservationConverted` 非空表示顺手把预定转成了入住。 */
export interface AddGuestResult {
  guestId: number
  reservationConverted: number | null
}

export interface CheckinDetail {
  /** 该房号当前的锁内序号；房间记录缺失时为 null */
  roomIndex: number | null
  checkin: Checkin
  guests: CheckinGuest[]
  /** 这张登记单关联的发卡记录；退房关单的判据就是这批卡是否都已失效 */
  cards: CheckinCard[]
}

/** 列表行：首个登记人 + 人数。 */
export interface CheckinRow {
  id: number
  roomNo: string
  /** 该房号当前的锁内序号；房间记录缺失时为 null（同房间多次入住会重复，分组看单号） */
  roomIndex: number | null
  checkInAt: string
  checkOutAt: string | null
  status: string
  guestCount: number
  firstGuestName: string | null
  note: string | null
}

/**
 * 证件类型：按拼音 A–Z 排序。
 * 需求里"社会保障卡""军官证"重复给了两次，这里去重。
 */
export const ID_TYPES = [
  '港澳通行证',
  '国内护照',
  '户口本',
  '驾驶证',
  '警官证',
  '军官证',
  '临时身份证',
  '其他',
  '社会保障卡',
  '身份证',
  '士兵证',
  '中华人民共和国旅行证',
] as const

export const GENDERS = ['男', '女'] as const

/** 读卡页 B 区：记录查询的四种结果。数据库报错绝不能落到 notFound。 */
export type RecordLookup =
  | {
      status: 'found'
      latest: IssuanceRecord
      count: number
      /** raw = 按卡内数据匹配；timestamp = 按卡号时间戳匹配 */
      matchedBy: 'raw' | 'timestamp'
    }
  | { status: 'notFound' }
  | { status: 'notApplicable'; reason: string }
  | { status: 'failed'; message: string }

export interface CardReadDto {
  card: CardSnapshot
  lookup: RecordLookup
}

export interface Room {
  roomNo: string
  roomIndex: number
  building: string | null
  floor: number | null
  roomTypeId: number | null
  roomTypeName: string | null
  /** 该房间最多同时有效的客卡数；null = 不限 */
  maxCards: number | null
  /** 房态 key */
  status: string
  active: boolean
  installCardAt: string | null
  createdAt: string
  /** 当前仍有效的客卡张数（按卡号去重、已退房不算） */
  activeCards: number
  /** 当前 open 登记单的住客数；没有 open 单为 0（登记 ≠ 发卡，人数 ≠ 卡数） */
  guestCount: number
  /** 当前有效卡里最晚的到期时间（ISO UTC）；续住写卡用它算默认天数 */
  activeUntil: string | null
}

/**
 * 新增房间的结果。
 *
 * `revivedFrom` 非空表示这次没有新建行，而是把一条已逻辑删除的房间记录
 * 改名后重新启用（序号复用），值是那条记录的旧房号。
 */
export interface RoomUpsert {
  room: Room
  revivedFrom: string | null
}

/** 房态定义（颜色/图标可自定义）。 */
export interface RoomStatus {
  key: string
  label: string
  icon: string
  color: string
  sortOrder: number
}

export interface CheckoutResult {
  roomNo: string
  cardsClosed: number
  status: string
  /** 非本系统卡退房时是否真的把卡擦成空白卡（TD-212） */
  erased: boolean
  /** 擦卡失败原因（要求擦卡但没擦成时才有值） */
  eraseError: string | null
}

/** 保存退房钟点的结果（TD-126 补充）：同时返回按新钟点重算的在用日租卡张数 */
export interface CheckoutHourResult {
  hour: number
  recalculated: number
}

/** 凌晨房归属时间（TD-225）：改完会重算在用日租卡 */
export interface MorningCutoffResult {
  /** 规范化后的 `HH:mm` */
  cutoff: string
  recalculated: number
}

/** 预定记录。日期是本地日期（YYYY-MM-DD），退房日不含（住 1 晚 = 今天→明天）。 */
export interface Reservation {
  id: number
  roomNo: string
  checkIn: string
  checkInTime: string
  checkOut: string
  checkOutTime: string
  guestName: string | null
  note: string | null
  /** booked = 有效；checked_in = 已入住；cancelled = 已取消（保留记录） */
  status: string
  operatorName: string | null
  createdAt: string
  updatedAt: string
  cancelledAt: string | null
}

/** 记录页预定查询条件（TD-167）：包含已取消 / 已入住；日期为入住日 YYYY-MM-DD */
/** 日历入住时段（含已退房历史，TD-215/日历总览）。 */
export interface GuestStay {
  id: number
  roomNo: string
  guestName: string | null
  kind: string
  days: number
  hours: number
  start: string
  end: string
  plannedEnd: string
  /** 入住状态：active / checked_out / invalidated / expired（过期未退房不再算在住） */
  status: string
  /** 关联的入住登记单（TD-221）；详情可展开全部住客 */
  checkinId: number | null
  /** 登记单住客人数（没有登记单为 0） */
  guestCount: number
}

export interface ReservationFilter {
  roomNo?: string | null
  guestName?: string | null
  /** booked / checked_in / cancelled；空 = 全部 */
  status?: string | null
  from?: string | null
  to?: string | null
  page?: number
  pageSize?: number
}

/**
 * 换房预览：源房能带走什么。
 *
 * `latest` 是源房最近一张仍有效的客卡记录 —— 换房时把客人信息和时长带到新房，
 * 前台不用重填。没有有效卡时全是空值。
 */
export interface RoomMovePreview {
  roomNo: string
  cards: number
  activeUntil: string | null
  latest: IssuanceRecord | null
}

/** 发卡记录行（记录页用，比读卡用的 IssuanceRecord 宽）。 */
export interface IssuanceRow {
  id: number
  createdAt: string
  roomNo: string | null
  kind: string
  days: number
  hours: number
  guestName: string | null
  guestGender: string | null
  guestIdType: string | null
  guestIdNumber: string | null
  guestNation: string | null
  guestAddress: string | null
  guestBirthDate: string | null
  guestNote: string | null
  operatorName: string | null
  result: string
  /** 统一状态（TD-229）：active / expired / checked_out / voided / failed，由后端算好 */
  status: string
  expiresAt: string | null
  checkedOutAt: string | null
  durationMs: number | null
  rawHex: string | null
  /** 身份证扩展字段（只来自读卡器；头像不入库） */
  idValidFrom: string | null
  idValidTo: string | null
  idAuthority: string | null
  idDeviceModel: string | null
  idDeviceSerial: string | null
}

/** 操作审计行；`detail` 是后端拼的文本（列名叫 detail_json 但实际不是 JSON）。 */
export interface AuditRow {
  id: number
  at: string
  action: string
  target: string | null
  detail: string | null
  result: string
  operatorName: string | null
}

export interface Page<T> {
  rows: T[]
  total: number
}

export interface CsvResult {
  path: string
  rows: number
  bytes: number
}

/** 记录页的筛选参数（时间为 ISO UTC 字符串） */
export interface LogFilter {
  roomNo?: string | null
  kind?: string | null
  from?: string | null
  to?: string | null
  page?: number
  pageSize?: number
  /** 入住记录视图（TD-118）：true 只返回填了身份信息的发卡 */
  guestOnly?: boolean
}

/** 备份结果：备份文件落在哪、多大。 */
export interface BackupResult {
  path: string
  bytes: number
}

/** 备份列表项。`isSafetyCopy` = 恢复前自动留的 .bak，不是正式备份。 */
export interface BackupEntry {
  path: string
  name: string
  bytes: number
  modifiedAt: string | null
  isSafetyCopy: boolean
  /** 自动备份（auto- 前缀）：保留份数只对这类生效 */
  isAuto: boolean
}

/** 自动备份设置。`intervalMinutes = 0` 表示关闭（默认 60）。 */
export interface BackupSettings {
  intervalMinutes: number
  keep: number
  lastAt: string | null
  dir: string
}

/** 首启向导状态：`done=false` 才需要弹。 */
export interface OnboardingStatus {
  done: boolean
  rooms: number
}

/** 房间导入结果 */
export interface ImportReport {
  added: number
  revived: number
  skipped: number
  failed: Array<{ line: number; reason: string }>
}

/** 诊断包导出结果：zip 路径、大小、以及"压缩失败"等告警。 */
export interface DiagnosticsResult {
  path: string
  bytes: number
  warnings: string[]
}

/** 恢复结果：恢复前自动留的副本 + 恢复后库里的数据量。 */
export interface RestoreResult {
  before: string
  schemaVersion: number
  rooms: number
  issuances: number
  reservations: number
  operators: number
  /** 当前已有、备份也有：保留当前密码 / 角色 / 启用状态 */
  operatorsPreserved: number
  /** 备份有、当前没有：导入但停用 */
  operatorsImportedDisabled: number
  /** 当前有、备份没有：补回当前账号 */
  operatorsReinserted: number
  /** 账号合并单条失败数 */
  operatorFailures: number
}

export interface CheckoutCardResult {
  roomNo: string
  /** 该房剩余有效卡；0 表示已转空脏房 */
  remaining: number
}

export interface Department {
  id: number
  name: string
  note: string | null
  active: boolean
  createdAt: string
  memberCount: number
}

export interface RoomType {
  id: number
  name: string
  sortOrder: number
  createdAt: string
  /** 标签颜色（#RRGGBB）；null = 中性灰 */
  color: string | null
  roomCount: number
}

export interface OperatorInfo {
  id: number
  login: string
  displayName: string
  role: Role
  departmentId: number | null
  departmentName: string | null
  active: boolean
  createdAt: string
}

export interface ReminderSettings {
  enabled: boolean
  minutes: number
  sound: boolean
  /** 语音播报（系统 TTS，音色随系统） */
  voice: boolean
}

/** 即将到期 / 刚过期的客卡。 */
export interface ExpiringCard {
  issuanceId: number
  roomNo: string | null
  roomIndex: number | null
  cardNo: number | null
  kind: string
  expiresAt: string
  days: number
  hours: number
}

export type Role = 'admin' | 'frontdesk'

export interface AuthStatus {
  initialized: boolean
  loggedIn: boolean
  login: string | null
  displayName: string | null
  role: Role | null
  minPasswordLen: number
}

/** 管理卡、房间、设备设置需要管理员；读卡与发客卡前台即可。 */
export function isAdmin(auth: AuthStatus | null): boolean {
  return auth?.role === 'admin'
}

export interface AppInfo {
  version: string
  dbPath: string
  runtimeDir: string
  driverSha256: string | null
  simulation: boolean
  /** 串口索引，0 = COM1 */
  port: number
  /** SDK 来源：env / resource / dev */
  sdkSource: string | null
  /** 调试构建（tauri dev / cargo run）为 true */
  dev: boolean
  /** 发布通道：dev = 调试构建或 DEV 安装包；release = 正式版 */
  channel: 'dev' | 'release'
}

export interface SelfTestDto {
  passed: number
  failed: number
  ok: boolean
}

export interface PortProbe {
  port: number
  com: string
  available: boolean
  readerId: string | null
}

/** 管理卡请求，与 Rust 端 `AdminCardRequest` 的 serde 表示一一对应。 */
export type AdminCardRequest =
  | { kind: 'master'; days?: number | null; hours?: number | null }
  | { kind: 'reset' }
  | { kind: 'clear' }
  | { kind: 'install'; roomNo: string }
  | { kind: 'checkout'; hour: number }
  | { kind: 'timeSync'; hour?: number | null; minute?: number | null; at?: string | null }
  | { kind: 'blank' }
  | { kind: 'raw'; p1: string; p2: string }

export interface IssuedCard {
  card: CardSnapshot
  room: Room | null
  /** 到期时间（UTC ISO8601 估算值）；无时长的卡型为 null。 */
  expiresAt: string | null
  /** 挂失卡发卡时作废的同房旧客卡张数（TD-218）；普通发卡为 0。 */
  invalidated: number
}

/** Rust 端 `CommandError` 的结构；invoke 抛出来就是它。 */
export interface CommandError {
  code: number
  message: string
  hint: string
}

export function isCommandError(e: unknown): e is CommandError {
  return typeof e === 'object' && e !== null && 'message' in e && 'hint' in e
}

export function describeError(e: unknown): string {
  if (isCommandError(e)) return `${e.message}（${e.hint}）`
  if (e instanceof Error) return e.message
  return String(e)
}

export const api = {
  authStatus: () => invoke<AuthStatus>('auth_status'),
  initializeAdmin: (displayName: string, password: string, confirmPassword: string) =>
    invoke<AuthStatus>('initialize_admin', { displayName, password, confirmPassword }),
  login: (login: string, password: string) => invoke<AuthStatus>('login', { login, password }),
  logout: () => invoke<AuthStatus>('logout'),
  appInfo: () => invoke<AppInfo>('app_info'),
  readCard: () => invoke<CardReadDto>('read_card'),
  /** 读身份证（9001 服务）。失败只影响自动填表，手填照样能登记。 */
  readIdCard: () => invoke<IdCardInfo>('read_id_card'),
  listRooms: () => invoke<Room[]>('list_rooms'),
  createRoom: (roomNo: string, roomIndex?: number, roomTypeId?: number) =>
    invoke<RoomUpsert>('create_room', { roomNo, roomIndex: roomIndex ?? null, roomTypeId: roomTypeId ?? null }),
  /** 改房号（显示名称），序号不变；房号不能与其它房间重复 */
  renameRoom: (roomNo: string, newRoomNo: string) =>
    invoke<Room>('rename_room', { roomNo, newRoomNo }),
  /** 逻辑删除房间：只置 active=0，序号不释放，历史发卡记录保留 */
  deleteRoom: (roomNo: string) => invoke<void>('delete_room', { roomNo }),
  setRoomType: (roomNo: string, roomTypeId: number | null) =>
    invoke<void>('set_room_type', { roomNo, roomTypeId }),

  listRoomStatuses: () => invoke<RoomStatus[]>('list_room_statuses'),
  /** 改单个房间的最大发卡数（TD-161）：`null` = 跟随全局默认；`0` = 不限；`n` = 具体张数 */
  setRoomMaxCards: (roomNo: string, maxCards: number | null) =>
    invoke<void>('set_room_max_cards', { roomNo, maxCards }),
  /** 全局默认最大发卡数（`0` = 不限；房间留空即跟随此值） */
  getMaxCardsDefault: () => invoke<number>('get_max_cards_default'),
  setMaxCardsDefault: (value: number) => invoke<number>('set_max_cards_default', { value }),
  setRoomStatus: (roomNo: string, status: string) =>
    invoke<void>('set_room_status', { roomNo, status }),
  updateRoomStatusStyle: (key: string, icon: string, color: string) =>
    invoke<void>('update_room_status_style', { key, icon, color }),
  checkoutRoom: (roomNo: string, clearCheckin = true) =>
    invoke<CheckoutResult>('checkout_room', { roomNo, clearCheckin }),
  /** 列出与 [from, to) 相交的有效预定（YYYY-MM-DD） */
  listReservations: (from: string, to: string, roomNo?: string | null) =>
    invoke<Reservation[]>('list_reservations', { from, to, roomNo: roomNo ?? null }),
  /** 记录页预定查询（TD-167）：含已取消 / 已入住，支持筛选与分页 */
  listReservationRecords: (f: ReservationFilter) =>
    invoke<Page<Reservation>>('list_reservation_records', {
      roomNo: f.roomNo ?? null,
      guestName: f.guestName ?? null,
      status: f.status ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
      page: f.page ?? 1,
      pageSize: f.pageSize ?? 20,
    }),
  /** 新建预定；checkOut 是退房日（不含）。重叠 / 日期非法由后端拒绝 */
  createReservation: (input: {
    roomNo: string
    checkIn: string
    checkInTime?: string
    checkOut: string
    checkOutTime?: string
    guestName?: string | null
    note?: string | null
  }) =>
    invoke<Reservation>('create_reservation', {
      roomNo: input.roomNo,
      checkIn: input.checkIn,
      checkInTime: input.checkInTime ?? '12:00',
      checkOut: input.checkOut,
      checkOutTime: input.checkOutTime ?? '12:00',
      guestName: input.guestName ?? null,
      note: input.note ?? null,
    }),
  /** 改期 / 改预定信息 */
  updateReservation: (
    id: number,
    input: {
      roomNo: string
      checkIn: string
      checkInTime?: string
      checkOut: string
      checkOutTime?: string
      guestName?: string | null
      note?: string | null
    },
  ) =>
    invoke<Reservation>('update_reservation', {
      id,
      roomNo: input.roomNo,
      checkIn: input.checkIn,
      checkInTime: input.checkInTime ?? '12:00',
      checkOut: input.checkOut,
      checkOutTime: input.checkOutTime ?? '12:00',
      guestName: input.guestName ?? null,
      note: input.note ?? null,
    }),
  /** 取消预定（保留记录，重复取消幂等） */
  cancelReservation: (id: number) => invoke<Reservation>('cancel_reservation', { id }),
  /** 缩小到系统托盘（关窗确认框的「缩小」）；点托盘图标或再启动程序恢复 */
  hideToTray: () => invoke<void>('hide_to_tray'),
  /** 真正退出程序（确认框的「退出」） */
  quitApp: () => invoke<void>('quit_app'),
  /** 换房第一步：查源房的有效卡 / 客人 / 到期时间（只读，不改库） */
  roomMovePreview: (roomNo: string) =>
    invoke<RoomMovePreview>('room_move_preview', { roomNo }),
  /**
   * 换房第二步：把源房全部有效客卡登记退房（+ 房态转退房态 + 审计）。
   * 卡里的房间序号是写死的，旧卡仍能开源房的门；新房必须重新写一张卡。
   */
  moveRoomOut: (roomNo: string, toRoomNo: string) =>
    invoke<CheckoutResult>('room_move_out', { roomNo, toRoomNo }),

  /** 立即备份数据库（管理员）；`dir` 留空则写到库同级的 backups\ 目录 */
  backupDatabase: (dir?: string | null) =>
    invoke<BackupResult>('backup_database', { dir: dir ?? null }),
  /** 列出已有备份（管理员） */
  listBackups: () => invoke<BackupEntry[]>('list_backups'),
  /** 是否需要跑首启向导 */
  getOnboardingStatus: () => invoke<OnboardingStatus>('get_onboarding_status'),
  /** 标记向导已完成（"跳过"也调它，免得每次开机都弹） */
  completeOnboarding: () => invoke<void>('complete_onboarding'),

  /** 导入初始房间（管理员）：传字节，后端按后缀识别 CSV / ini */
  importRooms: (fileName: string, bytes: number[]) =>
    invoke<ImportReport>('import_rooms', { fileName, bytes }),
  /** 导出诊断包（管理员）：脱敏库副本 + 清单 + 日志，打包成 zip */
  exportDiagnostics: () => invoke<DiagnosticsResult>('export_diagnostics'),
  /** 用备份覆盖当前库（管理员）：先只读校验，再把当前库另存为 .before-restore-*.bak */
  restoreDatabase: (path: string) => invoke<RestoreResult>('restore_database', { path }),
  /** 发卡记录（登录即可，前台也要查） */
  /** 日历入住时段：含已退房历史，按本地日期区间相交查询 */
  listGuestStays: (from: string, to: string) =>
    invoke<GuestStay[]>('list_guest_stays', { from, to }),
  listIssuances: (f: LogFilter) =>
    invoke<Page<IssuanceRow>>('list_issuances', {
      roomNo: f.roomNo ?? null,
      kind: f.kind ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
      page: f.page ?? 1,
      pageSize: f.pageSize ?? 50,
      guestOnly: f.guestOnly ?? false,
    }),
  /** 操作审计（登录即可） */
  listAudits: (f: { action?: string | null; from?: string | null; to?: string | null; page?: number; pageSize?: number }) =>
    invoke<Page<AuditRow>>('list_audits', {
      action: f.action ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
      page: f.page ?? 1,
      pageSize: f.pageSize ?? 50,
    }),
  /** 同一卡号的全部写入记录 */
  /** 导出当前筛选为 CSV（管理员）：后端落盘并返回路径 */
  /** 导出 CSV；`guestOnly=true` 时只导出填了身份信息的发卡（入住记录，TD-165） */
  /** 导出在用房间的「序号 ↔ 房号」对照表（`room_index.json`，管理员） */
  exportRoomIndexJson: () => invoke<CsvResult>('export_room_index_json'),
  exportIssuancesCsv: (f: LogFilter, guestOnly = false) =>
    invoke<CsvResult>('export_issuances_csv', {
      roomNo: f.roomNo ?? null,
      kind: f.kind ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
      guestOnly,
    }),
  /** 导出预定记录 CSV（管理员）：按当前筛选导出全量，不按分页（TD-205） */
  exportReservationsCsv: (f: {
    roomNo?: string | null
    guestName?: string | null
    status?: string | null
    from?: string | null
    to?: string | null
  }) =>
    invoke<CsvResult>('export_reservations_csv', {
      roomNo: f.roomNo ?? null,
      guestName: f.guestName ?? null,
      status: f.status ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
    }),

  /** 入住登记（TD-220）：建单；同房已有 open 单会直接复用 */
  createCheckin: (roomNo: string, note?: string | null) =>
    invoke<Checkin>('create_checkin', { roomNo, note: note ?? null }),
  /** 取该房当前 open 的登记单（没有返回 null） */
  getOpenCheckin: (roomNo: string) => invoke<Checkin | null>('get_open_checkin', { roomNo }),
  /** 登记单列表：房号 / 状态 / 入住日区间 + 分页 */
  listCheckins: (f: {
    roomNo?: string | null
    status?: string | null
    from?: string | null
    to?: string | null
    page?: number
    pageSize?: number
  }) =>
    invoke<Page<CheckinRow>>('list_checkins', {
      roomNo: f.roomNo ?? null,
      status: f.status ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
      page: f.page ?? 1,
      pageSize: f.pageSize ?? 20,
    }),
  /** 登记单详情（含全部住客，按 seq 排序） */
  getCheckin: (id: number) => invoke<CheckinDetail | null>('get_checkin', { id }),
  /** 追加住客（seq 自动递增） */
  addCheckinGuest: (checkinId: number, guest: GuestInput) =>
    invoke<AddGuestResult>('add_checkin_guest', { checkinId, guest }),
  updateCheckinGuest: (id: number, guest: GuestInput) =>
    invoke<void>('update_checkin_guest', { id, guest }),
  deleteCheckinGuest: (id: number) => invoke<void>('delete_checkin_guest', { id }),
  /** 关闭登记单（退房会自动关，这里给手动关用） */
  closeCheckin: (id: number) => invoke<void>('close_checkin', { id }),
  /** 导出登记单 CSV（管理员）：一张单 × 一位住客一行 */
  exportCheckinsCsv: (f: {
    roomNo?: string | null
    status?: string | null
    from?: string | null
    to?: string | null
  }) =>
    invoke<CsvResult>('export_checkins_csv', {
      roomNo: f.roomNo ?? null,
      status: f.status ?? null,
      from: f.from ?? null,
      to: f.to ?? null,
    }),

  /** 自动备份设置（管理员） */
  getBackupSettings: () => invoke<BackupSettings>('get_backup_settings'),
  setBackupSettings: (intervalMinutes: number, keep: number) =>
    invoke<BackupSettings>('set_backup_settings', { intervalMinutes, keep }),
  /** 导入外部备份文件：前端读字节传过来，后端落盘并校验是不是我们的库 */
  importBackupFile: (name: string, bytes: number[]) =>
    invoke<BackupEntry>('import_backup_file', { name, bytes }),
  /** 读卡退房 / 持卡退房：按卡号关闭这一张卡 */
  /**
   * 退房：持卡退房传 `cardNo`（后端按时间戳匹配记录）；无卡退房传 `issuanceId`。
   */
  checkoutCard: (input: {
    cardNo?: number | null
    issuanceId?: number | null
    noDevice?: boolean
    /** 退房时顺带关闭该房入住登记单（只关单，住客档案保留） */
    clearCheckin?: boolean
  }) =>
    invoke<CheckoutCardResult>('checkout_card', {
      cardNo: input.cardNo ?? null,
      issuanceId: input.issuanceId ?? null,
      noDevice: input.noDevice ?? false,
      clearCheckin: input.clearCheckin ?? false,
    }),
  /**
   * 强制退房：卡不是本系统发的，本地没有发卡记录，按前端从卡片里读出的房号
   * 直接写入退房数据（关闭该房全部有效卡 + 改房态）。调用前必须让用户二次确认。
   */
  forceCheckoutCard: (roomNo: string, erase = true, clearCheckin = true) =>
    invoke<CheckoutResult>('force_checkout_card', { roomNo, erase, clearCheckin }),
  /** 退房后要转成的房态 key（设置项） */
  getCheckoutStatus: () => invoke<string>('get_checkout_status'),
  setCheckoutStatus: (status: string) => invoke<void>('set_checkout_status', { status }),
  /** 把前端日志打到后端终端，排查时直接读日志，不靠截图 */
  logFrontend: (level: 'error' | 'warn' | 'info', message: string) =>
    invoke<void>('log_frontend', { level, message }),

  listDepartments: () => invoke<Department[]>('list_departments'),
  createDepartment: (name: string, note?: string) =>
    invoke<Department>('create_department', { name, note: note ?? null }),
  renameDepartment: (id: number, name: string) => invoke<void>('rename_department', { id, name }),
  setDepartmentActive: (id: number, active: boolean) =>
    invoke<void>('set_department_active', { id, active }),

  listRoomTypes: () => invoke<RoomType[]>('list_room_types'),
  createRoomType: (name: string, sortOrder?: number) =>
    invoke<RoomType>('create_room_type', { name, sortOrder: sortOrder ?? null }),
  renameRoomType: (id: number, name: string) => invoke<void>('rename_room_type', { id, name }),
  deleteRoomType: (id: number) => invoke<void>('delete_room_type', { id }),
  /** 房型标签颜色（null = 中性灰） */
  setRoomTypeColor: (id: number, color: string | null) =>
    invoke<void>('set_room_type_color', { id, color }),

  listOperators: () => invoke<OperatorInfo[]>('list_operators'),
  createOperator: (
    login: string,
    displayName: string,
    role: Role,
    departmentId: number | null,
    password: string,
  ) => invoke<void>('create_operator', { login, displayName, role, departmentId, password }),
  resetOperatorPassword: (operatorId: number, newPassword: string) =>
    invoke<void>('reset_operator_password', { operatorId, newPassword }),
  setOperatorActive: (operatorId: number, active: boolean) =>
    invoke<void>('set_operator_active', { operatorId, active }),
  changeOwnPassword: (oldPassword: string, newPassword: string) =>
    invoke<void>('change_own_password', { oldPassword, newPassword }),

  getReminderSettings: () => invoke<ReminderSettings>('get_reminder_settings'),
  /** 退房钟点（TD-126）：读 = 登录即可；写 = 管理员（0–23，返回保存值） */
  getCheckoutHour: () => invoke<number>('get_checkout_hour'),
  setCheckoutHour: (hour: number) =>
    invoke<CheckoutHourResult>('set_checkout_hour', { hour }),
  /** 凌晨房归属时间（TD-225）：`HH:mm`，默认 06:00 */
  getMorningCutoff: () => invoke<string>('get_morning_cutoff'),
  setMorningCutoff: (cutoff: string) =>
    invoke<MorningCutoffResult>('set_morning_cutoff', { cutoff }),
  setReminderSettings: (enabled: boolean, minutes: number, sound: boolean, voice?: boolean) =>
    invoke<ReminderSettings>('set_reminder_settings', {
      enabled,
      minutes,
      sound,
      // 不传 = null，后端按"保持库里现值"处理
      voice: voice ?? null,
    }),
  expiringCards: (withinMinutes: number) =>
    invoke<ExpiringCard[]>('expiring_cards', { withinMinutes }),
  issueInstallCard: (roomNo: string) => invoke<IssuedCard>('issue_install_card', { roomNo }),
  issueGuest: (
    roomNo: string,
    duration: { days: number } | { hours: number },
    idempotencyKey: string,
    /** 挂失卡（TD-150）：页1[2] 低 4 位 = 1，让该房间在用卡失效 */
    lost?: boolean,
  ) =>
    invoke<IssuedCard>('issue_guest', {
      roomNo,
      days: 'days' in duration ? duration.days : null,
      hours: 'hours' in duration ? duration.hours : null,
      idempotencyKey,
      lost: lost ?? false,
    }),
  issueAdminCard: (request: AdminCardRequest, confirm: boolean) =>
    invoke<IssuedCard>('issue_admin_card', {
      request,
      confirm,
    }),
  setPort: (port: number) => invoke<AppInfo>('set_port', { port }),
  runSelfTest: () => invoke<SelfTestDto>('run_selftest'),
  probePorts: () => invoke<PortProbe[]>('probe_ports'),
}

// ---------------------------------------------------------------- 时长档位
//
// 与后端 `rules.rs` 必须一致：
//   简单段 0x80|天数 —— 客卡 1–62、总控 1–63
//   小时段 0x40|小时 —— 客卡 1–62、总控 1–63
//   超过简单段只能走固定档位（月码 0xC0|月数）

export const GUEST_MAX_DAYS = 62
export const MASTER_MAX_DAYS = 63
export const GUEST_MAX_HOURS = 62
export const MASTER_MAX_HOURS = 63

export const LONG_DAY_TIERS = [90, 120, 150, 180, 210, 240, 300, 360, 390, 420, 480, 540, 600, 720, 900, 1095]

export interface SelectGroup {
  label: string
  options: { value: number; label: string }[]
}

/** 天数选项：简单段常用值 + 超过上限后的固定档位。 */
export function dayOptions(maxSimple: number): SelectGroup[] {
  const simple = Array.from(new Set([1, 2, 3, 5, 7, 14, 30, maxSimple])).sort((a, b) => a - b)
  return [
    { label: '常用', options: simple.map((v) => ({ value: v, label: `${v} 天` })) },
    {
      label: `长期（月码，最多 ${maxSimple} 天以外只能选这些）`,
      options: LONG_DAY_TIERS.map((v) => ({ value: v, label: `${v} 天` })),
    },
  ]
}

/** 小时选项：客卡到 62、总控到 63。 */
export function hourOptions(maxHours: number): SelectGroup[] {
  const simple = Array.from(new Set([1, 2, 3, 4, 6, 8, 12, 24, 48, maxHours])).sort((a, b) => a - b)
  return [{ label: '小时', options: simple.map((v) => ({ value: v, label: `${v} 小时` })) }]
}

/** 卡号显示成 0xXXXX；无卡号返回 '—'。 */
export function formatCardNo(cardNo: number | null): string {
  if (cardNo === null || cardNo === undefined) return '—'
  return `0x${cardNo.toString(16).toUpperCase().padStart(4, '0')}`
}

/**
 * 卡号 → `YYMMDDHHnn`（年/月/日/时/序号各两位），例如 `2711081601`。
 *
 * 数据全部来自卡内：`页2[3]` 的年月 + 卡号里的日/时 + 小时内序号（0 起算，显示时 +1）。
 * 数据不完整（无卡号、年月非法、无发卡时刻）时回退到十六进制显示，不猜。
 */
export function formatCardNoText(card: CardSnapshot): string {
  const hi = (card.cardYm >> 4) & 0x0f
  const month = card.cardYm & 0x0f
  if (month < 1 || month > 12 || card.issueDay < 1 || card.issueHour < 0) {
    return formatCardNo(card.cardNo)
  }
  const nowYear = new Date().getFullYear()
  const base = 2000 + 16 * Math.floor((nowYear - 2000) / 16)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad((base + hi) % 100)}${pad(month)}${pad(card.issueDay)}${pad(card.issueHour)}${pad(card.issueSeq + 1)}`
}

/**
 * 数据库里存的是 UTC（形如 `2026-11-05T09:57:27Z`），前台要看本地时间。
 * 解析失败就原样返回，绝不显示 `Invalid Date`。
 */
export function formatTime(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString('zh-CN', { hour12: false })
}

/** 本地时间 `YYYY-MM-DD HH:mm`（分钟精度）。日期时间相关展示统一用它。 */
export function formatMinute(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/**
 * 到期时间统一文案：`有效期至 2026-11-06 12:00（按发卡时间估算）`。
 *
 * 全系统只有这一个地方拼这句话 —— 读卡页、退房弹窗、发卡结果、顶栏提醒都用它，
 * 避免各处"有的显示发卡时间、有的显示到期时间、有的只有时长"。
 *
 * 为什么要标"估算"：锁按**卡内发卡时间**计算到期（日租 = 发卡日 + N 天的退房钟点、
 * 钟点 = 发卡时刻 + N 小时），前提是锁内时间已用校时卡校准、退房钟点已用退房钟点卡设置；
 * 这里显示的到期时刻是按**发卡时间 + 时长**推算的。无到期时间返回 null，调用方不渲染。
 */
export function formatExpiry(expiresAt: string | null | undefined): string | null {
  if (!expiresAt) return null
  const d = new Date(expiresAt)
  if (Number.isNaN(d.getTime())) return `有效期至 ${expiresAt}`
  return `有效期至 ${formatMinute(expiresAt)}（按发卡时间估算）`
}
