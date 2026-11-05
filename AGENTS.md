# AGENTS.md

本仓库是「智能门锁管理软件（LOCK_V8 / V8.9）」TEMIC 卡的**逆向成果 + 独立发卡 SDK**，
不是常规应用工程：没有解决方案文件、没有包管理、没有单元测试框架、没有版本库。
（产品工作区 `opencardwright/` 例外：它是独立的 Tauri + Rust 工程，有自己的构建链，见第 9 节。）

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
| `sdk/PMSLockSDK.cs` | 类库：`CardBuilder`（纯函数）、`LockCardReader`（P/Invoke）、配置/状态 | 见第 4 节约定 |
| `sdk/PMSLockCard.cs` | CLI，含 `selftest` 的 27 条黄金样本 | 加样本只追加 |
| `sdk/PMSLockSDK.Com.cs` | COM 可见封装（`PmsLockSdk.Reader`） | 改 ProgId/GUID 会破坏调用方 |
| `sdk/IOServer/LC2000/*.dll` | 原厂读卡器驱动（x86，随包分发） | **不要动**，见第 3 节哈希 |
| `sdk/LockCard.ini`、`sdk/CardNo.state` | 运行配置、16 位卡号计数器 | 状态文件是现场数据 |
| `data/NacreHis.json`、`data/RoomIndex.ini` | 数据库导出（303 表）、房号↔序号 | 只读参考 |

`sdk/` 会被**整目录拷到现场 32 位 Windows** 直接运行，所以不要在 `sdk/` 里塞构建中间物或文档。

---

## 2. 构建（裸 csc，无 MSBuild）

编译环境：`C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe`（.NET Framework 4.0，C# 5 语法上限，
不要用字符串插值 / `?.` / `nameof` 等新语法）。**必须 `/platform:x86`**，驱动是 32 位。

