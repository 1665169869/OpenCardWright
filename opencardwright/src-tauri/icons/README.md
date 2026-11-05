# 图标（打包前必须生成）

`tauri.conf.json` 的 `bundle.icon` 指向 `icons/icon.ico`，但本目录目前**没有图标文件**——
仓库里不放二进制美术资源，且生成图标需要 Tauri CLI。

打包前执行一次：

```powershell
npm run tauri icon path\to\your-logo.png
```

这会生成 `32x32.png`、`128x128.png`、`icon.ico`、`icon.icns` 等一整套文件。
没有图标时 `npm run tauri build` 会在打包阶段报错（`cargo check` 不受影响）。
