import { useCallback, useEffect, useRef, useState } from 'react'
import {
  Alert,
  Button,
  Card,
  ColorPicker,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import {
  api,
  describeError,
  formatTime,
  type BackupEntry,
  type BackupSettings,
  type Department,
  type OperatorInfo,
  type ReminderSettings,
  type Role,
  type RoomType,
} from '../api'
import { tryPreview } from '../utils/sound'
import PasswordRules from '../components/PasswordRules'
import DevicePanel from './DevicePanel'
import RoomsPanel from './RoomsPanel'
import { confirmIssue, passwordIssues } from '../utils/password'
import { loginIssue, textIssue } from '../utils/validate'
import { ICON_CHOICES, statusIcon } from '../components/StatusIcon'
import type { RoomStatus } from '../api'

const { Text, Paragraph } = Typography

export default function Settings({ onChanged }: { onChanged?: () => void }) {
  return (
    <Card title="设置">
      <Tabs
        items={[
          // 设备曾是顶级页签，现在收进设置里做第一个二级页签
          { key: 'device', label: '设备', children: <DevicePanel onChanged={onChanged} /> },
          { key: 'reminder', label: '到期提醒', children: <ReminderPanel onChanged={onChanged} /> },
          { key: 'checkout', label: '退房钟点', children: <CheckoutHourPanel onChanged={onChanged} /> },
          { key: 'departments', label: '部门', children: <DepartmentPanel onChanged={onChanged} /> },
          { key: 'operators', label: '用户', children: <OperatorPanel onChanged={onChanged} /> },
          { key: 'roomtypes', label: '房型', children: <RoomTypePanel onChanged={onChanged} /> },
          { key: 'roomstatus', label: '房态', children: <RoomStatusPanel onChanged={onChanged} /> },
          { key: 'rooms', label: '房间', children: <RoomsPanel onChanged={onChanged} /> },
          { key: 'backup', label: '备份', children: <BackupPanel onChanged={onChanged} /> },
        ]}
      />
    </Card>
  )
}

// ---------------------------------------------------------------- 提醒

function ReminderPanel({ onChanged }: { onChanged?: () => void }) {
  const [s, setS] = useState<ReminderSettings | null>(null)
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    api
      .getReminderSettings()
      .then(setS)
      .catch((e: unknown) => setErr(describeError(e)))
  }, [])

  const save = async () => {
    if (!s) return
    setBusy(true)
    setErr(null)
    setMsg(null)
    try {
      setS(await api.setReminderSettings(s.enabled, s.minutes, s.sound, s.voice))
      setMsg('已保存，下一条到期提醒会按新设置触发')
      // 必须通知 App 重新拉设置：轮询用的 minutes 在 App 里，不刷新就还是旧值
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  if (!s) return <Text type="secondary">加载中…</Text>

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space size="large" wrap>
        <Space>
          <Switch checked={s.enabled} onChange={(v) => setS({ ...s, enabled: v })} />
          <Text>启用到期提醒</Text>
        </Space>
        <Space>
          <Text>提前</Text>
          <InputNumber
            min={1}
            max={1440}
            value={s.minutes}
            onChange={(v) => setS({ ...s, minutes: typeof v === 'number' ? v : 30 })}
          />
          <Text>分钟开始提醒</Text>
        </Space>
        <Space>
          <Switch checked={s.sound} onChange={(v) => setS({ ...s, sound: v })} />
          <Text>提示声</Text>
        </Space>
        <Space>
          <Switch checked={s.voice} onChange={(v) => setS({ ...s, voice: v })} />
          <Text>语音播报（音色随系统，无中文语音时自动退回提示声）</Text>
        </Space>
      </Space>

      <Space>
        <Button type="primary" loading={busy} onClick={save}>
          保存
        </Button>
        <Button onClick={() => setMsg(tryPreview() ? '已播放提示音' : '当前环境不支持音频，只会弹窗')}>
          试听
        </Button>
      </Space>

      {msg ? <Alert type="success" showIcon message={msg} /> : null}
      {err ? <Alert type="error" showIcon message={err} /> : null}

      <Paragraph type="secondary" style={{ marginBottom: 0 }}>
        提醒基于**发卡时间**计算。卡内只有时长码、没有绝对到期时间，而锁实际是从"首次刷卡"起算，
        所以提醒可能比真实到期偏早；长于 62/63 天的月码卡本身到期日也是近似值。
      </Paragraph>
    </Space>
  )
}

