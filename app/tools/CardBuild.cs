// 仅供**模拟设备**使用：离线构一张卡（不打开串口、不写硬件）。
//
// 字节规则全部来自 SDK 的 CardBuilder / CardNoStore，这里只做参数解析和
// 28 字节组装（页1+页2，页3–7 补 0），不重复实现卡格式。
//
// 用法：
//   OCWCardBuild.exe --state <CardNo.state> --start 2403 --index 15 --days 1
//   OCWCardBuild.exe --state ... --index 15 --hours 3
//   OCWCardBuild.exe --state ... --index 15 --type install          （安装卡，不占卡号）
//   OCWCardBuild.exe --state ... --type master --days 30
//   OCWCardBuild.exe --state ... --type reset | clear
//   OCWCardBuild.exe --state ... --type checkout --hour 12
//   OCWCardBuild.exe --state ... --type timesync --hour 10 --minute 48
//   OCWCardBuild.exe --state ... --type raw --p1 5BEC092B --p2 13BF22AB
//   OCWCardBuild.exe --state ... --index 15 --days 1 --serial 240A   （不推进计数器）
//
// 卡号语义：安装卡与 raw 卡**不含卡号**（BuildInstall/BuildRaw 都不接收 cardNo），
// 所以只有这两类不推进 CardNo.state —— 真机发安装卡同样不吃卡号。
//
// 输出：{"ok":true,"roomIndex":15,"cardNo":"0x2404","raw":"5BEC..."}
//       不含卡号时 cardNo 为 null。

using System;
using System.Globalization;
using System.Text;
using PmsLockSdk;

internal static class CardBuild
{
    private static int Main(string[] args)
    {
        try
        {
            string state = null;
            string type = null;
            string p1Hex = null;
            string p2Hex = null;
            int index = -1;
            int days = 0;
            int hours = 0;
            int hour = -1;
            int minute = -1;
            int start = 0x2403;
            int serial = -1;

            for (int i = 0; i < args.Length; i++)
            {
                if (i + 1 >= args.Length) break;
                string key = args[i];
                string val = args[++i];
                switch (key)
                {
                    case "--state": state = val; break;
                    case "--type": type = val.ToLowerInvariant(); break;
                    case "--index": index = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--days": days = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--hours": hours = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--hour": hour = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--minute": minute = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--p1": p1Hex = val; break;
                    case "--p2": p2Hex = val; break;
                    case "--start": start = int.Parse(val, NumberStyles.HexNumber, CultureInfo.InvariantCulture); break;
                    case "--serial": serial = int.Parse(val, NumberStyles.HexNumber, CultureInfo.InvariantCulture); break;
                }
            }

            if (state == null) return Fail("缺少 --state");

            // 老调用（不带 --type）保持原语义：有 --hours 就是钟点客卡，否则日租客卡
            if (type == null) type = hours > 0 ? "guest-hour" : "guest-day";

            bool needsIndex = type == "guest-day" || type == "guest-hour" || type == "install";
            if (needsIndex && (index < 0 || index > 255)) return Fail("缺少或非法的 --index（0–255）");

            // 只有含卡号的卡型才推进计数器
            bool needsCardNo = type != "install" && type != "raw";
            int cardNo = -1;
            // CLI/构包器不再自增：卡号必须由调用方用 --serial 给（卡号由 app 侧算）
            if (needsCardNo && serial < 0)
            {
                Console.Error.WriteLine("× 需要 --serial 指定卡号（构包器不再自增）");
                return 1;
            }
            if (needsCardNo) cardNo = serial;

            byte[] p1, p2;
            switch (type)
            {
                case "guest-day":
                    if (days <= 0) return Fail("日租卡需要 --days");
                    CardBuilder.BuildGuestDay(index, days, cardNo, out p1, out p2);
                    break;
                case "guest-hour":
                    if (hours <= 0) return Fail("钟点卡需要 --hours");
                    CardBuilder.BuildGuestHour(index, hours, cardNo, out p1, out p2);
                    break;
                case "install":
                    CardBuilder.BuildInstall(index, out p1, out p2);
                    break;
                case "master":
                    // 两者都没给时按 SDK 默认：1 天
                    if (days <= 0 && hours <= 0) days = 1;
                    CardBuilder.BuildMaster(days, hours, cardNo, out p1, out p2);
                    break;
                case "reset":
                    CardBuilder.BuildReset(cardNo, out p1, out p2);
                    break;
                case "clear":
                    CardBuilder.BuildClear(cardNo, out p1, out p2);
                    break;
                case "checkout":
                    if (hour < 0) hour = 12;
                    CardBuilder.BuildCheckoutHour(hour, cardNo, out p1, out p2);
                    break;
                case "timesync":
                    if (hour < 0) hour = DateTime.Now.Hour;
                    if (minute < 0) minute = DateTime.Now.Minute;
                    CardBuilder.BuildTimeSync(hour, minute, cardNo, out p1, out p2);
                    break;
                case "raw":
                    if (string.IsNullOrEmpty(p1Hex) || string.IsNullOrEmpty(p2Hex))
                        return Fail("raw 卡需要 --p1 与 --p2");
                    CardBuilder.BuildRaw(p1Hex, p2Hex, out p1, out p2);
                    break;
                default:
                    return Fail("未知 --type：" + type);
            }

            byte[] raw = new byte[28];
            Array.Copy(p1, 0, raw, 0, 4);
            Array.Copy(p2, 0, raw, 4, 4);

            string cardNoJson = needsCardNo
                ? ("\"0x" + cardNo.ToString("X4", CultureInfo.InvariantCulture) + "\"")
                : "null";
            Console.WriteLine(
                "{\"ok\":true,\"type\":\"" + type + "\",\"roomIndex\":" + index +
                ",\"cardNo\":" + cardNoJson +
                ",\"raw\":\"" + ToHex(raw) + "\"}");
            return 0;
        }
        catch (Exception ex)
        {
            return Fail(ex.Message);
        }
    }

    private static int Fail(string msg)
    {
        string safe = msg.Replace("\\", "/").Replace("\"", "'").Replace("\r", " ").Replace("\n", " ");
        Console.WriteLine("{\"ok\":false,\"error\":{\"code\":9,\"msg\":\"" + safe + "\"}}");
        return 1;
    }

    /// <summary>SDK 的 Hex 是 internal，这里自己转一下（纯字节转十六进制，不涉及卡格式）。</summary>
    private static string ToHex(byte[] b)
    {
        var sb = new StringBuilder(b.Length * 2);
        for (int i = 0; i < b.Length; i++) sb.Append(b[i].ToString("X2", CultureInfo.InvariantCulture));
        return sb.ToString();
    }
}
