# OpenCardWright（产品工作区）

开源的**门锁发卡台**：给中小宾馆 / 公寓前台用的桌面程序，完成「读卡 → 发卡 → 留痕」，
替代已停止维护的智能门锁管理软件 V8.9 中的发卡环节。

- **最终用户**看 [`docs/使用手册.md`](docs/使用手册.md)（安装、首启、发卡、退房、管理卡风险、备份、FAQ）。
- **集成方**看仓库根 [`README.md`](../README.md) 的 SDK / CLI 章节。
- 本文件面向开发者：构建、架构、模块边界与踩过的坑。

> 本产品**不改** `sdk/` 的源码（卡格式留在那里），构建期用 `csc` 把它现编成运行时组件。
> 卡格式与锁行为的权威结论在仓库 `docs/`，本文件不重复。

---

## 1. 架构

```
React 19 + antd 5 (WebView2)
        │  Tauri IPC（src/api.ts 是唯一的 invoke 封装）
        ▼
OpenCardWright.exe (x64, Tauri 2 + Rust)
   ├─ commands.rs      71 条 #[tauri::command]，含命令 × 角色矩阵
   ├─ ocw-store        SQLite（rusqlite bundled），issuance 只追加
   └─ ocw-device       Process.Start（GBK 解码 + JSON/文本解析 + 模拟设备）
             │
             ▼
runtime\PMSLockCard.exe (x86, .NET 4.0，构建期从 sdk/ 现编)
             │
             ▼
LC32RFRW.dll → 发卡器（125 kHz）
```

**为什么不直接 FFI 驱动**：驱动是 32 位且版本极敏感。把卡格式留在已验证的 .NET 实现里，
产品只做编排与治理；设备层走**子进程 + JSON**，崩溃可隔离、可超时杀、可替换。
代价是必须处理 GBK 输出（见 §5）。

数据流要点：

- 发卡：`cardno.rs` 算好时间戳卡号 → CLI `issue ... --serial` → 读回卡片 → 写 `issuance` → 审计。
- 读卡：CLI `read --json` → `ocw-device` 解析 → 卡片数据（A 区）与记录查询（B 区）分开返回，
  **数据库异常不能吞成「查不到」**。
- 退房：持卡退房按卡号时间戳反查记录；无卡退房按记录 id；非本系统卡走「强制退房」。

---

## 2. 模块地图（改代码先定位）

| 位置 | 职责 |
|---|---|
| `src/` | React 19 + antd 5；`api.ts` 是唯一的 Tauri invoke 封装；页面在 `pages/`，复用组件在 `components/` |
| `src/utils/` | `validate.ts` / `password.ts` 双端校验文案、`reminder.ts`、`sound.ts` 提示音 |
| `src/idcard.ts` | 身份证读卡 WebSocket（默认 `ws://127.0.0.1:9001`，CSP `connect-src` 要同步放行） |
| `src-tauri/src/commands.rs` | 全部命令、DTO、错误码与**命令 × 角色矩阵**（测试机械校验） |
| `src-tauri/src/rules.rs` | 时长边界（客卡 62 / 总控 63）、长有效期档位；**与 `src/api.ts` 的档位表必须同步** |
| `src-tauri/src/cardno.rs` | 卡号时间戳编码（日 / 时 / 序号）、`CardNo.state` 原子读写与递推 |
| `src-tauri/src/auth.rs` | 会话、角色（admin / frontdesk）、Argon2 口令 |
| `src-tauri/src/room_import.rs` | 房号 INI / CSV 导入（坏行收集不致命，删房序号可复活） |
| `src-tauri/src/demo_import.rs` | 仅 `debug_assertions`：空库时从 `data/NacreHis.json` 建 30 个房间，发布版不编译 |
| `src-tauri/src/{provision,backup_job,logging,diagnostics}.rs` | 数据目录、自动备份、文件日志、诊断包 |
| `src-tauri/crates/ocw-store/` | SQLite schema 与查询（房间 / 发卡 / 预定 / 审计 / 备份设置） |
| `src-tauri/crates/ocw-device/` | CLI 子进程、GBK 解码、解析、模拟设备、离线构包器调用 |
| `scripts/build-sdk.ps1` | 编译 `../sdk` → `resources/sdk`，跑 46 项自检门禁，编 `OCWCardBuild.exe` |
| `build-release.ps1` | 一键正式版：WiX MSI → `app/output`，打印 SHA-256 |

**备份必须走 SQLite 在线备份 API**：`ocw_store::backup_to` → `rusqlite::backup::Backup::run_to_completion`，
逐页拷贝到完成（含 `-wal` 未 checkpoint 的数据，且不锁库），备份后把目标库设为 `journal_mode=DELETE`。
**不要改成 `std::fs::copy`**——文件级拷贝会漏 WAL，恢复后少掉最后一段事务。

---

## 3. 开发环境

