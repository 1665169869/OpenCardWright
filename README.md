# OpenCardWright — TEMIC 门锁卡逆向、发卡 SDK 与发卡台

对「智能门锁管理软件（LOCK_V8 / V8.9）」所用 125 kHz TEMIC 卡的完整逆向，
以及一套**不依赖原软件、不依赖加密狗**的独立发卡 SDK（`sdk/`）和开源发卡台
OpenCardWright（`app/`）。

> 原软件厂家已倒闭；本仓库是可直接部署的替代方案。
> 卡格式与逆向结论的权威来源是 `docs/`，代码是文档的实现。

---

## 一、项目组成

| 路径 | 内容 |
|---|---|
| `docs/` | 卡格式、调用链、驱动逆向、抓包证据、SDK 说明、接口文档 |
| `sdk/` | 可直接拷到 32 位 Windows 运行的 .NET 4.0 发卡 SDK、CLI、COM 封装与读卡器驱动 |
| `app/` | OpenCardWright：Tauri 2 + Rust + React 19 发卡台（房间/发卡/退房/记录/备份/用户） |
| `plan/` | 建设方案、`待办清单.md`（未完成/不排期索引）、`已完成.md`（归档）与 `td-xxx.md`（条目详情） |
| `data/` | 原厂数据库导出（`NacreHis.json`）与房号↔序号表（`RoomIndex.ini`） |
| `vendor/125k-sdk-v4.2/` | 原厂 SDK 留档，仅作逆向分析材料，不是实现路径 |

### 能做与不能做

| 能 | 不能 |
|---|---|
| 日租客卡（1–1095 天）、钟点客卡（1–62 小时） | 应急码设置卡、楼栋/楼层/应急/通道开门卡（**无样本，格式未知**） |
| 总控卡、复位卡、清除卡、安装卡、退房钟点卡、校时卡 | 读写 TEMIC 之外的 IC/M1/EM4305 卡 |
| 读卡并解析卡型、房号、天数/小时、卡号、年月、挂失位 | 绕过原软件授权/加密狗（本方案本就无需授权） |
| 挂失客卡（让同房旧卡失效） | 解释锁如何按日历月/钟点精确到期（部分机制仍待真锁验证） |

---

## 二、目录结构

```
OpenCardWright/
├─ README.md                          本文件
├─ docs/
│  ├─ TEMIC门锁卡技术规格.md           ★ 卡格式/卡型/卡号/管理卡/待验证项
│  ├─ 发卡调用链逆向.md                原厂主程序 → SmartLock → SM_RW_Card → 驱动
│  ├─ T5557-5567函数逆向.md            驱动 te_* 导出、帧层、扰码、CRC、配置字
│  ├─ 抓包样本与验证记录.md            原厂样本、实测结论、复现方法
│  ├─ SDK使用说明.md                   .NET / COM / CLI 三种调用方式
│  └─ 接口文档.md                      ★ 本仓库对外/对下接口汇总（驱动 + 高层 + SDK + CLI）
├─ sdk/                               可直接部署
│  ├─ PMSLockSDK.dll                  .NET 4.0 x86 类库
│  ├─ PMSLockSDK.Com.dll + .tlb       COM 封装（Delphi / C++ / VB）
│  ├─ PMSLockCard.exe                 命令行（JSON 进出）
│  ├─ LockCard.ini / RoomIndex.ini    运行配置与房号表
│  ├─ IOServer/LC2000/                读卡器驱动（随包附带）
│  ├─ tools/RawRead.exe               底层诊断工具
│  └─ demo/                           .NET 与 VBS 示例
├─ app/                               OpenCardWright 产品
│  ├─ src/                            React 页面与 api.ts
│  ├─ src-tauri/                      Rust 命令、SQLite 存储、设备适配
│  └─ docs/使用手册.md                 面向最终用户
├─ plan/待办清单.md                    未完成需求与来源登记
└─ data/                              原厂数据库导出与房号表
```

**把整个 `sdk/` 拷到任意 32 位 Windows 即可发卡**，无需安装原软件。

---

## 三、快速开始（CLI）

在 `sdk/` 目录下运行：

