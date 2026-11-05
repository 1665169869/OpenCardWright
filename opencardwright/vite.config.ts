import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri 会以 devUrl 指向 5173；端口必须固定，否则 WebView 连不上。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // .tmp 是 esbuild 的临时目录（Windows 上会被系统锁住 lnk*.tmp），
      // target / resources 是构建产物：都不该触发 HMR，否则 dev 会 EBUSY 崩掉。
      // 编辑器保存时产生的 `xxx.tsx~RF1234.TMP` 也是同样问题：Windows 上文件夹被短暂独占，
      // Vite 去 watch 它会直接 EBUSY 退出，所以连同 ~RF 临时名一起忽略。
      ignored: [
        '**/.tmp/**',
        '**/src-tauri/target/**',
        '**/src-tauri/resources/**',
        '**/release/**',
        '**/*.TMP',
        '**/*.tmp',
        '**/*~RF*',
      ],
    },
  },
  build: {
    // WebView2 基于 Chromium，这条目标足够新且稳定
    target: 'chrome105',
    outDir: 'dist',
    emptyOutDir: true,
  },
})