| 依赖 | 版本 | 说明 |
|---|---|---|
| Windows | 10 / 11 x64 | 目标平台 |
| Rust | stable（本机 1.99.1） | 需要 `x86_64-pc-windows-msvc` |
| **MSVC 生成工具** | VS Build Tools 2022 + VCTools + Windows 11 SDK | **必需**：没有 `link.exe` 连 `cargo check` 都跑不了 |
| Node.js | ≥ 20（本机 24.11.1） | 前端与 Tauri CLI |
| pnpm | 10.x（本机 10.29.3） | `pnpm-workspace.yaml` 的 `allowBuilds: esbuild` 不能删，否则 install 报 `ERR_PNPM_IGNORED_BUILDS` |
| .NET Framework | 4.8（系统自带） | 编译与运行 SDK 组件需要 `csc.exe` |
| WiX Toolset | **v3.14** | 只打 MSI 时需要（`candle.exe` / `light.exe`） |
| WebView2 Runtime | 随 Win11 预装 | Tauri 依赖 |

### 常用命令（在 `app/` 内）

```powershell
pnpm install
pnpm build:sdk      # 编译 ../sdk → resources/sdk，跑 46 项门禁；同时编 tools/CardBuild.cs → OCWCardBuild.exe
pnpm dev            # build:sdk + vite（只起前端，不含 Tauri 壳）
pnpm tauri dev      # 前端热更新 + Tauri 壳；改 Rust 会自动重编译 + 重启
pnpm build          # build:sdk + tsc --noEmit + vite build
pnpm exec tsc --noEmit   # 只做前端类型检查
```

### 测试（在 `src-tauri/` 内）

```powershell
cargo test --workspace                        # 全部；2026-11-10 实测 136 项全绿
cargo test -p ocw-store                       # 只跑存储层（56）
cargo test -p ocw-device --test sim_cardbuild # 单跑一个集成测试文件（5）
cargo test -p ocw-store insert_room           # 按用例名子串单跑
```

136 项 = 解析 27 + 真实 selftest 1 + 模拟构包 5 + 存储 56 + 命令层 47。

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

### 版本号

每次代码、行为或用户可见文档变动都要提构建号，**四处同步**：
`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock`，
并在根 `CHANGELOG.md` 记录本版结论。

格式 `26.M.DDD`：主版本 = 年份−2000（MSI 要求主版本 ≤ 255），次版本 = 月份，
补丁号 = 日×100 + 当日构建序号（11 月 9 日第 62 号 → `26.11.962`）。

---

## 5. 已知限制与踩坑

1. **MSVC 报 `D8050`（命令行过长，与中文路径无关）**：`libsqlite3-sys` 编 `sqlite3.c` 时命令行超长。
   项目**不覆盖** `CARGO_TARGET_DIR` / `DEBUG`，统一用默认 `app/src-tauri/target`；
   若本机默认 target 仍触发，按本机环境问题单独处理，不要写回仓库脚本。
2. **`vite build` 报 `Access is denied`**：安全软件锁住 `%TEMP%` 下 esbuild 的临时文件。
   把 TEMP 指到工作区内：`$env:TEMP = "$PWD\.tmp"; $env:TMP = $env:TEMP`。
3. **设备输出是 GBK，不是 UTF-8**：`PMSLockCard.exe` 往管道写控制台代码页，
   必须经 `ocw_device::cli::decode`；直接 `read_to_string` 遇中文会整体失败并返回空串。
   排查可用 `cargo run -p ocw-device --example probe -- <exe> <ini> <driverDir> 6 ports`。
4. **不要用 antd 的命令式弹窗**：`Modal.confirm` / `message.success` 依赖已被 React 19 移除的 API，
   调用会**静默失效**。一律用受控 `<Modal open>` 或 `App.useApp()`。
5. **HMR 会保留组件 state**：弹窗 / 抽屉的上次表单会残留，用自增计数当 `key` 强制重挂载。
6. **改 Rust 源码前先停 dev**：`tauri dev` 编译时占住源文件，保存会报 `ReplaceFileW EIO (Win32 1175)`。
7. **串口用「索引」不是 COM 号**：驱动按 `index + 1` 打开，**索引 2 = COM3**。
   `pnpm tauri dev` 若报 `EBUSY ... ~RF*.TMP`，是编辑器临时文件被短锁，`vite.config.ts` 已忽略这些名字。
8. **dev 时密码是 `Test12345`**：`import.meta.env.DEV` 下登录页预填，发布版为空。
9. **dev 首次启动会自动导房**：空库时读 `data/NacreHis.json` 的 `Kf_fh`，建 30 个房间 + 房型 + 房态；
   只在 `debug_assertions` 下编译，发布版不含。
10. **模拟设备也走真实卡号**：`OCW_SIM=1` 下由 `cardno.rs` 算卡号、经 `--serial` 传给
    `OCWCardBuild.exe` 离线构包，字节与真机一致；缺构包器时退回固定样本。
11. **看日志，不要看截图猜**：前端 `console.error/warn` 与未捕获异常会经 `log_frontend`
    转到后端日志（`[frontend][ERROR]`、`[guard]`、`[rust][PANIC]`）。
12. **输入校验是双端的**：前端 `src/utils/validate.ts` / `password.ts`，后端
    `commands.rs` 的 `require_*` 与 `auth.rs::validate_password`；**改一处必须改另一处**。
