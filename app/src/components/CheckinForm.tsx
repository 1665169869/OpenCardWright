import { DeleteOutlined, EditOutlined, IdcardOutlined, PlusOutlined } from '@ant-design/icons'
import { Alert, Button, Divider, Input, Modal, Popconfirm, Select, Space, Table, Tag, Typography } from 'antd'
import { useCallback, useEffect, useState } from 'react'
import dayjs from 'dayjs'
import { api, describeError, type CheckinCard, type CheckinGuest, type GuestInput } from '../api'

const { Text } = Typography

const GENDERS = [
  { value: '男', label: '男' },
  { value: '女', label: '女' },
]

const ID_TYPES = [
  { value: '身份证', label: '身份证' },
  { value: '护照', label: '护照' },
  { value: '其他', label: '其他' },
]

const emptyGuest = (): GuestInput => ({ idType: '身份证' })

/** `CheckinGuest` → 表单草稿 */
function toDraft(g: CheckinGuest): GuestInput {
  return {
    name: g.name ?? undefined,
    gender: g.gender ?? undefined,
    idType: g.idType ?? '身份证',
    idNumber: g.idNumber ?? undefined,
    nation: g.nation ?? undefined,
    address: g.address ?? undefined,
    birthDate: g.birthDate ?? undefined,
    note: g.note ?? undefined,
    idValidFrom: g.idValidFrom ?? undefined,
    idValidTo: g.idValidTo ?? undefined,
    idAuthority: g.idAuthority ?? undefined,
    deviceModel: g.deviceModel ?? undefined,
    deviceSerial: g.deviceSerial ?? undefined,
  }
}

/**
 * 入住登记表单（TD-220）。
 *
 * 与发卡解耦：可以先登记后发卡，也可以只登记不发卡。同一房间重复打开会复用
 * 已有的 open 单（后端 `create_checkin` 保证），不会重复建单。
 * 读身份证走 Rust 的 `read_id_card`（本机 9001 服务由硬件层负责）：读到即追加一位住客；
 * 读不到可以手填。前端不再自己连 WebSocket，也不需要知道厂商报文格式。
 *
 * TD-223：传入 `checkinId` 时直接加载该登记单（从日历详情进入编辑），不再新建。
 */
