import { useCallback, useEffect, useRef, useState } from 'react'
import {
  Alert,
  App as AntdApp,
  Button,
  Layout,
  Modal,
  Space,
  Spin,
  Tabs,
  Tag,
  Typography,
  type TabsProps,
} from 'antd'
import {
  HomeOutlined,
  IdcardOutlined,
  LogoutOutlined,
  ScanOutlined,
  HistoryOutlined,
  SettingOutlined,
} from '@ant-design/icons'
import {
  api,
  describeError,
  formatCardNo,
  formatExpiry,
  isAdmin,
  type AppInfo,
  type AuthStatus,
  type CardNoStatus,
  type ExpiringCard,
  type ReminderSettings,
} from './api'
import AuthGate from './pages/AuthGate'
import ReadCard from './pages/ReadCard'
import Logs from './pages/Logs'
import FirstRunWizard from './components/FirstRunWizard'
import RoomManager from './pages/RoomManager'
import AdminCards from './pages/AdminCards'
import Settings from './pages/Settings'
import ExpiryBell from './components/ExpiryBell'
import { playAlert, speak, unlockSound } from './utils/sound'
import { expirySpeech } from './utils/reminder'
import { listen } from '@tauri-apps/api/event'

const { Header, Content } = Layout
const { Title, Text } = Typography

/** 到期提醒轮询间隔：30 秒。 */
const REMINDER_POLL_MS = 30_000

