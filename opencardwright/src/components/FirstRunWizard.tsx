import { useEffect, useRef, useState } from 'react'
import { Alert, Button, Descriptions, InputNumber, Modal, Space, Steps, Tag, Typography } from 'antd'
import { api, describeError, type AppInfo, type ImportReport, type SelfTestDto } from '../api'

const { Text, Paragraph } = Typography

/**
 * 首启向导：环境 → 自检 → 导入房间。
 *
 * 做成浮在页面之上的受控 Modal，而不是一个页签：它是一次性流程，不该被切页签打断；
 * 也允许中途「跳过」（跳过也会写 `onboardingDone`，免得每次开机都被弹）。
 *
 * 判定"要不要弹"在 App 里：`setting.onboardingDone` 为 1 就永远不弹；
 * 老库（没有这个 setting 但已经有房间）视为已完成，升级用户不会被突然打扰。
 */
export default function FirstRunWizard({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  const [step, setStep] = useState(0)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [port, setPort] = useState<number>(6)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [test, setTest] = useState<SelfTestDto | null>(null)
  const [report, setReport] = useState<ImportReport | null>(null)
  const fileRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!open) return
    setStep(0)
    setTest(null)
    setReport(null)
    setErr(null)
    api
      .appInfo()
      .then((i) => {
        setInfo(i)
        setPort(i.port)
      })
      .catch((e) => setErr(describeError(e)))
  }, [open])

  const savePort = async () => {
    setBusy(true)
    setErr(null)
    try {
      setInfo(await api.setPort(port))
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const runTest = async () => {
    setBusy(true)
    setErr(null)
    try {
      setTest(await api.runSelfTest())
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
    }
  }

  const pickFile = async (file: File | null) => {
    if (!file) return
    setBusy(true)
    setErr(null)
    try {
      const buf = new Uint8Array(await file.arrayBuffer())
      setReport(await api.importRooms(file.name, Array.from(buf)))
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
      if (fileRef.current) fileRef.current.value = ''
    }
  }

  const finish = async () => {
    setBusy(true)
    try {
      await api.completeOnboarding()
    } catch {
      // 写标记失败不该拦住用户：下次还会弹，但至少现在能用
    } finally {
      setBusy(false)
      onClose()
    }
  }

  return (
    <Modal
      open={open}
      title="首次使用：三步准备好"
      width={640}
      closable={false}
      maskClosable={false}
      footer={
        <Space>
          <Button onClick={() => void finish()} disabled={busy}>
            跳过
          </Button>
          {step > 0 ? <Button onClick={() => setStep(step - 1)}>上一步</Button> : null}
          {step < 2 ? (
            <Button type="primary" onClick={() => setStep(step + 1)}>
              下一步
            </Button>
          ) : (
            <Button type="primary" loading={busy} onClick={() => void finish()}>
              完成
            </Button>
          )}
        </Space>
      }
    >
      <Steps
        size="small"
        current={step}
        style={{ marginBottom: 16 }}
        items={[{ title: '检查设备' }, { title: '运行自检' }, { title: '导入房间' }]}
      />

      {err ? <Alert type="error" showIcon message="出错了" description={err} /> : null}

      {step === 0 ? (
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <Descriptions
            column={1}
            size="small"
            items={[
              { key: 'v', label: '版本', children: info ? `v${info.version}` : '—' },
              {
                key: 'mode',
                label: '运行模式',
                children: info ? (
                  info.simulation ? (
                    <Tag color="orange">模拟设备（没有真实读卡器）</Tag>
                  ) : (
                    <Tag color="green">真实设备</Tag>
                  )
                ) : (
                  '—'
                ),
              },
              {
                key: 'sha',
                label: '驱动指纹',
                children: <Text code>{info?.driverSha256 ?? '未读到'}</Text>,
              },
            ]}
          />
          <Space>
            <Text>串口号</Text>
            <InputNumber
              min={0}
              max={255}
              value={port}
              onChange={(v) => setPort(Number(v ?? 0))}
            />
            <Text type="secondary">（0 = COM1；不确定就保持默认，之后可在设置→设备 里改）</Text>
            <Button loading={busy} onClick={() => void savePort()}>
              保存并切换
            </Button>
          </Space>
          <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
            这里不检查 .NET 版本：直接看下一步的自检能不能启动 —— 缺运行库时自检会报错，比读注册表可靠。
          </Paragraph>
        </Space>
      ) : null}

      {step === 1 ? (
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <Button type="primary" loading={busy} onClick={() => void runTest()}>
            运行自检（27 条黄金向量）
          </Button>
          {test ? (
            <Alert
              type={test.ok ? 'success' : 'error'}
              showIcon
              message={test.ok ? `自检通过：${test.passed} 项` : `自检失败：${test.failed} 项`}
              description={
                test.ok
                  ? '读写卡链路正常，可以继续导入房号。'
                  : '先按设置→设备 里的提示排查（串口被占用、驱动缺失最常见），也可以先跳过继续用。'
              }
            />
          ) : null}
        </Space>
      ) : null}

      {step === 2 ? (
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <Space>
            <Button loading={busy} onClick={() => fileRef.current?.click()}>
              选择房号文件（CSV / ini）…
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept=".csv,.ini,.txt"
              style={{ display: 'none' }}
              onChange={(e) => void pickFile(e.target.files?.[0] ?? null)}
            />
            <Text type="secondary" style={{ fontSize: 12 }}>
              格式：房号,序号,房型,楼层（后三列可空）
            </Text>
          </Space>
          {report ? (
            <Alert
              type={report.failed.length ? 'warning' : 'success'}
              showIcon
              message={`新增 ${report.added} 间 · 复活 ${report.revived} 间 · 跳过 ${report.skipped} 间 · 失败 ${report.failed.length}`}
              description={report.failed.slice(0, 5).map((f) => (
                <div key={f.line} style={{ fontSize: 12 }}>
                  第 {f.line} 行：{f.reason}
                </div>
              ))}
            />
          ) : (
            <Paragraph type="secondary" style={{ fontSize: 12 }}>
              也可以点「跳过」，之后在 设置 → 房间 里再导入。
            </Paragraph>
          )}
        </Space>
      ) : null}
    </Modal>
  )
}
