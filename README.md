# OpenCardWright

**TEMIC 125 kHz 门锁卡的独立发卡方案**：对「智能门锁管理软件（LOCK_V8 / V8.9）」所用卡片做了完整逆向，
并给出不依赖原软件、不依赖加密狗的两条使用路径。原厂已停运，本仓库可直接部署。

| 你想做什么 | 用哪个 |
|---|---|
| 前台日常发卡：读卡、发卡、续住、退房、房态与预定 | **OpenCardWright 发卡台**（`app/`），装 MSI 即用 |
| 在自家程序里集成发卡 | **硬件层** `rc-device`（Rust crate / 32 位 sidecar 的 NDJSON） |

卡格式与逆向结论以 [`docs/`](docs/) 为唯一权威来源，代码是文档的实现。
本轮实测基线（2026-11-10）：SDK 自检 **46/46**，产品 Rust 测试 **136 项**。

---

## 一、能做与不能做

| 能 | 不能 |
|---|---|
| 日租客卡（1–1095 天）、钟点客卡（1–62 小时） | 应急码设置卡、楼栋 / 楼层 / 应急 / 通道开门卡（**无样本，格式未知**） |
| 总控卡、复位卡、清除卡、安装卡、退房钟点卡、校时卡、空白卡 | 读写 TEMIC 之外的 IC / M1 / EM4305 卡 |
| 读卡并解析卡型、房号、序号、天数 / 小时、卡号、年月、挂失位 | 绕过原软件授权 / 加密狗（本方案本就无需授权） |
| 挂失客卡（发一张新卡让同房旧卡失效） | 解释锁固件内部的到期判定细节（只知按卡内发卡时间 + 锁内退房钟点） |

---

## 二、方式一：发卡台（`app/`）

Tauri 2 + Rust + React 19 的 Windows 桌面程序，数据存本机 SQLite，按角色登录。

**安装**：`app/output/OpenCardWright_<version>_x64.msi`（WiX 生成，中文安装页），
或按第六节自行构建。release 数据目录 `%PROGRAMDATA%\OpenCardWright\`，
debug 构建用 `%PROGRAMDATA%\OpenCardWright-dev\`，两者互不干扰。

**首次启动**：设管理员密码（8–128 位、必须含字母和数字）→ 跑首启向导 → 导入房间
（`RoomIndex.ini` / CSV）或逐个新增，并给每个房间做一张安装卡到锁上绑定序号。

| 模块 | 能做什么 |
|---|---|
| 读卡 | 卡型、房号、序号、时长、卡号、年月、挂失位、28 字节原文；数据库里没有记录也照常显示卡片数据 |
| 发卡 | 日租 / 钟点 / 挂失卡；写卡前强制预读；可录入客人身份信息（只存库，不写卡） |
| 续住 / 换房 | 续住默认「剩余天数 + 1」；换房先预览再关闭源房有效卡（旧卡仍能开原房，新房需重写） |
| 退房 | 顶栏「读卡退房」二级表单；房间右键支持持卡退房 / 无卡退房；非本系统卡走「强制退房」并二次确认 |
| 预定 | 按入住 / 退房日期预定，同房不允许重叠，可改期、取消；预定房发卡时自动预填姓名 |
| 房态日历 | 行 = 房间、列 = 间夜（12:00 → 次日 12:00）的矩阵，点空白格预定、点色块改期 / 取消 |
| 房间治理 | 新增房间自动分配序号、改名、逻辑删除；房型与房态可自定义颜色 / 图标；每房可设最大发卡数 |
| 管理卡 | 总控、复位、清除、安装、退房钟点、校时、原始直写、空白卡；二次确认 + 审计留痕 |
| 账号 | `admin` / `frontdesk` 两角色，可按部门管理；命令级权限由后端强制 |
| 运维 | 发卡 / 入住 / 审计 / 预定四类记录与 CSV 导出、诊断包、手动 + 自动备份、文件日志 |
| 设备 | 串口探测与切换、46 项自检、驱动指纹；无读卡器时设 `OCW_SIM=1` 走模拟设备 |

最终用户手册见 [`app/docs/使用手册.md`](app/docs/使用手册.md)。

> ⚠️ 门锁是人身与财产安全设备。复位卡能让旧卡复活，清除卡可能让锁变「未安装」，
> 管理卡操作请交由受训管理员，演示后不要把复位卡留在现场。

---

## 三、方式二：把硬件层嵌进你自己的程序

硬件层（驱动加载、卡字节编解码、身份证 9001 客户端）在独立仓库
[`rc-device-sdk`](https://github.com/1665169869/rc-device-sdk)，本仓库以 git submodule 挂在
`app/src-tauri/vendor/rc-device`。它**不含任何业务规则**（房号文本、房态、入住单），只认房间序号。

| 你的技术栈 | 用哪个 | 入口 |
|---|---|---|
| Rust | `rc-device` crate | `CardDevice` / `IdCardReader` trait |
| 任意语言（64 位宿主） | `rc-device-sidecar.exe`（32 位） | stdin/stdout 的 NDJSON |
| 无硬件演示 / CI | 同上加 `--sim` | 同一份协议，内存卡槽 |

```rust
use std::path::Path;
use rc_device::dll::DirectDllDevice;
use rc_device::{timestamp_card_no, CardDevice, CardIssuedAt, GuestDaySpec};