```bat
:: 1. 找读卡器、读一张卡、跑纯字节自检（不需要硬件）
PMSLockCard.exe ports
PMSLockCard.exe read
PMSLockCard.exe selftest

:: 2. 发一张日租客卡：房间序号 15、本小时第 3 张、2026-11-09 14:30 发卡、1 天
PMSLockCard.exe issue --type guest --room-index 15 --seq 3 --datetime "2026-11-09 14:30" --days 1

:: 3. 钟点客卡：同房、同小时第 4 张、3 小时
PMSLockCard.exe issue --type guest --room-index 15 --seq 4 --datetime "2026-11-09 14:30" --hours 3

:: 4. 总控卡（小时编码比客卡少 1；天编码相同）
; 无房卡用 --serial 显式卡号（十六进制）
PMSLockCard.exe issue --type master --serial 4B86 --datetime "2026-11-09 14:30" --days 30

:: 5. 管理卡
PMSLockCard.exe issue --type reset    --serial 4B87 --datetime "2026-11-09 14:30"
PMSLockCard.exe issue --type clear    --serial 4B88 --datetime "2026-11-09 14:30"
PMSLockCard.exe issue --type install  --room-index 15
PMSLockCard.exe issue --type checkout --hour 12 --serial 4B89 --datetime "2026-11-09 14:30"

:: 6. 校时卡（页1 写时/分位域，页2[0]=0x06）
PMSLockCard.exe issue --type timesync --serial 4B8A --datetime "2026-11-09 15:30"

:: 7. 原样写任意 8 字节（排障/未解卡型）
PMSLockCard.exe issue --type raw --p1 5BEC492B --p2 13BF22AB
```

> **卡号不再自增**：`issue` 必须拿到卡号。两种方式：
> 1. `--room-index + --seq + --datetime`，CLI 用时间戳算法生成卡号；
> 2. `--serial 4B83` 显式指定 16 位卡号（十六进制）。
>
> 只写 `issue --room 220 --days 1` 会因缺少卡号返回 6（卡号无效）。

---

## 四、CLI 参考

### 4.1 命令

| 命令 | 作用 | 需要硬件 |
|---|---|---|
| `PMSLockCard.exe ports` | 扫描 COM1–COM8，列出可用读卡器 | 读卡器 |
| `PMSLockCard.exe read [--json]` | 读卡并解析 8 字节 | 读卡器 + 卡 |
| `PMSLockCard.exe selftest` | 纯字节自检：46 项 | 否 |
| `PMSLockCard.exe issue ...` | 发卡（卡型由 `--type` 选择） | 读卡器 + 空白卡 |

### 4.2 `issue` 卡型

| `--type` | 含义 | 必需参数 |
|---|---|---|
| `guest`（默认）/ `day` | 日租客卡 | `--days` 1–1095 + 卡号 |
| `hour` | 钟点客卡 | `--hours` 1–62 + 卡号 |
| `master` | 总控卡 | `--days` 或 `--hours` + 卡号 |
| `reset` | 复位卡 | 卡号 |
| `clear` | 清除卡 | 卡号 |
| `install` | 安装卡 | `--room-index`（或 `--room`） |
| `checkout` | 退房钟点设置卡 | `--hour` 0–23 + 卡号 |
| `timesync` | 校时卡 | 卡号；时间取 `--datetime`（或旧式 `--room HH:MM`） |
| `raw` | 原样写 `--p1` / `--p2` | 两个 8 位十六进制串 |

### 4.3 参数

| 参数 | 说明 |
|---|---|
| `--room-index N` | 房间序号 1–254（推荐；产品侧使用） |
| `--room 220` | 房号，按 `RoomIndex.ini` 反查序号（兼容旧脚本） |
| `--days N` / `--hours N` | 有效期；`--hours` 优先于 `--days` |
| `--seq N` | 本小时内的序号 0–62；配合 `--datetime` 生成时间戳卡号 |
| `--datetime "YYYY-MM-DD HH:mm"` | 发卡时刻，同时决定页2[3] 年月；`--at` 同义 |
| `--serial HEX` | 显式卡号（十六进制，如 `4B83`）；与 `--room-index + --seq + --datetime` 同时给时以后者为准 |
| `--lost` | 挂失客卡（页1[2] 低 4 位 = 1；仅客卡有效） |
| `--hour N` | `checkout` 卡的退房钟点 0–23 |
| `--json` | JSON 输出（成功/失败都可用） |
| `--port N` / `--dir PATH` / `--ini PATH` | 串口索引 / 驱动目录 / 配置文件覆盖 |
| `--quiet` | 兼容参数，当前不影响输出 |
| `--reuse` / `--strict-cardno` | 兼容旧脚本的占位参数；卡号现已由调用方显式传入，不再自增/复用 |

### 4.4 JSON 输出

成功：

```json
{"ok":true,"card":{"kind":"日租客卡","room":"220","roomIndex":15,"days":1,
 "hours":0,"cardNo":"0x4B83","isLost":false,"issueDay":9,"issueHour":14,
 "issueSeq":3,"cardYm":171,"raw":"5BECF00081834BAB0000..."}}
```

`cardYm` 是页2[3] 的原始值（2026-11 = `0xAB` = 171）。

失败：

```json
{"ok":false,"error":{"code":6,"msg":"卡号无效"}}
```

### 4.5 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 参数错误 |
| 2 | 驱动 / 串口 / 握手失败 |
| 3 | 无卡或读卡失败 |
| 4 | 写卡失败 |
| 5 | 写后回读校验失败 |
| 6 | 卡号无效 |
| 7 | 不支持的卡型 |

