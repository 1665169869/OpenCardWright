/**
 * 身份证读卡器客户端（TD-135）。
 *
 * 设备是**独立的**：走华视身份证读卡服务的本地 WebSocket（默认 `ws://127.0.0.1:9001`），
 * 与发卡用的 125 kHz 读卡器（`PMSLockCard.exe` 子进程）完全无关。
 *
 * 协议（2026-11-10 真机实测）：连上后必须由客户端发读卡命令（`04`，不拿起模式），服务随后返回结果。
 * 各版本厂商服务的字段名不一致（英文 / 拼音缩写 / 中文），这里做宽容归一；
 * 归一不出任何字段就当成心跳忽略，直到超时——读卡器没放卡时不能让表单卡死。
 */

export const ID_CARD_WS_URL = 'ws://127.0.0.1:9001'

/** 读卡请求：服务不会主动推送，必须由客户端发起（04 = 读身份证，不拿起模式，超时 10 秒） */
const READ_CMD = '04?timeout=10000'

/** 读到的身份证信息，字段名与发卡表单的 `GuestInput` 对齐 */
export interface IdCardInfo {
  name?: string
  gender?: string
  nation?: string
  birthDate?: string
  address?: string
  idNumber?: string
  /** 证件有效期起 / 止（`YYYYMMDD`，原样保留） */
  idValidFrom?: string
  idValidTo?: string
  /** 签发机关 */
  idAuthority?: string
  /** 读卡设备型号 / 序列号 */
  deviceModel?: string
  deviceSerial?: string
}

/**
 * 厂商服务的 `|` 分隔文本协议（本机实测格式）。
 *
 * 字段顺序：设备型号 | 设备序列 | 证件类型 | 姓名 | 性别 | 民族 | 出生 | 地址 |
 * 身份证号 | 签发机关 | 有效期起 | 有效期止 | …（空位）… | 头像 base64（最后一项）
 *
 * **头像不读取**：拆出后直接丢弃，不进任何状态、不落库、不写日志。
 * 解析失败一律返回 null（调用方只报「格式不符」，**不回显原文**——里面有隐私）。
 */
export function parsePipePayload(text: string): IdCardInfo | null {
  if (!text.includes('|')) return null
  const f = text.split('|')
  if (f.length < 12) return null
  const at = (i: number) => (f[i] ?? '').trim()
  const info: IdCardInfo = {
    deviceModel: at(0) || undefined,
    deviceSerial: at(1) || undefined,
    name: at(3) || undefined,
    gender: normalizeGender(at(4)) || undefined,
    nation: at(5) || undefined,
    birthDate: normalizeBirth(at(6)) || undefined,
    address: at(7) || undefined,
    idNumber: at(8) || undefined,
    idAuthority: at(9) || undefined,
    idValidFrom: at(10) || undefined,
    idValidTo: at(11) || undefined,
  }
  // 头像（f[18] 及以后）根本不赋给任何变量，随函数返回被 GC
  return Object.values(info).some((v) => v) ? info : null
}

/** 同一含义的不同键名（华视各版本服务的输出差异）；只覆盖 JSON 协议里可映射的字段 */
type AliasKey = 'name' | 'gender' | 'nation' | 'birthDate' | 'address' | 'idNumber'
const ALIASES: Record<AliasKey, string[]> = {
  name: ['name', 'Name', '姓名', 'xm', 'XM'],
  gender: ['sex', 'Sex', 'gender', '性别', 'xb', 'XB'],
  nation: ['nation', 'Nation', '民族', 'mz', 'MZ'],
  birthDate: ['birth', 'birthday', 'Birth', '出生', '出生日期', 'csrq', 'CSRQ'],
  address: ['address', 'Address', '住址', '家庭住址', 'dz', 'DZ'],
  idNumber: ['idno', 'idNo', 'idNumber', 'cardNo', '身份证号', '身份证号码', 'gmsfhm', 'GMSFHM'],
}

function pick(obj: Record<string, unknown>, keys: string[]): string {
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.trim() !== '') return v.trim()
    if (typeof v === 'number') return String(v)
  }
  return ''
}

