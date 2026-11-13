# AGENTS.md

本仓库是「智能门锁管理软件（LOCK_V8 / V8.9）」TEMIC 卡的**逆向成果**，
并在其上开发开源发卡台 **OpenCardWright**。

2026-11-14 起，卡格式实现与门锁驱动（原 `sdk/` 那套 .NET SDK / CLI）已整体迁到独立仓库
**rc-device-sdk**，以 git submodule 挂在 `app/src-tauri/vendor/rc-device`：
产品侧不再持有任何卡字节知识，硬件兼容（驱动加载、卡格式、9001 身份证服务）全部由它负责。
仓库已纳入 git，见第 8 节。

改动前先读 `docs/`，那里是唯一权威来源；代码是文档的实现。

---

## 1. 目录职责

| 路径 | 内容 | 可否随意改 |
|---|---|---|
| `docs/TEMIC门锁卡技术规格.md` | 卡格式、卡型、卡号规则、锁行为、待验证项 | 结论需实测支撑才能改 |
| `docs/发卡调用链逆向.md` | 原厂 `NacreHis → SmartLock → SM_RW_Card → LC32RFRW` 调用链、IDA 地址 | 只追加新证据 |
| `docs/T5557-5567函数逆向.md` | 驱动 `te_*` 导出、串口帧、扰码、CRC、配置字 | 同上 |
| `docs/抓包样本与验证记录.md` | 原厂字节样本表、实测结论、复现方法 | 新样本**必须**登记在这里 |
| `docs/SDK使用说明.md` | .NET / COM / CLI 三种用法、错误码、FAQ | 接口变了才改 |
| `docs/接口文档.md` | 卡格式、驱动 `te_*`、`SM_RW_Card`、SDK/COM/CLI 接口汇总（对外/对下） | 接口变了才改 |
| `app/src-tauri/vendor/rc-device/` | **git submodule**：硬件兼容层（`rc_device` crate）。驱动加载、卡字节编解码、46 项黄金向量自检、身份证 9001 客户端、`rc-device-sidecar` 32 位宿主与 `--sim` 模拟后端 | 在**它自己的仓库**里改，改完提交并更新子模块指针 |
| `rc-device` 的 `drivers/LC2000/*.dll` | 原厂读卡器驱动（x86，随包分发，手动拷进产品资源，见第 2 节） | **不要动**，见第 3 节哈希 |
| `data/NacreHis.json`、`data/RoomIndex.ini` | 数据库导出（303 表）、房号↔序号 | 只读参考 |
| `vendor/125k-sdk-v4.2/` | 原厂 SDK 开发模块留档（IDA / dumpbin 分析材料） | 只读；结论进 `docs/` |
| `app/` | 开源发卡台产品，**独立的 Tauri + Rust 工程**（手动 cargo 编 sidecar → `pnpm` → `build-release.ps1`） | 见第 9 节 |
| `app/docs/使用手册.md`、根 `CHANGELOG.md` | 面向最终用户的手册、版本变更 | 只写结论；未完成项进 `plan/待办清单.md` |
| `app/build-release.ps1`、`.github/workflows/ci.yml` | 一键 MSI 发布（需 WiX Toolset v3）、CI 门禁（sidecar selftest / cargo test / cargo check / 前端） | 发布链变了要同步 |
| `plan/` | 建设方案（v0.1 作废留档、v0.2 已批准）、`待办清单.md`（索引）、`已完成.md`（摘要）与 `td-xxx.md`（条目详情） | 方案改结论要留痕；待办清单每轮更新，见第 10 节 |

`sdk/` 已在 2026-11-14 整体移除（历史见 git）。`plan/td-*.md` 里出现的 `sdk/...` 路径
都是**当时快照**的历史记录，不是现行路径；现行实现看 `app/src-tauri/vendor/rc-device/src/`。

---

## 2. 构建（硬件层手动编，不依赖脚本）

卡字节与驱动绑定不在本仓库编译。硬件层由开发者手动用 cargo 编出来，四步（在 `app/` 内执行）：

