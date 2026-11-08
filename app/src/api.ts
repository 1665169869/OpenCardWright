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
  | { status: 'found'; latest: IssuanceRecord; count: number }
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
}

/** 预定记录。日期是本地日期（YYYY-MM-DD），退房日不含（住 1 晚 = 今天→明天）。 */
export interface Reservation {
  id: number
  roomNo: string
  checkIn: string
  checkOut: string
  guestName: string | null
  note: string | null
  /** booked = 有效；cancelled = 已取消（保留记录） */
  status: string
  operatorName: string | null
  createdAt: string
  updatedAt: string
  cancelledAt: string | null
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
  /** 源房当前所有有效客卡的卡号：换房前读卡核对"是不是本房原卡"用 */
  activeCardNos: number[]
}

/** 发卡记录行（记录页用，比读卡用的 IssuanceRecord 宽）。 */
export interface IssuanceRow {
  id: number
  createdAt: string
  roomNo: string | null
  kind: string
  days: number
  hours: number
  cardNo: number | null
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
  expiresAt: string | null
  checkedOutAt: string | null
  durationMs: number | null
  rawHex: string | null
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
  cardNo?: number | null
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
  operators: number
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
  /** tauri dev 构建为 true */
  dev: boolean
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

export type WaterLevel = 'normal' | 'warn' | 'blocked'

export interface CardNoStatus {
  current: number | null
  stateFile: number | null
  maxIssued: number | null
  level: WaterLevel
  /** 警戒线（设置里可改） */
  warnAt: number
  /** 硬阻断线，到这条线只能发管理卡 */
  blockAt: number
  statePath: string
}

/** 管理卡请求，与 Rust 端 `AdminCardRequest` 的 serde 表示一一对应。 */
export type AdminCardRequest =
  | { kind: 'master'; days?: number | null; hours?: number | null }
  | { kind: 'reset' }
  | { kind: 'clear' }
  | { kind: 'install'; roomNo: string }
  | { kind: 'checkout'; hour: number }
  | { kind: 'timeSync'; hour?: number | null; minute?: number | null }
  | { kind: 'blank' }
  | { kind: 'raw'; p1: string; p2: string }

export interface IssuedCard {
  card: CardSnapshot
  room: Room | null
  /** 到期时间（UTC ISO8601 估算值）；无时长的卡型为 null。 */
  expiresAt: string | null
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
  /** 房间参数：最多同时有效的客卡数（null = 不限） */
  setRoomMaxCards: (roomNo: string, maxCards: number | null) =>
    invoke<void>('set_room_max_cards', { roomNo, maxCards }),

  listRoomStatuses: () => invoke<RoomStatus[]>('list_room_statuses'),
  setRoomStatus: (roomNo: string, status: string) =>
    invoke<void>('set_room_status', { roomNo, status }),
  updateRoomStatusStyle: (key: string, icon: string, color: string) =>
    invoke<void>('update_room_status_style', { key, icon, color }),
  checkoutRoom: (roomNo: string) => invoke<CheckoutResult>('checkout_room', { roomNo }),
  /** 列出与 [from, to) 相交的有效预定（YYYY-MM-DD） */
  listReservations: (from: string, to: string, roomNo?: string | null) =>
    invoke<Reservation[]>('list_reservations', { from, to, roomNo: roomNo ?? null }),
  /** 新建预定；checkOut 是退房日（不含）。重叠 / 日期非法由后端拒绝 */
  createReservation: (input: {
    roomNo: string
    checkIn: string
    checkOut: string
    guestName?: string | null
    note?: string | null
  }) =>
    invoke<Reservation>('create_reservation', {
      roomNo: input.roomNo,
      checkIn: input.checkIn,
      checkOut: input.checkOut,
      guestName: input.guestName ?? null,
      note: input.note ?? null,
    }),
  /** 改期 / 改预定信息 */
  updateReservation: (
    id: number,
    input: {
      roomNo: string
      checkIn: string
      checkOut: string
      guestName?: string | null
      note?: string | null
    },
  ) =>
    invoke<Reservation>('update_reservation', {
      id,
      roomNo: input.roomNo,
      checkIn: input.checkIn,
      checkOut: input.checkOut,
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
   * `cardNo` 是换房前读到的卡号，只用于审计留痕（非原卡经二次确认后同样带上）。
   */
  moveRoomOut: (roomNo: string, toRoomNo: string, cardNo?: number | null) =>
    invoke<CheckoutResult>('room_move_out', { roomNo, toRoomNo, cardNo: cardNo ?? null }),

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
  listIssuances: (f: LogFilter) =>
    invoke<Page<IssuanceRow>>('list_issuances', {
      roomNo: f.roomNo ?? null,
      cardNo: f.cardNo ?? null,
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
  cardHistory: (cardNo: number) => invoke<IssuanceRow[]>('card_history', { cardNo }),
  /** 导出当前筛选为 CSV（管理员）：后端落盘并返回路径 */
  exportIssuancesCsv: (f: LogFilter) =>
    invoke<CsvResult>('export_issuances_csv', {
      roomNo: f.roomNo ?? null,
      cardNo: f.cardNo ?? null,
      kind: f.kind ?? null,
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
  checkoutCard: (cardNo: number, noDevice?: boolean) =>
    invoke<CheckoutCardResult>('checkout_card', { cardNo, noDevice }),
  /**
   * 强制退房：卡不是本系统发的，本地没有发卡记录，按前端从卡片里读出的房号
   * 直接写入退房数据（关闭该房全部有效卡 + 改房态）。调用前必须让用户二次确认。
   */
  forceCheckoutCard: (roomNo: string, cardNo: number) =>
    invoke<CheckoutResult>('force_checkout_card', { roomNo, cardNo }),
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
  setCheckoutHour: (hour: number) => invoke<number>('set_checkout_hour', { hour }),
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
    guest?: GuestInput,
    /** 长有效期卡（>62 天）的确认词，必须原样等于「顶高水位」 */
    confirmLong?: string | null,
  ) =>
    invoke<IssuedCard>('issue_guest', {
      roomNo,
      days: 'days' in duration ? duration.days : null,
      hours: 'hours' in duration ? duration.hours : null,
      idempotencyKey,
      guest: guest ?? null,
      confirmLong: confirmLong ?? null,
    }),
  issueAdminCard: (request: AdminCardRequest, confirm: boolean, confirmLong?: string | null) =>
    invoke<IssuedCard>('issue_admin_card', {
      request,
      confirm,
      confirmLong: confirmLong ?? null,
    }),
  cardNoStatus: () => invoke<CardNoStatus>('card_no_status'),
  setWaterWarnAt: (hex: string) => invoke<CardNoStatus>('set_water_warn_at', { hex }),
  resetWaterMark: (startAt: string) => invoke<CardNoStatus>('reset_water_mark', { startAt }),
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

/** 长有效期卡（天数进入月码段）发卡前必须原样输入的确认词，与后端 rules.rs 保持一致 */
export const LONG_CONFIRM_WORD = '顶高水位'

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
 * 为什么要标"估算"：卡内只有时长码，没有绝对到期时刻，锁实际从**首次刷卡**起算；
 * 这里显示的到期时刻是按**发卡时间 + 时长**推算的。无到期时间返回 null，调用方不渲染。
 */
export function formatExpiry(expiresAt: string | null | undefined): string | null {
  if (!expiresAt) return null
  const d = new Date(expiresAt)
  if (Number.isNaN(d.getTime())) return `有效期至 ${expiresAt}`
  return `有效期至 ${formatMinute(expiresAt)}（按发卡时间估算）`
}