export default function App() {
  const { notification } = AntdApp.useApp()
  const [auth, setAuth] = useState<AuthStatus | null>(null)
  const [bootError, setBootError] = useState<string | null>(null)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [status, setStatus] = useState<CardNoStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  // 房间列表的版本号：房间页一变就 +1，让发卡页/管理卡页重新拉列表
  const [roomsVersion, setRoomsVersion] = useState(0)
  // 设置页的版本号：房型/房态的颜色图标改完就 +1，房间页据此重新拉数据（不用手动刷新）
  const [settingsVersion, setSettingsVersion] = useState(0)

  // 到期提醒
  const [reminder, setReminder] = useState<ReminderSettings | null>(null)
  const [reminders, setReminders] = useState<ExpiringCard[]>([])
  const alerted = useRef<Set<number>>(new Set())
  /** 首启向导：登录后查一次，只在空库且没做过时弹 */
  const [needWizard, setNeedWizard] = useState(false)
  /** 关窗确认框（TD-109）：Rust 拦截 CloseRequested 后发 close-confirm，这里弹框让用户选 */
  const [closeAsk, setCloseAsk] = useState(false)

  // 第一次点击任何地方就解锁音频（浏览器自动播放策略）
  useEffect(() => {
    const handler = () => {
      unlockSound()
      document.removeEventListener('pointerdown', handler)
    }
    document.addEventListener('pointerdown', handler)
    return () => document.removeEventListener('pointerdown', handler)
  }, [])

  useEffect(() => {
    api
      .authStatus()
      .then(setAuth)
      .catch((e: unknown) => setBootError(describeError(e)))
  }, [])

  const refreshStatus = useCallback(() => {
    api
      .cardNoStatus()
      .then(setStatus)
      .catch(() => setStatus(null))
  }, [])

  const refreshInfo = useCallback(() => {
    api
      .appInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(describeError(e)))
    refreshStatus()
  }, [refreshStatus])

  const loggedIn = auth?.loggedIn === true
  useEffect(() => {
    if (loggedIn) refreshInfo()
  }, [loggedIn, refreshInfo])

  // 登录后查一次首启向导状态（只在"没做过"时弹）
  useEffect(() => {
    if (!loggedIn) {
      setNeedWizard(false)
      return
    }
    api
      .getOnboardingStatus()
      .then((s) => setNeedWizard(!s.done))
      .catch(() => setNeedWizard(false))
  }, [loggedIn])

  // 登录后加载提醒设置
  useEffect(() => {
    if (!loggedIn) {
      setReminder(null)
      setReminders([])
      return
    }
    api
      .getReminderSettings()
      .then(setReminder)
      .catch(() => setReminder(null))
  }, [loggedIn])

  // 关窗确认（TD-109）：Rust 拦截 CloseRequested 后发 close-confirm，弹框让用户选
  // 「缩小到托盘（默认）/ 退出程序 / 取消」。取消 = 继续使用，什么都不动。
  useEffect(() => {
    if (!loggedIn) return
    let alive = true
    const un = listen('close-confirm', () => {
      if (alive) setCloseAsk(true)
    })
    return () => {
      alive = false
      void un.then((f) => f())
    }
  }, [loggedIn])

  // 到期提醒轮询：只有开提醒才跑，避免无谓的数据库查询
  useEffect(() => {
    if (!loggedIn || !reminder?.enabled) {
      setReminders([])
      return
    }
    let alive = true

    const tick = async () => {
      try {
        const list = await api.expiringCards(reminder.minutes)
        if (!alive) return
        setReminders(list)
        for (const c of list) {
          if (alerted.current.has(c.issuanceId)) continue
          alerted.current.add(c.issuanceId)
          const expired = new Date(c.expiresAt).getTime() < Date.now()
          notification.warning({
            key: `expiry-${c.issuanceId}`,
            message: `${expired ? '客房卡已过期' : '客房卡即将到期'}：${c.roomNo ?? '未知房号'}`,
            description: `${c.kind} · 卡号 ${formatCardNo(c.cardNo)} · ${formatExpiry(c.expiresAt) ?? '到期时间未知'}`,
            duration: 20,
            placement: 'topRight',
          })
          // 语音优先；语音不可用（无中文音色/合成失败）才回退提示音，避免同时响两遍
          let spoke = false
          if (reminder.voice) spoke = speak(expirySpeech(c, Date.now()))
          if (!spoke && reminder.sound) playAlert()
        }
      } catch {
        // 轮询失败静默，下一轮再试；不要用弹窗骚扰前台
      }
    }

    void tick()
    const timer = window.setInterval(() => {
      void tick()
    }, REMINDER_POLL_MS)
    return () => {
      alive = false
      window.clearInterval(timer)
    }
  }, [loggedIn, reminder, notification])

  const onRoomsChanged = useCallback(() => {
    setRoomsVersion((v) => v + 1)
    refreshStatus()
  }, [refreshStatus])

  const logout = async () => {
    try {
      const next = await api.logout()
      setAuth(next)
      setInfo(null)
      setStatus(null)
      // 换账号后同一张卡要能再播一次，否则"上一个前台看过就不再提醒"
      alerted.current.clear()
      setReminders([])
    } catch (e) {
      setError(describeError(e))
    }
  }

  if (bootError) {
    return (
      <div style={{ padding: 48 }}>
        <Alert type="error" showIcon message="启动失败" description={bootError} />
      </div>
    )
  }

  if (!auth) {
    return (
      <div style={{ minHeight: '100vh', display: 'grid', placeItems: 'center' }}>
        <Space direction="vertical" align="center" size="small">
          <Spin size="large" />
          <Text type="secondary">正在检查本机状态…</Text>
        </Space>
      </div>
    )
  }

  if (!auth.loggedIn) {
    return <AuthGate auth={auth} onSuccess={setAuth} />
  }

  const admin = isAdmin(auth)

  // 一级导航。图标沿用项目里的 antd 线性 Outlined 风格（与右键菜单、房态图标一致）。
  const tabs: NonNullable<TabsProps['items']> = [
    // 房间放最前面：前台最常用的是"选房间 → 发卡/退房"，
    // 读卡更多是偶尔核对一张卡时才用。
    {
      key: 'rooms',
      icon: <HomeOutlined />,
      label: '房间',
      children: (
        <RoomManager
          onRoomsChanged={onRoomsChanged}
          canAdmin={admin}
          settingsVersion={settingsVersion}
        />
      ),
    },
    { key: 'read', icon: <ScanOutlined />, label: '读卡', children: <ReadCard /> },
    { key: 'logs', icon: <HistoryOutlined />, label: '记录', children: <Logs canAdmin={admin} /> },
  ]

  if (admin) {
    tabs.push(
      {
        key: 'admin',
        icon: <IdcardOutlined />,
        label: '管理卡',
        children: (
          <AdminCards
            roomsVersion={roomsVersion}
            onChanged={() => {
              refreshStatus()
              refreshInfo()
            }}
          />
        ),
      },
      {
        key: 'settings',
        icon: <SettingOutlined />,
        label: '设置',
        children: (
          <Settings
            onChanged={() => {
              refreshInfo()
              // 水位警戒线、退房后房态这些设置改完，顶部的状态条要跟着变
              refreshStatus()
              // 房型标签颜色、房态图标颜色改完，房间页要立刻重画，不能等手动刷新
              setSettingsVersion((v) => v + 1)
              api
                .getReminderSettings()
                .then(setReminder)
                .catch(() => undefined)
            }}
          />
        ),
      },
    )
  }

  return (
    <Layout style={{ minHeight: '100%' }}>
      <Header
        style={{
          background: '#fff',
          borderBottom: '1px solid #f0f0f0',
          // 顶栏固定：内容滚动时水位 / 到期提醒 / 角色始终可见（TD-108）
          position: 'sticky',
          top: 0,
          zIndex: 10,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          paddingInline: 24,
        }}
      >
        <Space size="middle" align="center">
          <Title level={4} style={{ margin: 0 }}>
            OpenCardWright
          </Title>
          <Text type="secondary">门锁发卡台</Text>
          {info ? <Tag>v{info.version}</Tag> : null}
          {info?.simulation ? <Tag color="orange">模拟设备</Tag> : info ? <Tag color="green">设备就绪</Tag> : null}
        </Space>

        <Space size="middle">
          <ExpiryBell items={reminders} />
          <Text
            style={{
              fontSize: 12,
              color:
                status?.level === 'blocked'
                  ? '#cf1322'
                  : status?.level === 'warn'
                    ? '#d46b08'
                    : undefined,
              fontWeight: status && status.level !== 'normal' ? 600 : undefined,
            }}
            type={status && status.level !== 'normal' ? undefined : 'secondary'}
          >
            水位 {status ? formatCardNo(status.current) : '—'}
            {status ? ` / 警戒 ${formatCardNo(status.warnAt)}` : ''}
          </Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            驱动 {info?.driverSha256 ? info.driverSha256.slice(0, 12).toLowerCase() : '未识别'}
          </Text>
          <Tag color={admin ? 'blue' : 'default'}>
            {auth.displayName || auth.login} · {admin ? '管理员' : '前台'}
          </Tag>
          <Button size="small" icon={<LogoutOutlined />} onClick={logout}>
            退出登录
          </Button>
        </Space>
      </Header>

      <Content className="ocw-content">
        {error ? (
          <Alert type="error" showIcon style={{ marginBottom: 16 }} message="出错了" description={error} />
        ) : null}

        {info?.simulation ? (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 16 }}
            message="当前是模拟设备模式"
            description="没有找到内置的 PMSLockCard.exe（或设置了 OCW_SIM）。发卡会用 SDK 的 CardBuilder 离线构包并推进卡号计数器（卡号、序号、字节都与真机一致），只是不会真的写卡。"
          />
        ) : null}

        {!admin ? (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 16 }}
            message="当前是前台账号"
            description="可以读卡、发客卡、查记录；管理卡、房间维护、设备设置需要管理员账号。"
          />
        ) : null}

        {status && status.level !== 'normal' ? (
          <Alert
            type={status.level === 'blocked' ? 'error' : 'warning'}
            showIcon
            style={{ marginBottom: 16 }}
            message={
              status.level === 'blocked'
                ? `卡号水位已到 ${formatCardNo(status.current)}，已停止发客卡`
                : `卡号水位已过警戒线 ${formatCardNo(status.warnAt)}（当前 ${formatCardNo(status.current)}）`
            }
            description={
              status.level === 'blocked'
                ? '继续发卡会让部分旧卡被锁拒开。请先给全部门锁各刷一次复位卡，然后到「设置 → 水位」重置到低位。'
                : '请给全部门锁各刷一次复位卡（把各房的有效卡号归零），然后到「设置 → 水位」把水位重置到低位；没刷复位就拨低，新卡会被锁当成旧卡拒开。'
            }
          />
        ) : null}

        <Tabs defaultActiveKey="rooms" items={tabs} />
        <FirstRunWizard open={needWizard} onClose={() => setNeedWizard(false)} />

        {/* 关窗确认（TD-109）：默认「缩小到托盘」，后台继续跑到期提醒与自动备份 */}
        <Modal
          open={closeAsk}
          title="关闭 OpenCardWright？"
          // antd 默认 top=100px 在高窗口下偏上；centered 垂直居中
          centered
          footer={null}
          closable
          maskClosable={false}
          onCancel={() => setCloseAsk(false)}
        >
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Text type="secondary">
              缩小到系统托盘后程序继续运行（到期提醒、自动备份不受影响），
              点托盘图标可恢复窗口；托盘菜单里也能真正退出。
            </Text>
            <Space wrap>
              <Button
                type="primary"
                onClick={() => {
                  setCloseAsk(false)
                  void api.hideToTray()
                }}
              >
                缩小到托盘（推荐）
              </Button>
              <Button danger onClick={() => void api.quitApp()}>
                退出程序
              </Button>
              <Button onClick={() => setCloseAsk(false)}>取消</Button>
            </Space>
          </Space>
        </Modal>
      </Content>
    </Layout>
  )
}
