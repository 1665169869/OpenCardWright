// =====================================================================
//  PMSLockCard.exe — TEMIC 门锁卡命令行工具
//
//  用法：
//    PMSLockCard.exe read [--json]
//    PMSLockCard.exe issue --room 220 --days 1 [--serial 2448] [--json]
//    PMSLockCard.exe issue --room 220 --hours 3
//    PMSLockCard.exe issue --type master|reset|clear|checkout --hour 12
//    PMSLockCard.exe issue --type install --room 220
//    PMSLockCard.exe issue --raw --p1 5BEC492B --p2 13BF22AB
//    PMSLockCard.exe selftest
//    PMSLockCard.exe ports
//
//  退出码：0 成功 / 1 参数 / 2 驱动或串口 / 3 无卡 / 4 写入失败
//          5 回读校验失败 / 6 卡号无效 / 7 不支持
// =====================================================================
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using PmsLockSdk;

static class Program
{
    static int Main(string[] args)
    {
        try
        {
            if (args.Length == 0) { Usage(); return 1; }
            string verb = args[0].ToLowerInvariant();
            var opt = ParseArgs(args, 1);

            if (verb == "selftest") return SelfTest();
            if (verb == "ports") return Ports(opt);
            if (verb == "read") return Read(opt);
            if (verb == "issue") return Issue(opt);
            Usage();
            return 1;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine("× " + ex.Message);
            return 1;
        }
    }

    // ---------------- 参数 ----------------

    sealed class Opt
    {
        public string Room; public int Days; public int Hours; public string Type = "guest";
        public int Serial = -1; public int Hour = 12; public int Port = -1;
        public string Dir; public string Ini; public string P1, P2;
        public bool Json, Reuse, StrictCardNo, Quiet;
        public string At;
        public int Seq = -1;
        public bool Lost;
        /// <summary>房间序号 1–254（CLI 不再用房号发卡，`--room-index`）</summary>
        public int RoomIndex = -1;
    }

    static Opt ParseArgs(string[] a, int start)
    {
        var o = new Opt();
        for (int i = start; i < a.Length; i++)
        {
            switch (a[i])
            {
                case "--room": o.Room = a[++i]; break;
                case "--room-index": o.RoomIndex = int.Parse(a[++i]); break;
                case "--datetime": o.At = a[++i]; break;
                case "--days": o.Days = int.Parse(a[++i]); break;
                case "--hours": o.Hours = int.Parse(a[++i]); break;
                case "--type": o.Type = a[++i].ToLowerInvariant(); break;
                case "--serial": o.Serial = ParseHex(a[++i]); break;
                case "--hour": o.Hour = int.Parse(a[++i]); break;
                case "--port": o.Port = int.Parse(a[++i]); break;
                case "--dir": o.Dir = a[++i]; break;
                case "--ini": o.Ini = a[++i]; break;
                case "--p1": o.P1 = a[++i]; break;
                case "--p2": o.P2 = a[++i]; break;
                case "--json": o.Json = true; break;
                case "--reuse": o.Reuse = true; break;
                case "--strict-cardno": o.StrictCardNo = true; break;
                case "--at": o.At = a[++i]; break;
                case "--seq": o.Seq = int.Parse(a[++i]); break;
                case "--lost": o.Lost = true; break;
                case "--quiet": o.Quiet = true; break;
                default: throw new ArgumentException("未知参数 " + a[i]);
            }
        }
        return o;
    }

    static int ParseHex(string s)
    {
        return int.Parse(s, NumberStyles.HexNumber, CultureInfo.InvariantCulture);
    }

    static LockCardOptions LoadOptions(Opt o)
    {
        string baseDir = AppDomain.CurrentDomain.BaseDirectory;
        string ini = o.Ini ?? Path.Combine(baseDir, "LockCard.ini");
        var opt = LockCardOptions.Load(ini, baseDir);
        if (o.Dir != null) opt.DriverDir = o.Dir;
        if (o.Port >= 0) opt.Port = o.Port;
        return opt;
    }

    // ---------------- read ----------------