13. **`-BundleDriver:$false`**：`scripts/build-sdk.ps1` 支持不打包驱动（改为首启导入），默认打包。
14. **总控卡页1 有变体**：真机上见过 `5B EC F1 FF`，产品侧按 `B3 = 0xFF` 纠正回总控卡；
    `sdk/` 未改动。
15. **到期提醒是估算**：卡内只有时长码，没有绝对到期时间；产品按「发卡时间 + 时长」估算，
    月码卡（>62/63 天）的到期日本身也是近似。
16. **代码签名未做**（TD-504）。

---

## 6. 命令 × 角色对照表

后端 **71 条** `#[tauri::command]`。这张表是**可执行的**：`commands.rs` 的
`auth_matrix_matches_every_command` 会用 `include_str!` 扫源码逐条比对，
删掉任何一处 `require_admin` 都会让测试变红；新增命令必须先登记到 `MATRIX`。

| 级别 | 数量 | 命令 |
|---|---|---|
| 公开（无需登录） | 7 | `app_info` `auth_status` `initialize_admin` `login` `logout` `log_frontend` `probe_ports` |
| 登录即可（前台日常 + 只读） | 29 | `read_card` `list_rooms` `run_selftest` `issue_guest` `list_room_types` `list_room_statuses` `set_room_status` `get_max_cards_default` `room_move_preview` `room_move_out` `checkout_room` `checkout_card` `force_checkout_card` `get_checkout_status` `change_own_password` `get_reminder_settings` `get_checkout_hour` `expiring_cards` `list_issuances` `list_guest_stays` `list_audits` `get_onboarding_status` `list_reservations` `list_reservation_records` `create_reservation` `update_reservation` `cancel_reservation` `hide_to_tray` `quit_app` |
| 管理员 | 36 | `set_port` `create_room` `delete_room` `rename_room` `issue_install_card` `issue_admin_card` `list_departments` `create_department` `rename_department` `set_department_active` `create_room_type` `rename_room_type` `delete_room_type` `set_room_type` `set_room_max_cards` `set_max_cards_default` `update_room_status_style` `backup_database` `list_backups` `restore_database` `set_room_type_color` `set_checkout_status` `list_operators` `create_operator` `reset_operator_password` `set_operator_active` `set_reminder_settings` `set_checkout_hour` `get_backup_settings` `set_backup_settings` `import_backup_file` `export_diagnostics` `export_issuances_csv` `export_reservations_csv` `import_rooms` `complete_onboarding` |

约定：

- 错误码 **401 = 未登录**、**403 = 权限不足**、**409 = 达到发卡上限**；前端按 `code` 区分提示。
- 只读命令不要锁给管理员——前台查房、读卡、看到期是日常动作。
- 新增 / 改名命令必须同步本表；公开命令要在 PR 里说明理由。

---

## 7. 业务铁律（改房间 / 卡片代码前必读）

| 规则 | 原因 |
|---|---|
| 房间序号绑上锁后**永不复用** | 序号写进卡内页1（`5B EC` 之后的字节）；换成新序号后，旧卡不能再用于新房 |
| 删房是**逻辑删除**（`room.active=0`） | 历史发卡记录要留档，序号仍占位，`list_rooms` 返回全量 |
| 新增房间用到已删序号 = **复活那条记录改房名** | 否则撞 `room_index UNIQUE`；同时重置房态 / 发卡上限 / 安装卡记录 |
| 有有效卡时**不许改名、不许删房** | 发卡、退房、改上限都按 `room_no` 对账 |
| 「有效卡」口径 | `result='ok' AND card_no IS NOT NULL AND expires_at IS NOT NULL AND checked_out_at IS NULL AND expires_at > now` |
| 发卡 / 续住可在**空净房 / 未满房 / 预定房**点 | 房态是人工维护的业务字段，只做前端拦截；预定房发卡时自动预填姓名 |
| 持卡退房**只允许客卡** | 退房会擦卡（写零），管理卡放上去会被擦成空白；前后端都拦 |
| 发卡上限 | 全局默认 2 张（`0` = 不限）+ 房间可覆盖（留空跟随全局）；达上限返回 409 |
| 让旧卡失效 | 只发**挂失卡**（`页1[2]` 低 4 位 = 1），与「同房多发卡」是两套机制 |

---

## 8. 其它

- **目录**：`src/` 前端、`src-tauri/` Rust、`scripts/build-sdk.ps1`、`build-release.ps1`、
  `fixtures/`（样本说明，实际样本内联在 `ocw-device` 里）、`docs/使用手册.md`。
- **构建产物**：`src-tauri/resources/sdk/`（gitignore）、`app/output/`（交付目录，gitignore）。
- **未完成项**只认 [`../plan/待办清单.md`](../plan/待办清单.md)；本文件与根 README 只写现状。
- **许可**：MIT，见 [`../LICENSE`](../LICENSE)。可自由商用、修改、闭源分发，只需保留版权与许可声明；
  随包分发的厂商驱动 `LC32RFRW.dll` 不属本项目版权，见 `NOTICE`。
