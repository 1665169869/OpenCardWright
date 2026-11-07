import { useState } from 'react'
import { DatePicker, Input, Modal, Space, Tag, Typography } from 'antd'
import dayjs from 'dayjs'
import { api, describeError, type Reservation, type Room } from '../api'

const { Text } = Typography

/** 本地日期（YYYY-MM-DD），用 dayjs 与 DatePicker 的值对齐。 */
const localDay = (offsetDays = 0): string => dayjs().add(offsetDays, 'day').format('YYYY-MM-DD')

const nights = (a: string, b: string) =>
  Math.max(0, dayjs(b).startOf('day').diff(dayjs(a).startOf('day'), 'day'))

/**
 * 预定弹窗：新建 / 改期 / 取消。
 *
 * 语义（2026-11-06 用户确认）：入住—退房日期区间，**退房日不含**；
 * 未来预定不改房态，只有区间覆盖今天时后端才把空房标成「预定房」。
 * 重叠 / 日期非法由后端拒绝，这里原样展示错误。
 *
 * 使用方每次打开换 `key`（`reserveSession`）强制重挂载 —— HMR 会保留组件 state，
 * 只靠 `open` 变化清表单，首帧仍会带着上一次的数据。
 */
export default function ReservationModal({
  open,
  room,
  editing,
  initialDate,
  onClose,
  onSaved,
}: {
  open: boolean
  /** 新建时的目标房间 */
  room: Room | null
  /** 改期时的原预定；与 room 二选一，editing 优先 */
  editing: Reservation | null
  /** 新建时预填入住日（日历空白格点进来的那天）；默认今天 */
  initialDate?: string | null
  onClose: () => void
  onSaved: (msg: string) => void
}) {
  const isEdit = editing !== null
  // lazy 初始化：组件由 key 强制重挂载，挂载时的 editing/room 就是本次的目标
  const [checkIn, setCheckIn] = useState<string | null>(
    editing?.checkIn ?? initialDate ?? localDay(0),
  )
  const [checkOut, setCheckOut] = useState<string | null>(
    editing?.checkOut ??
      (initialDate ? dayjs(initialDate).add(1, 'day').format('YYYY-MM-DD') : localDay(1)),
  )
  const [guestName, setGuestName] = useState(editing?.guestName ?? '')
  const [note, setNote] = useState(editing?.note ?? '')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  /** 取消预定的两步确认：第一次点变成「再点一次确认」 */
  const [cancelArmed, setCancelArmed] = useState(false)

  const targetRoomNo = editing?.roomNo ?? room?.roomNo ?? ''

  const submit = async () => {
    if (!checkIn || !checkOut || !targetRoomNo) return
    setBusy(true)
    setErr(null)
    try {
      const input = {
        roomNo: targetRoomNo,
        checkIn,
        checkOut,
        guestName: guestName.trim() || null,
        note: note.trim() || null,
      }
      if (editing) {
        await api.updateReservation(editing.id, input)
        onSaved(`${targetRoomNo} 预定已改期：${checkIn} → ${checkOut}（退房日不含）`)
      } else {
        await api.createReservation(input)
        onSaved(
          `${targetRoomNo} 已预定：${checkIn} → ${checkOut}（住 ${nights(checkIn, checkOut)} 晚）`,
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

  const canSubmit = !busy && !!checkIn && !!checkOut && checkOut > checkIn && targetRoomNo !== ''

  return (
    <Modal
      open={open}
      title={isEdit ? `改期 / 编辑预定 · ${editing.roomNo}` : `预定房间 · ${room?.roomNo ?? ''}`}
      okText={isEdit ? '保存' : '确认预定'}
      cancelText="关闭"
      confirmLoading={busy}
      okButtonProps={{ disabled: !canSubmit }}
      onOk={() => void submit()}
      onCancel={onClose}
      destroyOnHidden
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <Space wrap size="large">
          <span>
            <Text type="secondary">入住</Text>
            <div style={{ marginTop: 4 }}>
              <DatePicker
                value={checkIn ? dayjs(checkIn) : null}
                onChange={(_, s) => setCheckIn(typeof s === 'string' && s ? s : null)}
                style={{ width: 150 }}
                // 改期允许保留原入住日；新建不允许选过去
                disabledDate={(d) => !isEdit && d.isBefore(localDay(0), 'day')}
              />
            </div>
          </span>
          <span>
            <Text type="secondary">退房（不含当日）</Text>
            <div style={{ marginTop: 4 }}>
              <DatePicker
                value={checkOut ? dayjs(checkOut) : null}
                onChange={(_, s) => setCheckOut(typeof s === 'string' && s ? s : null)}
                style={{ width: 150 }}
                disabledDate={(d) =>
                  checkIn ? d.isBefore(dayjs(checkIn).add(1, 'day'), 'day') : false
                }
              />
            </div>
          </span>
        </Space>

        {checkIn && checkOut && checkOut > checkIn ? (
          <Text type="secondary">
            住 {nights(checkIn, checkOut)} 晚：{checkIn} 至 {checkOut}（退房日不占房）
          </Text>
        ) : (
          <Text type="secondary">退房日期必须晚于入住日期</Text>
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
            <Tag color="blue">改期不影响在场客卡</Tag>
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