---

## 五、卡内格式（8 字节有效数据）

卡片是 T5557，只有页1、页2 可写；页3–7 全 0。

```
页1 = 5B EC | ((序号 & 0x0F) << 4 | 低4位) | (序号 >> 4)
页2 = 时长码 | 卡号低字节 | 卡号高字节 | 年月
```

### 5.1 时长码

| 卡型 | 小时 | 天 |
|---|---|---|
| 客卡 | `0x40 \| (小时+1)` | `0x80 \| 天数`（1–63） |
| 总控卡 | `0x40 \| 小时` | `0x80 \| 天数`（1–63） |
| 超过 63 天 | — | `0xC0 \| round(天数 / 30)`（30 天/月） |

例：90 天 → `0xC3`（3 月）；1095 天 → `0xE4`（36 月）。

**日租客卡与钟点客卡唯一差异是 `页2[0]`**：页1、卡号、年月完全相同。
卡型靠时长码段区分：`0x80/0xC0` = 天/月，`0x40` = 小时。

### 5.2 卡号（时间戳编码）

```
16 位 = (日 & 0x1F) << 11 | (时 & 0x1F) << 6 | 小时内序号(0–62)
```

- 例：11 日 12 点、本小时第 53 张（seq=52）→ `0x5B34`。
- 满 63 张后进位到下一小时、序号归 0；跨日同理。
- **卡号不含年月**；跨月同一天同一小时同一序号会重复，需靠锁的高水位/复位卡管理。
- 读写卡时按小端存：`p2[1]` 低字节、`p2[2]` 高字节。

### 5.3 年月（页2[3]）

```
高 4 位 = (年 − 2000) & 0x0F
低 4 位 = 月
```

还原时 `年 = 2000 + 16 × ⌊(当前年 − 2000) / 16⌋ + 高4位`，**16 年一循环**。
2026-11 写 `0xAB`，2027-11 写 `0xBB`。

### 5.4 挂失

客卡 `页1[2]` 低 4 位 = 1 即挂失（`--lost`）。挂失卡本身是客卡，刷一次门让同房同卡型的旧卡失效。
管理卡/校时卡没有挂失概念；校时卡 `页1[2]` 的低 4 位是时间位域，不能当挂失。

### 5.5 管理卡

| 卡型 | 页1 | 页2 |
|---|---|---|
| 总控卡 | `5B EC F0 FF` | `时长码 \| 卡号低 \| 卡号高 \| 年月` |
| 复位卡 | `5B EC 0A 00` | `00 \| 卡号低 \| 卡号高 \| 年月` |
| 清除卡 | `5B EC 9B E5` | `C7 \| 卡号低 \| 卡号高 \| 年月` |
| 安装卡 | `5B EC ((序号<<4)\|0x0C) (序号>>4)` | `46 F2 00 01`（固定，不含卡号） |
| 退房钟点卡 | `5B EC 7B <小时>` | `FF \| 卡号低 \| 卡号高 \| 年月` |
| 校时卡 | `5B EC p1[2] p1[3]` | `06 \| 卡号低 \| 卡号高 \| 年月` |

校时卡页1 位域（2026-11-08 七组样本验证）：

```
p1[2] = 16 × (分 & 0x0F) + 9
p1[3] = (分 >> 4) + ((4 × 时) & 0x7C)
```

页2 首字节 `0x06` 是明文；旧卡可能读到 `0x14`（历史加扰残留），解析时两者都识别。

空白卡/已退房：前 8 字节全 0，或页2 为 `12 12 12 12`。

---

## 六、锁的规则（实测）

锁对**每个房间 × 每种卡型**分别维护一个「当前有效卡号」（高水位）：

| 新卡号 | 结果 |
|---|---|
| 大于当前有效号 | 开门，并更新高水位 |
| 等于当前有效号 | 开门 |
| 小于当前有效号 | 拒绝 |

- 日租卡与钟点卡两套高水位**互不影响**。
- 写卡本身不改变锁内记录；**只有刷过才生效**。
- 复位卡清空该房间的卡记录（高水位归零），旧卡恢复可用。
- 新锁初始化顺序：**复位卡 → 清除卡 → 安装卡 → 校时卡**。

### 现场风险

- **1095 天卡刷一次门会把该房间高水位顶到 3 年后**，之后正常卡全废，只能用复位卡恢复。
- 不要在测试/演示现场留下复位卡：任何持卡人都能让旧卡复活。
- 总控卡是否受客房高水位限制仍**【待定】**，不要据此放宽卡号保护。
- 清除卡/安装卡的真实锁效果尚未做受控验证。

---

## 七、SDK / COM 集成

