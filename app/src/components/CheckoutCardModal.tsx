import { useEffect, useMemo, useState } from 'react'
import { ExclamationCircleOutlined, ScanOutlined } from '@ant-design/icons'
import { Alert, Button, Modal, Space, Typography } from 'antd'
import {
  api,
  describeError,
  formatExpiry,
  formatTime,
  type CardReadDto,
  type Room,
} from '../api'
import { CardSnapshotView } from './CardSnapshotView'

const { Text } = Typography

/**
 * 读卡退房 / 持卡退房的二级表单。
 *
 * 流程刻意拆成两步：**先由用户点「读卡」把卡读出来核对，再点「确认退房」**。
 * 不在打开弹窗时自动读卡 —— 前台往往还要先翻卡盒找卡，自动读只会在没放卡时
 * 先报一个失败，反而干扰判断。
 *
 * `target` 非空表示从某房间的右键菜单进来，此时会校验卡是否属于该房间。
 *
 * 卡不是本系统发的（本地查不到发卡记录）时按卡号退不了房，改为「强制退房」：
 * 点下去先弹二次确认，说明会把退房数据**直接写进本地库**（关闭该房全部有效卡 +
 * 改房态），用户确认后才执行。
 */
export function CheckoutCardModal({
  open,
  target,
  rooms,
  onClose,
  onDone,
}: {
  open: boolean
  /** 从房间右键进入时的目标房间；顶栏「读卡退房」为 null */
  target: Room | null
  /** 当前房间列表：卡片里只有锁内序号时用它换房号 */
  rooms: Room[]
  onClose: () => void
  /** 退房成功：回传提示文案，由父级刷新列表并展示 */
  onDone: (message: string) => void
}) {
  const [reading, setReading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [data, setData] = useState<CardReadDto | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** 强制退房的二次确认（非本系统卡才用得到） */
  const [forceOpen, setForceOpen] = useState(false)

  // 每次打开都从"还没读卡"开始，避免上一次读到的卡被误当成这一次的
  useEffect(() => {
    if (!open) return
    setReading(false)
    setBusy(false)
    setData(null)
    setError(null)
    setForceOpen(false)
  }, [open])

  const card = data?.card ?? null
  const cardNo = card?.cardNo ?? null
  /** 本地没有这张卡的发卡记录：大概率不是本系统（或其它机器）发的 */
  const foreign = data?.lookup.status === 'notFound'

  /** 卡是否属于目标房间（顶栏进入时不校验） */
  const belongsToTarget =
    target === null ||
    card === null ||
    card.room === target.roomNo ||
    card.roomIndex === target.roomIndex

  /**
   * 强制退房要退哪个房间：
   * 右键进来的房间 → 卡片里的房号 → 本机房间表里按锁内序号找。
   * 三者都没有就退不了（不能凭空写一条退房数据）。
   */
  const roomNo = useMemo(() => {
    if (target) return target.roomNo
    if (!card) return null
    if (card.room) return card.room
    return rooms.find((r) => r.roomIndex === card.roomIndex)?.roomNo ?? null
  }, [target, card, rooms])

  /** 房号必须在本机房间表里，否则写入退房数据时会报"房号不存在" */
  const roomKnown = target !== null || (roomNo !== null && rooms.some((r) => r.roomNo === roomNo))
  const targetRoom = roomNo === null ? undefined : rooms.find((r) => r.roomNo === roomNo)

  /**
   * 不能退房的原因（null = 可以退）。
   * 先读卡再退卡：没读卡时按钮就该是灰的，而不是点了才报错。
   */
  const blockReason: string | null = (() => {
    if (!card) return '还没有读卡：请先把卡放到读卡器上，再点「读卡」'
    // 读卡退房只处理客卡（日租 / 钟点）；管理卡会被擦成空白，破坏仍需在场的开门工具
    if (card.kind !== 'guestDay' && card.kind !== 'guestHour') {
      return `这张卡是「${card.kindLabel}」，读卡退房只处理日租/钟点客卡`
    }
    if (cardNo === null) return `这张卡没有卡号（${card.kindLabel}），不能用读卡退房`
    if (!belongsToTarget && target) {
      return `这张卡属于 ${card.room ?? `序号 ${card.roomIndex}`}，不是 ${target.roomNo} 房`
    }
    // 非本系统卡只能按房间写退房数据，房号对不上就没法安全执行
    if (foreign && !roomKnown) {
      return `这张卡没有发卡记录，卡片里也没有本机认识的房号（序号 ${card.roomIndex}），无法强制退房；请用右键菜单的「无卡退房」`
    }
    return null
  })()

  const read = async () => {
    setReading(true)
    setError(null)
    setForceOpen(false)
    try {
      setData(await api.readCard())
    } catch (e) {
      setError(describeError(e))
      setData(null)
    } finally {
      setReading(false)
    }
  }

  /** 正常退房：按卡号只关闭这一张卡 */
  const checkout = async () => {
    if (cardNo === null) return
    setBusy(true)
    setError(null)
    try {
      const res = await api.checkoutCard({ cardNo })
      onDone(
        `${res.roomNo} 房：已退房，剩余有效卡 ${res.remaining} 张` +
          (res.remaining === 0 ? '（房态已按设置转换）' : ''),
      )
      onClose()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  /** 强制退房：按房间关闭有效卡 + 把卡擦成空白卡（用户已在二次确认里确认过） */
  const forceCheckout = async () => {
    if (cardNo === null || roomNo === null) return
    setBusy(true)
    setError(null)
    try {
      const res = await api.forceCheckoutCard(roomNo)
      setForceOpen(false)
      const eraseText = res.erased
        ? '，卡已擦成空白卡'
        : res.eraseError
          ? `，未擦卡（${res.eraseError}）`
          : ''
      onDone(
        `${res.roomNo} 房：非本系统卡，已强制退房（关闭 ${res.cardsClosed} 张有效卡${eraseText}，房态已按设置转换）`,
      )
      onClose()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const lookup = data?.lookup
  const recordExpiry = lookup?.status === 'found' ? formatExpiry(lookup.latest.expiresAt) : null
  const recordText =
    lookup?.status === 'found'
      ? `发卡记录：${lookup.latest.roomNo ?? '—'} · ${lookup.latest.kind} · ${formatTime(lookup.latest.createdAt)}` +
        (recordExpiry ? ` · ${recordExpiry}` : '')
      : lookup?.status === 'notFound'
        ? '未查到该卡的发卡记录（可能是原厂软件或其它机器发的）'
        : null

  return (
    <>
      <Modal
        open={open}
        title={target ? `持卡退房 · ${target.roomNo} 房` : '读卡退房'}
        width={620}
        onCancel={onClose}
        footer={
          <Space>
            <Button icon={<ScanOutlined />} loading={reading} onClick={read}>
              {card ? '重新读卡' : '读卡'}
            </Button>
            <Button onClick={onClose}>取消</Button>
            <Button
              type="primary"
              danger
              icon={foreign ? <ExclamationCircleOutlined /> : undefined}
              disabled={blockReason !== null}
              loading={busy}
              onClick={() => (foreign ? setForceOpen(true) : void checkout())}
            >
              {foreign ? '强制退房' : '确认退房'}
            </Button>
          </Space>
        }
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {target
              ? `流程：把 ${target.roomNo} 房的卡放到读卡器上 → 点「读卡」核对 → 按提示确认退房（非本系统卡需二次确认）。`
              : '流程：把要退的客卡放到读卡器上 → 点「读卡」核对 → 按提示确认退房（非本系统卡需二次确认）。'}
          </Text>

          {error ? <Alert type="error" showIcon message="操作失败" description={error} /> : null}

          {card ? (
            <>
              <CardSnapshotView
                card={card}
                expiresAt={lookup?.status === 'found' ? lookup.latest.expiresAt : null}
                createdAt={lookup?.status === 'found' ? lookup.latest.createdAt : null}
              />
              {recordText ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {recordText}
                </Text>
              ) : null}
              {blockReason ? (
                <Alert type="warning" showIcon message={blockReason} />
              ) : foreign ? (
                <Alert
                  type="warning"
                  showIcon
                  message="这张卡不是本系统发出的，不能按卡号退房"
                  description={
                    <>
                      这张卡在本地没有发卡记录。点「强制退房」会先弹二次确认，
                      确认后<b>关闭该房全部有效卡，并把卡擦成空白卡</b>（卡要放在读卡器上）；
                      擦卡失败不会阻断退房，但会明确提示原因。
                    </>
                  }
                />
              ) : (
                <Alert type="success" showIcon message="核对无误后点右下角「确认退房」。" />
              )}
            </>
          ) : (
            <Alert
              type="info"
              showIcon
              message={blockReason ?? '请先读卡'}
              description="退房只关闭这一张卡；作废该房间全部有效卡请用右键菜单里的「无卡退房」。"
            />
          )}
        </Space>
      </Modal>

      {/* 非本系统卡：强制退房二次确认（用受控 Modal，不用 Modal.confirm） */}
      <Modal
        open={forceOpen}
        title="确认强制退房？"
        okText="确认强制退房"
        cancelText="取消"
        okButtonProps={{ danger: true }}
        confirmLoading={busy}
        onOk={forceCheckout}
        onCancel={() => setForceOpen(false)}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Alert
            type="warning"
            showIcon
            message="这张卡不是本系统发出的"
            description="这张卡在本地没有发卡记录，无法按卡号退房。"
          />
          <div>
            继续将<b>强制退房</b>：
            <ul style={{ margin: '6px 0 0', paddingInlineStart: 20 }}>
              <li>
                把 <b>{roomNo ?? '—'} 房</b>当前全部有效卡
                {targetRoom ? `（${targetRoom.activeCards} 张）` : ''}标记为已退房
              </li>
              <li>
                把读卡器上的这张卡<b>擦成空白卡</b>（卡要放好；失败不会阻断退房，会在结果里说明）
              </li>
              <li>房态改为设置里的「退房后房态」</li>
              <li>审计日志记一条「强制退房」，注明卡号与是否擦卡</li>
            </ul>
          </div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            擦卡写的是全零 8 字节，与原厂「退房回收卡」是否完全等价仍待实测（TD-143）；
            擦卡失败时这张卡仍可能开门，需要再擦一次或改用其它方式处理。
          </Text>
        </Space>
      </Modal>
    </>
  )
}
