import { useCallback, useEffect, useState } from 'react'
import { Alert, Button, Card, Descriptions, Select, Space, Table, Tag, Typography } from 'antd'
import { api, describeError, formatTime, type AppInfo, type PortProbe, type SelfTestDto } from '../api'

const { Text } = Typography

const PORT_OPTIONS = Array.from({ length: 8 }, (_, i) => ({
  value: i,
  label: `COM${i + 1}（索引 ${i}）`,
}))

/**
 * 设备页：选串口、探测、跑自检。
 *
 * 串口是可变的运行时状态——保存后会重建设备实例并写入 setting，下次启动沿用。
 */
export default function DevicePanel({ onChanged }: { onChanged?: () => void }) {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [port, setPort] = useState<number>(6)
  const [probes, setProbes] = useState<PortProbe[] | null>(null)
  const [selfTest, setSelfTest] = useState<SelfTestDto | null>(null)
  const [busy, setBusy] = useState<'port' | 'probe' | 'test' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const i = await api.appInfo()
      setInfo(i)
      setPort(i.port)
    } catch (e) {
      setError(describeError(e))
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const savePort = async () => {
    setBusy('port')
    setError(null)
    setNotice(null)
    try {
      const i = await api.setPort(port)
      setInfo(i)
      setNotice(`已切换到 COM${i.port + 1}`)
      onChanged?.()
    } catch (e) {
      setError(describeError(e))
    } finally {
      setBusy(null)
    }
  }

  const probe = async () => {
    setBusy('probe')
    setError(null)
    try {
      const list = await api.probePorts()
      setProbes(list)
      if (list.every((p) => !p.available)) {
        setNotice('没有探测到可用读卡器：检查 USB、驱动，以及原门锁软件是否占着串口')
      } else {
        setNotice(null)
      }
    } catch (e) {
      setError(describeError(e))
    } finally {
      setBusy(null)
    }
  }

  const selftest = async () => {
    setBusy('test')
    setError(null)
    try {
      setSelfTest(await api.runSelfTest())
    } catch (e) {
      setError(describeError(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {/* 模拟设备的横幅只在主界面顶部显示一次，这里不再重复 */}
      <Card title="串口" size="small">
        <Space wrap>
          <Select style={{ width: 200 }} value={port} onChange={setPort} options={PORT_OPTIONS} />
          <Button type="primary" loading={busy === 'port'} onClick={savePort}>
            保存并切换
          </Button>
          <Button loading={busy === 'probe'} onClick={probe}>
            探测端口
          </Button>
          <Button loading={busy === 'test'} onClick={selftest}>
            运行自检
          </Button>
        </Space>
        <div style={{ marginTop: 12 }}>
          <Text type="secondary">
            索引 0 = COM1，索引 6 = COM7（驱动内部按 index + 1 打开串口）。原门锁软件会独占串口，切换前先退出它。
          </Text>
        </div>
      </Card>

      {error ? <Alert type="error" showIcon message="操作失败" description={error} /> : null}
      {notice ? <Alert type="success" showIcon message={notice} /> : null}

      {selfTest ? (
        <Alert
          type={selfTest.ok ? 'success' : 'error'}
          showIcon
          message={selfTest.ok ? `自检通过：${selfTest.passed} / ${selfTest.failed}` : '自检未通过'}
          description={`${selfTest.passed} 通过，${selfTest.failed} 失败（27 条黄金向量，不需要读卡器）`}
        />
      ) : null}

      {probes ? (
        <Card title="探测结果" size="small">
          <Table<PortProbe>
            rowKey="com"
            size="small"
            pagination={false}
            dataSource={probes}
            columns={[
              { title: '串口', dataIndex: 'com', width: 100 },
              { title: '索引', dataIndex: 'port', width: 80 },
              {
                title: '状态',
                dataIndex: 'available',
                width: 100,
                render: (ok: boolean) => (ok ? <Tag color="green">可用</Tag> : <Tag>无响应</Tag>),
              },
              { title: '读卡器', dataIndex: 'readerId', render: (v: string | null) => v ?? '—' },
              {
                title: '操作',
                width: 120,
                render: (_: unknown, row: PortProbe) =>
                  row.available ? (
                    <Button
                      size="small"
                      onClick={() => {
                        setPort(row.port)
                      }}
                    >
                      选这个
                    </Button>
                  ) : null,
              },
            ]}
          />
        </Card>
      ) : null}

      <Card title="诊断信息" size="small">
        <Descriptions
          column={1}
          size="small"
          items={[
            { key: 'ver', label: '版本', children: info ? `v${info.version}` : '—' },
            {
              key: 'mode',
              label: '运行模式',
              children: info ? (
                <Space>
                  {info.simulation ? <Tag color="orange">模拟设备</Tag> : <Tag color="green">真实设备</Tag>}
                  {info.dev ? <Tag>dev 构建</Tag> : null}
                  {info.sdkSource ? <Tag>SDK 来源：{info.sdkSource}</Tag> : null}
                </Space>
              ) : (
                '—'
              ),
            },
            {
              key: 'port',
              label: '当前串口',
              children: info ? `COM${info.port + 1}（索引 ${info.port}）` : '—',
            },
            {
              key: 'driver',
              label: '驱动指纹',
              children: info?.driverSha256 ? <Text code>{info.driverSha256}</Text> : <Text type="secondary">未识别</Text>,
            },
            {
              key: 'runtime',
              label: '运行时目录',
              children: info ? <Text code>{info.runtimeDir}</Text> : '—',
            },
            {
              key: 'db',
              label: '数据库',
              children: info ? <Text code>{info.dbPath}</Text> : '—',
            },
            {
              key: 'now',
              label: '本机时间',
              children: formatTime(new Date().toISOString()),
            },
          ]}
        />
      </Card>
    </Space>
  )
}
