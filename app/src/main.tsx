// antd v5 官方 React 19 兼容补丁：必须在任何 antd 组件之前 import。
// 没有它，Modal.confirm / message 这类静态方法在 React 19 下会静默失效（之前踩过）。
import '@ant-design/v5-patch-for-react-19'
import React from 'react'
import ReactDOM from 'react-dom/client'
import { App as AntdApp, ConfigProvider } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import App from './App'
import { api } from './api'
import './styles.css'

/**
 * 把前端的 error / warn 转发到后端终端。
 *
 * WebView 的 console 只能开 devtools 才看得到，排查时很别扭；
 * 转发后直接读 `npm run tauri dev` 的日志即可，不用截图 OCR。
 */
function forward(level: 'error' | 'warn' | 'info', parts: unknown[]): void {
  const text = parts
    .map((p) => {
      if (p instanceof Error) return `${p.name}: ${p.message}\n${p.stack ?? ''}`
      if (typeof p === 'string') return p
      try {
        return JSON.stringify(p)
      } catch {
        return String(p)
      }
    })
    .join(' ')
  // 后端单行打印，截断避免刷屏；失败时静默（不能因为日志再抛异常）
  void api.logFrontend(level, text.slice(0, 4000)).catch(() => undefined)
}

const rawError = console.error.bind(console)
const rawWarn = console.warn.bind(console)
console.error = (...args: unknown[]) => {
  forward('error', args)
  rawError(...args)
}
console.warn = (...args: unknown[]) => {
  forward('warn', args)
  rawWarn(...args)
}
window.addEventListener('error', (e) => forward('error', [e.message, e.error]))
window.addEventListener('unhandledrejection', (e) => forward('error', ['未处理的 Promise 拒绝', e.reason]))

const root = document.getElementById('root')
if (!root) {
  throw new Error('缺少 #root 挂载点')
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <ConfigProvider locale={zhCN} theme={{ token: { borderRadius: 6 } }}>
      {/* antd v5 的静态 Modal.confirm 在 React 19 下不工作（ReactDOM.render 已移除）。
          包一层 App，组件里用 App.useApp() 拿到 context 版 modal/message。 */}
      <AntdApp>
        <App />
      </AntdApp>
    </ConfigProvider>
  </React.StrictMode>,
)