```powershell
# 1) 编 32 位 sidecar（驱动只有 x86，64 位进程加载不了）
cargo build --manifest-path src-tauri/vendor/rc-device/Cargo.toml `
  --bin rc-device-sidecar --release --target i686-pc-windows-msvc `
  --target-dir src-tauri/target/sidecar

# 2) 摆成 sidecar 默认查找的布局（exe 同目录的 drivers/LC2000）
New-Item -ItemType Directory -Force -Path src-tauri/resources/device/drivers/LC2000 | Out-Null
Copy-Item src-tauri/target/sidecar/i686-pc-windows-msvc/release/rc-device-sidecar.exe src-tauri/resources/device/ -Force
Copy-Item src-tauri/vendor/rc-device/drivers/LC2000/* src-tauri/resources/device/drivers/LC2000/ -Recurse -Force

# 3) 门禁：46 项黄金向量（--sim 不加载驱动；selftest 本来就是纯计算）
@('{"id":1,"op":"selftest"}','{"id":2,"op":"shutdown"}') |
  & src-tauri/resources/device/rc-device-sidecar.exe --sim

# 4) 起开发环境（前端 + Tauri 壳）
pnpm tauri dev
```

- 第 3 步的响应必须 `"passed":46,"failed":0`；对不上就停下，不要上机。
- `resources/device/` 是构建产物，不入库（`app/.gitignore`）。驱动指纹由程序启动时**现算**
  SHA-256（`provision.rs`），不再有 `manifest.json` 之类的构建清单要维护。
- 驱动哈希必须等于 `F2F483E0…BCCBC0`（102,492 字节、92 导出）；不符只在日志与设置页告警，
  但卡格式结论对该版本未必成立，发布前必须在测试机跑一次真实读写。
- **发布打包（`pnpm tauri build`）之前必须先跑第 1–2 步**：MSI 打包的就是 `resources/device`，
  没准备就会带着「模拟设备」出门。
- 改硬件行为（卡字节、驱动调用、超时、模拟）一律在 **rc-device-sdk 仓库**里改并提交，
  然后在本仓库更新子模块指针；**不要**把卡字节逻辑写回产品侧。

## 3. 验证（不需要读卡器）

```powershell
cargo test --manifest-path app/src-tauri/vendor/rc-device/Cargo.toml   # 硬件层：201 单测 + 契约 + sidecar
cargo test --workspace --manifest-path app/src-tauri/Cargo.toml         # 产品侧 156 项（含真起 sidecar --sim）
```

- 46 项 = 33 条字节向量 + 7 条卡号编码（含进位/跨日）+ 6 条时长码，取自
  `docs/抓包样本与验证记录.md` §1（31 行原厂样本）与月数/校时实验表；校时卡 6 条已进向量。
- `selftest` 是纯函数、不打开串口，随时可跑；构建前按第 2 节第 3 步手工跑一遍。
- 需要硬件时：在 `vendor/rc-device` 内
  `cargo run --bin probe --target i686-pc-windows-msvc`；
  看模拟路径用 `cargo run --bin probe -- --sim`。
  驱动哈希不符、串口被 `NacreHis.exe` 占用都会失败。

改任何组包逻辑后，**先让 46 项全绿**再谈上机。

## 4. 代码约定

1. **分层不许破**：字节格式只写在 `rc-device` 的 `builder`（纯函数、可离线验证）；
   串口/驱动调用只写在 `src/dll/`（唯一 `unsafe` 区）。产品侧（`commands.rs` / `state.rs`）
   只允许出现 `rc_device::*Spec`，**不要**自己拼字节或 CLI 参数。
2. **错误码一套**：硬件层给 `DeviceErrorPayload`（`sdkCode` 0–11、`kind`、`hint`、`deviceFault`），
   产品侧 `ocw-device::DeviceError` 原样搬运，命令层只把它映射成 `CommandError`；
   不要再引入第二套码，也不要按进程退出码猜语义。
3. **卡号与 `CardNo.state`**：`CardNo.state` 只是内部计数器，由产品侧 `cardno.rs` 在发卡时读写；
   `setting.startCardNo` 不再读取，现有 `CardNo.state` **保留、不重置**。
   `issuance.card_no` **恢复写入**（16 位作业流水号）；没有流水号的卡型（安装卡、空白卡、
   原始直写）记 NULL。**它不当唯一键**——不含年月，跨月同日同时同序会重复；
   发卡记录之间的关联键仍是 `raw_hex`（28 字节整卡指纹），主键是自增 `id`。
4. **文档同步**：改格式要同时 (a) 在 `抓包样本与验证记录.md` 登记样本，(b) 在 `selftest` 加向量，
   (c) 更新技术规格对应小节。三者缺一视为未完成。
