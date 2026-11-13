# OpenCardWright（产品工作区）

开源的**门锁发卡台**：给中小宾馆 / 公寓前台用的桌面程序，完成「读卡 → 发卡 → 留痕」，
替代已停止维护的智能门锁管理软件 V8.9 中的发卡环节。

- **最终用户**看 [`docs/使用手册.md`](docs/使用手册.md)（安装、首启、发卡、退房、管理卡风险、备份、FAQ）。
- **集成方**看仓库根 [`README.md`](../README.md) 的 SDK / CLI 章节。
- 本文件面向开发者：构建、架构、模块边界与踩过的坑。

> 卡格式与驱动的实现已迁到独立仓库 **rc-device-sdk**，以 git submodule 挂在
> `src-tauri/vendor/rc-device`；本产品只做编排与治理，**不持有任何卡字节知识**。
> 卡格式与锁行为的权威结论在仓库 `docs/`，本文件不重复。

---

## 1. 架构

```
React 19 + antd 5 (WebView2)
        │  Tauri IPC（src/api.ts 是唯一的 invoke 封装）
        ▼
OpenCardWright.exe (x64, Tauri 2 + Rust)
   ├─ commands.rs      84 条 #[tauri::command]，含命令 × 角色矩阵
   ├─ ocw-store        SQLite（rusqlite bundled），issuance 只追加
   ├─ ocw-device       常驻 sidecar 会话（NDJSON）+ 硬件快照 → DTO + 进程内模拟
   └─ rc_device::idcard_ws   身份证 9001 客户端（进程内，独立设备）
             │
             ▼
runtime\rc-device-sidecar.exe (x86, 构建期由 cargo 编出)
             │
             ▼
LC32RFRW.dll → 发卡器（125 kHz）
```

**为什么用 32 位 sidecar**：驱动只有 x86 版本，64 位主程序加载不了。把门锁卡操作放进
常驻的 32 位子进程、用 stdin/stdout 的 NDJSON 对话，换来的是崩溃可隔离、超时可杀进程、
可切 `--sim` 内存后端做演示——**代价是产品侧没有任何卡字节知识**（这是刻意的）。

数据流要点：

- 发卡：`cardno.rs` 递推「卡号 + 发卡时刻」→ `write_guest_day/hour`（typed spec）→ 卡片 DTO → 写 `issuance` → 审计。
- 读卡：`read_card` → 硬件层 28 字节快照 → DTO → 按 `room_index` 回查房号；卡片数据（A 区）与记录查询（B 区）分开返回，
  **数据库异常不能吞成「查不到」**。
- 退房：持卡退房按卡内原始数据 / 卡号时间戳反查记录；无卡退房按记录 id；非本系统卡走「强制退房」。
- 身份证：`read_id_card` → 硬件层连本机 9001 服务 → 归一化字段；失败只影响自动填表。

---

## 2. 模块地图（改代码先定位）