let dev = DirectDllDevice::with_auto_port(Path::new("drivers/LC2000"));
dev.open()?;

let now = CardIssuedAt { year: 2026, month: 11, day: 14, hour: 15 };
let card_no = timestamp_card_no(&now, 1)?;
let card = dev.write_guest_day(&GuestDaySpec {
    room_index: 15, // 12 bit；0–4094 可分配给真实房间
    days: 1,
    card_no,
    lost: false,
    issued_at: now,
})?;
println!("{}", card.raw_hex());
```

- `room_index` 是硬件层唯一的身份：房号文本由业务层自己维护，读写卡时只做映射。
- **不要分配 4095**——它与总控卡字节相同。
- 卡片字节与 46 项黄金向量由硬件层负责；业务侧不要再写一套编码。
- 身份证读卡器（`IdCardReader`，本机 `ws://127.0.0.1:9001`）与门锁卡是**两个独立设备**，
  任一故障都不影响另一个。

### 3.1 sidecar 协议（64 位宿主）

`LC32RFRW.dll` 只有 x86 版本，64 位程序加载不了，所以门锁卡操作交给 32 位 sidecar，
一行一个 JSON（NDJSON）：

```text
{"id":1,"op":"read_card"}
{"id":2,"op":"write_guest_day","spec":{"roomIndex":15,"days":1,"cardNo":9220,"lost":false,"issuedAt":{"year":2026,"month":11,"day":14,"hour":15}}}
{"id":3,"op":"write_raw","spec":{"p1":[91,236,240,0],"p2":[129,4,36,171]}}
{"id":4,"op":"shutdown"}
```

响应 `{"id":2,"ok":true,"result":{...}}`；失败是
`{"id":2,"ok":false,"error":{"kind":"no_card","sdkCode":5,"hint":"…","message":"…"}}`。
`kind` 是稳定分类、`sdkCode` 对齐原 SDK 的 0–11、`hint` 是给现场看的中文建议。

| 启动参数 | 作用 |
|---|---|
| `--driver-dir <目录>` | 指定驱动目录（默认 exe 同目录 `drivers/LC2000`） |
| `--sim` | 内存模拟后端，不加载驱动、不开串口 |
| `--sim-card blank\|guest-day\|guest-hour\|none` | 模拟卡槽的初始内容 |