5. **证据标注沿用文档惯例**：`【实测】`=有抓包或联机证据，`【推断】`=由证据归纳，`【待定】`=机制未证实。
   不要把 `【推断】` 悄悄升级成结论。

## 5. 硬性事实（改动时不要违背）

卡内只有 8 字节有效数据（7 页 × 4 字节，页 3–7 全 0）：

```
页1 = 5B EC | ((序号 & 0x0F) << 4 | 低4位) | (序号 >> 4)
页2 = 时长码 | 卡号低 | 卡号高 | **年月**（高 4 位 = `(年−2000) & 0x0F`、低 4 位 = 月；还原时 `年 = 2000 + 16×⌊(当前年−2000)/16⌋ + 高4位`，**16 年一循环**。**不是常量**：2026-11 写 `0xAB`、2027-11 写 `0xBB`，已实测。见 `docs/TEMIC门锁卡技术规格.md` §4.2）
```

| 项 | 规则 |
|---|---|
| 时长码（天） | `0x80 ｜ 天数`（1–63）；`>63` 天走 `0xC0 ｜ round(天数/30)` |
| 时长码（小时） | 客卡 `0x40 ｜ (小时+1)`；总控卡 `0x40 ｜ 小时`（差 1，实测） |
| 总控卡页1 | `5B EC F0 FF` |
| 安装卡页2 | 固定 `46 F2 00 01`，**不含**作业流水号 |
| 校时卡 | 编码的是 **`YYMMDDHHmm`**（年月日时分）：`YYMM` 在 `页2[3]`、`DD` 在卡号日段、`HH:mm` 在页1 位域（`页1[2] = 16 × (分 & 0x0F) + 9`、`页1[3] = (分 >> 4) + ((4 × 时) & 0x7C)`）；页2 首字节 **`0x06`**（明文，IDA: `LOBYTE(v14[2]) = 6`）；页1[2] 必须满足 `& 0x09 == 0x09`，否则读卡返回错误 4。旧公式 `16×当日分钟+953` 已废弃 |
| 作业流水号 | **10 位十进制 `YYMMDDHHII`**（年 2 + 月 2 + 日 2 + 时 2 + 小时内序号 2；如 `2611081100` = 2026-11-08 11 时第 00 号）。写卡时拆两处：`页2[3]` 存 `YYMM`（高 4 位年、低 4 位月），16 位卡号存 `DDHHII` = `(日 & 0x1F) << 11 ｜ (时 & 0x1F) << 6 ｜ 序号(0–62)`；满 63 张则小时 +1、序号归 0。客卡 / 总控 / 复位 / 清除 / 退房钟点 / 校时共用。产品侧按发卡时刻生成并写卡，同时把 16 位 `DDHHII` **落库到 `issuance.card_no`**（安装卡 / 空白卡 / 原始直写记 NULL）；年月随整卡指纹留在 `raw_hex`，**记录关联不用卡号**（跨月会重复），继续按 `raw_hex` 匹配 |
| CLI 时间源 | `issue --datetime "YYYY-MM-DD HH:mm"`（`--at` 同义）驱动 `CardBuilder.Now`（`页2[3]` 的 `YYMM`）；**`DDHHII` = `日<<11 ｜ 时<<6 ｜ seq`** 用同一个时间 + `--seq` 算出，两段天然同源。CLI 不再维护计数器；`--serial` 可显式指定 `DDHHII` 部分 |
| 挂失 | `页1[2]` **低 4 位** = 1 即挂失（`BuildGuestDay/Hour(..., lost)`）；原 `LowNibble`（`天数&3==1`）已删除，那会让 1 天卡/钟点卡被误标挂失 |
| 时长码·月 | 超 63 天走 `0xC0 \| 月数`，**按 30 天/月换算**（90→3 月、1095→**36 月**）；锁按日历月执行 |
| 到期语义 | **锁按卡内发卡时间计算，不依赖首次刷卡**：日租卡 = 发卡日 + N 天的**退房钟点**（由退房钟点卡写进锁，如 12 点 → 次日 12:00）；钟点卡 = 发卡时刻（小时级）+ N 小时。前提：锁内时间已用校时卡校准、退房钟点已用退房钟点卡设置；卡内没有分钟 |
| 发卡数量 | **全局默认 2 张**（「设置 → 发卡数量」可改，`0` = 不限）+ **房间可单独覆盖**（留空跟随全局、`0` = 不限、`n` = 具体张数）。有效卡张数达上限时发卡直接返回 409。同房多张卡各用新的作业流水号，**不复用首卡流水号** |
| 让旧卡失效 | 只能发**挂失卡**（`页1[2]` 低 4 位 = 1）；与「同房多发卡」是两套独立机制 |
| 卡号字节序 | `DDHHII` 16 位、低字节在前；`N_H << 8 ｜ N_L` |
| 退房匹配 | 持卡退房：`页2[3]` 取 `YYMM`、卡号解出 `DDHHII`，拼回完整作业流水号后结合房号 + 卡型反查 `issuance`；无卡退房：按记录 id 关闭。**同号复用已取消**，每张卡独立流水号 |
| 读卡退房范围 | **只允许客卡**（日租/钟点）。持卡退房会擦卡（写零），若把总控/复位/安装/校时卡放上去会被擦成空白；后端 `checkout_card` 与前端 `CheckoutCardModal` 都会拦 |
| 备份完整性 | 备份走 `ocw_store::backup_to` → `rusqlite::backup::Backup::run_to_completion`（SQLite 在线备份 API）：**逐页拷贝到完成 = 全量**，含 `-wal` 里未 checkpoint 的数据，且不锁库。**不要改成 `std::fs::copy`**——文件级拷贝会漏 WAL，恢复后少掉最后一段事务 |
| 恢复账号 | 恢复整库后按 `login` 合并：当前已有账号保留密码 / 角色 / 启用状态；备份新增账号导入但停用；当前独有账号补回（部门置空）。`RestoreResult` 返回房间 / 发卡 / 预定 / 账号数与账号处理计数；合并失败不回滚整库，可用 `.bak` 回滚 |