| 位置 | 职责 |
|---|---|
| `src/` | React 19 + antd 5；`api.ts` 是唯一的 Tauri invoke 封装；页面在 `pages/`，复用组件在 `components/`。一级菜单只有「房间 / 读卡 / 记录」（管理员再加「管理卡 / 设置」）：发卡、续住、退房、入住登记、日历都在「房间」里完成（网格 / 列表 / 日历三视图），不单开入口 |
| `src/utils/` | `validate.ts` / `password.ts` 双端校验文案、`reminder.ts`、`sound.ts` 提示音 |
| `src-tauri/src/commands.rs` | 全部命令、DTO、错误码与**命令 × 角色矩阵**（测试机械校验） |
| `src-tauri/src/rules.rs` | 时长边界（客卡 62 / 总控 63）、长有效期档位；**与 `src/api.ts` 的档位表必须同步** |
| `src-tauri/src/cardno.rs` | `CardNo.state` 原子读写 + 时钟回拨保护；编码/进位交给 `rc_device` |
| `src-tauri/src/auth.rs` | 会话、角色（admin / frontdesk）、Argon2 口令 |
| `src-tauri/src/room_import.rs` | 房号 INI / CSV 导入（坏行收集不致命，删房序号可复活）；GBK 解码在本文件 |
| `src-tauri/src/demo_import.rs` | 仅 `debug_assertions`：空库时从 `data/NacreHis.json` 建 30 个房间，发布版不编译 |
| `src-tauri/src/{provision,backup_job,logging,diagnostics}.rs` | 数据目录（复制 `resources/device`）、自动备份、文件日志、诊断包 |
| `src-tauri/crates/ocw-store/` | SQLite schema 与查询（房间 / 发卡 / 预定 / 审计 / 备份设置） |
| `src-tauri/crates/ocw-device/` | 常驻 sidecar 会话、DTO 映射、错误翻译、进程内模拟后端 |
| `src-tauri/vendor/rc-device/` | **git submodule**：硬件兼容层（驱动、卡字节、46 项自检、身份证、sidecar） |
| `build-release.ps1` | 一键正式版：WiX MSI → `app/output`，打印 SHA-256 |

**备份必须走 SQLite 在线备份 API**：`ocw_store::backup_to` → `rusqlite::backup::Backup::run_to_completion`，
逐页拷贝到完成（含 `-wal` 未 checkpoint 的数据，且不锁库），备份后把目标库设为 `journal_mode=DELETE`。
**不要改成 `std::fs::copy`**——文件级拷贝会漏 WAL，恢复后少掉最后一段事务。

---

## 3. 开发环境

| 依赖 | 版本 | 说明 |
|---|---|---|
| Windows | 10 / 11 x64 | 目标平台 |
| Rust | stable（本机 1.99.1） | 需要 `x86_64-pc-windows-msvc` + **`i686-pc-windows-msvc`**（sidecar 是 32 位） |
| **MSVC 生成工具** | VS Build Tools 2022 + VCTools + Windows 11 SDK | **必需**：没有 `link.exe` 连 `cargo check` 都跑不了 |
| Node.js | ≥ 20（本机 24.11.1） | 前端与 Tauri CLI |
| pnpm | 10.x（本机 10.29.3） | `pnpm-workspace.yaml` 的 `allowBuilds: esbuild` 不能删，否则 install 报 `ERR_PNPM_IGNORED_BUILDS` |
| WiX Toolset | **v3.14** | 只打 MSI 时需要（`candle.exe` / `light.exe`） |
| WebView2 Runtime | 随 Win11 预装 | Tauri 依赖 |

> 2026-11-14 起不再需要 .NET Framework / `csc`：硬件层（原 `sdk/`）已迁到 rc-device-sdk 子模块。

### 3.1 硬件层（手动构建，打包前必须做）

没有构建脚本，手动三步（在 `app/` 内）：

```powershell
# 1) 编 32 位 sidecar（驱动只有 x86）
cargo build --manifest-path src-tauri/vendor/rc-device/Cargo.toml `
  --bin rc-device-sidecar --release --target i686-pc-windows-msvc `
  --target-dir src-tauri/target/sidecar

# 2) 拷到产品资源（sidecar 默认从 exe 同目录的 drivers/LC2000 加载驱动）
New-Item -ItemType Directory -Force -Path src-tauri/resources/device/drivers/LC2000 | Out-Null
Copy-Item src-tauri/target/sidecar/i686-pc-windows-msvc/release/rc-device-sidecar.exe src-tauri/resources/device/ -Force
Copy-Item src-tauri/vendor/rc-device/drivers/LC2000/* src-tauri/resources/device/drivers/LC2000/ -Recurse -Force

# 3) 门禁：46 项黄金向量（--sim 不加载驱动）
@('{"id":1,"op":"selftest"}','{"id":2,"op":"shutdown"}') |
  & src-tauri/resources/device/rc-device-sidecar.exe --sim
```

