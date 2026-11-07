import { useRef, useState } from 'react'
import { Alert, Button, Card, Descriptions, Space, Tag, Typography } from 'antd'
import { api, describeError, type ImportReport } from '../api'

const { Text, Paragraph } = Typography

/**
 * 房间面板：初始房号导入（CSV / ini）。
 *
 * 本机没有文件选择插件，沿用备份导入的做法：`<input type="file">` 读字节 → 后端解析落库。
 * 后端用 `insert_room_reporting`，所以「序号命中已删除房间」会复活那间房，不会新建。
 */
export default function RoomsPanel({ onChanged }: { onChanged?: () => void }) {
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [report, setReport] = useState<ImportReport | null>(null)
  const fileRef = useRef<HTMLInputElement | null>(null)

  const pick = async (file: File | null) => {
    if (!file) return
    setBusy(true)
    setErr(null)
    setReport(null)
    try {
      const buf = new Uint8Array(await file.arrayBuffer())
      const r = await api.importRooms(file.name, Array.from(buf))
      setReport(r)
      onChanged?.()
    } catch (e) {
      setErr(describeError(e))
    } finally {
      setBusy(false)
      if (fileRef.current) fileRef.current.value = ''
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Card type="inner" size="small" title="文件格式">
        <Paragraph style={{ marginBottom: 4 }}>
          <Text strong>CSV</Text>（UTF-8 或 Excel 另存的 GBK 都行）：
          <Text code>房号,序号,房型,楼层</Text> —— 序号、房型、楼层可留空，
          留空时序号自动分配。首行写「房号,…」会当表头跳过。
        </Paragraph>
        <Paragraph style={{ marginBottom: 0 }}>
          <Text strong>ini</Text>：<Text code>房号=序号</Text>（与运行目录的 `RoomIndex.ini` 同格式），
          `;`/`#` 开头是注释。
        </Paragraph>
      </Card>

      <Space wrap>
        <Button type="primary" loading={busy} onClick={() => fileRef.current?.click()}>
          选择文件并导入…
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept=".csv,.ini,.txt"
          style={{ display: 'none' }}
          onChange={(e) => void pick(e.target.files?.[0] ?? null)}
        />
        <Text type="secondary" style={{ fontSize: 12 }}>
          重复导入是安全的：已存在的房号会跳过，不会覆盖房型或房态。
        </Text>
      </Space>

      {err ? <Alert type="error" showIcon message="导入失败" description={err} /> : null}

      {report ? (
        <Card size="small" title="导入结果">
          <Descriptions
            column={4}
            size="small"
            items={[
              { key: 'added', label: '新增', children: <Tag color="green">{report.added}</Tag> },
              {
                key: 'revived',
                label: '复活已删房',
                children: <Tag color="blue">{report.revived}</Tag>,
              },
              { key: 'skipped', label: '跳过（已存在）', children: <Tag>{report.skipped}</Tag> },
              {
                key: 'failed',
                label: '失败',
                children: <Tag color={report.failed.length ? 'red' : 'default'}>{report.failed.length}</Tag>,
              },
            ]}
          />
          {report.failed.length ? (
            <Alert
              type="warning"
              showIcon
              style={{ marginTop: 8 }}
              message="部分行没有导入"
              description={
                <Space direction="vertical" size={2}>
                  {report.failed.slice(0, 10).map((f) => (
                    <Text key={f.line} style={{ fontSize: 12 }}>
                      第 {f.line} 行：{f.reason}
                    </Text>
                  ))}
                  {report.failed.length > 10 ? (
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      还有 {report.failed.length - 10} 行…
                    </Text>
                  ) : null}
                </Space>
              }
            />
          ) : null}
        </Card>
      ) : null}
    </Space>
  )
}
