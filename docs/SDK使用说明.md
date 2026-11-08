# PMSLockSDK 使用说明

> 目标：第三方 PMS 用最少代码完成"读卡 / 发卡"。
> 运行环境：**32 位进程**；.NET Framework 4.0；驱动随包附带的 `sdk\IOServer\LC2000\LC32RFRW.dll`。
>
> 本 SDK 是唯一实现，只依赖读卡器驱动，**不依赖原门锁软件、不依赖加密狗**。

---

## 1. 三种调用方式，按你的技术栈选

| 你的 PMS 是 | 用哪个 | 说明 |
|---|---|---|
| .NET (C#/VB.NET) | `PMSLockSDK.dll` | 直接引用，类型安全 |
| Delphi / C++ / VB6 | `PMSLockSDK.Com.dll` | COM 组件，`CreateObject("PmsLockSdk.Reader")` |
| 任意（含 Java/Python/Node） | `PMSLockCard.exe` | 命令行，支持 `--json` |

> 64 位宿主请调用 `PMSLockCard.exe`（32 位子进程），不要直接加载 DLL。

---

## 2. 首次部署

1. 把 `sdk\` 整个目录拷到 PMS 机器（例如 `D:\PMS\LockCard\`）。
2. 确认 `..\data\RoomIndex.ini` 存在（房号 ↔ 序号），或改 `LockCard.ini` 里的 `RoomIndex=`。
3. 改 `LockCard.ini`：
   ```ini
   [Reader]
   DriverDir=D:\Program Files\智能门锁管理软件\IOServer\LC2000
   Port=6            ; 6 = COM7
   Password=12000206
   [Card]
   ; 【已废弃 2026-11-08】SDK 不再读 StartCardNo，卡号由产品侧维护
   StartCardNo=2403
   ```
4. 关闭门锁软件（它独占串口），然后：
   ```bat
   PMSLockCard.exe ports      :: 确认读卡器在哪个口
   PMSLockCard.exe read       :: 确认能读到卡
   ```

---

## 3. C# 用法

```csharp
using PmsLockSdk;

var opt = LockCardOptions.Load(@"D:\PMS\LockCard\LockCard.ini",
                               AppDomain.CurrentDomain.BaseDirectory);

using (var r = new LockCardReader(opt))
{
    int rc = r.Open();                       // 0 = 成功
    if (rc != 0) throw new Exception(LockResult.Describe(rc));

    CardInfo cur;
    if (r.ReadCard(out cur) == 0)
        Console.WriteLine(cur.Describe() + "  " + cur.RawHex);

    CardInfo written;
    // 卡号必须由调用方传入（示例 0x4B83；产品侧用「时间 + 小时内序号」算）
    rc = r.IssueGuestDay("209", 1, CardNoPolicy.Increment, 0x4B83, out written);
    if (rc != 0) throw new Exception(LockResult.Describe(rc));
    Console.WriteLine("已发卡 卡号=0x" + written.CardNo.ToString("X4"));
}
```

### API 一览

| 方法 | 说明 |
|---|---|
| `Open()` / `Close()` | 打开/关闭串口（内部固定 `config_parameter(0)`） |
| `ReadCard(out CardInfo)` | 读卡并解析（卡型、房号、天数/小时、卡号） |
| `IssueGuestDay(room, days, policy, cardNo, out info)` | 日租客卡 1–1095 天；卡号必填 |
| `IssueGuestHour(room, hours, policy, cardNo, out info)` | 钟点客卡 1–62 小时；卡号必填 |
| `IssueMaster(policy, cardNo, out info)` | 总控卡；卡号必填 |
| `IssueReset(policy, cardNo, out info)` | 复位卡；卡号必填 |
| `IssueClear(policy, cardNo, out info)` | 清除卡；卡号必填 |
| `IssueInstall(room, out info)` | 安装卡（只含房间序号，无卡号） |
| `IssueCheckoutHour(hour, policy, cardNo, out info)` | 退房钟点设置卡；卡号必填 |
| `IssueTimeSync(hour, minute, cardNo, out info)` | 校时卡；卡号必填 |
| `WriteRaw(p1Hex, p2Hex, out info)` | 直写 8 字节（校时卡等） |
| `CardBuilder.TimestampCardNo(at, seq)` / `NextStamp` | 卡号时间戳编码/递推（纯函数） |
| `Rooms.TryGetIndex(room, out idx)` | 房号 → 序号 |

**卡号策略**：SDK 不再自增/复用。卡号由调用方按「日/时/序号」时间戳编码算好后传入
（1–0xFFFF）；`CardNoPolicy` 仅为旧签名兼容，当前不改变行为。无卡号重载会返回
`ErrCardNo=9`，不要再用 `IssueGuestDay(room, days, out info)` 这种旧签名。

---

## 4. COM 用法（Delphi / VBS）

注册（管理员，32 位）：
```bat
C:\Windows\Microsoft.NET\Framework\v4.0.30319\regasm.exe PMSLockSDK.Com.dll /tlb /codebase
```

VBScript：
```vbs
Set r = CreateObject("PmsLockSdk.Reader")
rc = r.Open("D:\Program Files\智能门锁管理软件\IOServer\LC2000", 6)
WScript.Echo r.Read()
rc = r.IssueGuest("220", 1)
WScript.Echo r.LastMessage & " 卡号=" & r.LastCardNo
r.Close
```

Delphi：
```pascal
var R: OleVariant;
begin
  R := CreateOleObject('PmsLockSdk.Reader');
  if R.Open('D:\...\IOServer\LC2000', 6) <> 0 then Exit;
  Memo1.Lines.Add(R.Read);
  R.IssueGuest('220', 1);
  Memo1.Lines.Add(R.LastMessage);
  R.Close;
end;
```

---

## 5. 命令行用法

```bat
PMSLockCard.exe read --json
PMSLockCard.exe ports
PMSLockCard.exe selftest          :: 46 项纯字节自检

:: 客卡：卡号 = 时间戳（--room-index + --seq + --datetime）
PMSLockCard.exe issue --type guest --room-index 15 --seq 3 --datetime "2026-11-08 14:30" --days 1
PMSLockCard.exe issue --type guest --room-index 15 --seq 4 --datetime "2026-11-08 14:30" --hours 3
PMSLockCard.exe issue --type guest --room-index 15 --seq 5 --datetime "2026-11-08 14:30" --days 1 --lost

:: 无房卡/管理卡：用 --serial 显式卡号（十六进制）
PMSLockCard.exe issue --type master   --serial 4B86 --datetime "2026-11-08 14:30" --days 30
PMSLockCard.exe issue --type reset    --serial 4B87 --datetime "2026-11-08 14:30"
PMSLockCard.exe issue --type clear    --serial 4B88 --datetime "2026-11-08 14:30"
PMSLockCard.exe issue --type checkout --hour 12 --serial 4B89 --datetime "2026-11-08 14:30"
PMSLockCard.exe issue --type timesync --serial 4B8A --datetime "2026-11-08 15:30"
PMSLockCard.exe issue --type install --room-index 15

:: 原样写 8 字节
PMSLockCard.exe issue --type raw --p1 5BEC492B --p2 13BF22AB
```

JSON 输出：
```json
{"ok":true,"card":{"kind":"日租客卡","room":"220","roomIndex":15,"days":1,
                   "hours":0,"cardNo":"0x2404","raw":"5BECF100810424AB0000..."}}
```

退出码：

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 参数错误 |
| 2 | 驱动/串口/握手失败 |
| 3 | 无卡或读卡失败 |
| 4 | 写卡失败 |
| 5 | 写后回读校验失败 |
| 6 | 卡号无效 |
| 7 | 不支持的卡型 |

---

## 6. 常见问题

| 现象 | 原因 / 处理 |
|---|---|
| `Open` 返回 3（串口失败） | 门锁软件 `NacreHis.exe` 占着串口，先关掉它 |
| `Open` 返回 5（无卡） | 卡没放正 / 卡不是 125kHz TEMIC / 读卡器未插 |
| 写卡成功但锁不开 | ① 卡号比锁内已见的最大值小（见技术规格第 6 节）；② 房门/安装卡没配好 |
| 回读校验失败 | 卡被别的系统用其它密码锁住了，或卡型不对 |
| 房号找不到 | `--room-index` 传的是房间序号（1–254），不是房号；用 `PMSLockCard.exe read` 看卡上序号，或补 `RoomIndex.ini` |

---

## 7. 自检

`PMSLockCard.exe selftest` 不接硬件，纯字节比对 **46 项**（33 条字节向量 + 7 条卡号编码 + 6 条时长码）；全部通过说明算法实现正确。
