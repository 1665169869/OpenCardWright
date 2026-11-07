# 图标（已生成并入库）

`tauri.conf.json` 的 `bundle.icon` 指向 `icons/icon.ico`。本目录的资源由 Tauri CLI 从
「门锁 + 日历 + 房卡」主视觉（TD-901，2026-11-07）生成，**已随仓库提交**，打包不需要再生成。

要换图标时（在 `opencardwright/` 下）：

```powershell
npm run tauri icon path\to\your-logo.png --fit contain
```

- 源图建议 ≥1024×1024、正方形；非正方形会报错，按提示用 `--fit contain`（居中补边、不裁内容）
  或 `--fit cover`（裁剪填满）。
- 会生成 `32x32.png`、`64x64.png`、`128x128.png`、`128x128@2x.png`、`icon.ico`、`icon.icns`、
  `Square*Logo.png`、`StoreLogo.png` 与 `android/` 一整套。
- 生效范围：NSIS 安装包图标、窗口 / 任务栏图标、系统托盘图标（均取这里的资源，改完需重新
  `npm run tauri build` 才会进安装包）。