驱动侧固定参数：写密码 `12000206`，必须 `config_parameter(0)`（明文，加扰模式 `3` 会"能读不能写"），
写卡走 `te_setuppwm_asc` + `te_writepwm_asc`（`te_write` 对空白卡无效），写完立刻回读可能拿到旧值
（`WriteAndVerify` 已重试 3 次）。

## 6. 现场风险（不要为了验证而做的事）

- 复位卡本身不设防：任何持卡人都能让旧卡复活，测试/演示时不要留在现场。
- 清除卡、安装卡**尚未在锁上验证过真实效果**（字节已与原厂一致）；刷清除卡可能让锁变"未安装"。
- `lockcode_temic`（`Xt_cs` 表）只被原厂高层 `tem_*_sdk` 使用，本方案不用，但换机/迁移时要保持一致。

## 7. 文档一致性现状

2026-11-05 已把文档里的「vendor / 原厂方案」主用说法删净；
`vendor/125k-sdk-v4.2/` 仅作分析材料留档，不是实现路径：

| 曾经的偏差 | 处理 |
|---|---|
| `SDK使用说明.md` 顶部把 `vendor\` 说成主用方案 | 已删；现只说本 SDK 是唯一实现 |
| 自检样本数写成 19 | 已改为 **27**（README / SDK使用说明 / 技术规格 §12 一致） |
| `PMSLockSDK.cs` 头注释写"机制待定" | 已改为按作业流水号写入 |
| 旧逆向结论「锁按房间 × 卡型比较高水位」 | **已废止**（2026-11-10）：现行模型只有作业流水号（`YYMMDDHHII`，10 位）；`docs/` 相关段落标【已废止】，产品不再有水位表述 |
| 技术规格残留早期"校时卡（未解）"推导 | 已删除，只保留 §5.3 实测公式 |
| `发卡调用链逆向.md` §8 曾提供 `VendorBridge.exe` | 已标注为历史记录，仓库不再提供该工具 |

**仍存在的不一致（无害）**：

- `RoomIndex.ini` 有两份（`data/` 与历史 `sdk/` 副本）；`sdk/` 已于 2026-11-14 移除，
  现行只用 `data/RoomIndex.ini` 作参考——房号↔序号的真值在 SQLite 的 `room` 表。
- `plan/td-*.md` 里的 `sdk/PMSLockSDK.cs`、`PMSLockCard.exe` 路径都是**当时快照**；
  这些条目记录历史，不要照着它们定位现行代码。
- `plan/待办清单.md` 正文完成记录里的数字滞后（TD-150 仍写 39 项 / 126 项测试）；
  判定完成看 §1 索引与代码实跑。

## 8. 环境注记

- 2026-11-06 本机 `pwsh` 7、`npm` 均可用；2026-11-07 起前端包管理改用
  **pnpm**（2026-11-14 实测手动编 sidecar 后 selftest 46/46 通过）。`app/pnpm-workspace.yaml` 的
  `allowBuilds: esbuild` 不能删，否则 `pnpm install` 会以 `ERR_PNPM_IGNORED_BUILDS` 失败。
  若某会话 `pwsh` 启动失败（`0xC0000142` DLL 初始化失败），先确认能否编译，
  不要把“没跑过命令”当成验证通过。
- 硬件层构建需要 **Rust + `i686-pc-windows-msvc`**（`rustup target add`）；
  2026-11-14 起**不再需要** `csc` / .NET Framework。
- **`cargo` 可能不在 PATH**：本机绝对路径 `C:\Users\Administrator\.cargo\bin\cargo.exe`。
  产品测试在 `app/src-tauri/` 内跑；硬件层测试用
  `cargo test --manifest-path app/src-tauri/vendor/rc-device/Cargo.toml`（仓库根没有 `Cargo.toml`）。
- **`D8050` 与中文路径无关**：真因是 `libsqlite3-sys` 编 `sqlite3.c` 时命令行过长。项目现在
  **不再覆盖** `CARGO_TARGET_DIR` / `DEBUG`，统一使用 Tauri/Cargo 默认的 `app/src-tauri/target`。
  若本机默认 target 仍触发 D8050，作为本机环境问题单独处理，不要写回仓库脚本。
- **双通道（正式版 / DEV）**：`build-release.ps1 -Channel dev` 用 `src-tauri/tauri.dev.conf.json`
  覆盖 identifier（`…desktop.dev`）/ productName（`门锁发卡台 Dev`）/ 窗口标题 / WiX `upgradeCode`，
  并设编译期 `OCW_CHANNEL=dev`；数据目录因此分开（`OpenCardWright` / `OpenCardWright-dev`），
  两版可同时安装、同时打开（串口独占，读卡器只有一边能连）。顶栏标签读 `AppInfo.channel`：
  正式版品牌渐变、DEV 橙色。
- 驱动版本敏感：结论只对 SHA-256 `F2F483E08BA8048FFD1C57E66CB35D9E32C1C1E280DC5F2FF100A5D881BCCBC0`
  的 `LC32RFRW.dll`（102,492 字节、92 个导出）成立。厂商升级驱动需重新评估。
- **版本号**：三段 `A.B.C`，**没有独立的构建号段**——Windows Installer 的
  `ProductVersion` 只认 `major.minor.build` 三段，每段 ≤65535（历史上把年份塞进主版本
  就撞过 `app version major number cannot be greater than 255`）：
  - `A` 主版本：大量功能重做或有不兼容改动时 +1（`1.x.x` → `2.0.0`）；
  - `B` 次版本：增加新功能或改进现有功能时 +1，`C` 归 0（`1.0.x` → `1.1.0`）；
  - `C` 构建号：**每打一次包 +1**（同日重打包也 +1）。它同时充当 MSI 的 build 段，
    这样重打包才能覆盖安装——同版本 MSI 会被判定为「已安装」而装不上。
  新体系从 `1.0.0` 起，旧的 `26.M.DDD`（日×100+序号）已废止。
  每次改动在 `CHANGELOG.md` 记一条并写明这次为什么升这一段；**四处同步**：
  `app/package.json`、`app/src-tauri/Cargo.toml`、`app/src-tauri/tauri.conf.json`、
  `app/src-tauri/Cargo.lock`。不要只改一个位置；版本号不一致会导致安装包、关于页和
  Cargo 产物对不上。
- 版本库：仓库已纳入 git（远程 `origin` = `github.com/1665169869/OpenCardWright`）。
  **未明确要求不得提交**：除非用户当轮明确说要「提交 / commit / 推送」，否则只改工作区、在回复里列出改动，
  **不要 `git add` / `git commit` / `git push`**；需要提交时先给 `git status` 摘要等用户点头。
  `app/src-tauri/resources/device/` 是构建产物，不入库；子模块改动要在 rc-device-sdk 仓库里单独提交并更新指针。

---

## 9. 产品工作区 `app/`

2026-11-05 起，在本仓库内并行开发开源产品 **OpenCardWright**（方案见
`plan/开源门锁发卡软件-建设方案-v0.2.md`）。两者的关系是**默认只读依赖**（例外见下表），不是同一套构建：

| 关系 | 说明 |
|---|---|
| 硬件层的定位 | 产品的**设备实现**，已迁到独立仓库 `rc-device-sdk`（git submodule，`app/src-tauri/vendor/rc-device`）。构建期用 cargo 编 32 位 `rc-device-sidecar`，产物打进 `src-tauri/resources/device/` |
| 能否修改硬件层 | **可以，但要在它自己的仓库里改**：改完提交到 rc-device-sdk，再更新本仓库的子模块指针；卡字节、驱动调用、超时、模拟都属于硬件层 |
| `docs/` 的定位 | 卡格式与锁行为的**唯一权威来源**；产品实现与硬件层都必须与之一致 |
| 回归口径 | 硬件层自带 46 项黄金向量；构建前按第 2 节第 3 步手工跑一遍 |
| 构建命令 | 在 `app/` 内：按第 2 节手动编 sidecar → `pnpm tauri build --bundles msi`（或发布脚本 `build-release.ps1`） |
| 编译环境坑 | MSVC `D8050` 是 **命令行过长**（`sqlite3.c`），不是中文路径；项目使用 Cargo/Tauri 默认 target，不覆盖 `CARGO_TARGET_DIR` / `DEBUG`。详见 `app/README.md` §5 |

新产品自己的约定（技术栈、目录、阶段）以 `app/README.md` 为准。

### 产品侧模块（改代码先定位）

| 位置（相对 `app/`） | 职责 |
|---|---|
| `src/` | React 19 + antd 5 前端；`api.ts` 是唯一的 Tauri invoke 封装，页面在 `pages/`，复用组件在 `components/`，校验与提示音在 `utils/`。**前端不再接触硬件协议**：门锁卡与身份证都只经 `api.ts` 调命令 |
| `src-tauri/src/commands.rs` | 全部 `#[tauri::command]`：发卡 / 退房 / 房间 / 管理卡 / 备份 / 诊断 / 登录 / 读身份证，以及命令 × 角色矩阵 |
| `src-tauri/src/rules.rs` | 时长边界（客卡 62 / 总控 63）、长有效期档位与确认词；**与 `src/api.ts` 的档位表必须同步** |
| `src-tauri/src/cardno.rs` | `CardNo.state` 原子读写 + 时钟回拨保护；卡号编码与进位交给 `rc_device::{timestamp_card_no, next_stamp}` |
| `src-tauri/src/auth.rs` | 会话、角色（admin / frontdesk）、Argon2 口令 |
| `src-tauri/crates/ocw-store/` | SQLite schema 与查询（rusqlite bundled）；`issuance` 只追加，序号唯一 |
| `src-tauri/crates/ocw-device/` | 设备门面：常驻 sidecar 会话（NDJSON）、硬件快照 → 界面 DTO、错误翻译、进程内模拟后端 |
| `src-tauri/src/{provision,backup_job,logging,diagnostics}.rs` | 运行目录（复制 `resources/device`）、自动备份、文件日志、诊断包。**备份走 SQLite 在线备份 API**（`ocw_store::backup_to` → `rusqlite::backup::Backup::run_to_completion`），逐页拷贝到完成，**是全量备份**（含 `-wal` 里未 checkpoint 的数据）且不锁库；备份后把目标库设为 `journal_mode=DELETE`，避免「db + db-wal 两个文件才算完整」。**不要改成 `std::fs::copy`**——那会漏掉 WAL 中未刷盘的事务 |
| `src-tauri/src/room_import.rs` | 房号 INI / CSV 导入（坏行收集不致命、删房序号可复活）；GBK 解码在本文件，与设备层无关 |
| `src-tauri/src/demo_import.rs` | 仅 `debug_assertions`：空库时从 `data/NacreHis.json` 的 `Kf_fh` 建 30 个房间，发布版不编译 |

