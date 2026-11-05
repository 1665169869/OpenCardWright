import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  CloseCircleOutlined,
  FieldTimeOutlined,
  IdcardOutlined,
  LogoutOutlined,
} from '@ant-design/icons'
import {
  Alert,
  Button,
  Card,
  Drawer,
  Dropdown,
  Empty,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import {
  api,
  describeError,
  formatCardNo,
  formatTime,
  type Room,
  type RoomStatus,
  type RoomType,
} from '../api'
import { roomNoIssue } from '../utils/validate'
import { statusIcon } from '../components/StatusIcon'
import IssueForm from '../components/IssueForm'

const { Text, Title } = Typography

/**
 * 房间管理（格子视图 + 房间抽屉）。
 *
 * 每个格子按**房态**着色（不再按房型），点开抽屉可以：改房态、改房型/上限、
 * 直接发客卡、退房、制作安装卡。
 */
export default function RoomManager({
  onRoomsChanged,
  canAdmin,
}: {
  onRoomsChanged?: () => void
  canAdmin: boolean
}) {
  const [rooms, setRooms] = useState<Room[]>([])
  const [roomTypes, setRoomTypes] = useState<RoomType[]>([])
  const [statuses, setStatuses] = useState<RoomStatus[]>([])
  const [roomNo, setRoomNo] = useState('')
  const [index, setIndex] = useState<number | null>(null)
  const [newTypeId, setNewTypeId] = useState<number | null>(null)
  const [filterType, setFilterType] = useState<number | 'all'>('all')
  const [filterStatus, setFilterStatus] = useState<string | 'all'>('all')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [roomNoErr, setRoomNoErr] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  // 抽屉里的房间
  const [active, setActiveRoom] = useState<Room | null>(null)
  const [editTypeId, setEditTypeId] = useState<number | null>(null)
  const [editMax, setEditMax] = useState<number | null>(null)
  const [savingEdit, setSavingEdit] = useState(false)

  const [installOpen, setInstallOpen] = useState(false)
  // 发卡只从右键菜单进入，抽屉里不再放发卡表单
  const [issueRoom, setIssueRoom] = useState<Room | null>(null)
  const [issueMode, setIssueMode] = useState<'new' | 'renew'>('new')

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
      setActiveRoom((prev) => (prev ? (roomList.find((r) => r.roomNo === prev.roomNo) ?? null) : null))
    } catch (e) {
      setError(describeError(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const statusOf = (key: string): RoomStatus | undefined => statuses.find((s) => s.key === key)

  /** 房型标签颜色（在设置→房型里自定义；没设就是中性灰）。 */
  const typeColor = (typeId: number | null): string | undefined =>
    typeId === null ? undefined : (roomTypes.find((t) => t.id === typeId)?.color ?? undefined)

  const visible = useMemo(
    () =>
      rooms.filter(
        (r) =>
          (filterType === 'all' || r.roomTypeId === filterType) &&
          (filterStatus === 'all' || r.status === filterStatus),
      ),
    [rooms, filterType, filterStatus],
  )

  const openRoom = (room: Room) => {
    setActiveRoom(room)
    setEditTypeId(room.roomTypeId)
    setEditMax(room.maxCards)
    setError(null)
    setNotice(null)
  }

  const addRoom = async () => {
    const issue = roomNoIssue(roomNo)
    setRoomNoErr(issue)
    if (issue) return
    setLoading(true)
    setError(null)
    setNotice(null)
    try {
      const room = await api.createRoom(roomNo.trim(), index ?? undefined, newTypeId ?? undefined)
      setRoomNo('')
      setIndex(null)
      setNotice(`已新增 ${room.roomNo}（序号 ${room.roomIndex}）。制作安装卡后到该房门锁刷一次即可。`)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setLoading(false)
    }
  }

  const changeStatus = async (room: Room, key: string) => {
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
      if ((editMax ?? null) !== (active.maxCards ?? null)) {
        await api.setRoomMaxCards(active.roomNo, editMax ?? null)
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

  /** 读卡退房 / 持卡退房：读一张卡，按卡号退房；给了 target 就先校验这张卡属于该房。 */
  const readAndCheckout = async (target?: Room) => {
    setLoading(true)
    setError(null)
    setNotice(null)
    try {
      const read = await api.readCard()
      const card = read.card
      if (card.cardNo === null) {
        setError('这张卡没有卡号（安装卡/复位卡之类），不能用读卡退房')
        return
      }
      if (target) {
        const sameRoom =
          (card.room !== null && card.room === target.roomNo) || card.roomIndex === target.roomIndex
        if (!sameRoom) {
          setError(
            `这张卡属于 ${card.room ?? `序号 ${card.roomIndex}`}，不是 ${target.roomNo} 房，已取消`,
          )
          return
        }
      }
      const res = await api.checkoutCard(card.cardNo)
      setNotice(
        `${res.roomNo} 房：卡号 ${formatCardNo(card.cardNo)} 已退房，剩余有效卡 ${res.remaining} 张` +
          (res.remaining === 0 ? '（房态已转空脏房）' : ''),
      )
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setLoading(false)
    }
  }

  /** 无卡退房：按房间关闭全部有效卡，不需要卡。 */
  const noCardCheckout = async (room: Room) => {
    setLoading(true)
    setError(null)
    setNotice(null)
    try {
      const res = await api.checkoutRoom(room.roomNo)
      setNotice(`${room.roomNo} 已退房：关闭 ${res.cardsClosed} 张有效卡，房态改为空脏房`)
      await reload()
      onRoomsChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setLoading(false)
    }
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

  const typeOptions = roomTypes.map((t) => ({ value: t.id, label: t.name }))
  const activeStatus = active ? statusOf(active.status) : undefined

  return (
    <>
      <Card
        title="房间"
        extra={
          <Button size="small" onClick={() => void reload()}>
            刷新
          </Button>
        }
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Space wrap>
            {canAdmin ? (
              <>
                <Input
                  placeholder="房号，例如 260"
                  value={roomNo}
                  onChange={(e) => {
                    setRoomNo(e.target.value)
                    if (roomNoErr) setRoomNoErr(null)
                  }}
                  style={{ width: 150 }}
                  status={roomNoErr ? 'error' : undefined}
                />
                <InputNumber
                  placeholder="序号（留空自动分配）"
                  min={0}
                  max={255}
                  value={index ?? undefined}
                  onChange={(v) => setIndex(typeof v === 'number' ? v : null)}
                  style={{ width: 180 }}
                />
                <Select
                  allowClear
                  placeholder="房型（可留空）"
                  style={{ width: 140 }}
                  value={newTypeId ?? undefined}
                  onChange={(v) => setNewTypeId(typeof v === 'number' ? v : null)}
                  options={typeOptions}
                />
                <Button type="primary" loading={loading} onClick={addRoom}>
                  新增房号
                </Button>
              </>
            ) : null}
            <Button loading={loading} onClick={() => readAndCheckout()}>
              读卡退房
            </Button>
            {roomNoErr ? (
              <Text type="danger" style={{ fontSize: 12 }}>
                {roomNoErr}
              </Text>
            ) : null}
          </Space>

          <Space wrap>
            <Text type="secondary">房型</Text>
            <Select
              style={{ width: 150 }}
              value={filterType}
              onChange={(v) => setFilterType(v as number | 'all')}
              options={[
                { value: 'all', label: `全部（${rooms.length}）` },
                ...roomTypes.map((t) => ({ value: t.id, label: `${t.name}（${t.roomCount}）` })),
              ]}
            />
            <Text type="secondary">房态</Text>
            <Select
              style={{ width: 150 }}
              value={filterStatus}
              onChange={(v) => setFilterStatus(v as string | 'all')}
              options={[
                { value: 'all', label: '全部房态' },
                ...statuses.map((s) => ({ value: s.key, label: s.label })),
              ]}
            />
            <Text type="secondary" style={{ fontSize: 12 }}>
              点房间卡片可直接发卡、改房态、退房。
            </Text>
          </Space>

          {error ? <Alert type="error" showIcon message={error} /> : null}
          {notice ? <Alert type="success" showIcon message={notice} /> : null}

          {visible.length === 0 ? (
            <Empty description="没有符合条件的房间" />
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
                const overLimit =
                  room.maxCards !== null && room.activeCards >= room.maxCards
                return (
                  <Dropdown
                    key={room.roomNo}
                    trigger={['contextMenu']}
                    menu={{
                      items: [
                        { key: 'issue', icon: <IdcardOutlined />, label: '发行房间门卡' },
                        { key: 'renew', icon: <FieldTimeOutlined />, label: '续住写卡' },
                        { type: 'divider' },
                        { key: 'checkoutCard', icon: <LogoutOutlined />, label: '持卡退房' },
                        { key: 'checkoutNoCard', icon: <CloseCircleOutlined />, label: '无卡退房' },
                      ],
                      onClick: ({ key }) => {
                        if (key === 'checkoutCard') void readAndCheckout(room)
                        else if (key === 'checkoutNoCard') void noCardCheckout(room)
                        else if (key === 'issue') {
                          setIssueMode('new')
                          setIssueRoom(room)
                        } else if (key === 'renew') {
                          setIssueMode('renew')
                          setIssueRoom(room)
                        }
                      },
                    }}
                  >
                  <Card
                    size="small"
                    hoverable
                    onClick={() => openRoom(room)}
                    style={{
                      borderTop: `3px solid ${color}`,
                      background: `${color}0f`,
                      opacity: room.active ? 1 : 0.55,
                    }}
                    styles={{ body: { padding: 12 } }}
                  >
                    <Space direction="vertical" size={6} style={{ width: '100%' }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                        <Title level={4} style={{ margin: 0, letterSpacing: 1 }}>
                          {room.roomNo}
                        </Title>
                        <Tooltip title={st?.label ?? room.status}>
                          <span style={{ color, fontSize: 18 }}>{statusIcon(st?.icon ?? '')}</span>
                        </Tooltip>
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
                        <Text style={{ fontSize: 12, color: overLimit ? '#cf1322' : '#595959' }} strong>
                          {room.activeCards}
                          {room.maxCards === null ? '' : `/${room.maxCards}`}
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
        </Space>
      </Card>

      <Drawer
        open={active !== null}
        onClose={() => setActiveRoom(null)}
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
                <div>
                  <Text type="secondary">最大发卡数</Text>
                  <InputNumber
                    style={{ width: 150, display: 'block', marginTop: 4 }}
                    min={1}
                    max={99}
                    placeholder="不限"
                    value={editMax ?? undefined}
                    onChange={(v) => setEditMax(typeof v === 'number' ? v : null)}
                  />
                </div>
                <Button loading={savingEdit} onClick={saveParams}>
                  保存参数
                </Button>
                <Button onClick={() => setInstallOpen(true)}>制作安装卡</Button>
              </Space>
            ) : null}

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

      {/* 发行房间门卡 / 续住写卡（只从右键菜单进入） */}
      <Modal
        open={issueRoom !== null}
        title={
          issueRoom
            ? `${issueMode === 'renew' ? '续住写卡' : '发行房间门卡'} · ${issueRoom.roomNo}`
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
            defaultDays={issueMode === 'renew' ? renewDays(issueRoom) : 1}
            renewFrom={issueMode === 'renew' ? issueRoom.activeUntil : null}
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
    </>
  )
}