`op` 取值：`open` / `close` / `is_open` / `read_card` / `erase_card` / `write_guest_day` /
`write_guest_hour` / `write_master` / `write_install` / `write_reset` / `write_clear` /
`write_checkout_hour` / `write_timesync` / `write_raw` / `probe_ports` / `set_port` /
`selftest` / `device_info` / `shutdown`。

---

## 五、卡格式摘要

卡内只有 **8 字节**有效数据（7 页 × 4 字节，页 3–7 全 0）。完整推导、样本与证据等级见
[`docs/TEMIC门锁卡技术规格.md`](docs/TEMIC门锁卡技术规格.md) 与
[`docs/抓包样本与验证记录.md`](docs/抓包样本与验证记录.md)。

```
页1 = 5B EC | ((序号 & 0x0F) << 4 | 低4位) | (序号 >> 4)
页2 = 时长码 | 卡号低字节 | 卡号高字节 | 年月
```

| 项 | 结论 |
|---|---|
| 时长码（天） | `0x80 ｜ 天数`（1–63）；超过 63 天走 `0xC0 ｜ round(天数/30)`（月码） |
| 时长码（小时） | 客卡 `0x40 ｜ (小时+1)`；总控卡 `0x40 ｜ 小时`（差 1） |
| 卡号 | 16 位：`(日 & 0x1F) << 11 ｜ (时 & 0x1F) << 6 ｜ 小时内序号(0–62)`；低字节在前 |
| 年月 | `页2[3]` 高 4 位 = `(年−2000) & 0x0F`、低 4 位 = 月；**16 年一循环**，2026-11 = `0xAB` |
| 挂失 | 客卡 `页1[2]` 低 4 位 = 1；挂失卡本身就是客卡，刷一次门让同房同卡型旧卡失效 |
| 空白卡 / 已退房 | 8 字节全 0，或 `页2` 为 `12 12 12 12` |

管理卡（页1 固定；页2 除安装卡外都带作业流水号与年月）：

| 卡型 | 页1 | 页2 |
|---|---|---|
| 总控卡 | `5B EC F0 FF` | `时长码 \| 流水号低 \| 流水号高 \| 年月` |
| 复位卡 | `5B EC 0A 00` | `00 \| 流水号低 \| 流水号高 \| 年月` |
| 清除卡 | `5B EC 9B E5` | `C7 \| 流水号低 \| 流水号高 \| 年月` |
| 安装卡 | `5B EC ((序号<<4)\|0x0C) (序号>>4)` | `46 F2 00 01`（固定，不含作业流水号） |
| 退房钟点卡 | `5B EC 7B <小时>` | `FF \| 流水号低 \| 流水号高 \| 年月` |
| 校时卡 | `5B EC <时/分位域>` | `06 \| 流水号低 \| 流水号高 \| 年月` |

校时卡编码 `YYMMDDHHmm`（年月日时分）：`YYMM` 在 `页2[3]`、`DD` 在卡号日段、
`HH:mm` 在页1 位域（`p1[2] = 16×(分 & 0x0F)+9`、`p1[3] = (分>>4)+(4×时 & 0x7C)`）。

日租与钟点客卡的唯一差异是 `页2[0]`：页1、作业流水号、年月完全相同，卡型靠时长码段区分。

**到期语义**：锁按**卡内发卡时间**计算，不依赖首次刷卡——日租卡到期 = 发卡日 + N 天的
**退房钟点**（由退房钟点卡写进锁，如 12 点 → 次日 12:00），钟点卡到期 = 发卡时刻 + N 小时。
前提是锁内时间已用校时卡校准、退房钟点已用退房钟点卡设置。

---

## 六、作业流水号

卡号在业务上叫**作业流水号**，固定 **10 位十进制 `YYMMDDHHII`**（年 2 + 月 2 + 日 2 + 时 2 + 小时内序号 2）：

```
作业流水号 = YY MM DD HH II
例：2611081100  →  2026-11-08 11 时第 00 号
```