| 你的 PMS | 用哪个 | 入口 |
|---|---|---|
| .NET（C#/VB.NET） | `PMSLockSDK.dll` | `LockCardReader` / `CardBuilder` |
| Delphi / C++ / VB6 | `PMSLockSDK.Com.dll` | `CreateObject("PmsLockSdk.Reader")` |
| 任意（Java/Python/Node） | `PMSLockCard.exe` | 命令行 + `--json` |

```csharp
using PmsLockSdk;

var opt = LockCardOptions.Load(@"D:\PMS\LockCard\LockCard.ini",
                               AppDomain.CurrentDomain.BaseDirectory);
using (var r = new LockCardReader(opt))
{
    if (r.Open() != 0) throw new Exception("打开读卡器失败");
    CardInfo card;
    if (r.ReadCard(out card) == 0) Console.WriteLine(card.Describe());
}
```

- `CardBuilder` 是静态纯函数，可离线构包/单测；`LockCardReader` 线程不安全，一个实例一个串口。
- 卡号由调用方传入（`PrepareCardNo` 要求 1–0xFFFF）；SDK 不再自增、不读写 `CardNo.state`。
- COM 注册（管理员，32 位）：`regasm.exe PMSLockSDK.Com.dll /tlb /codebase`。
- 驱动写密码默认 `12000206`，统一走 `config_parameter(0)` 明文模式；不要留下扰码状态。

---

## 八、OpenCardWright 产品（`app/`）

技术栈：Tauri 2 + Rust + React 19 + antd 5；SQLite（`ocw-store`）；设备适配层 `ocw-device`。

```powershell
cd app
pnpm install
pnpm tauri dev       # 开发（前端热更新 + Tauri 壳）
pnpm tauri build     # 正式包（NSIS）

# 本机编译环境：
$env:CARGO_TARGET_DIR = 'C:\ocw-target'
$env:DEBUG = 'false'
```

- 首次打包前先 `pnpm setup:nsis`（本机联网下载 NSIS 会超时；已缓存可跳过）。
- 构建产物：`门锁发卡台_<version>_x64-setup.exe`（中文文件名）。
- `productName = 门锁发卡台`；快捷方式、开始菜单、安装目录、卸载项、安装页面均为中文。
- 安装器/卸载器图标：`app/src-tauri/icons/icon.ico`（`bundle.windows.nsis.installerIcon`）。
- **交付时把安装包复制为 ASCII 文件名（如 `C:\ocw-setup\setup.exe`）再运行**，避免 NSIS 在中文文件名下报
  `Error writing temporary file. Make sure your temp folder is valid.`
- 旧版若安装的是 ASCII `productName=OpenCardWright`，安装目录不同，**先卸载旧版再装新版**。
- 数据目录：release 用 `%PROGRAMDATA%\OpenCardWright\`，debug 用 `%PROGRAMDATA%\OpenCardWright-dev\`。

---

## 九、文档索引

| 想了解 | 看哪份 |
|---|---|
| 卡怎么发、字节怎么算 | `docs/TEMIC门锁卡技术规格.md` |
| 原厂软件怎么调用 | `docs/发卡调用链逆向.md` |
| 驱动 `te_*` 函数细节 | `docs/T5557-5567函数逆向.md` |
| 每条结论的证据 | `docs/抓包样本与验证记录.md` |
| 怎么在自己程序里调用 | `docs/SDK使用说明.md` |
| 对外/对下接口汇总 | `docs/接口文档.md` |
| 最终用户怎么用发卡台 | `app/docs/使用手册.md` |
| 这版改了什么 | `CHANGELOG.md` |
| 还有什么没做 | `plan/待办清单.md`（索引）+ `plan/td-xxx.md`（详情） |
| 已完成事项 | `plan/已完成.md` |

---

## 十、验收状态与已知限制

**2026-11-09**：

- CLI `selftest`：**46 通过，0 失败**（33 条字节向量 + 7 条卡号编码 + 6 条时长码）。
- 产品 Rust 测试：**130 项全绿**（设备 33 + 存储 51 + 命令层 46）。
- 正式包：`OpenCardWright_2026.11.9+0057_x64-setup.exe`。
- 历史现场验收：用本 SDK 写出的 220 房日租卡**实测开门成功**（2026-11-05）。

**已知限制**：

- 应急码设置卡、楼栋/楼层/应急/通道开门卡：**无样本，格式未知**，SDK 不生成。
- 清除卡、安装卡的真实锁效果未做受控验证。
- 总控卡是否受客房高水位限制：**【待定】**。
- 钟点卡/日租卡的精确到期语义：部分依赖锁内时间基准，仍需真锁验证。
- 校时卡是否真正校准锁 RTC：待真锁验证。
- 挂失卡的锁端效果：格式已实现，现场效果仍需按门锁批次确认。
- 卡号不含年月；跨月重复号需靠高水位/复位卡管理，不要手工跳号。
