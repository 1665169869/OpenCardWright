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
    }

    static Opt ParseArgs(string[] a, int start)
    {
        var o = new Opt();
        for (int i = start; i < a.Length; i++)
        {
            switch (a[i])
            {
                case "--room": o.Room = a[++i]; break;
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

            CardInfo info = null;
            switch (o.Type)
            {
                case "guest":
                case "day":
                    if (o.Hours > 0)
                        rc = r.IssueGuestHour(o.Room, o.Hours, policy, o.Serial, out info);
                    else
                        rc = r.IssueGuestDay(o.Room, o.Days <= 0 ? 1 : o.Days, policy, o.Serial, out info);
                    break;
                case "hour":
                    rc = r.IssueGuestHour(o.Room, o.Hours <= 0 ? 1 : o.Hours, policy, o.Serial, out info);
                    break;
                case "master": rc = r.IssueMaster(policy, o.Serial, out info); break;
                case "reset": rc = r.IssueReset(policy, o.Serial, out info); break;
                case "clear": rc = r.IssueClear(policy, o.Serial, out info); break;
                case "install": rc = r.IssueInstall(o.Room, out info); break;
                case "checkout": rc = r.IssueCheckoutHour(o.Hour, policy, o.Serial, out info); break;
                case "timesync":
                    {
                        int hh = 0, mm = 0;
                        var now = DateTime.Now;
                        hh = now.Hour; mm = now.Minute;
                        if (o.Room != null && o.Room.Contains(":"))
                        {
                            var parts = o.Room.Split(':');
                            hh = int.Parse(parts[0]); mm = int.Parse(parts[1]);
                        }
                        rc = r.IssueTimeSync(hh, mm, out info);
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
        public string Name; public CardKind Kind; public int RoomIndex, Days, Hours, Hour, CardNo;
        public string P1, P2;
    }

    static readonly Vector[] Vectors = new[]
    {
        new Vector{ Name="220/5天",   Kind=CardKind.GuestDay, RoomIndex=15, Days=5, CardNo=0x2288, P1="5BECF100", P2="858822AB" },
        new Vector{ Name="209/1天",   Kind=CardKind.GuestDay, RoomIndex=6,  Days=1, CardNo=0x2250, P1="5BEC6100", P2="815022AB" },
        new Vector{ Name="260/1天",   Kind=CardKind.GuestDay, RoomIndex=39, Days=1, CardNo=0x2281, P1="5BEC7102", P2="818122AB" },
        new Vector{ Name="201/1天",   Kind=CardKind.GuestDay, RoomIndex=0,  Days=1, CardNo=0x224A, P1="5BEC0100", P2="814A22AB" },
        new Vector{ Name="205/3天",   Kind=CardKind.GuestDay, RoomIndex=3,  Days=3, CardNo=0x2249, P1="5BEC3000", P2="834922AB" },
        new Vector{ Name="205/4天",   Kind=CardKind.GuestDay, RoomIndex=3,  Days=4, CardNo=0x2249, P1="5BEC3000", P2="844922AB" },
        new Vector{ Name="220/1天",   Kind=CardKind.GuestDay, RoomIndex=15, Days=1, CardNo=0x2403, P1="5BECF100", P2="810324AB" },
        new Vector{ Name="220/1小时", Kind=CardKind.GuestHour, RoomIndex=15, Hours=1,  CardNo=0x228A, P1="5BECF100", P2="428A22AB" },
        new Vector{ Name="220/3小时", Kind=CardKind.GuestHour, RoomIndex=15, Hours=3,  CardNo=0x2289, P1="5BECF100", P2="448922AB" },
        new Vector{ Name="220/6小时", Kind=CardKind.GuestHour, RoomIndex=15, Hours=6,  CardNo=0x228B, P1="5BECF100", P2="478B22AB" },
        new Vector{ Name="220/62小时",Kind=CardKind.GuestHour, RoomIndex=15, Hours=62, CardNo=0x228C, P1="5BECF100", P2="7F8C22AB" },
        new Vector{ Name="220/1小时#2",Kind=CardKind.GuestHour,RoomIndex=15, Hours=1,  CardNo=0x2404, P1="5BECF100", P2="420424AB" },
        new Vector{ Name="总控卡1天",  Kind=CardKind.Master,   Days=1, CardNo=0x2248, P1="5BECF0FF", P2="814822AB" },
        new Vector{ Name="总控2小时",  Kind=CardKind.Master,   Hours=2, CardNo=0x2BC2, P1="5BECF0FF", P2="42C22BAB" },
        new Vector{ Name="总控3小时",  Kind=CardKind.Master,   Hours=3, CardNo=0x2BC3, P1="5BECF0FF", P2="43C32BAB" },
        new Vector{ Name="总控2天",    Kind=CardKind.Master,   Days=2, CardNo=0x2BC4, P1="5BECF0FF", P2="82C42BAB" },
        new Vector{ Name="总控63天",   Kind=CardKind.Master,   Days=63, CardNo=0x2BC7, P1="5BECF0FF", P2="BFC72BAB" },
        new Vector{ Name="总控90天",   Kind=CardKind.Master,   Days=90, CardNo=0x2BC8, P1="5BECF0FF", P2="C3C82BAB" },
        new Vector{ Name="总控120天",  Kind=CardKind.Master,   Days=120, CardNo=0x2BC9, P1="5BECF0FF", P2="C4C92BAB" },
        new Vector{ Name="总控360天",  Kind=CardKind.Master,   Days=360, CardNo=0x2BCB, P1="5BECF0FF", P2="CCCB2BAB" },
        new Vector{ Name="总控1095天", Kind=CardKind.Master,   Days=1095, CardNo=0x2BCD, P1="5BECF0FF", P2="E3CD2BAB" },
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
        sb.Append("\"raw\":\"").Append(c.RawHex).Append("\"");
        sb.Append("}}");
        return sb.ToString();
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


