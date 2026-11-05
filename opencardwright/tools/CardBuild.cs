// 仅供**模拟设备**使用：离线构一张卡（不打开串口、不写硬件）。
//
// 字节规则全部来自 SDK 的 CardBuilder / CardNoStore，这里只做参数解析和
// 28 字节组装（页1+页2，页3–7 补 0），不重复实现卡格式。
//
// 用法：
//   OCWCardBuild.exe --state <CardNo.state> --start 2403 --index 15 --days 1
//   OCWCardBuild.exe --state ... --index 15 --hours 3
//   OCWCardBuild.exe --state ... --index 15 --days 1 --serial 240A   （不推进计数器）
//
// 输出：{"ok":true,"roomIndex":15,"cardNo":"0x2404","raw":"5BEC..."}

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
            int index = -1;
            int days = 0;
            int hours = 0;
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
                    case "--index": index = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--days": days = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--hours": hours = int.Parse(val, CultureInfo.InvariantCulture); break;
                    case "--start": start = int.Parse(val, NumberStyles.HexNumber, CultureInfo.InvariantCulture); break;
                    case "--serial": serial = int.Parse(val, NumberStyles.HexNumber, CultureInfo.InvariantCulture); break;
                }
            }

            if (state == null) return Fail("缺少 --state");
            if (index < 0 || index > 255) return Fail("缺少或非法的 --index（0–255）");
            if (days <= 0 && hours <= 0) return Fail("--days 或 --hours 必须给一个");

            // 计数器语义也交给 SDK：Next() 会先落盘再返回，和真机发卡完全一致
            int cardNo = serial >= 0 ? serial : new CardNoStore(state, start).Next();

            byte[] p1, p2;
            if (hours > 0) CardBuilder.BuildGuestHour(index, hours, cardNo, out p1, out p2);
            else CardBuilder.BuildGuestDay(index, days, cardNo, out p1, out p2);

            byte[] raw = new byte[28];
            Array.Copy(p1, 0, raw, 0, 4);
            Array.Copy(p2, 0, raw, 4, 4);

            Console.WriteLine(
                "{\"ok\":true,\"roomIndex\":" + index +
                ",\"cardNo\":\"0x" + cardNo.ToString("X4", CultureInfo.InvariantCulture) +
                "\",\"raw\":\"" + ToHex(raw) + "\"}");
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
