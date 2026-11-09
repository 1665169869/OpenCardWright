import { useState } from 'react'
import { DatePicker, Input, InputNumber, Modal, Select, Space, Tag, TimePicker, Typography } from 'antd'
import dayjs from 'dayjs'
import { api, describeError, type Reservation, type Room } from '../api'

const { Text } = Typography

/** 本地日期（YYYY-MM-DD），用 dayjs 与 DatePicker 的值对齐。 */
const localDay = (offsetDays = 0): string => dayjs().add(offsetDays, 'day').format('YYYY-MM-DD')

const dateDiff = (a: string, b: string) =>
  Math.max(0, dayjs(b).startOf('day').diff(dayjs(a).startOf('day'), 'day'))

/**
 * 预定弹窗：新建 / 改期 / 改房 / 取消。
 *
 * 语义：入住—退房日期区间，退房日不含；退房时间不占房。
 * 日期用 RangePicker 合并，晚数只是退房日期的快捷方式；
 * 凌晨房（同一天入住、退房）仍由完整日期时间判断，不强制至少 1 晚。
 */
export default function ReservationModal({
  open,
  rooms,
  room,
  editing,
  initialDate,
  onClose,
  onSaved,
}: {
  open: boolean
  /** 启用中的房间；编辑历史预定时也允许保留原房间 */
  rooms: Room[]
  /** 新建时的目标房间；顶部「新增预定」打开时为 null */
  room: Room | null
  /** 改期时的原预定；与 room 二选一，editing 优先 */
  editing: Reservation | null
  /** 新建时预填入住日（日历空白格点进来的那天）；默认今天 */
  initialDate?: string | null
  onClose: () => void
  onSaved: (msg: string) => void
}) {
  const isEdit = editing !== null
  const initialIn = editing?.checkIn ?? initialDate ?? localDay(0)
  const initialOut =
    editing?.checkOut ??
    (initialDate ? dayjs(initialDate).add(1, 'day').format('YYYY-MM-DD') : localDay(1))

  // lazy 初始化：组件由 key 强制重挂载，挂载时的 editing/room 就是本次的目标
  const [roomNo, setRoomNo] = useState(editing?.roomNo ?? room?.roomNo ?? '')
  const [checkIn, setCheckIn] = useState<string | null>(initialIn)
  const [checkInTime, setCheckInTime] = useState(editing?.checkInTime ?? '12:00')
  const [checkOut, setCheckOut] = useState<string | null>(initialOut)
  const [checkOutTime, setCheckOutTime] = useState(editing?.checkOutTime ?? '12:00')
  const [nightsCount, setNightsCount] = useState<number>(() => dateDiff(initialIn, initialOut))
  const [guestName, setGuestName] = useState(editing?.guestName ?? '')
  const [note, setNote] = useState(editing?.note ?? '')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  /** 取消预定的两步确认：第一次点变成「再点一次确认」 */
  const [cancelArmed, setCancelArmed] = useState(false)

  const roomOptions = rooms
    .filter((r) => r.active || r.roomNo === editing?.roomNo || r.roomNo === room?.roomNo)
    .map((r) => ({ value: r.roomNo, label: `${r.roomNo}${r.roomTypeName ? ` · ${r.roomTypeName}` : ''}` }))

  /** RangePicker 同时改入住/退房：晚数由日期反算。 */
  const changeRange = (dates: [dayjs.Dayjs | null, dayjs.Dayjs | null] | null) => {
    const start = dates?.[0] ?? null
    const end = dates?.[1] ?? null
    setCheckIn(start ? start.format('YYYY-MM-DD') : null)
    setCheckOut(end ? end.format('YYYY-MM-DD') : null)
    if (start && end) setNightsCount(dateDiff(start.format('YYYY-MM-DD'), end.format('YYYY-MM-DD')))
  }

  /** 改晚数：只移动退房日期，保留退房时间（凌晨房可设为 0）。 */
  const changeNights = (value: number | null) => {
    const n = Math.max(0, Math.floor(value ?? 0))
    setNightsCount(n)
    if (checkIn) setCheckOut(dayjs(checkIn).add(n, 'day').format('YYYY-MM-DD'))
  }

  const submit = async () => {
    if (!checkIn || !checkOut || !roomNo) return
    setBusy(true)
    setErr(null)
    try {
      const input = {
        roomNo,
        checkIn,
        checkInTime,
        checkOut,
        checkOutTime,
        guestName: guestName.trim() || null,
        note: note.trim() || null,
      }
      if (editing) {
        const moved = editing.roomNo !== roomNo
        await api.updateReservation(editing.id, input)
        onSaved(
          moved
            ? `预定已从 ${editing.roomNo} 房改到 ${roomNo} 房：${checkIn} ${checkInTime} → ${checkOut} ${checkOutTime}`
            : `${roomNo} 预定已改期：${checkIn} ${checkInTime} → ${checkOut} ${checkOutTime}（退房时间不占房）`,
        )
      } else {
        await api.createReservation(input)
        onSaved(
          nightsCount === 0
            ? `${roomNo} 已预定：${checkIn} ${checkInTime} → ${checkOut} ${checkOutTime}（凌晨房 / 当天退房）`
            : `${roomNo} 已预定：${checkIn} ${checkInTime} → ${checkOut} ${checkOutTime}（住 ${nightsCount} 晚）`,
        )
      }
      onClose()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const doCancel = async () => {
    if (!editing) return
    setBusy(true)
    setErr(null)
    try {
      await api.cancelReservation(editing.id)
      onSaved(`${editing.roomNo} ${editing.checkIn} → ${editing.checkOut} 的预定已取消`)
      onClose()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
      setCancelArmed(false)
    }
  }

  const checkInAt = checkIn ? `${checkIn}T${checkInTime}` : null
  const checkOutAt = checkOut ? `${checkOut}T${checkOutTime}` : null
  const dateOrderOk = !!checkInAt && !!checkOutAt && checkOutAt > checkInAt
  const dateRangeOk = isEdit || !checkIn || checkIn >= localDay(0)
  const canSubmit = !busy && !!roomNo && dateOrderOk && dateRangeOk

  return (
    <Modal
      open={open}
      title={isEdit ? `改期 / 编辑预定 · ${editing.roomNo}` : roomNo ? `预定房间 · ${roomNo}` : '新增预定'}
      okText={isEdit ? '保存' : '确认预定'}
      cancelText="关闭"
      confirmLoading={busy}
      okButtonProps={{ disabled: !canSubmit }}
      onOk={() => void submit()}
      onCancel={onClose}
      destroyOnHidden
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <div>
          <Text type="secondary">房间</Text>
          <Select
            showSearch
            optionFilterProp="label"
            value={roomNo || undefined}
            placeholder="选择房间"
            onChange={(v) => setRoomNo(v)}
            options={roomOptions}
            style={{ width: '100%', marginTop: 4 }}
            status={!roomNo ? 'warning' : undefined}
          />
        </div>

        <div>
          <Text type="secondary">入住 / 退房日期</Text>
          <div style={{ marginTop: 4 }}>
            <DatePicker.RangePicker
              value={checkIn && checkOut ? [dayjs(checkIn), dayjs(checkOut)] : null}
              onChange={(dates) =>
                changeRange(dates as [dayjs.Dayjs | null, dayjs.Dayjs | null] | null)
              }
              format="YYYY-MM-DD"
              style={{ width: 300 }}
              allowClear={false}
              // 新建不允许选过去；编辑历史预定保留原日期
              disabledDate={(d) => !isEdit && d.isBefore(dayjs().startOf('day'))}
            />
            <Space wrap size={8} style={{ marginTop: 6 }}>
              <Text type="secondary">入住时间</Text>
              <TimePicker
                value={dayjs(checkInTime, 'HH:mm')}
                format="HH:mm"
                minuteStep={5}
                allowClear={false}
                onChange={(_, s) => setCheckInTime(typeof s === 'string' && s ? s : '12:00')}
                style={{ width: 90 }}
              />
              <Text type="secondary">退房时间</Text>
              <TimePicker
                value={dayjs(checkOutTime, 'HH:mm')}
                format="HH:mm"
                minuteStep={5}
                allowClear={false}
                onChange={(_, s) => setCheckOutTime(typeof s === 'string' && s ? s : '12:00')}
                style={{ width: 90 }}
              />
            </Space>
          </div>
        </div>

        <Space>
          <Text type="secondary">晚数</Text>
          <InputNumber
            min={0}
            max={365}
            value={nightsCount}
            onChange={changeNights}
            style={{ width: 100 }}
          />
          <Text type="secondary">改晚数会自动调整退房日期；凌晨房可填 0</Text>
        </Space>

        {checkIn && checkOut && dateOrderOk ? (
          <Text type="secondary">
            {nightsCount === 0
              ? `凌晨房：${checkIn} ${checkInTime} → ${checkOut} ${checkOutTime}（当天退房）`
              : `住 ${nightsCount} 晚：${checkIn} ${checkInTime} 至 ${checkOut} ${checkOutTime}（退房时间不占房）`}
          </Text>
        ) : (
          <Text type="danger">
            {!dateRangeOk
              ? '入住日期不能早于今天'
              : '退房时间必须晚于入住时间；凌晨房请把退房日期设为同一天并确认退房时间'}
          </Text>
        )}

        <div>
          <Text type="secondary">客人姓名（可选，只存数据库不写卡）</Text>
          <Input
            value={guestName}
            onChange={(e) => setGuestName(e.target.value)}
            maxLength={40}
            placeholder="如：张三"
            style={{ marginTop: 4 }}
          />
        </div>
        <div>
          <Text type="secondary">备注（可选）</Text>
          <Input.TextArea
            value={note}
            onChange={(e) => setNote(e.target.value)}
            maxLength={200}
            rows={2}
            style={{ marginTop: 4 }}
          />
        </div>

        {err ? <Text type="danger">{err}</Text> : null}

        {isEdit ? (
          <Space>
            <Tag color="blue">改期 / 改房不影响在场客卡</Tag>
            <a
              style={{ color: cancelArmed ? '#cf1322' : undefined }}
              onClick={() => (cancelArmed ? void doCancel() : setCancelArmed(true))}
            >
              {cancelArmed ? '再点一次，确认取消该预定' : '取消这条预定'}
            </a>
          </Space>
        ) : null}
      </Space>
    </Modal>
  )
}