/** `19900101` / `1990-01-01` / `1990/1/1` → `YYYY-MM-DD`（表单是 `<input type="date">`） */
function normalizeBirth(raw: string): string {
  const m = raw.match(/^(\d{4})[-/年.](\d{1,2})[-/月.](\d{1,2})/)
  if (m) {
    const pad = (s: string) => s.padStart(2, '0')
    return `${m[1]}-${pad(m[2])}-${pad(m[3])}`
  }
  const digits = raw.replace(/[^0-9]/g, '')
  if (digits.length === 8) {
    return `${digits.slice(0, 4)}-${digits.slice(4, 6)}-${digits.slice(6, 8)}`
  }
  return ''
}

/** 有的服务给「男/女」，有的给 `M/F` 或 `1/2` */
function normalizeGender(raw: string): string {
  const v = raw.trim()
  if (v === '男' || v === 'M' || v === 'm' || v === '1') return '男'
  if (v === '女' || v === 'F' || v === 'f' || v === '2') return '女'
  return v
}

/** 把服务推送的任意 JSON 归一成 `IdCardInfo`；一个可用字段都没有就返回 null（当心跳丢掉） */
export function normalizeIdCard(payload: unknown): IdCardInfo | null {
  if (!payload || typeof payload !== 'object') return null
  const box = payload as Record<string, unknown>
  // 有些版本把结果包在 data / result / info 里
  const wrapped = (box.data ?? box.result ?? box.info) as Record<string, unknown> | undefined
  const src = wrapped && typeof wrapped === 'object' ? wrapped : box

  const info: IdCardInfo = {
    name: pick(src, ALIASES.name) || undefined,
    gender: normalizeGender(pick(src, ALIASES.gender)) || undefined,
    nation: pick(src, ALIASES.nation) || undefined,
    birthDate: normalizeBirth(pick(src, ALIASES.birthDate)) || undefined,
    address: pick(src, ALIASES.address) || undefined,
    idNumber: pick(src, ALIASES.idNumber) || undefined,
  }
  return Object.values(info).some((v) => v) ? info : null
}

/**
 * 连上 9001，等第一条含身份证字段的推送。
 *
 * 失败一律抛中文说明的 Error（由调用方展示）：连不上、服务主动断开、超时（含「身份证没放上去」）。
 * 成功即关闭连接，不常驻——避免与厂商服务抢端口。
 */
export function readIdCard(timeoutMs = 8000): Promise<IdCardInfo> {
  return new Promise((resolve, reject) => {
    let settled = false
    let ws: WebSocket | undefined

    const finish = (fn: () => void) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      // 不再向厂商服务发「停止读卡」命令：04 现在就是读卡命令，超时/失败只断开连接
      try {
        ws?.close()
      } catch {
        // 关闭异常无关紧要
      }
      fn()
    }

    const timer = setTimeout(() => {
      finish(
        () =>
          reject(
            new Error(
              `读卡超时（${Math.round(timeoutMs / 1000)} 秒）：确认身份证读卡服务在运行，` +
                '并且身份证已经放到读卡器上',
            ),
          ),
      )
    }, timeoutMs)

    try {
      ws = new WebSocket(ID_CARD_WS_URL)
    } catch (e) {
      finish(() => reject(new Error(`无法连接身份证读卡服务（${ID_CARD_WS_URL}）：${String(e)}`)))
      return
    }

    ws.onopen = () => {
      // 服务不会主动推送，必须由客户端发起读卡请求（不发就永远等不到数据）
      try {
        ws?.send(READ_CMD)
      } catch {
        // 发送失败交给超时/错误分支收敛
      }
    }

    ws.onmessage = (ev: MessageEvent) => {
      const text = String(ev.data)
      // 优先按 `|` 文本协议解析（本机实测格式，含设备/证件/有效期字段）
      const pipe = parsePipePayload(text)
      if (pipe) {
        finish(() => resolve(pipe))
        return
      }
      let payload: unknown
      try {
        payload = JSON.parse(text)
      } catch {
        return // 既不是 `|` 也不是 JSON（日志/心跳），继续等
      }
      const info = normalizeIdCard(payload)
      if (!info) return // 空壳/心跳，继续等
      finish(() => resolve(info))
    }

    ws.onerror = () => {
      finish(() =>
        reject(
          new Error(
            `连不上身份证读卡服务（${ID_CARD_WS_URL}）：确认华视读卡服务已启动、占用本机 9001 端口`,
          ),
        ),
      )
    }

    ws.onclose = () => {
      finish(() =>
        reject(
          new Error(`身份证读卡服务主动断开（${ID_CARD_WS_URL}）：确认读卡器已插好、服务没有退出`),
        ),
      )
    }
  })
}