产品工作区常用命令（在 `app/` 内）：

```powershell
pnpm install
# 硬件层：按第 2 节手动编 sidecar 并拷到 resources/device（打包前必须做）
pnpm dev            # vite（只起前端，不含 Tauri 壳）
pnpm tauri dev      # 前端热更新 + Tauri 壳；改 Rust 会自动重编译+重启
pnpm build          # tsc --noEmit + vite build
pnpm exec tsc --noEmit       # 只做前端类型检查
pnpm tauri build --bundles msi   # 出 WiX MSI 安装包；不要用 cargo build --release（出的是 dev 壳）
pwsh -NoProfile -File build-release.ps1   # 一键正式版：MSI → app/output，并打印 SHA-256
```

打包前置条件：**WiX Toolset v3**（`candle.exe` / `light.exe`），用 `-WixBin` 或 PATH 指定。
NSIS / Inno Setup 已废弃，`pnpm setup:nsis` 与相关脚本都不存在了。

测试在 `src-tauri/` 内跑，几种粒度都常用：

```powershell
cargo test --workspace                                   # 产品侧全部；2026-11-14 实测 57 项全绿
cargo test -p ocw-store                                  # 只跑存储层（56）
cargo test -p ocw-device --test sidecar_session          # 真起 sidecar（--sim）的端到端（10）
cargo test -p ocw-store insert_room                      # 按用例名子串单跑
cargo test --manifest-path vendor/rc-device/Cargo.toml   # 硬件层：201 单测 + 10 契约 + 6 sidecar
```