- 响应必须是 `"passed":46,"failed":0`。
- 只改前端不用重复这一步；改了硬件层（子模块）才需要重编。
- 没准备 `resources/device` 时程序会退化为模拟设备并打日志提示（`provision.rs`）。
- 驱动指纹由程序启动时**现算** SHA-256，不需要 `manifest.json` 之类的构建清单。

### 常用命令（在 `app/` 内）

```powershell
pnpm install
# 硬件层：手动编 sidecar 并拷到 resources/device（命令见上面 §3.1，打包前必须做）
pnpm dev            # vite（只起前端，不含 Tauri 壳）
pnpm tauri dev      # 前端热更新 + Tauri 壳；改 Rust 会自动重编译 + 重启
pnpm build          # tsc --noEmit + vite build
pnpm exec tsc --noEmit   # 只做前端类型检查
```

### 测试（在 `src-tauri/` 内）

```powershell
cargo test --workspace                        # 全部；2026-11-14 实测 144 项全绿
cargo test -p ocw-store                       # 只跑存储层（56）
cargo test -p ocw-device --test sidecar_session # 真起 sidecar（--sim）的端到端（10）
cargo test -p ocw-store insert_room           # 按用例名子串单跑
cargo test --manifest-path vendor/rc-device/Cargo.toml   # 硬件层：201 单测 + 10 契约 + 6 sidecar
```

144 = 主 crate 57（命令层 / 卡号 / 规则 / 日志）+ 设备层 31（21 单测 + 10 集成）+ 存储 56。
硬件层（子模块）另有 201 + 10 + 6，单独跑。

> 本机 `cargo` 不在 PATH，可用绝对路径 `C:\Users\Administrator\.cargo\bin\cargo.exe`；
> 测试必须在 `app/src-tauri/` 内跑（仓库根没有 `Cargo.toml`）。

---

## 4. 打包发布

```powershell
pnpm tauri build --bundles msi   # 由 Tauri 调用 WiX v3 生成 MSI

# 一键正式版（推荐）
pwsh -NoProfile -File build-release.ps1
# 可选：-OutputDir D:\delivery  -SkipCopy  -WixBin "C:\Program Files (x86)\WiX Toolset v3.14\bin"
```

- 脚本执行 `pnpm tauri build --bundles msi`，从 `src-tauri/target/release/bundle/msi/` 取最新 MSI，
  复制到 `app/output/OpenCardWright_<version>_x64.msi` 并打印 SHA-256。
- 安装器是 **MSI（WiX v3）**，不再使用 Inno Setup / NSIS；安装页面语言由
  `tauri.conf.json` 的 `bundle.windows.wix.language` 控制（当前 `zh-CN`）。
