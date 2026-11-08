import { useEffect, useState } from 'react'
import { Alert, Button, Modal, Space, Table, Typography } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import {
  api,
  describeError,
  formatTime,
  type IssuanceRow,
  type Room,
} from '../api'

const { Text } = Typography

/** 无卡退房选卡弹窗（TD-121）：列出该房全部有效客卡，**多选**后只退选中的。
 *  默认不勾选（防误退）；确认后循环 checkout_card（按卡号退，后端零改动）。
 *  0 张有效卡时列表空态、按钮禁用。 */
export default function NoCardCheckoutModal({
  open,
  room,
  onClose,
  onDone,
}: {
  open: boolean
  /** 目标房间；null 时不渲染内容 */
  room: Room | null
  onClose: () => void
  /** 全部选中卡退成功后的回调（含剩余张数提示） */
  onDone: (msg: string) => void
}) {
  const [rows, setRows] = useState<IssuanceRow[]>([])
  /** 选中的**记录 id**（rowKey = id —— 同号多张各占一行，不能再用卡号当 key）；初始为空 = 默认不勾选 */
  const [selected, setSelected] = useState<number[]>([])
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const roomNo = room?.roomNo ?? ''

  /** 按房拉有效客卡（与抽屉同口径）：result='ok'、有卡号、未退房、未过期、日租/钟点；
   *  按「张」逐条列出（TD-127）——同号多张各占一行；退房按卡号执行，同号会一起关。 */
  const load = async () => {
    if (!roomNo) return
    setLoading(true)
    setErr(null)
    try {
      const p = await api.listIssuances({ roomNo, page: 1, pageSize: 200 })
      const now = Date.now()
      const valid = p.rows.filter(
        (r) =>
          r.result === 'ok' &&
          r.checkedOutAt === null &&
          !!r.expiresAt &&
          Date.parse(r.expiresAt) > now &&
          (r.kind === '日租客卡' || r.kind === '钟点客卡'),
      )
      setRows(valid.sort((a, b) => a.createdAt.localeCompare(b.createdAt) || a.id - b.id))
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setLoading(false)
    }
  }

  // 打开时拉数据并清空选择；切换房间重新拉
  useEffect(() => {
    if (!open) return
    setSelected([])
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, roomNo])

  const submit = async () => {
    if (!room || selected.length === 0) return
    setBusy(true)
    setErr(null)
    // 选中行按发卡记录 id 逐条退房；每张卡一条记录，不再按卡号去重。
    const selectedRows = rows.filter((r) => selected.includes(r.id))
    const done: number[] = []
    const failed: string[] = []
    let remaining: number | null = null
    for (const [i, row] of selectedRows.entries()) {
      try {
        // 无卡退房：用户手上没有卡，跳过读卡器（不读卡、不擦卡），只做数据库标记
        const res = await api.checkoutCard({ issuanceId: row.id, noDevice: true })
        done.push(row.id)
        remaining = res.remaining
      } catch (e) {
        failed.push(`第 ${i + 1} 张：${describeError(e)}`)
      }
    }
    setBusy(false)

    if (failed.length === 0) {
      onDone(`${room.roomNo} 已退 ${selectedRows.length} 张有效卡，剩余 ${remaining ?? 0} 张`)
      onClose()
      return
    }
    // 部分失败：留在弹窗，重拉列表并清掉已退记录，失败原因列出
    setErr(
      `退房部分失败（成功 ${done.length} 张卡、失败 ${failed.length} 张卡）：${failed.join('；')}`,
    )
    setSelected((prev) => {
      const gone = new Set(done)
      return prev.filter((id) => !gone.has(id))
    })
    await load()
  }

  const columns: ColumnsType<IssuanceRow> = [
    { title: '卡型', dataIndex: 'kind', width: 120 },
    {
      title: '时长',
      width: 90,
      render: (_: unknown, r: IssuanceRow) =>
        r.days > 0 ? `${r.days} 天` : `${r.hours} 小时`,
    },
    {
      title: '到期',
      dataIndex: 'expiresAt',
      width: 160,
      render: (v: string | null) => (v ? formatTime(v) : '—'),
    },
    {
      title: '客人',
      dataIndex: 'guestName',
      width: 100,
      render: (v: string | null) => v ?? '—',
    },
  ]

  return (
    <Modal
      open={open}
      title={`无卡退房 · ${roomNo}`}
      footer={null}
      onCancel={() => {
        if (!busy) onClose()
      }}
      width={640}
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <Text>
          {roomNo} 当前有 <b>{rows.length}</b> 张有效卡，勾选要退的卡（可多选），只退选中的。
        </Text>
        <Text type="secondary" style={{ fontSize: 12 }}>
          同卡号的多张卡（客人要两张的那批）会一起退房——物理上是同一张逻辑卡，锁按卡号识别。
        </Text>

        {err ? <Alert type="error" showIcon message="退房失败" description={err} /> : null}

        <Table<IssuanceRow>
          size="small"
          rowKey="id"
          loading={loading}
          dataSource={rows}
          locale={{ emptyText: '该房没有有效客卡' }}
          pagination={false}
          rowSelection={{
            selectedRowKeys: selected,
            onChange: (keys) => setSelected(keys as number[]),
          }}
          columns={columns}
        />

        <Space wrap>
          <Button
            type="primary"
            danger
            loading={busy}
            disabled={selected.length === 0}
            onClick={() => void submit()}
          >
            退选中的卡（{selected.length}）
          </Button>
          <Button disabled={busy} onClick={onClose}>
            取消
          </Button>
        </Space>
      </Space>
    </Modal>
  )
}