    static int Read(Opt o)
    {
        var opt = LoadOptions(o);
        using (var r = new LockCardReader(opt))
        {
            int rc = r.Open();
            if (rc != 0) return Fail(rc, o);
            CardInfo info;
            rc = r.ReadCard(out info);
            if (rc != 0) return Fail(rc, o);
            if (o.Json) Console.WriteLine(Json(info));
            else
            {
                Console.WriteLine("卡型  : " + CardInfo.KindText(info.Kind));
                if (info.Room != null) Console.WriteLine("房号  : " + info.Room);
                if (info.RoomIndex >= 0) Console.WriteLine("序号  : " + info.RoomIndex);
                if (info.Kind == CardKind.GuestDay) Console.WriteLine("天数  : " + info.Days);
                if (info.Kind == CardKind.GuestHour) Console.WriteLine("小时  : " + info.Hours);
                if (info.Kind == CardKind.CheckoutHour) Console.WriteLine("钟点  : " + info.CheckoutHour);
                if (info.CardNo >= 0) Console.WriteLine("卡号  : 0x" + info.CardNo.ToString("X4"));
                Console.WriteLine("原始  : " + info.RawHex);
                Console.WriteLine("读卡器: " + (r.ReaderId ?? "(未知)"));
            }
            return 0;
        }
    }

    // ---------------- issue ----------------

    static int Issue(Opt o)
    {
        var opt = LoadOptions(o);
        var policy = o.Reuse ? CardNoPolicy.ReuseCard : CardNoPolicy.Increment;

        using (var r = new LockCardReader(opt))
        {
            int rc = r.Open();
            if (rc != 0) return Fail(rc, o);

            if (!string.IsNullOrEmpty(o.At))
            {
                DateTime at;
                if (!DateTime.TryParse(o.At, out at))
                {
                    Console.Error.WriteLine("× --at 时间格式不正确，例：\"2026-11-08 14\" 或 \"2026-11-08 14:30\"");
                    return 1;
                }
                // 页2[3] 是「年+月」，由 CardBuilder.Now 决定（CLI 不再维护卡号状态）
                CardBuilder.Now = at;
            }

            // 序号 → 房号（CLI 不再传房号；用 RoomIndex.ini 反查，只为兼容 SDK 的 string room 重载）
            string room = o.Room;
            if (o.RoomIndex > 0)
            {
                room = r.Rooms.RoomOf(o.RoomIndex);
                if (room == null)
                {
                    Console.Error.WriteLine("× --room-index " + o.RoomIndex + " 在 RoomIndex.ini 里找不到对应房号");
                    return 1;
                }
            }
            // 卡号 = 日<<11 | 时<<6 | seq（日期时间 + 小时内序号，CLI 内部算，不再自增）
            int cardNo = o.Serial;
            if (o.RoomIndex > 0 && o.Seq >= 0 && !string.IsNullOrEmpty(o.At))
            {
                DateTime at0;
                if (!DateTime.TryParse(o.At, out at0))
                {
                    Console.Error.WriteLine("× --datetime 格式不正确，例：\"2026-11-08 14:30\"");
                    return 1;
                }
                cardNo = CardBuilder.TimestampCardNo(at0, o.Seq);
            }

            CardInfo info = null;
            switch (o.Type)
            {
                case "guest":
                case "day":
                    if (o.Hours > 0)
                        rc = r.IssueGuestHour(room, o.Hours, policy, cardNo, o.Lost, out info);
                    else
                        rc = r.IssueGuestDay(room, o.Days <= 0 ? 1 : o.Days, policy, cardNo, o.Lost, out info);
                    break;
                case "hour":
                    rc = r.IssueGuestHour(room, o.Hours <= 0 ? 1 : o.Hours, policy, cardNo, out info);
                    break;
                case "master": rc = r.IssueMaster(policy, cardNo, out info); break;
                case "reset": rc = r.IssueReset(policy, cardNo, out info); break;
                case "clear": rc = r.IssueClear(policy, cardNo, out info); break;
                case "install": rc = r.IssueInstall(room, out info); break;
                case "checkout": rc = r.IssueCheckoutHour(o.Hour, policy, cardNo, out info); break;
                case "timesync":
                    {
                        int hh = 0, mm = 0;
                        // 校时卡写「年+月」（页2[3]）+ 当日时分（页1）；`--at` 指定时取它，
                        // 否则取当前时间。（2026-11-08 实测：页1 与日期无关，年月只在页2[3]）
                        var now = CardBuilder.Now;
                        hh = now.Hour; mm = now.Minute;
                        if (o.Room != null && o.Room.Contains(":"))
                        {
                            var parts = o.Room.Split(':');
                            hh = int.Parse(parts[0]); mm = int.Parse(parts[1]);
                        }
                        rc = r.IssueTimeSync(hh, mm, cardNo, out info);
                        break;
                    }
                case "raw":
                    if (o.P1 == null || o.P2 == null) { Console.Error.WriteLine("× --raw 需要 --p1 和 --p2"); return 1; }
                    rc = r.WriteRaw(o.P1, o.P2, out info);
                    break;
                default:
                    Console.Error.WriteLine("× 未知卡型 " + o.Type);
                    return 1;
            }

            if (rc != 0) return Fail(rc, o);
            if (o.Json) Console.WriteLine(Json(info));
            else
            {
                Console.WriteLine("✓ " + info.Describe());
                Console.WriteLine("  原始: " + info.RawHex);
            }
            return 0;
        }
    }

