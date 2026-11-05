import { useState } from 'react'
import { Alert, Button, Card, Input, Space, Tag, Typography } from 'antd'
import { api, describeError, type AuthStatus } from '../api'
import PasswordRules from '../components/PasswordRules'
import { confirmIssue, passwordIssues } from '../utils/password'

const { Title, Text } = Typography

/**
 * 登录 / 首次初始化。
 *
 * 首次启动时没有默认口令：必须由人在现场设置 admin 的密码。
 */
export default function AuthGate({
  auth,
  onSuccess,
}: {
  auth: AuthStatus
  onSuccess: (next: AuthStatus) => void
}) {
  const firstRun = !auth.initialized

  // 调试期默认密码：只在 dev 构建里预填，发布版是空的
  const DEV_PASSWORD = import.meta.env.DEV ? 'Test12345' : ''

  const [login, setLogin] = useState('admin')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState(DEV_PASSWORD)
  const [confirm, setConfirm] = useState(DEV_PASSWORD)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // 点过一次提交后才标红，避免一进页面就一片红
  const [attempted, setAttempted] = useState(false)

  const issues = passwordIssues(password, 'admin')
  const confirmErr = confirmIssue(password, confirm)

  const submit = async () => {
    if (busy) return
    setAttempted(true)
    setError(null)

    if (firstRun) {
      if (issues.length > 0) {
        setError(`密码不符合规则：${issues.join('；')}`)
        return
      }
      if (confirmErr) {
        setError(confirmErr)
        return
      }
    } else {
      if (!login.trim()) {
        setError('请填写账号')
        return
      }
      if (!password) {
        setError('请填写密码')
        return
      }
    }

    setBusy(true)
    try {
      const next = firstRun
        ? await api.initializeAdmin(displayName.trim() || '管理员', password, confirm)
        : await api.login(login.trim(), password)
      onSuccess(next)
    } catch (e) {
      setError(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div style={{ minHeight: '100vh', display: 'grid', placeItems: 'center', background: '#f5f5f5' }}>
      <Card style={{ width: 420 }}>
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <div>
            <Title level={4} style={{ marginBottom: 4 }}>
              {firstRun ? '首次使用：设置管理员密码' : '登录 OpenCardWright'}
            </Title>
            <Text type="secondary">
              {firstRun
                ? '这台机器还没有账号。请为超级管理员 admin 设置密码，之后用它管理房间、管理卡和设备。'
                : '发卡台需要登录后使用。默认管理员账号是 admin。'}
            </Text>
          </div>

          {firstRun ? (
            <>
              <div>
                <Text type="secondary">账号</Text>
                <Space.Compact style={{ width: '100%' }}>
                  <Input value="admin" disabled />
                  <Tag
                    color="blue"
                    style={{ margin: 0, height: 32, display: 'flex', alignItems: 'center' }}
                  >
                    超级管理员
                  </Tag>
                </Space.Compact>
              </div>
              <div>
                <Text type="secondary">显示名（可留空）</Text>
                <Input
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                  placeholder="例如：前台店长"
                />
              </div>
              <div>
                <Text type="secondary">密码</Text>
                <Input.Password
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={`至少 ${auth.minPasswordLen} 位，含字母和数字`}
                  status={attempted && issues.length > 0 ? 'error' : undefined}
                />
                <PasswordRules issues={attempted ? issues : []} />
              </div>
              <div>
                <Text type="secondary">确认密码</Text>
                <Input.Password
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                  onPressEnter={submit}
                  status={attempted && confirmErr ? 'error' : undefined}
                />
                {attempted && confirmErr ? (
                  <Text type="danger" style={{ fontSize: 12 }}>
                    {confirmErr}
                  </Text>
                ) : (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    再输入一次，防止打错。
                  </Text>
                )}
              </div>
            </>
          ) : (
            <>
              <div>
                <Text type="secondary">账号</Text>
                <Input value={login} onChange={(e) => setLogin(e.target.value)} />
              </div>
              <div>
                <Text type="secondary">密码</Text>
                <Input.Password
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  onPressEnter={submit}
                  autoFocus
                />
              </div>
            </>
          )}

          {error ? <Alert type="error" showIcon message={error} /> : null}

          <Button type="primary" block loading={busy} onClick={submit}>
            {firstRun ? '设置密码并进入' : '登录'}
          </Button>
        </Space>
      </Card>
    </div>
  )
}
