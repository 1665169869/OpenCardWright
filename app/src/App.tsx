import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
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
  Tooltip,
  Typography,
  type TabsProps,
} from 'antd'
import {
  HomeOutlined,
  IdcardOutlined,
  LogoutOutlined,
  ScanOutlined,
  HistoryOutlined,
  LoadingOutlined,
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
  type ExpiringCard,
  type ReminderSettings,
} from './api'
import AuthGate from './pages/AuthGate'
import ExpiryBell from './components/ExpiryBell'
import { playAlert, speak, unlockSound } from './utils/sound'
import { expirySpeech } from './utils/reminder'
import { listen } from '@tauri-apps/api/event'

const { Header, Content } = Layout
const { Title, Text } = Typography

// 一级页面按需加载（TD-902）：首屏只加载当前页签，切到其它页时再拉对应 chunk。
const ReadCard = lazy(() => import('./pages/ReadCard'))
const Logs = lazy(() => import('./pages/Logs'))
const FirstRunWizard = lazy(() => import('./components/FirstRunWizard'))
const RoomManager = lazy(() => import('./pages/RoomManager'))
const AdminCards = lazy(() => import('./pages/AdminCards'))
const Settings = lazy(() => import('./pages/Settings'))

/** 到期提醒轮询间隔：30 秒。 */
const REMINDER_POLL_MS = 30_000

export default function App() {
  const { notification } = AntdApp.useApp()
  const [auth, setAuth] = useState<AuthStatus | null>(null)
  const [bootError, setBootError] = useState<string | null>(null)
  const [info, setInfo] = useState<AppInfo | null>(null)
  /** 启动探测（TD-138）：appInfo 未回来之前顶栏显示「探测设备中…」 */
  const [infoLoading, setInfoLoading] = useState(true)
  /** 设备可用性：null = 还没有结果；先由 appInfo 快判，再由后台 probePorts 覆盖 */
  const [deviceOk, setDeviceOk] = useState<boolean | null>(null)
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

  const refreshInfo = useCallback(() => {
    setInfoLoading(true)
    api
      .appInfo()
      .then((i) => {
        setInfo(i)
        // 快判（TD-138）：SDK 运行时副本就绪才算设备可用——毫秒级，先给顶栏 tag 定型
        setDeviceOk(i.driverSha256 !== null && i.sdkSource !== null)
        // 后台再真探测串口（逐个开 COM1–COM8，比较慢）；结果覆盖快判，失败就保留快判
        api
          .probePorts()
          .then((list) => setDeviceOk(list.some((p) => p.available)))
          .catch(() => {})
      })
      .catch((e: unknown) => {
        setError(describeError(e))
        setDeviceOk(false)
      })
      .finally(() => setInfoLoading(false))
  }, [])

  const loggedIn = auth?.loggedIn === true
  // 设备探测不依赖登录（`probe_ports` 已是 Public 级）：挂载即探测，
  // 登录状态变化后再刷一次，避免登录页顶栏一直卡在「探测设备中…」。
  useEffect(() => {
    refreshInfo()
  }, [refreshInfo])

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
  }, [])

  const logout = async () => {
    try {
      const next = await api.logout()
      setAuth(next)
      setInfo(null)
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
            门锁发卡台
          </Title>
          {info ? <Tag>v{info.version}</Tag> : null}
          {infoLoading ? (
            <Tag icon={<LoadingOutlined spin />} color="processing">
              探测设备中…
            </Tag>
          ) : info?.simulation ? (
            <Tag color="orange">模拟设备</Tag>
          ) : deviceOk === false ? (
            <Tooltip title="检查读卡器 USB / 驱动，或原门锁软件是否占着串口；可去「设置 → 设备 → 探测端口」重试">
              <Tag color="orange">未探测到设备</Tag>
            </Tooltip>
          ) : deviceOk === true ? (
            <Tag color="green">设备就绪</Tag>
          ) : (
            <Tag>等待探测…</Tag>
          )}
        </Space>

        <Space size="middle">
          <ExpiryBell items={reminders} />
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

        {/* destroyOnHidden：切走即卸载、切回重新挂载 —— 每个页面进入时都会拉一次最新数据。
            记录页的生命周期状态（未到期/已到期/已退房）、房间页的有效卡数都不能是切页前的旧值。 */}
        <Suspense fallback={<div style={{ padding: 24, textAlign: 'center' }}><Spin /></div>}>
          <Tabs defaultActiveKey="rooms" items={tabs} destroyOnHidden />
          <FirstRunWizard open={needWizard} onClose={() => setNeedWizard(false)} />
        </Suspense>

        {/* 关窗确认（TD-109）：默认「缩小到托盘」，后台继续跑到期提醒与自动备份 */}
        <Modal
          open={closeAsk}
          title="关闭门锁发卡台？"
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