- `productName = 门锁发卡台`；快捷方式、安装目录、卸载项均为中文。
- 数据目录不随卸载删除：release 用 `%PROGRAMDATA%\OpenCardWright\`，
  debug / `tauri dev` 用 `%PROGRAMDATA%\OpenCardWright-dev\`（`provision.rs::data_root()`）。
- **不要用 `cargo build --release` 出发布版**：那是连 `localhost:5173` 的 dev 壳。

### WiX 模板与安装向导素材

- `src-tauri/wix/main.wxs` 是**项目自带**的 MSI 模板（`bundle.windows.wix.template`）：
  从 Tauri CLI 2.12.x 的内置模板提取后，加入了「升级提示对话框」与安装器要读的 `Version` 注册表值。
  **升级 Tauri CLI 后必须重新对照官方模板**：构建一次，比较
  `src-tauri/target/release/wix/x64/main.wxs`（渲染结果）与仓库模板，确认新版本引入的字段没漏。
  fragment（`wix.fragmentPaths`）里的 `<UI>` 不会被追加到 Product 的 UI，UI 改动只能进模板。
- `src-tauri/icons/wix/banner.bmp`（493×58）与 `dialog.bmp`（493×312）是安装向导的横幅/侧图，
  由 `icons/icon.png` 生成；换品牌视觉时两张一起换（尺寸必须一致，WiX 会校验）。
- 升级行为：`upgradeCode` 固定为 `07e67703-7451-5273-9e63-2f4716f3a875`，**不要改**，
  否则 Windows 会把新版当成另一个应用；安装目录从
  `HKCU\Software\opencardwright\门锁发卡台` 的 `InstallDir` 读取并沿用，`Version` 供升级提示使用。

### 正式版 / DEV 双通道

`-Channel dev` 打出来的安装包与正式版**完全隔离**，可以同时安装、同时打开：

| 项 | 正式版 | DEV |
|---|---|---|
| `identifier` | `io.opencardwright.desktop` | `io.opencardwright.desktop.dev` |
| `productName`（安装目录 / 快捷方式） | 门锁发卡台 | 门锁发卡台 Dev |
| WiX `upgradeCode` | `07e67703-…f3a875` | `3c9a7f21-…b9d4e63` |
| 数据目录 | `%PROGRAMDATA%\OpenCardWright\` | `%PROGRAMDATA%\OpenCardWright-dev\` |
| 顶栏标签 | 品牌渐变「1.0.0 正式版」 | 橙色「1.0.0 DEV」 |
| 产物名 | `OpenCardWright_1.0.0_x64.msi` | `OpenCardWright_1.0.0_dev_x64.msi` |

```powershell
pwsh -NoProfile -File build-release.ps1                 # 正式版
pwsh -NoProfile -File build-release.ps1 -Channel dev    # DEV 安装包
```

- DEV 用 `tauri.dev.conf.json` 覆盖配置（`pnpm tauri build --config src-tauri/tauri.dev.conf.json`）
  并设编译期 `OCW_CHANNEL=dev`；`provision.rs::channel()` 据此决定数据目录与 `AppInfo.channel`。
- 两个版本能同时打开（single-instance 按 identifier 隔离）；但**串口是独占资源**，
  同时运行时只有一边能连读卡器。
- 版本号两个通道共用：同一轮改动两边用同一个 `A.B.C`，只靠 `-Channel` 区分产物。

### 版本号

三段 `A.B.C`，与 Windows Installer 的 `major.minor.build` 一一对应（每段 ≤65535）：

| 段 | 含义 | 何时变 |
|---|---|---|
| `A` | 主版本 | 大量功能重做 / 不兼容改动（`1.x.x` → `2.0.0`） |
| `B` | 次版本 | 增加或改进功能（`1.0.x` → `1.1.0`，`C` 归 0） |
| `C` | 构建号 | **每打一次包 +1**（同日重打包也 +1），同时充当 MSI 的 build 段 |

没有独立的第四段：MSI 只认三段，多出来的构建号段无处安放；把 `C` 当构建号，
重打包才能覆盖安装。每次改动在根 `CHANGELOG.md` 记一条并说明为什么升这一段，
**四处同步**：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、
`src-tauri/Cargo.lock`。

---

## 5. 已知限制与踩坑

1. **MSVC 报 `D8050`（命令行过长，与中文路径无关）**：`libsqlite3-sys` 编 `sqlite3.c` 时命令行超长。
   项目**不覆盖** `CARGO_TARGET_DIR` / `DEBUG`，统一用默认 `app/src-tauri/target`；
   若本机默认 target 仍触发，按本机环境问题单独处理，不要写回仓库脚本。
2. **`vite build` 报 `Access is denied`**：安全软件锁住 `%TEMP%` 下 esbuild 的临时文件。
   把 TEMP 指到工作区内：`$env:TEMP = "$PWD\.tmp"; $env:TMP = $env:TEMP`。
3. **设备通信只有 NDJSON**：sidecar 的 stdout 只写响应行、日志一律走 stderr；
   产品侧按行读 + 按 `id` 配对，超时即杀进程重建会话（**写卡不重试**）。
   排查可用 `cargo run --bin probe --target i686-pc-windows-msvc`（在 `vendor/rc-device` 内）。
4. **不要用 antd 的命令式弹窗**：`Modal.confirm` / `message.success` 依赖已被 React 19 移除的 API，
   调用会**静默失效**。一律用受控 `<Modal open>` 或 `App.useApp()`。
5. **HMR 会保留组件 state**：弹窗 / 抽屉的上次表单会残留，用自增计数当 `key` 强制重挂载。
6. **改 Rust 源码前先停 dev**：`tauri dev` 编译时占住源文件，保存会报 `ReplaceFileW EIO (Win32 1175)`。
7. **串口用「索引」不是 COM 号**：驱动按 `index + 1` 打开，**索引 2 = COM3**。
   `pnpm tauri dev` 若报 `EBUSY ... ~RF*.TMP`，是编辑器临时文件被短锁，`vite.config.ts` 已忽略这些名字。
8. **dev 时密码是 `Test12345`**：`import.meta.env.DEV` 下登录页预填，发布版为空。
9. **dev 首次启动会自动导房**：空库时读 `data/NacreHis.json` 的 `Kf_fh`，建 30 个房间 + 房型 + 房态；
   只在 `debug_assertions` 下编译，发布版不含。
10. **模拟由硬件层提供**：`OCW_SIM=1` 时用进程内 `SimCardDevice`（或 sidecar `--sim`），
    写卡字节仍走 `rc_device::builder`、读回走 `parser`，与真机逐字节一致；只是写进内存卡槽。
11. **看日志，不要看截图猜**：前端 `console.error/warn` 与未捕获异常会经 `log_frontend`
    转到后端日志（`[frontend][ERROR]`、`[guard]`、`[rust][PANIC]`）。
12. **输入校验是双端的**：前端 `src/utils/validate.ts` / `password.ts`，后端
    `commands.rs` 的 `require_*` 与 `auth.rs::validate_password`；**改一处必须改另一处**。
13. **驱动随包分发**：手动构建时把 `drivers/LC2000/*` 一起拷进 `resources/device`（命令见 §3.1）；
    暂不支持「不打包驱动」的安装方式（需要时另开待办）。
14. **总控卡页1 有变体**：真机上见过 `5B EC F1 FF`，产品侧按 `B3 = 0xFF` 纠正回总控卡
    （`ocw-device/src/map.rs`；业务库序号是 u8，这个判断只能留在产品侧）。
15. **到期提醒是估算**：卡内只有时长码，没有绝对到期时间；产品按「发卡时间 + 时长」估算，
    月码卡（>62/63 天）的到期日本身也是近似。
16. **代码签名未做**（TD-504）。

---

## 6. 命令 × 角色对照表

后端 **84 条** `#[tauri::command]`。这张表是**可执行的**：`commands.rs` 的
`auth_matrix_matches_every_command` 会用 `include_str!` 扫源码逐条比对，
删掉任何一处 `require_admin` 都会让测试变红；新增命令必须先登记到 `MATRIX`。

| 级别 | 数量 | 命令 |
|---|---|---|
| 公开（无需登录） | 7 | `app_info` `auth_status` `initialize_admin` `login` `logout` `log_frontend` `probe_ports` |
| 登录即可（前台日常 + 只读） | 39 | `read_card` `read_id_card` `list_rooms` `run_selftest` `issue_guest` `list_room_types` `list_room_statuses` `set_room_status` `get_max_cards_default` `room_move_preview` `room_move_out` `checkout_room` `checkout_card` `force_checkout_card` `get_checkout_status` `change_own_password` `get_reminder_settings` `get_checkout_hour` `get_morning_cutoff` `expiring_cards` `list_issuances` `list_guest_stays` `list_audits` `get_onboarding_status` `list_reservations` `list_reservation_records` `create_reservation` `update_reservation` `cancel_reservation` `create_checkin` `get_open_checkin` `list_checkins` `get_checkin` `add_checkin_guest` `update_checkin_guest` `delete_checkin_guest` `close_checkin` `hide_to_tray` `quit_app` |
| 管理员 | 39 | `set_port` `create_room` `delete_room` `rename_room` `issue_install_card` `issue_admin_card` `list_departments` `create_department` `rename_department` `set_department_active` `create_room_type` `rename_room_type` `delete_room_type` `set_room_type` `set_room_max_cards` `set_max_cards_default` `update_room_status_style` `backup_database` `list_backups` `restore_database` `set_room_type_color` `set_checkout_status` `list_operators` `create_operator` `reset_operator_password` `set_operator_active` `set_reminder_settings` `set_checkout_hour` `set_morning_cutoff` `get_backup_settings` `set_backup_settings` `import_backup_file` `export_diagnostics` `export_room_index_json` `export_issuances_csv` `export_reservations_csv` `export_checkins_csv` `import_rooms` `complete_onboarding` |

约定：

- 错误码 **401 = 未登录**、**403 = 权限不足**、**409 = 达到发卡上限**；前端按 `code` 区分提示。
- 只读命令不要锁给管理员——前台查房、读卡、看到期是日常动作。
- 新增 / 改名命令必须同步本表；公开命令要在 PR 里说明理由。

---

## 7. 业务铁律（改房间 / 卡片代码前必读）

| 规则 | 原因 |
|---|---|
| 序号范围 **0–4094**（12 bit） | 序号 = `B2` 高 4 位 << 4 \| `B3`。**4095 与总控卡页1（`5B EC F0 FF`）字节相同**，分给房间会让该房客卡被读成总控卡；4080–4094 的页1 是 `*FFF`，与总控卡只差 B2 高 4 位，填号要二次确认 |
| 房间序号绑上锁后**永不复用** | 序号写进卡内页1（`5B EC` 之后的字节）；换成新序号后，旧卡不能再用于新房 |
| 删房是**逻辑删除**（`room.active=0`） | 历史发卡记录要留档，序号仍占位，`list_rooms` 返回全量 |
| 新增房间用到已删序号 = **复活那条记录改房名** | 否则撞 `room_index UNIQUE`；同时重置房态 / 发卡上限 / 安装卡记录 |
| 有有效卡时**不许改名、不许删房** | 发卡、退房、改上限都按 `room_no` 对账 |
| 「有效卡」口径 | `result='ok' AND card_no IS NOT NULL AND expires_at IS NOT NULL AND checked_out_at IS NULL AND expires_at > now` |
| 发卡 / 续住可在**空净房 / 未满房 / 预定房**点 | 房态是人工维护的业务字段，只做前端拦截；预定房发卡时自动预填姓名 |
| 登记 ≠ 发卡 | 两者入口独立：发卡只写卡、**不改房态**；登记只录住客。发卡时该房若有 open 登记单，卡会弱关联到那张单；退房时该单关联卡全失效才自动关单，**退房弹窗勾「同时清除入住登记」则立即关单（只关不删，档案保留）** |
| 持卡退房**只允许客卡** | 退房会擦卡（写零），管理卡放上去会被擦成空白；前后端都拦 |
| 发卡上限 | 全局默认 2 张（`0` = 不限）+ 房间可覆盖（留空跟随全局）；达上限返回 409 |
| 让旧卡失效 | 只发**挂失卡**（`页1[2]` 低 4 位 = 1），与「同房多发卡」是两套机制 |

---

## 8. 其它

- **目录**：`src/` 前端、`src-tauri/` Rust、`build-release.ps1`、
  `vendor/rc-device/`（硬件层子模块）、`docs/使用手册.md`。
- **构建产物**：`src-tauri/resources/device/`（gitignore）、`app/output/`（交付目录，gitignore）。
- **未完成项**只认 [`../plan/待办清单.md`](../plan/待办清单.md)；本文件与根 README 只写现状。
- **许可**：MIT，见 [`../LICENSE`](../LICENSE)。可自由商用、修改、闭源分发，只需保留版权与许可声明；
  随包分发的厂商驱动 `LC32RFRW.dll` 不属本项目版权，见 `NOTICE`。