写进卡内时拆成两处：

| 段 | 含义 | 卡内位置 |
|---|---|---|
| `YYMM` | 年 + 月 | `页2[3]`：高 4 位 `(年−2000) & 0x0F`、低 4 位月；16 年循环，2026-11 = `AB`、2027-11 = `BB` |
| `DDHHII` | 日 + 时 + 小时内序号 | 16 位卡号：`(日 & 0x1F) << 11 ｜ (时 & 0x1F) << 6 ｜ 序号(0–62)`，低字节在前 |

- 序号 0–62；同一小时满 63 张后进位到下一小时、序号归 0。
- 客卡、总控卡、复位卡、清除卡、退房钟点卡、校时卡共用同一套编号；安装卡不带作业流水号（`页2` 固定 `46 F2 00 01`）。
- 产品与 SDK 都按发卡时刻生成完整 `YYMMDDHHII`，写卡时拆两处；不另存、不展示。
- 读卡时把两处拼回完整编号；`页2[3]` 缺失（空白卡 / 安装卡）时只显示 `DDHHII` 部分。
- 复位卡用于让该房间的旧卡恢复可用；清除卡 / 安装卡的真实锁效果尚未做受控验证。

完整推导与历史样本见 [`docs/TEMIC门锁卡技术规格.md`](docs/TEMIC门锁卡技术规格.md) 与
[`docs/抓包样本与验证记录.md`](docs/抓包样本与验证记录.md)。

---

## 七、构建与发布

### 7.1 硬件层（`rc-device-sdk` 子模块，手动构建）

不再需要 `csc` / .NET Framework，也没有构建脚本。在 `app/` 内手动执行：

```powershell
# 编 32 位 sidecar（驱动只有 x86）
cargo build --manifest-path src-tauri/vendor/rc-device/Cargo.toml `
  --bin rc-device-sidecar --release --target i686-pc-windows-msvc `
  --target-dir src-tauri/target/sidecar

# 拷到产品资源目录（sidecar 默认从 exe 同目录的 drivers/LC2000 加载驱动）
New-Item -ItemType Directory -Force -Path src-tauri/resources/device/drivers/LC2000 | Out-Null
Copy-Item src-tauri/target/sidecar/i686-pc-windows-msvc/release/rc-device-sidecar.exe src-tauri/resources/device/ -Force
Copy-Item src-tauri/vendor/rc-device/drivers/LC2000/* src-tauri/resources/device/drivers/LC2000/ -Recurse -Force

# 门禁：46 项黄金向量（--sim 不加载驱动）
@('{"id":1,"op":"selftest"}','{"id":2,"op":"shutdown"}') |
  & src-tauri/resources/device/rc-device-sidecar.exe --sim
```

响应里必须是 `"passed":46,"failed":0`。**打包前必须做这一步**——MSI 打包的就是
`src-tauri/resources/device`，没准备会带着模拟设备出门。

改卡字节 / 驱动调用 / 超时 / 模拟：在 rc-device-sdk 仓库里改并按它的 AGENTS 提交，
然后更新本仓库的子模块指针。

### 7.2 产品（`app/`）

```powershell
cd app
pnpm install
pnpm tauri dev      # 开发（前端热更新 + Tauri 壳）
pnpm tauri build --bundles msi   # 正式包（WiX Toolset v3）
```

一键正式版（需先装 WiX v3 的 `candle.exe` / `light.exe`）：

```powershell
cd app
pwsh -NoProfile -File build-release.ps1
# 可选：-OutputDir D:\delivery  -SkipCopy  -WixBin "C:\Program Files (x86)\WiX Toolset v3.14\bin"
```

产物：`app/output/OpenCardWright_<version>_x64.msi`（脚本同时打印 SHA-256）。
环境依赖、踩坑（MSVC `D8050`、esbuild 临时文件）见
[`app/README.md`](app/README.md)。