export default function CheckinForm({
  open,
  roomNo,
  checkinId: initialCheckinId,
  prefill,
  onClose,
  onSaved,
}: {
  open: boolean
  roomNo?: string | null
  checkinId?: number | null
  /** 预定带过来的客人信息：只在打开时填进空草稿，不覆盖用户已输入的内容 */
  prefill?: { name?: string; note?: string }
  onClose: () => void
  onSaved?: () => void
}) {
  const [room, setRoom] = useState(roomNo ?? '')
  const [checkinId, setCheckinId] = useState<number | null>(null)
  const [status, setStatus] = useState<string>('')
  const [guests, setGuests] = useState<CheckinGuest[]>([])
  /** 这张登记单关联的发卡记录（登记 → 卡的反向可见） */
  const [cards, setCards] = useState<CheckinCard[]>([])
  const [draft, setDraft] = useState<GuestInput>(emptyGuest)
  const [editingId, setEditingId] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [reading, setReading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  /** 展示的是「最近一张已关闭的历史单」（只读）——需要继续登记时点「开新单」（TD-224） */
  const [historyOnly, setHistoryOnly] = useState(false)

  const load = useCallback(async (id: number) => {
    const d = await api.getCheckin(id)
    setGuests(d?.guests ?? [])
    setCards(d?.cards ?? [])
    if (d) setStatus(d.checkin.status)
  }, [])

  /** 点「开始登记」/「开新单」：新建（或复用 open）登记单 */
  const start = useCallback(
    async (roomNoArg: string) => {
      setErr(null)
      setMsg(null)
      setBusy(true)
      try {
        const c = await api.createCheckin(roomNoArg)
        setCheckinId(c.id)
        setStatus(c.status)
        setRoom(c.roomNo)
        setHistoryOnly(false)
        await load(c.id)
        onSaved?.()
      } catch (e) {
        setErr(describeError(e))
      } finally {
        setBusy(false)
      }
    },
    [load, onSaved],
  )

  /**
   * 打开某房间时的加载顺序（TD-224）：open 单 → 最近一张历史单（只读）→ 空。
   *
   * 以前直接调 `create_checkin`，没 open 单就建空单，导致「打开看不到已登记信息」。
   * `createIfNone` 为真时（右键进入 / 点「开始登记」），该房从未登记过就直接建单。
   */
  const bootstrap = useCallback(
    async (roomNoArg: string, createIfNone: boolean) => {
      setErr(null)
      setMsg(null)
      setBusy(true)
      try {
        const openOne = await api.getOpenCheckin(roomNoArg)
        if (openOne) {
          setCheckinId(openOne.id)
          setStatus(openOne.status)
          setRoom(openOne.roomNo)
          setHistoryOnly(false)
          await load(openOne.id)
          return
        }
        const page = await api.listCheckins({ roomNo: roomNoArg, page: 1, pageSize: 1 })
        const recent = page.rows?.[0]
        if (recent) {
          setCheckinId(recent.id)
          setStatus(recent.status)
          setRoom(recent.roomNo)
          setHistoryOnly(true)
          await load(recent.id)
          setMsg('该房最近一张登记单已关闭；需要继续登记请点「开新单」')
          return
        }
        // 该房从未登记过
        setCheckinId(null)
        setStatus('')
        setGuests([])
        setHistoryOnly(false)
        if (createIfNone) {
          setBusy(false)
          await start(roomNoArg)
        }
      } catch (e) {
        setErr(describeError(e))
      } finally {
        setBusy(false)
      }
    },
    [load, start],
  )

  useEffect(() => {
    if (!open) return
    // 预定带过来的客人信息直接进草稿：此时刚清空，不会覆盖用户输入；
    // 用户后续编辑不会被这条 effect 反复改回（它只在打开/换房时跑）。
    setDraft(
      prefill?.name || prefill?.note
        ? { ...emptyGuest(), name: prefill.name, note: prefill.note }
        : emptyGuest(),
    )
    setEditingId(null)
    setErr(null)
    setMsg(null)
    setHistoryOnly(false)
    const r = (roomNo ?? '').trim()
    setRoom(r)
    // TD-223：指定了登记单就直接加载它（从日历详情进入编辑），不新建也不找最近单
    if (initialCheckinId != null) {
      setCheckinId(initialCheckinId)
      void load(initialCheckinId).catch((e) => setErr(describeError(e)))
      return
    }
    if (r) {
      void bootstrap(r, true)
    } else {
      setCheckinId(null)
      setGuests([])
      setStatus('')
    }
    // bootstrap 依赖 load / start，这里只按“打开”和预填房号触发
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, roomNo, initialCheckinId])

  const addGuest = async () => {
    if (checkinId === null) return
    if (!draft.name?.trim() && !draft.idNumber?.trim()) {
      setErr('至少填姓名或证件号')
      return
    }
    setBusy(true)
    setErr(null)
    setMsg(null)
    try {
      const res = await api.addCheckinGuest(checkinId, draft)
      setDraft(emptyGuest())
      await load(checkinId)
      setMsg(
        res.reservationConverted
          ? '已添加一位住客，并把今天的预定转为「已入住」'
          : '已添加一位住客',
      )
      onSaved?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const saveEdit = async () => {
    if (editingId === null) return
    setBusy(true)
    setErr(null)
    setMsg(null)
    try {
      await api.updateCheckinGuest(editingId, draft)
      setDraft(emptyGuest())
      setEditingId(null)
      if (checkinId !== null) await load(checkinId)
      setMsg('已保存住客资料')
      onSaved?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const removeGuest = async (id: number) => {
    setBusy(true)
    setErr(null)
    setMsg(null)
    try {
      await api.deleteCheckinGuest(id)
      if (checkinId !== null) await load(checkinId)
      if (editingId === id) {
        setEditingId(null)
        setDraft(emptyGuest())
      }
      onSaved?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const readAndAdd = async () => {
    if (checkinId === null) return
    setReading(true)
    setErr(null)
    setMsg(null)
    try {
      const info = await api.readIdCard()
      // TD-224：读到空结果（只有设备型号之类）时不要落一条空白住客
      const hasName = !!info.name?.trim()
      const hasId = !!info.idNumber?.trim()
      if (!hasName && !hasId) {
        setErr('身份证读出内容为空（没有姓名也没有证件号），未写入住客；请重新放证或改用手填')
        return
      }
      // 硬件层给的是 `string | null`；表单要 `string | undefined`——空值一律当没填，
      // 免得把字面量 "null" 写进数据库。
      const guest: GuestInput = {
        idType: '身份证',
        name: info.name ?? undefined,
        gender: info.gender ?? undefined,
        nation: info.nation ?? undefined,
        birthDate: info.birthDate ?? undefined,
        address: info.address ?? undefined,
        idNumber: info.idNumber ?? undefined,
        idValidFrom: info.idValidFrom ?? undefined,
        idValidTo: info.idValidTo ?? undefined,
        idAuthority: info.idAuthority ?? undefined,
        deviceModel: info.deviceModel ?? undefined,
        deviceSerial: info.deviceSerial ?? undefined,
      }
      const res = await api.addCheckinGuest(checkinId, guest)
      await load(checkinId)
      const who = info.name ?? info.idNumber ?? '一位住客'
      setMsg(
        res.reservationConverted
          ? `已读卡并添加：${who}；今天的预定已转为「已入住」`
          : `已读卡并添加：${who}`,
      )
      onSaved?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setReading(false)
    }
  }

  const closeCheckin = async () => {
    if (checkinId === null) return
    setBusy(true)
    setErr(null)
    try {
      await api.closeCheckin(checkinId)
      setStatus('closed')
      onSaved?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const editing = editingId !== null
  const openState = status === 'open'

  return (
    <Modal
      open={open}
      title={room ? `入住登记 · ${room} 房` : '入住登记'}
      width={820}
      onCancel={onClose}
      footer={
        <Space>
          {openState ? (
            // 空单（还没有住客）不允许关闭：关掉也没任何信息可留档，只会把该房的 open 单搞乱。
            <Popconfirm
              title="关闭这张登记单？"
              description="关闭后不能再加住客；该房需要重新登记时要点「开新单」。"
              okText="关闭登记单"
              okButtonProps={{ danger: true }}
              cancelText="取消"
              onConfirm={() => void closeCheckin()}
              disabled={guests.length === 0}
            >
              <Button
                danger
                loading={busy}
                disabled={guests.length === 0}
                title={guests.length === 0 ? '还没有登记住客，不能关闭空单' : undefined}
              >
                关闭登记单
              </Button>
            </Popconfirm>
          ) : null}
          <Button onClick={onClose}>关闭</Button>
        </Space>
      }
      destroyOnHidden
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        {!roomNo ? (
          <Space>
            <Input
              placeholder="房号"
              value={room}
              onChange={(e) => setRoom(e.target.value)}
              style={{ width: 160 }}
            />
            <Button
              type="primary"
              loading={busy}
              disabled={!room.trim()}
              onClick={() => void bootstrap(room.trim(), true)}
            >
              开始登记
            </Button>
          </Space>
        ) : null}

        {checkinId !== null ? (
          <Space size="middle" wrap>
            <Tag color={openState ? 'green' : 'default'}>{openState ? '登记中' : '已关闭'}</Tag>
            <Text type="secondary" style={{ fontSize: 12 }}>
              共 {guests.length} 位住客；发卡时会自动关联这张登记单
            </Text>
            {historyOnly ? (
              <Button size="small" type="primary" loading={busy} onClick={() => void start(room)}>
                开新单
              </Button>
            ) : null}
          </Space>
        ) : null}

        {prefill?.name || prefill?.note ? (
          <Alert
            type="info"
            showIcon
            message={`已按预定带入客人信息${prefill.name ? `（${prefill.name}）` : ''}，请补证件后点「添加住客」`}
          />
        ) : null}

        {err ? <Alert type="error" showIcon message="操作失败" description={err} /> : null}
        {msg ? <Alert type="success" showIcon message={msg} /> : null}

        {checkinId !== null ? (
          <>
            <Space wrap>
              <Button
                icon={<IdcardOutlined />}
                loading={reading}
                disabled={!openState}
                onClick={() => void readAndAdd()}
              >
                读取身份证并追加
              </Button>
              <Text type="secondary" style={{ fontSize: 12 }}>
                读一张自动加一位；读不到可以在下面手填
              </Text>
            </Space>

            <Table<CheckinGuest>
              size="small"
              rowKey="id"
              dataSource={guests}
              pagination={false}
              locale={{ emptyText: '还没有登记住客' }}
              columns={[
                { title: '#', dataIndex: 'seq', width: 46 },
                {
                  title: '姓名',
                  dataIndex: 'name',
                  width: 110,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '性别',
                  dataIndex: 'gender',
                  width: 66,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '证件号',
                  dataIndex: 'idNumber',
                  width: 200,
                  render: (v: string | null) => v ?? '—',
                },
                {
                  title: '有效期',
                  width: 170,
                  render: (_: unknown, g: CheckinGuest) =>
                    g.idValidFrom || g.idValidTo ? `${g.idValidFrom ?? '—'} ~ ${g.idValidTo ?? '—'}` : '—',
                },
                {
                  title: '操作',
                  width: 120,
                  render: (_: unknown, g: CheckinGuest) => (
                    <Space size={4}>
                      <Button
                        size="small"
                        type="text"
                        icon={<EditOutlined />}
                        disabled={!openState}
                        onClick={() => {
                          setEditingId(g.id)
                          setDraft(toDraft(g))
                          setMsg(null)
                        }}
                      />
                      <Button
                        size="small"
                        type="text"
                        danger
                        icon={<DeleteOutlined />}
                        disabled={!openState}
                        onClick={() => void removeGuest(g.id)}
                      />
                    </Space>
                  ),
                },
              ]}
            />

            <Divider style={{ margin: '8px 0' }}>关联的卡</Divider>
            {cards.length === 0 ? (
              <Text type="secondary" style={{ fontSize: 12 }}>
                还没有关联的发卡记录；发卡时会自动挂到本房登记中的单上。
              </Text>
            ) : (
              <Space direction="vertical" size={4} style={{ width: '100%' }}>
                {cards.map((c) => {
                  const closed = !!c.checkedOutAt || !!c.invalidatedAt
                  // 界面只认 10 位作业流水号：不要再用十六进制卡号，看不出日期
                  const no = c.workNo ?? '无流水号'
                  // 到期按本地时区格式化；凌晨房归属日 + 退房钟点已经在后端算好
                  const until = c.expiresAt
                    ? dayjs(c.expiresAt).format('YYYY年M月D日 HH:mm')
                    : '—'
                  return (
                    <Text key={c.id} style={{ fontSize: 12 }}>
                      <Tag color={closed ? 'default' : 'green'}>{closed ? '已退/作废' : '在用'}</Tag>
                      {c.kind} · 作业流水号 {no} · {until}
                    </Text>
                  )
                })}
              </Space>
            )}

            <Divider style={{ margin: '8px 0' }}>手填住客</Divider>

            <Space wrap>
              <Input
                placeholder="姓名"
                value={draft.name ?? ''}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                style={{ width: 120 }}
                disabled={!openState}
              />
              <Select
                placeholder="性别"
                value={draft.gender}
                onChange={(v) => setDraft({ ...draft, gender: v })}
                allowClear
                style={{ width: 88 }}
                options={GENDERS}
                disabled={!openState}
              />
              <Input
                placeholder="民族"
                value={draft.nation ?? ''}
                onChange={(e) => setDraft({ ...draft, nation: e.target.value })}
                style={{ width: 100 }}
                disabled={!openState}
              />
              <Input
                placeholder="出生日期 YYYY-MM-DD"
                value={draft.birthDate ?? ''}
                onChange={(e) => setDraft({ ...draft, birthDate: e.target.value })}
                style={{ width: 160 }}
                disabled={!openState}
              />
              <Select
                placeholder="证件类型"
                value={draft.idType}
                onChange={(v) => setDraft({ ...draft, idType: v })}
                style={{ width: 110 }}
                options={ID_TYPES}
                disabled={!openState}
              />
              <Input
                placeholder="证件号"
                value={draft.idNumber ?? ''}
                onChange={(e) => setDraft({ ...draft, idNumber: e.target.value })}
                style={{ width: 200 }}
                disabled={!openState}
              />
              <Input
                placeholder="住址"
                value={draft.address ?? ''}
                onChange={(e) => setDraft({ ...draft, address: e.target.value })}
                style={{ width: 240 }}
                disabled={!openState}
              />
              <Input
                placeholder="备注"
                value={draft.note ?? ''}
                onChange={(e) => setDraft({ ...draft, note: e.target.value })}
                style={{ width: 160 }}
                disabled={!openState}
              />
              {editing ? (
                <Space>
                  <Button type="primary" loading={busy} onClick={() => void saveEdit()}>
                    保存修改
                  </Button>
                  <Button
                    onClick={() => {
                      setEditingId(null)
                      setDraft(emptyGuest())
                    }}
                  >
                    取消编辑
                  </Button>
                </Space>
              ) : (
                <Button
                  type="primary"
                  icon={<PlusOutlined />}
                  loading={busy}
                  disabled={!openState || busy}
                  onClick={() => void addGuest()}
                >
                  添加住客
                </Button>
              )}
            </Space>
          </>
        ) : (
          <Text type="secondary">选择房间后开始登记。</Text>
        )}
      </Space>
    </Modal>
  )
}