// ---------------------------------------------------------------- 退房钟点（TD-126）

function CheckoutHourPanel({ onChanged }: { onChanged?: () => void }) {
  const [hour, setHour] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    api
      .getCheckoutHour()
      .then(setHour)
      .catch((e: unknown) => setErr(describeError(e)))
  }, [])

  const save = async () => {
    if (hour === null) return
    setBusy(true)
    setErr(null)
    setMsg(null)
    try {
      setHour(await api.setCheckoutHour(hour))
      setMsg('已保存；发卡页与日租卡到期时间将按此钟点计算')
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  if (hour === null) return <Text type="secondary">加载中…</Text>

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Text>退房钟点</Text>
        <InputNumber
          min={0}
          max={23}
          value={hour}
          onChange={(v) => setHour(typeof v === 'number' ? v : 12)}
        />
        <Text type="secondary">点（0–23，默认 12）</Text>
        <Button type="primary" loading={busy} onClick={() => void save()}>
          保存
        </Button>
      </Space>
      <Text type="secondary" style={{ fontSize: 12 }}>
        影响：发卡页「预计退房时刻」展示、日租卡写库到期时间（退房日的该钟点）、管理卡「退房
        钟点卡」发卡取值。
      </Text>
      {msg ? <Text type="success">{msg}</Text> : null}
      {err ? <Text type="danger">{err}</Text> : null}
    </Space>
  )
}

// ---------------------------------------------------------------- 部门