---

## 八、已知限制

- 应急码设置卡、楼栋 / 楼层 / 应急 / 通道开门卡：无样本，格式未知，SDK 不生成。
- 清除卡、安装卡的真实锁效果未做受控验证。
- 挂失卡的锁端效果仍需按门锁批次现场确认；到期判定依赖锁内时间（校时卡）与退房钟点（退房钟点卡）设置正确。
- 作业流水号不含年月；跨月同一天同一小时同一序号会重复，可用复位卡恢复。
- 安装包未做代码签名（TD-504）。

---

## 九、目录与文档

```
OpenCardWright/
├─ docs/                 卡格式、调用链、驱动逆向、抓包证据、SDK 与接口文档
├─ app/                  OpenCardWright 发卡台（Tauri + Rust + React）
│  └─ src-tauri/vendor/rc-device/   硬件层子模块（rc-device-sdk：驱动、卡字节、sidecar）
├─ data/                 原厂数据库导出（NacreHis.json）与房号↔序号表
├─ plan/                 建设方案、待办清单、已完成摘要与 td-xxx 详情
├─ vendor/125k-sdk-v4.2/ 原厂 SDK 留档，仅作分析材料
└─ CHANGELOG.md          版本变更
```

| 想了解 | 看哪份 |
|---|---|
| 卡怎么发、字节怎么算 | [`docs/TEMIC门锁卡技术规格.md`](docs/TEMIC门锁卡技术规格.md) |
| 原厂软件怎么调用驱动 | [`docs/发卡调用链逆向.md`](docs/发卡调用链逆向.md) |
| 驱动 `te_*` 与帧层细节 | [`docs/T5557-5567函数逆向.md`](docs/T5557-5567函数逆向.md) |
| 每条结论的证据 | [`docs/抓包样本与验证记录.md`](docs/抓包样本与验证记录.md) |
| 在自家程序里调用 | [`docs/SDK使用说明.md`](docs/SDK使用说明.md) |
| 对外 / 对下接口汇总 | [`docs/接口文档.md`](docs/接口文档.md) |
| 原厂 SDK v4.2 分析 | [`docs/原厂SDK-v4.2-分析.md`](docs/原厂SDK-v4.2-分析.md)、[`docs/原厂SDK-v4.2-文档摘要.md`](docs/原厂SDK-v4.2-文档摘要.md) |
| 最终用户怎么用发卡台 | [`app/docs/使用手册.md`](app/docs/使用手册.md) |
| 这版改了什么 | [`CHANGELOG.md`](CHANGELOG.md) |
| 还有什么没做 | [`plan/待办清单.md`](plan/待办清单.md) + [`plan/td-xxx.md`](plan/) |

---

## 十、验收状态

**2026-11-10**

- CLI `selftest`：**46 通过，0 失败**（33 条字节向量 + 7 条卡号编码 + 6 条时长码）。
- 产品 Rust 测试：**136 项全绿**（解析 27 + 真实 selftest 1 + 模拟构包 5 + 存储 56 + 命令层 47）。
- 正式包（上一版验收）：`OpenCardWright_26.11.962_x64.msi`（WiX Toolset v3）。
- 历史现场验收：用本 SDK 写出的 220 房日租卡**实测开门成功**（2026-11-05）。

sdk 的全部结论只对 SHA-256 `F2F483E08BA8048FFD1C57E66CB35D9E32C1C1E280DC5F2FF100A5D881BCCBC0`
的 `LC32RFRW.dll`（102,492 字节、92 个导出）成立；厂商升级驱动需重新评估。

---

## 十一、许可

**MIT**，见 [`LICENSE`](LICENSE)。可自由使用、修改、商用与闭源分发，只需保留版权与许可声明。
随包分发的厂商驱动 `LC32RFRW.dll` 不属本项目版权，见 [`app/NOTICE`](app/NOTICE)。
