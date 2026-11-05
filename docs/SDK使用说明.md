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
    rc = r.IssueGuestDay("209", 1, out written);     // 209 房 1 天
    if (rc != 0) throw new Exception(LockResult.Describe(rc));
    Console.WriteLine("已发卡 卡号=0x" + written.CardNo.ToString("X4"));
}
```

### API 一览

| 方法 | 说明 |
|---|---|
| `Open()` / `Close()` | 打开/关闭串口（内部固定 `config_parameter(0)`） |
| `ReadCard(out CardInfo)` | 读卡并解析（卡型、房号、天数/小时、卡号） |
| `IssueGuestDay(room, days, out info)` | 日租客卡 1~127 天 |
| `IssueGuestHour(room, hours, out info)` | 钟点客卡 1~63 小时 |
| `IssueMaster(out info)` | 总控卡 |
| `IssueReset(out info)` | 复位卡 |
| `IssueClear(out info)` | 清除卡 |
| `IssueInstall(room, out info)` | 安装卡（只含房间序号） |
| `IssueCheckoutHour(hour, out info)` | 退房钟点设置卡 |
| `WriteRaw(p1Hex, p2Hex, out info)` | 直写 8 字节（校时卡等） |
| `CardNos.Next()` / `EnsureAbove(v)` | 卡号计数器 |
| `Rooms.TryGetIndex(room, out idx)` | 房号 → 序号 |

**卡号策略**：默认 `CardNoPolicy.Increment`（每张 +1）。
要模拟原软件"续写同一张卡"（卡号不变），传 `CardNoPolicy.ReuseCard`。

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
PMSLockCard.exe issue --room 220 --days 1
PMSLockCard.exe issue --room 220 --hours 3
PMSLockCard.exe issue --type master
PMSLockCard.exe issue --type install --room 220
PMSLockCard.exe issue --type checkout --hour 12
PMSLockCard.exe issue --raw --p1 5BEC492B --p2 13BF22AB
PMSLockCard.exe selftest
PMSLockCard.exe ports
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
| 房号找不到 | `RoomIndex.ini` 里没有该房号，用 `--room` 传序号数字，或补表 |

---

## 7. 自检

`PMSLockCard.exe selftest` 不接硬件，纯字节比对 27 组原厂样本；全部通过说明算法实现正确。