在 `sdk\` 目录下按顺序：

```bat
csc /target:library /platform:x86 /out:PMSLockSDK.dll /doc:PMSLockSDK.xml PMSLockSDK.cs
csc /target:exe     /platform:x86 /out:PMSLockCard.exe /r:PMSLockSDK.dll PMSLockCard.cs
csc /target:library /platform:x86 /out:PMSLockSDK.Com.dll /r:PMSLockSDK.dll PMSLockSDK.Com.cs
```

- 第 1 条来自 `PMSLockSDK.cs` 头注释；第 2、3 条按同样方式推导（`PMSLockCard.cs` 头注释只列用法，
  未给编译行），exe/COM 的 `/r:PMSLockSDK.dll` 是必需项（`using PmsLockSdk`）。
- COM 注册（管理员，32 位）：`regasm.exe PMSLockSDK.Com.dll /tlb /codebase`。
- 源码含中文注释与中文输出：若编译后乱码，加 `/codepage:65001`，**不要**转成 GBK 保存。
- 交付物是 `.dll/.exe`，仓库里已提交编译结果；改源码后要同步重建，否则现场跑的是旧二进制。

## 3. 验证（不需要读卡器）

```bat
cd sdk
PMSLockCard.exe selftest
```

- 期望末行 `自检结果: 27 通过, 0 失败`；失败时进程返回 `5`。
- 27 条向量取自 `docs/抓包样本与验证记录.md` 的原厂样本（该表共 28 行，校时卡样本未进向量；
  总控 90/120/360/1095 天来自月数编码实验表）。`NAME` 字段可与表对号。
- **纯字节回归**：`CardBuilder` 是静态纯函数，`selftest` 不打开串口，可随时跑。
- 需要硬件时才用 `PMSLockCard.exe ports` / `read`；驱动哈希不符、串口被 `NacreHis.exe` 占用都会失败。

改任何组包逻辑后，**先让 `selftest` 全绿**再谈上机。

## 4. 代码约定

1. **分层不许破**：字节格式只写在 `CardBuilder`（静态、无 IO、可离线验证）；
   串口/驱动调用只写在 `LockCardReader`（实例、线程不安全、一个实例一个串口）。
   不要为了省事在 CLI 里拼字节或在 `CardBuilder` 里碰句柄。
2. **错误码两套，不要混**：`LockResult` 是 SDK 码（0–11），`PMSLockCard.cs` 的 `Fail()`
   把它映射成进程退出码（0–7）。新增 SDK 码时，末尾追加并在 `Fail()` 补映射，别改既有值。
3. **卡号状态文件**：`CardNo.state` 保存 16 位十六进制当前值（现为 `2B8F`，由现场发卡推进）。
   `Next()` 会先落盘再返回；`Save()` 用 `tmp + Move` 原子替换。不要手工回滚此文件来"重测"。
4. **文档同步**：改格式要同时 (a) 在 `抓包样本与验证记录.md` 登记样本，(b) 在 `selftest` 加向量，
   (c) 更新技术规格对应小节。三者缺一视为未完成。
5. **证据标注沿用文档惯例**：`【实测】`=有抓包或联机证据，`【推断】`=由证据归纳，`【待定】`=机制未证实。
   不要把 `【推断】` 悄悄升级成结论。

## 5. 硬性事实（改动时不要违背）

卡内只有 8 字节有效数据（7 页 × 4 字节，页 3–7 全 0）：

```
页1 = 5B EC | ((序号 & 0x0F) << 4 | 低4位) | (序号 >> 4)
页2 = 时长码 | 卡号低 | 卡号高 | AB
```

| 项 | 规则 |
|---|---|
| 时长码（天） | `0x80 ｜ 天数`（1–63）；`>63` 天走 `0xC0 ｜ round(天数/31)` |
| 时长码（小时） | 客卡 `0x40 ｜ (小时+1)`；总控卡 `0x40 ｜ 小时`（差 1，实测） |
| 总控卡页1 | `5B EC F0 FF` |
| 安装卡页2 | 固定 `46 F2 00 01`，**不含**卡号 |
| 校时卡 | `值 = 16 × 当日分钟数 + 953`，页2 首字节 `0x14`（照抄，含义未定） |
| 卡号 | 16 位、低字节在前；`N_H << 8 ｜ N_L` |
| 锁的判定 | 每个房间 × 每种卡型各自维护高水位：大于→开并更新，等于→开，小于→拒 |
| 日租 / 钟点 | 两套高水位**互不影响** |
| 复位卡 | 清空该房间卡记录（高水位归零），且不受卡号大小限制 |

驱动侧固定参数：写密码 `12000206`，必须 `config_parameter(0)`（明文，加扰模式 `3` 会"能读不能写"），
写卡走 `te_setuppwm_asc` + `te_writepwm_asc`（`te_write` 对空白卡无效），写完立刻回读可能拿到旧值
（`WriteAndVerify` 已重试 3 次）。

## 6. 现场风险（不要为了验证而做的事）

- **不要跳卡号**：`CardNo.state` 要紧贴原厂序列。跳到 `0x4700` 量级会让同类型软件卡长期失效。
- **1095 天卡刷一次门会把该房间高水位顶到 3 年后**，之后正常卡全废，只能用复位卡恢复。
- 复位卡本身不设防：任何持卡人都能让旧卡复活，测试/演示时不要留在现场。
- 清除卡、安装卡**尚未在锁上验证过真实效果**（字节已与原厂一致）；刷清除卡可能让锁变"未安装"。
- `lockcode_temic`（`Xt_cs` 表）只被原厂高层 `tem_*_sdk` 使用，本方案不用，但换机/迁移时要保持一致。

## 7. 文档一致性现状

2026-11-05 已统一，**当前不再有"vendor / 原厂方案"相关内容**：

| 曾经的偏差 | 处理 |
|---|---|
| `SDK使用说明.md` 顶部把 `vendor\` 说成主用方案 | 已删；现只说本 SDK 是唯一实现 |
| 自检样本数写成 19 | 已改为 **27**（README / SDK使用说明 / 技术规格 §12 一致） |
| `PMSLockSDK.cs` 头注释写"机制待定" | 已改为实测结论（按卡型高水位） |
| 技术规格残留早期"校时卡（未解）"推导 | 已删除，只保留 §5.3 实测公式 |
| `发卡调用链逆向.md` §8 曾提供 `VendorBridge.exe` | 已标注为历史记录，仓库不再提供该工具 |

**仍存在的不一致（无害）**：

- `RoomIndex.ini` 有两份（`data/` 与 `sdk/`）；运行时读 `sdk/` 侧（`LockCard.ini` 用相对路径），
  增删房间要两份同步。

## 8. 环境注记

- 本会话 `pwsh` 启动失败（`exit code 3221225794` = `0xC0000142` DLL 初始化失败），
  全程只能用 `read` / `glob` / `grep` 读仓库。若你遇到同样现象，先确认能否编译，不要把
  "没跑过命令"当成验证通过。
- 驱动版本敏感：结论只对 SHA-256 `F2F483E08BA8048FFD1C57E66CB35D9E32C1C1E280DC5F2FF100A5D881BCCBC0`
  的 `LC32RFRW.dll`（102,492 字节、92 个导出）成立。厂商升级驱动需重新评估。
- 无版本库：改动前自己备份，尤其 `sdk/CardNo.state` 与已编译的二进制。

---

## 9. 产品工作区 `opencardwright/`

2026-11-05 起，在本仓库内并行开发开源产品 **OpenCardWright**（方案见
`plan/开源门锁发卡软件-建设方案-v0.2.md`）。两者的关系是**只读依赖**，不是同一套构建：

| 关系 | 说明 |
|---|---|
| `sdk/` 的定位 | 产品的**设备实现与参考实现**。产品在构建期用 `csc` 从 `sdk/*.cs` 现编，产物打进 `src-tauri/resources/sdk/` |
| 能否修改 `sdk/` | **不能**（沿用第 0 节约束）。需要改设备行为时，改的是产品的适配层，不是 SDK |
| `docs/` 的定位 | 卡格式与锁行为的**唯一权威来源**；产品实现必须与之一致 |
| 回归口径 | `sdk` 自带的 27 条黄金向量；产品构建流水线会跑 `PMSLockCard.exe selftest` 做门禁 |
| 构建命令 | 在 `opencardwright/` 内：`pwsh -File scripts/build-sdk.ps1` → `npm run build:sdk`；随后 `npm run tauri build` |
| 编译环境坑 | 仓库路径含中文时 MSVC 会报 `D8050`：设 `CARGO_TARGET_DIR` 到 ASCII 路径 + `DEBUG=false`。详见 `opencardwright/README.md` §4 |

新产品自己的约定（技术栈、目录、阶段）以 `opencardwright/README.md` 为准，不要套用本文件的 csc 构建命令。