> 2026-11-14 迁移后：产品侧 57 项（含鉴权矩阵 84 条命令）；设备层 21 单测 + 10 集成；
> 硬件层 201 单测 + 10 契约 + 6 sidecar。GBK 解码只剩在 `room_import.rs`（CSV 导入），与设备无关。

无读卡器时用模拟设备：应用启动环境变量 `OCW_SIM=1`（或用 sidecar 的 `--sim`）。
两种模拟写出的字节与真机逐字节相同（都走 `rc_device::builder`），只是写进内存卡槽。

**运行时数据不在仓库里**，且**调试与正式环境分目录存放**（`provision.rs::data_root()`）：

| 构建 | 数据目录 |
|---|---|
| `pnpm tauri dev` / `cargo run`（debug） | `%PROGRAMDATA%\OpenCardWright-dev\` |
| 安装包（release） | `%PROGRAMDATA%\OpenCardWright\` |

两个目录里都有 `opencardwright.db`（SQLite）+ `runtime\`（构建期从 `src-tauri/resources/device` 拷入，
含 sidecar 与 `drivers/LC2000`）。
debug 构建在空库时会自动导入 `data/NacreHis.json` 的 `Kf_fh`，建 30 个房间 + 房型 + 房态；发布版不含这段。
查数据时先确认自己看的是哪个目录，不要指望仓库内有 `*.db`。

### 房间与卡片的业务铁律（改房间/发卡相关代码前必读）

| 规则 | 原因 |
|---|---|
| 登记 ≠ 发卡 | 两者是独立入口：发卡只写卡与落库、**不改房态**；登记只录住客与房态、不碰读卡器。发卡时若该房有登记中的单，卡会挂到那张单上（弱关联）；退房时**该单关联的卡全部失效**才自动关单。退房弹窗可勾「同时清除入住登记信息」（默认勾选），勾了立即强制关单——**只关不删**，住客档案仍在记录页 |
| 预定 → 入住单 | 三层职责固定：`reservation`（预定）→ `checkin`（入住单）→ `checkin_guest`（登记单）。登记单可挂 `checkin.reservation_id`；**第一位住客加成功**时把该房今天覆盖的 `booked` 预定转成 `checked_in` 并回写关联（`convert_reservation_for_room`）——空单不消耗预定，同房多条只转最早一条。发卡路径仍走 `check_in_reservations`（按房号+日期批量标已入住、不写关联，因为发卡时可能还没建单） |
| 序号范围 **0–4094**（12 bit） | 序号 = `B2` 高 4 位 << 4 \| `B3`。**4095 与总控卡页1（`5B EC F0 FF`）字节相同**，分给房间会让该房客卡被读成总控卡——schema `CHECK`、`insert_room_reporting`、导入器三处都拦。4080–4094 的页1 是 `*FFF`，与总控卡只差 B2 高 4 位，前端填号会二次确认 |
| 房间序号绑上锁后**永不复用** | 序号写进卡内页1（`5B EC` 之后的字节）；换成新序号后，旧卡不能再用于新房 |
| 删房是**逻辑删除**（`room.active=0`） | 历史发卡记录要留档，序号仍占位，`list_rooms` 返回全量 |
| 新增房间用到已删序号 = **复活那条记录改房名**，不插新行 | 否则撞 `room_index UNIQUE`；同时重置房态/发卡上限/安装卡记录 |
| 有有效卡时**不许改名、不许删房** | 发卡、退房、改上限全按 `room_no` 对账，改名会让在场卡对不上号 |
| 「有效卡」口径 | `result='ok' AND card_no IS NOT NULL AND expires_at IS NOT NULL AND checked_out_at IS NULL AND expires_at > now` |
| 发卡 / 续住写卡可在**空净房 / 未满房 / 预定房**点（预定房发卡时自动预填预定姓名） | 房态是人工维护的业务字段，只做前端拦截，后端不硬拦（旧流程允许先发卡再改房态） |

### 前端两个反复踩的坑

- **HMR 会保留组件 state**，弹窗/抽屉里上次的表单数据会残留。修法是给组件加一个自增计数
  当 `key` 强制重挂载（`RoomManager` 的 `checkoutSession` 就是为此）。
- npm 时代 `pnpm install` 会重写 `package-lock.json` 的 `libc` 元数据产生无意义 diff；2026-11-07 起改用
  **pnpm**（`pnpm-lock.yaml`），此坑基本消失；若 lock 出现无意义 diff，`git checkout -- app/pnpm-lock.yaml` 还原。

产品侧高频踩坑（详见 `app/README.md` §5）：禁用 antd 命令式弹窗（React 19 已移除其依赖的
API，调用静默失效）；写卡**超时不要重试**（重试会把同一张卡写两遍、卡号推进两次）。

- 新增/改名 Tauri 命令必须同步 `app/README.md` §6 的「命令 × 角色」表；
  `commands.rs` 的 `auth_matrix_matches_every_command` 会机械校验，漏登记即红。
- 时长边界改一处不算完：`src-tauri/src/rules.rs` 与 `src/api.ts` 的档位表必须同步。

计划与未完成项的登记规则见第 10 节。

---

## 10. 计划与待办

`plan/` 是仓库里唯一的计划载体，`plan/待办清单.md` 是**未完成需求的唯一权威**。
写在这里才算数；只存在于对话、用户目录或本机记忆里的计划，换一台电脑就没了。

| 规则 | 说明 |
|---|---|
| 唯一载体 | 未完成项登记在 `plan/待办清单.md` §1 索引；每个条目的具体实施需求写在 `plan/td-xxx.md` |
| 已完成归档 | `plan/已完成.md` 只保留摘要；每个已完成条目的详细记录写在对应 `plan/td-xxx.md` |
| 索引优先 | `plan/待办清单.md` §1 索引是状态权威；`td-xxx.md` 的现状/完成记录里的项数/hash/测试数可能滞后，动手前用 `git log` 与实跑复核 |
| 每轮收口 | 一轮开发结束、**提交之前**，把本轮没做完的项写进对应 `td-xxx.md`，「状态 / 现状 / 验收标准」三样齐全；完成后摘要移入 `已完成.md`，详情留在/写入 `td-xxx.md`；没有新增就把状态改准 |
| 进度不再写方案 | `plan/…建设方案-v0.2.md` §15 只记已完成与偏差，不再追加「未实现」；新需求一律进待办清单（索引 + `td-xxx.md`） |
| 其它文档只写结论 | `app/docs/使用手册.md` 面向使用者写「现在能不能用」，`CHANGELOG.md` 面向版本写「这版有什么」，`app/README.md` 面向开发者写构建与踩坑，都只放指针、不复制条目 |
| 不引入外部跟踪 | 不用 GitHub Issues 或在线看板：另一台电脑可能没网、没 gh 登录 |
| 来源可追溯 | 每条待办在清单 §7「来源登记表」有出处；登记前先查表，避免同一需求登记两次 |
| 待拍板显式化 | 需要用户决定的（有效期怎么存、音色怎么来、某功能是否废弃）必须写进条目「待拍板」并给建议默认，不许自行假设 |
| 大改先出方案 | 涉及**数据库表字段的增删改**、或耦合过高的重构（改跨模块边界 / 改公共数据结构 / 动发布链），**先给 2–3 个方案（含各自利弊与推荐）让用户选**，选中后再实施；不许直接开工 |

用户目录下的 agent 计划（如 `~/.workbuddy/plans/`）**只当草稿**：它不入库，
交付前必须把未完成项搬进 `plan/待办清单.md` 与对应的 `td-xxx.md`，否则下一个接手的人看不到。