    // ---------------- ports ----------------

    static int Ports(Opt o)
    {
        var opt = LoadOptions(o);
        int found = 0;
        for (int p = 0; p <= 7; p++)
        {
            var probe = LockCardOptions.Load(o.Ini ?? Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "LockCard.ini"),
                                             AppDomain.CurrentDomain.BaseDirectory);
            if (o.Dir != null) probe.DriverDir = o.Dir;
            probe.Port = p;
            using (var r = new LockCardReader(probe))
            {
                int rc = r.Open();
                string name = "COM" + (p + 1);
                if (rc == 0)
                {
                    Console.WriteLine(name + "  可用   读卡器=" + (r.ReaderId ?? "(未知)"));
                    found++;
                }
                else if (rc != LockResult.ErrPort)
                {
                    Console.WriteLine(name + "  可打开但异常  " + LockResult.Describe(rc));
                }
            }
        }
        if (found == 0) { Console.WriteLine("未找到可用读卡器（检查 USB / 驱动 / 是否被门锁软件占用）"); return 2; }
        return 0;
    }

    // ---------------- selftest ----------------

    sealed class Vector
    {
        public string Name; public CardKind Kind; public int RoomIndex, Days, Hours, Hour, Minute, CardNo;
        public string P1, P2;
    }

    static readonly Vector[] Vectors = new[]
    {
        new Vector{ Name="220/5天",   Kind=CardKind.GuestDay, RoomIndex=15, Days=5, CardNo=0x2288, P1="5BECF000", P2="858822AB" },
        new Vector{ Name="209/1天",   Kind=CardKind.GuestDay, RoomIndex=6,  Days=1, CardNo=0x2250, P1="5BEC6000", P2="815022AB" },
        new Vector{ Name="260/1天",   Kind=CardKind.GuestDay, RoomIndex=39, Days=1, CardNo=0x2281, P1="5BEC7002", P2="818122AB" },
        new Vector{ Name="201/1天",   Kind=CardKind.GuestDay, RoomIndex=0,  Days=1, CardNo=0x224A, P1="5BEC0000", P2="814A22AB" },
        new Vector{ Name="205/3天",   Kind=CardKind.GuestDay, RoomIndex=3,  Days=3, CardNo=0x2249, P1="5BEC3000", P2="834922AB" },
        new Vector{ Name="205/4天",   Kind=CardKind.GuestDay, RoomIndex=3,  Days=4, CardNo=0x2249, P1="5BEC3000", P2="844922AB" },
        new Vector{ Name="220/1天",   Kind=CardKind.GuestDay, RoomIndex=15, Days=1, CardNo=0x2403, P1="5BECF000", P2="810324AB" },
        new Vector{ Name="220/1小时", Kind=CardKind.GuestHour, RoomIndex=15, Hours=1,  CardNo=0x228A, P1="5BECF000", P2="428A22AB" },
        new Vector{ Name="220/3小时", Kind=CardKind.GuestHour, RoomIndex=15, Hours=3,  CardNo=0x2289, P1="5BECF000", P2="448922AB" },
        new Vector{ Name="220/6小时", Kind=CardKind.GuestHour, RoomIndex=15, Hours=6,  CardNo=0x228B, P1="5BECF000", P2="478B22AB" },
        new Vector{ Name="220/62小时",Kind=CardKind.GuestHour, RoomIndex=15, Hours=62, CardNo=0x228C, P1="5BECF000", P2="7F8C22AB" },
        new Vector{ Name="220/1小时#2",Kind=CardKind.GuestHour,RoomIndex=15, Hours=1,  CardNo=0x2404, P1="5BECF000", P2="420424AB" },
        new Vector{ Name="总控卡1天",  Kind=CardKind.Master,   Days=1, CardNo=0x2248, P1="5BECF0FF", P2="814822AB" },
        new Vector{ Name="总控2小时",  Kind=CardKind.Master,   Hours=2, CardNo=0x2BC2, P1="5BECF0FF", P2="42C22BAB" },
        new Vector{ Name="总控3小时",  Kind=CardKind.Master,   Hours=3, CardNo=0x2BC3, P1="5BECF0FF", P2="43C32BAB" },
        new Vector{ Name="总控2天",    Kind=CardKind.Master,   Days=2, CardNo=0x2BC4, P1="5BECF0FF", P2="82C42BAB" },
        new Vector{ Name="总控63天",   Kind=CardKind.Master,   Days=63, CardNo=0x2BC7, P1="5BECF0FF", P2="BFC72BAB" },
        new Vector{ Name="总控90天",   Kind=CardKind.Master,   Days=90, CardNo=0x2BC8, P1="5BECF0FF", P2="C3C82BAB" },
        // 校时卡（2026-11-08 IDA 解出位域，7 组实测样本验证）：
        //   页1[2] = 16*(分 & 0xF) + 9；页1[3] = (分 >> 4) + ((4*时) & 0x7C)
        // 注意 P2[3] 是运行时「年+月」（SelfTest 固定 Now=2026-11-08 → 0xAB），
        // 所以下面把实测样本的 P2[3]（8B/BB）统一按 0xAB 写，只验证卡号与页1。
        new Vector{ Name="校时08:30", Kind=CardKind.TimeSync, Hour=8,  Minute=30, CardNo=0x4200, P1="5BECE921", P2="060042AB" },
        new Vector{ Name="校时12:00", Kind=CardKind.TimeSync, Hour=12, Minute=0,  CardNo=0x4300, P1="5BEC0930", P2="060043AB" },
        new Vector{ Name="校时19:47", Kind=CardKind.TimeSync, Hour=19, Minute=47, CardNo=0x44C0, P1="5BECF94E", P2="06C044AB" },
        new Vector{ Name="校时22:00", Kind=CardKind.TimeSync, Hour=22, Minute=0,  CardNo=0x4580, P1="5BEC0958", P2="068045AB" },
        new Vector{ Name="校时11日19:47", Kind=CardKind.TimeSync, Hour=19, Minute=47, CardNo=0x5CC0, P1="5BECF94E", P2="06C05CAB" },
        new Vector{ Name="校时16:35", Kind=CardKind.TimeSync, Hour=16, Minute=35, CardNo=0x4C01, P1="5BEC3942", P2="06014CAB" },
        new Vector{ Name="总控120天",  Kind=CardKind.Master,   Days=120, CardNo=0x2BC9, P1="5BECF0FF", P2="C4C92BAB" },
        new Vector{ Name="总控360天",  Kind=CardKind.Master,   Days=360, CardNo=0x2BCB, P1="5BECF0FF", P2="CCCB2BAB" },
        new Vector{ Name="总控1095天", Kind=CardKind.Master,   Days=1095, CardNo=0x2BCD, P1="5BECF0FF", P2="E4CD2BAB" },
        new Vector{ Name="复位卡",     Kind=CardKind.Reset,    CardNo=0x2441, P1="5BEC0A00", P2="004124AB" },
        new Vector{ Name="清除卡",     Kind=CardKind.Clear,    CardNo=0x2442, P1="5BEC9BE5", P2="C74224AB" },
        new Vector{ Name="安装卡220",  Kind=CardKind.Install,  RoomIndex=15, CardNo=-1, P1="5BECFC00", P2="46F20001" },
        new Vector{ Name="安装卡201",  Kind=CardKind.Install,  RoomIndex=0,  CardNo=-1, P1="5BEC0C00", P2="46F20001" },
        new Vector{ Name="钟点卡12点", Kind=CardKind.CheckoutHour, Hour=12, CardNo=0x228F, P1="5BEC7B0C", P2="FF8F22AB" },
        new Vector{ Name="钟点卡14点", Kind=CardKind.CheckoutHour, Hour=14, CardNo=0x2290, P1="5BEC7B0E", P2="FF9022AB" },
    };

    static int SelfTest()
    {
        int pass = 0, fail = 0;
        // 页2[3] 现在是「年+月」编码，向量里的 0xAB 对应 2026-11。
        // 固定时间基准，避免黄金向量随真实日期漂移（2027 年会变成 0xBB）。
        CardBuilder.Now = new DateTime(2026, 11, 8, 14, 0, 0);
        foreach (var v in Vectors)
        {
            byte[] p1, p2;
            switch (v.Kind)
            {
                case CardKind.GuestDay: CardBuilder.BuildGuestDay(v.RoomIndex, v.Days, v.CardNo, out p1, out p2); break;
                case CardKind.GuestHour: CardBuilder.BuildGuestHour(v.RoomIndex, v.Hours, v.CardNo, out p1, out p2); break;
                case CardKind.Master: CardBuilder.BuildMaster(v.Days > 0 ? v.Days : 1, v.Hours, v.CardNo, out p1, out p2); break;
                case CardKind.Reset: CardBuilder.BuildReset(v.CardNo, out p1, out p2); break;
                case CardKind.Clear: CardBuilder.BuildClear(v.CardNo, out p1, out p2); break;
                case CardKind.Install: CardBuilder.BuildInstall(v.RoomIndex, out p1, out p2); break;
                case CardKind.CheckoutHour: CardBuilder.BuildCheckoutHour(v.Hour, v.CardNo, out p1, out p2); break;
                case CardKind.TimeSync: CardBuilder.BuildTimeSync(v.Hour, v.Minute, v.CardNo, out p1, out p2); break;
                default: throw new InvalidOperationException();
            }
            string g1 = HexOf(p1), g2 = HexOf(p2);
            bool ok = g1 == v.P1 && g2 == v.P2;

            // 反解析校验
            if (ok)
            {
                var raw = new byte[28];
                Array.Copy(p1, 0, raw, 0, 4);
                Array.Copy(p2, 0, raw, 4, 4);
                var info = CardBuilder.Parse(raw);
                if (info.Kind != v.Kind) ok = false;
                if (v.Kind == CardKind.GuestDay && (info.Days != v.Days || info.RoomIndex != v.RoomIndex)) ok = false;
                if (v.Kind == CardKind.GuestHour && (info.Hours != v.Hours || info.RoomIndex != v.RoomIndex)) ok = false;
                if (v.Kind == CardKind.Install && info.RoomIndex != v.RoomIndex) ok = false;
                if (v.Kind == CardKind.CheckoutHour && info.CheckoutHour != v.Hour) ok = false;
                if (v.CardNo >= 0 && info.CardNo != v.CardNo) ok = false;
            }

            if (ok) { pass++; Console.WriteLine("  ✓ " + v.Name.PadRight(12) + " " + g1 + " / " + g2); }
            else
            {
                fail++;
                Console.WriteLine("  × " + v.Name.PadRight(12) + " 期望 " + v.P1 + "/" + v.P2 +
                                  "  实得 " + g1 + "/" + g2);
            }
        }
        Console.WriteLine();
        // ---- 卡号时间戳编码（TD-150）----
        DateTime t0 = new DateTime(2026, 11, 8, 14, 0, 0);
        int[] seqs = new int[] { 0, 1, 62 };
        int[] wants = new int[] { 0x4380, 0x4381, 0x43BE };
        for (int i = 0; i < seqs.Length; i++)
        {
            int got = CardBuilder.TimestampCardNo(t0, seqs[i]);
            if (got == wants[i]) pass++;
            else
            {
                fail++;
                Console.WriteLine("  × 卡号 seq=" + seqs[i] + " 期望 " + wants[i].ToString("X4") + " 实际 " + got.ToString("X4"));
            }
        }

        // 解码回环：实测样本 0x5B34 = 11 日 12 点、卡内序号 52（即本小时第 53 张）
        int d, h, s;
        CardBuilder.DecodeTimestampCardNo(0x5B34, out d, out h, out s);
        if (d == 11 && h == 12 && s == 52) pass++;
        else { fail++; Console.WriteLine("  × 解码 0x5B34 得到 " + d + "日" + h + "点seq" + s); }

        // 第 64 张：满 63 张后进位到下一小时、n 归 1
        DateTime at;
        int nth;
        CardBuilder.NextStamp(0x43BE, t0, out at, out nth);   // 上次 = 14 点 seq62 = 第 63 张
        int carried = CardBuilder.TimestampCardNo(at, nth - 1);
        if (nth == 1 && carried == 0x43C0) pass++;
        else { fail++; Console.WriteLine("  × 第64张进位：期望 nth=1/0x43C0，实际 nth=" + nth + "/" + carried.ToString("X4")); }

        // 第 65 张（时钟仍是 14:00）：必须仍在 15 点，不能退回 14 点
        CardBuilder.NextStamp(0x43C0, t0, out at, out nth);
        int after = CardBuilder.TimestampCardNo(at, nth - 1);
        if (nth == 2 && after == 0x43C1) pass++;
        else { fail++; Console.WriteLine("  × 第65张：期望 nth=2/0x43C1（15点），实际 nth=" + nth + "/" + after.ToString("X4")); }

        // 跨日进位：23 点发满 → 次日 0 点第 1 张
        DateTime t1 = new DateTime(2026, 11, 8, 23, 30, 0);
        int lastOfDay = CardBuilder.TimestampCardNo(t1, 62);
        CardBuilder.NextStamp(lastOfDay, t1, out at, out nth);
        int nextDay = CardBuilder.TimestampCardNo(at, nth - 1);
        if (nth == 1 && nextDay == (9 << 11)) pass++;
        else { fail++; Console.WriteLine("  × 跨日进位：期望 nth=1/0x4800，实际 nth=" + nth + "/" + nextDay.ToString("X4")); }

        // ---- 时长码月数（TD-145，30 天/月）----
        int[][] durCases = new int[][]
        {
            new int[] { 63, 0xBF },    // 63 天：0x80|63，不改月
            new int[] { 64, 0xC2 },    // 64/30≈2.1 → 2 月
            new int[] { 90, 0xC3 },    // 3 月
            new int[] { 150, 0xC5 },   // 5 月
            new int[] { 360, 0xCC },   // 12 月
            new int[] { 1095, 0xE4 },  // 36 月（原 31 天/月会算成 35）
        };
        for (int i = 0; i < durCases.Length; i++)
        {
            byte got = CardBuilder.DurationDays(durCases[i][0]);
            if (got == (byte)durCases[i][1]) pass++;
            else
            {
                fail++;
                Console.WriteLine("  × DurationDays(" + durCases[i][0] + ") 期望 "
                    + ((byte)durCases[i][1]).ToString("X2") + " 实际 " + got.ToString("X2"));
            }
        }

        Console.WriteLine("自检结果: " + pass + " 通过, " + fail + " 失败");
        return fail == 0 ? 0 : 5;
    }

    static string HexOf(byte[] b)
    {
        var sb = new StringBuilder(b.Length * 2);
        foreach (var x in b) sb.Append(x.ToString("X2"));
        return sb.ToString();
    }

    // ---------------- 输出 ----------------

    static int Fail(int rc, Opt o)
    {
        if (o.Json)
            Console.WriteLine("{\"ok\":false,\"error\":{\"code\":" + rc + ",\"msg\":\"" + JsonEscape(LockResult.Describe(rc)) + "\"}}");
        else
            Console.Error.WriteLine("× [" + rc + "] " + LockResult.Describe(rc));
        // SDK 码 → 进程退出码
        switch (rc)
        {
            case LockResult.ErrParam: return 1;
            case LockResult.ErrDriver:
            case LockResult.ErrPort:
            case LockResult.ErrShake: return 2;
            case LockResult.ErrNoCard:
            case LockResult.ErrRead: return 3;
            case LockResult.ErrWrite: return 4;
            case LockResult.ErrVerify: return 5;
            case LockResult.ErrCardNo: return 6;
            case LockResult.ErrUnsupported: return 7;
            default: return 1;
        }
    }

    static string Json(CardInfo c)
    {
        var sb = new StringBuilder();
        sb.Append("{\"ok\":true,\"card\":{");
        sb.Append("\"kind\":\"").Append(CardInfo.KindText(c.Kind)).Append("\",");
        sb.Append("\"room\":").Append(c.Room == null ? "null" : "\"" + JsonEscape(c.Room) + "\"").Append(',');
        sb.Append("\"roomIndex\":").Append(c.RoomIndex).Append(',');
        sb.Append("\"days\":").Append(c.Kind == CardKind.GuestDay ? c.Days : 0).Append(',');
        sb.Append("\"hours\":").Append(c.Kind == CardKind.GuestHour ? c.Hours : 0).Append(',');
        sb.Append("\"cardNo\":").Append(c.CardNo >= 0 ? "\"0x" + c.CardNo.ToString("X4") + "\"" : "null").Append(',');
        // 卡内解析字段（TD-150/156）：前端「卡号 YYMMDDHHnn」与「入住时间」依赖它们，
        // 缺了就只能回退成十六进制显示。
        sb.Append("\"isLost\":").Append(c.IsLost ? "true" : "false").Append(',');
        sb.Append("\"issueDay\":").Append(c.IssueDay).Append(',');
        sb.Append("\"issueHour\":").Append(c.IssueHour).Append(',');
        sb.Append("\"issueSeq\":").Append(c.IssueSeq).Append(',');
        sb.Append("\"cardYm\":").Append(CardYmByte(c)).Append(',');
        sb.Append("\"raw\":\"").Append(c.RawHex).Append("\"");
        sb.Append("}}");
        return sb.ToString();
    }

    /// <summary>页2[3]（年 + 月）原始字节：从 RawHex 第 8 个字节取（hex 字符 14..16）。</summary>
    static int CardYmByte(CardInfo c)
    {
        var h = c.RawHex;
        if (h == null || h.Length < 16) return 0;
        int v;
        return int.TryParse(h.Substring(14, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out v) ? v : 0;
    }

    static string JsonEscape(string s)
    {
        if (s == null) return "";
        return s.Replace("\\", "\\\\").Replace("\"", "\\\"");
    }

    static void Usage()
    {
        Console.WriteLine("PMSLockCard — TEMIC 门锁卡工具");
        Console.WriteLine("  read [--json]");
        Console.WriteLine("  issue --room 220 --days 1     [--serial 2448] [--json] [--reuse]");
        Console.WriteLine("  issue --room 220 --hours 3");
        Console.WriteLine("  issue --type master|reset|clear");
        Console.WriteLine("  issue --type install --room 220");
        Console.WriteLine("  issue --type checkout --hour 12");
        Console.WriteLine("  issue --raw --p1 5BEC492B --p2 13BF22AB");
        Console.WriteLine("  selftest                       纯字节自检（不需读卡器）");
        Console.WriteLine("  ports                          扫描可用串口");
        Console.WriteLine("  其它: --port N  --dir 驱动目录  --ini 配置文件");
    }
}