function DepartmentPanel({ onChanged }: { onChanged?: () => void }) {
  const [list, setList] = useState<Department[]>([])
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<Department | null>(null)
  const [renameText, setRenameText] = useState('')
  const [renameErr, setRenameErr] = useState<string | null>(null)
  const [nameErr, setNameErr] = useState<string | null>(null)

  const reload = useCallback(async () => {
    try {
      setList(await api.listDepartments())
    } catch (e) {
      setErr(describeError(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const create = async () => {
    const issue = textIssue(name, '部门名称', 1, 32)
    setNameErr(issue)
    if (issue) return
    setBusy(true)
    setErr(null)
    try {
      await api.createDepartment(name.trim())
      setName('')
      setMsg('部门已创建')
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const toggle = async (d: Department) => {
    try {
      await api.setDepartmentActive(d.id, !d.active)
      await reload()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  const doRename = async () => {
    if (!renaming) return
    const issue = textIssue(renameText, '部门名称', 1, 32)
    setRenameErr(issue)
    if (issue) return
    try {
      await api.renameDepartment(renaming.id, renameText.trim())
      setRenaming(null)
      setRenameErr(null)
      await reload()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Input
          placeholder="部门名称，例如 客房部"
          value={name}
          onChange={(e) => {
            setName(e.target.value)
            if (nameErr) setNameErr(null)
          }}
          onPressEnter={create}
          style={{ width: 240 }}
          status={nameErr ? 'error' : undefined}
        />
        <Button type="primary" loading={busy} onClick={create}>
          新增部门
        </Button>
        {nameErr ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {nameErr}
          </Text>
        ) : null}
      </Space>

      {msg ? <Alert type="success" showIcon message={msg} /> : null}
      {err ? <Alert type="error" showIcon message={err} /> : null}

      <Table<Department>
        rowKey="id"
        size="small"
        dataSource={list}
        pagination={false}
        columns={[
          { title: '部门', dataIndex: 'name' },
          { title: '人数', dataIndex: 'memberCount', width: 80 },
          {
            title: '状态',
            dataIndex: 'active',
            width: 100,
            render: (v: boolean) => (v ? <Tag color="green">启用</Tag> : <Tag>停用</Tag>),
          },
          { title: '创建', dataIndex: 'createdAt', render: (v: string) => formatTime(v) },
          {
            title: '操作',
            width: 160,
            render: (_: unknown, d: Department) => (
              <Space>
                <Button
                  size="small"
                  onClick={() => {
                    setRenaming(d)
                    setRenameText(d.name)
                  }}
                >
                  改名
                </Button>
                <Button size="small" onClick={() => toggle(d)}>
                  {d.active ? '停用' : '启用'}
                </Button>
              </Space>
            ),
          },
        ]}
      />

      <Modal
        open={renaming !== null}
        title="部门改名"
        okText="保存"
        cancelText="取消"
        onOk={doRename}
        onCancel={() => {
          setRenaming(null)
          setRenameErr(null)
        }}
      >
        <Input
          value={renameText}
          onChange={(e) => {
            setRenameText(e.target.value)
            if (renameErr) setRenameErr(null)
          }}
          status={renameErr ? 'error' : undefined}
        />
        {renameErr ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {renameErr}
          </Text>
        ) : null}
      </Modal>
    </Space>
  )
}

// ---------------------------------------------------------------- 用户

function OperatorPanel({ onChanged }: { onChanged?: () => void }) {
  const [list, setList] = useState<OperatorInfo[]>([])
  const [departments, setDepartments] = useState<Department[]>([])
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)

  const [creating, setCreating] = useState(false)
  const [login, setLogin] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [role, setRole] = useState<Role>('frontdesk')
  const [departmentId, setDepartmentId] = useState<number | null>(null)
  const [password, setPassword] = useState('')
  const [password2, setPassword2] = useState('')
  // 字段级错误：只在点过提交后显示
  const [fieldErr, setFieldErr] = useState<{ login?: string; name?: string; pw?: string[]; confirm?: string }>({})

  const [resetting, setResetting] = useState<OperatorInfo | null>(null)
  const [newPassword, setNewPassword] = useState('')
  const [newPassword2, setNewPassword2] = useState('')
  const [resetErr, setResetErr] = useState<{ pw?: string[]; confirm?: string }>({})

  const reload = useCallback(async () => {
    try {
      const [ops, deps] = await Promise.all([api.listOperators(), api.listDepartments()])
      setList(ops)
      setDepartments(deps)
    } catch (e) {
      setErr(describeError(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const create = async () => {
    setErr(null)
    const loginErr = loginIssue(login) ?? undefined
    const nameErr = displayName.trim() ? (textIssue(displayName, '显示名', 1, 32) ?? undefined) : undefined
    const pwErrs = passwordIssues(password, login)
    const cfErr = confirmIssue(password, password2) ?? undefined
    setFieldErr({ login: loginErr, name: nameErr, pw: pwErrs, confirm: cfErr })
    if (loginErr || nameErr || pwErrs.length > 0 || cfErr) {
      return
    }
    try {
      await api.createOperator(login.trim(), displayName.trim() || login.trim(), role, departmentId, password)
      setCreating(false)
      setLogin('')
      setDisplayName('')
      setPassword('')
      setPassword2('')
      setDepartmentId(null)
      setRole('frontdesk')
      setFieldErr({})
      setMsg('用户已创建')
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  const doReset = async () => {
    if (!resetting) return
    const pwErrs = passwordIssues(newPassword, resetting.login)
    const cfErr = confirmIssue(newPassword, newPassword2) ?? undefined
    setResetErr({ pw: pwErrs, confirm: cfErr })
    if (pwErrs.length > 0 || cfErr) {
      return
    }
    try {
      await api.resetOperatorPassword(resetting.id, newPassword)
      setResetting(null)
      setNewPassword('')
      setNewPassword2('')
      setResetErr({})
      setMsg('密码已重置，请通知该用户')
    } catch (e) {
      setErr(describeError(e))
    }
  }

  const toggle = async (o: OperatorInfo) => {
    try {
      await api.setOperatorActive(o.id, !o.active)
      await reload()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Button type="primary" onClick={() => setCreating(true)}>
          新增用户
        </Button>
        <Text type="secondary">前台只能读卡、发客卡；管理卡、房间、设备与设置需要管理员。</Text>
      </Space>

      {msg ? <Alert type="success" showIcon message={msg} /> : null}
      {err ? <Alert type="error" showIcon message={err} /> : null}

      <Table<OperatorInfo>
        rowKey="id"
        size="small"
        dataSource={list}
        pagination={false}
        columns={[
          { title: '账号', dataIndex: 'login', width: 140 },
          { title: '显示名', dataIndex: 'displayName', width: 140 },
          {
            title: '角色',
            dataIndex: 'role',
            width: 100,
            render: (v: Role) => (v === 'admin' ? <Tag color="blue">管理员</Tag> : <Tag>前台</Tag>),
          },
          { title: '部门', dataIndex: 'departmentName', render: (v: string | null) => v ?? '—' },
          {
            title: '状态',
            dataIndex: 'active',
            width: 90,
            render: (v: boolean) => (v ? <Tag color="green">启用</Tag> : <Tag>停用</Tag>),
          },
          { title: '创建', dataIndex: 'createdAt', render: (v: string) => formatTime(v) },
          {
            title: '操作',
            width: 180,
            render: (_: unknown, o: OperatorInfo) => (
              <Space>
                <Button
                  size="small"
                  onClick={() => {
                    setResetting(o)
                    setNewPassword('')
                  }}
                >
                  重置密码
                </Button>
                <Button size="small" onClick={() => toggle(o)}>
                  {o.active ? '停用' : '启用'}
                </Button>
              </Space>
            ),
          },
        ]}
      />

      <Modal
        open={creating}
        title="新增用户"
        okText="创建"
        cancelText="取消"
        onOk={create}
        onCancel={() => setCreating(false)}
      >
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <div>
            <Input
              placeholder="登录账号（3–32 位，字母/数字/下划线）"
              value={login}
              onChange={(e) => setLogin(e.target.value)}
              status={fieldErr.login ? 'error' : undefined}
            />
            {fieldErr.login ? (
              <Text type="danger" style={{ fontSize: 12 }}>
                {fieldErr.login}
              </Text>
            ) : null}
          </div>
          <div>
            <Input
              placeholder="显示名（留空就用登录名）"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              status={fieldErr.name ? 'error' : undefined}
            />
            {fieldErr.name ? (
              <Text type="danger" style={{ fontSize: 12 }}>
                {fieldErr.name}
              </Text>
            ) : null}
          </div>
          <Select
            style={{ width: '100%' }}
            value={role}
            onChange={(v) => setRole(v as Role)}
            options={[
              { value: 'frontdesk', label: '前台（读卡、发客卡）' },
              { value: 'admin', label: '管理员（全部权限）' },
            ]}
          />
          <Select
            style={{ width: '100%' }}
            allowClear
            placeholder="所属部门（可留空）"
            value={departmentId ?? undefined}
            onChange={(v) => setDepartmentId(typeof v === 'number' ? v : null)}
            options={departments.filter((d) => d.active).map((d) => ({ value: d.id, label: d.name }))}
          />
          <div>
            <Input.Password
              placeholder="初始密码"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              status={fieldErr.pw && fieldErr.pw.length > 0 ? 'error' : undefined}
            />
            <PasswordRules issues={fieldErr.pw ?? []} />
          </div>
          <div>
            <Input.Password
              placeholder="再输一次初始密码"
              value={password2}
              onChange={(e) => setPassword2(e.target.value)}
              status={fieldErr.confirm ? 'error' : undefined}
            />
            {fieldErr.confirm ? (
              <Text type="danger" style={{ fontSize: 12 }}>
                {fieldErr.confirm}
              </Text>
            ) : null}
          </div>
        </Space>
      </Modal>

      <Modal
        open={resetting !== null}
        title={resetting ? `重置 ${resetting.login} 的密码` : ''}
        okText="重置"
        cancelText="取消"
        onOk={doReset}
        onCancel={() => {
          setResetting(null)
          setNewPassword('')
          setNewPassword2('')
          setResetErr({})
        }}
      >
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <div>
            <Input.Password
              placeholder="新密码"
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              status={resetErr.pw && resetErr.pw.length > 0 ? 'error' : undefined}
            />
            <PasswordRules issues={resetErr.pw ?? []} />
          </div>
          <div>
            <Input.Password
              placeholder="再输一次新密码"
              value={newPassword2}
              onChange={(e) => setNewPassword2(e.target.value)}
              status={resetErr.confirm ? 'error' : undefined}
            />
            {resetErr.confirm ? (
              <Text type="danger" style={{ fontSize: 12 }}>
                {resetErr.confirm}
              </Text>
            ) : null}
          </div>
        </Space>
      </Modal>
    </Space>
  )
}

// ---------------------------------------------------------------- 房型

function RoomTypePanel({ onChanged }: { onChanged?: () => void }) {
  const [list, setList] = useState<RoomType[]>([])
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<RoomType | null>(null)
  const [renameText, setRenameText] = useState('')
  const [renameErr, setRenameErr] = useState<string | null>(null)
  const [nameErr, setNameErr] = useState<string | null>(null)

  const reload = useCallback(async () => {
    try {
      setList(await api.listRoomTypes())
    } catch (e) {
      setErr(describeError(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const create = async () => {
    const issue = textIssue(name, '房型名称', 1, 32)
    setNameErr(issue)
    if (issue) return
    setBusy(true)
    setErr(null)
    try {
      await api.createRoomType(name.trim())
      setName('')
      setMsg('房型已创建，可到房间页给房间分配')
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const doRename = async () => {
    if (!renaming) return
    const issue = textIssue(renameText, '房型名称', 1, 32)
    setRenameErr(issue)
    if (issue) return
    try {
      await api.renameRoomType(renaming.id, renameText.trim())
      setRenaming(null)
      setRenameErr(null)
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  const remove = async (t: RoomType) => {
    try {
      await api.deleteRoomType(t.id)
      setMsg(`已删除房型 ${t.name}`)
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  /** 房型标签颜色：选完立即保存，房间卡片上的标签马上跟着变色。 */
  const saveColor = async (t: RoomType, color: string | null) => {
    setErr(null)
    try {
      await api.setRoomTypeColor(t.id, color)
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Input
          placeholder="房型名称，例如 大床房"
          value={name}
          onChange={(e) => {
            setName(e.target.value)
            if (nameErr) setNameErr(null)
          }}
          onPressEnter={create}
          style={{ width: 240 }}
          status={nameErr ? 'error' : undefined}
        />
        <Button type="primary" loading={busy} onClick={create}>
          新增房型
        </Button>
        {nameErr ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {nameErr}
          </Text>
        ) : null}
      </Space>

      {msg ? <Alert type="success" showIcon message={msg} /> : null}
      {err ? <Alert type="error" showIcon message={err} /> : null}

      <Table<RoomType>
        rowKey="id"
        size="small"
        dataSource={list}
        pagination={false}
        columns={[
          { title: '房型', dataIndex: 'name' },
          { title: '房间数', dataIndex: 'roomCount', width: 90 },
          {
            title: '标签颜色',
            width: 170,
            render: (_: unknown, t: RoomType) => (
              <ColorPicker
                value={t.color ?? '#8c8c8c'}
                showText
                allowClear
                onChangeComplete={(c) => void saveColor(t, c.toHexString())}
                onClear={() => void saveColor(t, null)}
              />
            ),
          },
          { title: '创建', dataIndex: 'createdAt', render: (v: string) => formatTime(v) },
          {
            title: '操作',
            width: 160,
            render: (_: unknown, t: RoomType) => (
              <Space>
                <Button
                  size="small"
                  onClick={() => {
                    setRenaming(t)
                    setRenameText(t.name)
                  }}
                >
                  改名
                </Button>
                <Button size="small" danger disabled={t.roomCount > 0} onClick={() => remove(t)}>
                  删除
                </Button>
              </Space>
            ),
          },
        ]}
      />
      <Text type="secondary">还有房间在用的房型不能删，先把房间改到别的房型。</Text>

      <Modal
        open={renaming !== null}
        title="房型改名"
        okText="保存"
        cancelText="取消"
        onOk={doRename}
        onCancel={() => {
          setRenaming(null)
          setRenameErr(null)
        }}
      >
        <Input
          value={renameText}
          onChange={(e) => {
            setRenameText(e.target.value)
            if (renameErr) setRenameErr(null)
          }}
          status={renameErr ? 'error' : undefined}
        />
        {renameErr ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {renameErr}
          </Text>
        ) : null}
      </Modal>
    </Space>
  )
}

// ---------------------------------------------------------------- 房态

/**
 * 房态颜色/图标自定义。
 *
 * key 是固定用途（空净/空脏/预定/未满/已满/维修/其他），只允许改图标与颜色；
 * 房间格子、房态筛选、退房流程都按这里的配置渲染。
 */
function RoomStatusPanel({ onChanged }: { onChanged?: () => void }) {
  const [list, setList] = useState<RoomStatus[]>([])
  const [draft, setDraft] = useState<Record<string, { icon: string; color: string }>>({})
  const [checkoutStatus, setCheckoutStatus] = useState<string>('vacant_dirty')
  const [err, setErr] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)

  const reload = useCallback(async () => {
    try {
      const [s, cs] = await Promise.all([api.listRoomStatuses(), api.getCheckoutStatus()])
      setList(s)
      setDraft(Object.fromEntries(s.map((x) => [x.key, { icon: x.icon, color: x.color }])))
      setCheckoutStatus(cs)
    } catch (e) {
      setErr(describeError(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  /** 退房后自动转成的房态：选完立刻保存。 */
  const saveCheckoutStatus = async (key: string) => {
    setErr(null)
    try {
      await api.setCheckoutStatus(key)
      setCheckoutStatus(key)
      const label = list.find((s) => s.key === key)?.label ?? key
      setMsg(`退房后房态已设为「${label}」`)
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  const patch = (key: string, next: Partial<{ icon: string; color: string }>) => {
    setDraft((prev) => ({ ...prev, [key]: { ...prev[key], ...next } }))
  }

  const save = async (s: RoomStatus) => {
    const d = draft[s.key]
    if (!d) return
    setErr(null)
    try {
      await api.updateRoomStatusStyle(s.key, d.icon, d.color)
      setMsg(`${s.label} 的图标/颜色已保存`)
      await reload()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {msg ? <Alert type="success" showIcon message={msg} /> : null}
      {err ? <Alert type="error" showIcon message={err} /> : null}

      <Space wrap>
        <Text>退房后房态</Text>
        <Select
          style={{ width: 160 }}
          value={checkoutStatus}
          onChange={(v) => void saveCheckoutStatus(v)}
          options={list.map((s) => ({
            value: s.key,
            label: (
              <Space size={6}>
                <span style={{ color: s.color }}>{statusIcon(s.icon)}</span>
                <span>{s.label}</span>
              </Space>
            ),
          }))}
        />
        <Text type="secondary" style={{ fontSize: 12 }}>
          读卡退房 / 持卡退房 / 无卡退房完成后，房间自动转成这个房态
        </Text>
      </Space>

      <Table<RoomStatus>
        rowKey="key"
        size="small"
        dataSource={list}
        pagination={false}
        columns={[
          {
            title: '房态',
            dataIndex: 'label',
            width: 120,
            render: (label: string, s: RoomStatus) => (
              <Tag color={draft[s.key]?.color ?? s.color} icon={statusIcon(draft[s.key]?.icon ?? s.icon)}>
                {label}
              </Tag>
            ),
          },
          {
            title: '图标',
            width: 200,
            render: (_: unknown, s: RoomStatus) => (
              <Select
                size="small"
                style={{ width: 180 }}
                value={draft[s.key]?.icon ?? s.icon}
                onChange={(v) => patch(s.key, { icon: v })}
                options={ICON_CHOICES.map((name) => ({
                  value: name,
                  label: (
                    <Space>
                      {statusIcon(name)}
                      <span>{name}</span>
                    </Space>
                  ),
                }))}
              />
            ),
          },
          {
            title: '颜色',
            width: 160,
            render: (_: unknown, s: RoomStatus) => (
              <ColorPicker
                value={draft[s.key]?.color ?? s.color}
                onChangeComplete={(c) => patch(s.key, { color: c.toHexString() })}
                showText
              />
            ),
          },
          {
            title: '操作',
            width: 90,
            render: (_: unknown, s: RoomStatus) => (
              <Button size="small" onClick={() => save(s)}>
                保存
              </Button>
            ),
          },
        ]}
      />
      <Text type="secondary">
        房态用途固定，只改外观；房间卡片、筛选和退房都会用这里的颜色与图标。
      </Text>
    </Space>
  )
}

// ---------------------------------------------------------------- 备份 / 恢复

/**
 * 备份与恢复。
 *
 * 三件事：手动备份 / 自动备份（可设间隔，默认 1 小时）/ 恢复（含导入外部文件）。
 * 备份走 SQLite 在线备份 API（WAL 里未 checkpoint 的事务也会带走），
 * 恢复的顺序是「只读校验 → 另存当前库 → 覆盖 → 迁移」，任何一步失败都不动现有数据。
 */
function BackupPanel({ onChanged }: { onChanged?: () => void }) {
  const [items, setItems] = useState<BackupEntry[]>([])
  const [cfg, setCfg] = useState<BackupSettings | null>(null)
  /** 间隔 0 = 关闭；这里存的是"开关 + 分钟数"两个独立状态，避免关掉后丢失填过的值 */
  const [enabled, setEnabled] = useState(true)
  const [interval, setIntervalMin] = useState(60)
  const [keep, setKeep] = useState(24)
  const [savingCfg, setSavingCfg] = useState(false)
  const [busy, setBusy] = useState(false)
  const [importing, setImporting] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [target, setTarget] = useState<BackupEntry | null>(null)
  const [restoring, setRestoring] = useState(false)
  const fileRef = useRef<HTMLInputElement | null>(null)

  const load = useCallback(async () => {
    try {
      const [list, settings] = await Promise.all([api.listBackups(), api.getBackupSettings()])
      setItems(list)
      setCfg(settings)
      setEnabled(settings.intervalMinutes > 0)
      setIntervalMin(settings.intervalMinutes > 0 ? settings.intervalMinutes : 60)
      setKeep(settings.keep)
    } catch (e) {
      setErr(describeError(e))
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const doBackup = async () => {
    setBusy(true)
    setErr(null)
    setMsg(null)
    try {
      const r = await api.backupDatabase()
      setMsg(`已备份到 ${r.path}（${(r.bytes / 1024).toFixed(1)} KB）`)
      await load()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const saveCfg = async () => {
    setSavingCfg(true)
    setErr(null)
    setMsg(null)
    try {
      const next = await api.setBackupSettings(enabled ? interval : 0, keep)
      setCfg(next)
      setMsg(
        next.intervalMinutes > 0
          ? `自动备份已开启：每 ${next.intervalMinutes} 分钟一次，保留最近 ${next.keep} 份`
          : '自动备份已关闭（已生成的备份文件保留）',
      )
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setSavingCfg(false)
    }
  }

  /** 导入外部备份：把文件读成字节交后端落盘 + 校验，成功后出现在列表里等用户点「恢复」。 */
  const onPickFile = async (file: File | null) => {
    if (!file) return
    setImporting(true)
    setErr(null)
    setMsg(null)
    try {
      const buf = new Uint8Array(await file.arrayBuffer())
      const entry = await api.importBackupFile(file.name, Array.from(buf))
      setMsg(`已导入 ${entry.name}（${(entry.bytes / 1024).toFixed(1)} KB），确认无误后点该行的「恢复」`)
      await load()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setImporting(false)
      if (fileRef.current) fileRef.current.value = ''
    }
  }

  const doRestore = async () => {
    if (!target) return
    setRestoring(true)
    setErr(null)
    setMsg(null)
    try {
      const r = await api.restoreDatabase(target.path)
      const name = target.name
      setTarget(null)
      setMsg(
        `已从 ${name} 恢复：房间 ${r.rooms} · 发卡记录 ${r.issuances} · 账号 ${r.operators}。` +
          `恢复前的数据另存为 ${r.before}。请退出登录后重新进入，确认数据无误。`,
      )
      await load()
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setRestoring(false)
    }
  }

  const bytesText = (n: number) =>
    n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(2)} MB` : `${(n / 1024).toFixed(1)} KB`

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Alert
        type="info"
        showIcon
        message="备份包含房间、房态、发卡记录、账号与设置（不含卡号计数器）"
        description="备份是单个自包含的 .db，可以直接拷到另一台机器；卡号计数器在运行目录的 CardNo.state 里，换机时要另外拷。"
      />

      <Card type="inner" size="small" title="自动备份">
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <Space wrap align="center">
            <Switch
              checked={enabled}
              onChange={(v) => setEnabled(v)}
              checkedChildren="开启"
              unCheckedChildren="关闭"
            />
            <Text>每</Text>
            <InputNumber
              min={5}
              max={1440}
              value={interval}
              disabled={!enabled}
              onChange={(v) => setIntervalMin(Number(v ?? 60))}
              style={{ width: 90 }}
            />
            <Text>分钟备份一次（默认 60）</Text>
            <Text>保留最近</Text>
            <InputNumber
              min={1}
              max={500}
              value={keep}
              onChange={(v) => setKeep(Number(v ?? 24))}
              style={{ width: 80 }}
            />
            <Text>份</Text>
            <Button type="primary" loading={savingCfg} onClick={() => void saveCfg()}>
              保存
            </Button>
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            只清理自动产生的 auto-*.db，手动备份与恢复前的安全副本永远不会被删。
            上次备份：{cfg?.lastAt ? formatTime(cfg.lastAt) : '还没有'}；目录：{cfg?.dir ?? '—'}
          </Text>
        </Space>
      </Card>

      <Space wrap>
        <Button type="primary" loading={busy} onClick={() => void doBackup()}>
          立即备份
        </Button>
        <Button loading={importing} onClick={() => fileRef.current?.click()}>
          导入外部备份…
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept=".db,.bak,application/x-sqlite3,application/octet-stream"
          style={{ display: 'none' }}
          onChange={(e) => void onPickFile(e.target.files?.[0] ?? null)}
        />
        <Button onClick={() => void load()}>刷新列表</Button>
      </Space>

      {err ? <Alert type="error" showIcon message="操作失败" description={err} /> : null}
      {msg ? <Alert type="success" showIcon message="完成" description={msg} /> : null}

      <Table<BackupEntry>
        size="small"
        rowKey="path"
        dataSource={items}
        pagination={{ pageSize: 10, hideOnSinglePage: true }}
        locale={{ emptyText: '还没有备份。点「立即备份」生成第一份。' }}
        columns={[
          { title: '文件', dataIndex: 'name', ellipsis: true },
          {
            title: '大小',
            dataIndex: 'bytes',
            width: 100,
            render: (n: number) => bytesText(n),
          },
          {
            title: '时间',
            dataIndex: 'modifiedAt',
            width: 170,
            render: (v: string | null) => (v ? formatTime(v) : '—'),
          },
          {
            title: '类型',
            width: 120,
            render: (_: unknown, row: BackupEntry) =>
              row.isSafetyCopy ? (
                <Tag color="orange">恢复前副本</Tag>
              ) : row.isAuto ? (
                <Tag>自动</Tag>
              ) : (
                <Tag color="blue">手动</Tag>
              ),
          },
          {
            title: '操作',
            width: 90,
            render: (_: unknown, row: BackupEntry) => (
              <Button size="small" danger onClick={() => setTarget(row)}>
                恢复
              </Button>
            ),
          },
        ]}
      />

      <Modal
        open={target !== null}
        title="确认用这个备份覆盖当前数据？"
        okText="覆盖并恢复"
        okButtonProps={{ danger: true }}
        cancelText="取消"
        confirmLoading={restoring}
        onOk={() => void doRestore()}
        onCancel={() => setTarget(null)}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Alert
            type="warning"
            showIcon
            message={`将用 ${target?.name ?? ''} 覆盖现在的房间 / 发卡记录 / 账号`}
            description="恢复期间请不要发卡或刷卡。当前数据会先自动另存为 opencardwright.db.before-restore-*.bak。"
          />
          <Text type="secondary" style={{ fontSize: 12 }}>
            备份来自其它机器或更早版本时，恢复后会自动补跑数据库迁移；若备份的 schema 比本程序新，会被直接拒绝。
          </Text>
        </Space>
      </Modal>
    </Space>
  )
}
