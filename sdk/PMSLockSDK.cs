// =====================================================================
//  PMSLockSDK.cs — 智能门锁 TEMIC 卡 发卡 / 读卡 函数包（.NET 4.0 / x86）
//
//  编译：
//    csc /target:library /platform:x86 /out:PMSLockSDK.dll /doc:PMSLockSDK.xml PMSLockSDK.cs
//
//  依赖：32 位进程；IOServer\LC2000\LC32RFRW.dll
//
//  卡内格式（实测）：
//    页1 = 5B EC | ((序号 & 0x0F) << 4 | 低4位) | (序号 >> 4)
//    页2 = 时长码 | 卡号低字节 | 卡号高字节 | 0xAB
//      日租：0x80|天数      钟点：0x40|(小时+1)
//      低4位：钟点恒 1；日租 (天数 & 3)==1 ? 1 : 0
//      时长码(天)：1~63 -> 0x80|天数；>63 -> 0xC0|round(天数/31)
//      总控卡小时：0x40|小时（客卡是 0x40|(小时+1)）
//
//  卡号规则（2026-11-05 实测确认）：锁对每种卡型分别维护一个"当前有效卡号"，
//  新卡号 >= 它才能开（等于也开），小于则拒；日租与钟点互不影响。
//  SDK 默认递增发号，且要求紧贴原软件序列（CardNo.state 不要乱跳）。
// =====================================================================
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace PmsLockSdk
{
    /// <summary>卡类型</summary>
    public enum CardKind
    {
        Unknown = 0,
        GuestDay,       // 日租客卡
        GuestHour,      // 钟点客卡
        Master,         // 总控卡
        Reset,          // 复位卡
        Clear,          // 清除卡
        Install,        // 安装卡
        CheckoutHour,   // 退房钟点设置卡
        TimeSync,       // 校时卡（时间字段未解，仅能识别）
        Blank           // 空白卡 / 已退房
    }

    /// <summary>返回码</summary>
    public static class LockResult
    {
        public const int OK = 0;
        public const int ErrParam = 1;          // 参数非法
        public const int ErrDriver = 2;         // 驱动 DLL 缺失/加载失败
        public const int ErrPort = 3;           // 串口打开失败（多为被原软件占用）
        public const int ErrShake = 4;          // 握手失败
        public const int ErrNoCard = 5;         // 无卡 / 卡型不符
        public const int ErrRead = 6;           // 读卡失败
        public const int ErrWrite = 7;          // 写页失败
        public const int ErrVerify = 8;         // 回读校验失败
        public const int ErrCardNo = 9;         // 卡号无效（越界 / 小于当前状态）
        public const int ErrUnsupported = 10;   // 该卡型暂不支持
        public const int ErrState = 11;         // 状态文件读写失败

        public static string Describe(int code)
        {
            switch (code)
            {
                case OK: return "成功";
                case ErrParam: return "参数非法";
                case ErrDriver: return "驱动 DLL 加载失败（检查 DriverDir）";
                case ErrPort: return "串口打开失败（可能被门锁软件占用，请先关闭它）";
                case ErrShake: return "读卡器握手失败（检查连接/供电）";
                case ErrNoCard: return "未检测到卡片或卡型不符";
                case ErrRead: return "读卡失败";
                case ErrWrite: return "写卡失败";
                case ErrVerify: return "写后回读校验失败";
                case ErrCardNo: return "卡号无效";
                case ErrUnsupported: return "该卡型暂不支持自动生成";
                case ErrState: return "卡号状态文件读写失败";
                default: return "未知错误 " + code;
            }
        }
    }

    /// <summary>卡号生成策略</summary>
    public enum CardNoPolicy
    {
        /// <summary>总是取下一个递增号（默认，最安全）</summary>
        Increment,
        /// <summary>沿用卡上已有的卡号（模拟原软件"续写同一张卡"）</summary>
        ReuseCard
    }

    /// <summary>读出的卡片信息</summary>
    public sealed class CardInfo
    {
        public byte[] Raw28;            // 28 字节原始数据
        public CardKind Kind = CardKind.Unknown;
        public int RoomIndex = -1;      // 房间序号（-1 = 不适用）
        public string Room;             // 房号（由 RoomTable 反查，可能为 null）
        public int Days;                // 日租天数
        public int Hours;               // 钟点小时数
        public int CheckoutHour = -1;   // 退房钟点卡的钟点
        public int CardNo = -1;         // 16 位卡号（-1 = 无）
        public bool IsBlank;            // 空白/已退房
        public bool IsLost;             // 挂失卡（页1[2] 低 4 位 = 1；TD-150 实测）
        public int IssueDay;            // 发卡日（从卡号解码；TD-150）
        public int IssueHour;           // 发卡小时
        public int IssueSeq;            // 小时内序号（0–62）
        public bool IsMonthEncoded;     // 时长字段是 0xC_ 段（月数编码，天数近似）

        public string RawHex { get { return Raw28 == null ? "" : Hex.ToHex(Raw28, Raw28.Length); } }

        public string Describe()
        {
            if (IsBlank) return "空白卡 / 已退房";
            var sb = new StringBuilder();
            sb.Append(KindText(Kind));
            if (Room != null) sb.Append(" 房号=").Append(Room);
            else if (RoomIndex >= 0) sb.Append(" 序号=").Append(RoomIndex);
            if (Kind == CardKind.GuestDay) sb.Append(" 天数=").Append(Days);
            if (Kind == CardKind.GuestHour) sb.Append(" 小时=").Append(Hours);
            if (Kind == CardKind.CheckoutHour) sb.Append(" 退房钟点=").Append(CheckoutHour);
            if (CardNo >= 0)
            {
                sb.Append(" 卡号=0x").Append(CardNo.ToString("X4"));
                if (IssueDay > 0)
                    sb.Append(" 发卡=").Append(IssueDay).Append("日")
                      .Append(IssueHour).Append("点").Append(IssueSeq).Append("序");
            }
            if (IsLost) sb.Append(" 【挂失】");
            return sb.ToString();
        }

        public static string KindText(CardKind k)
        {
            switch (k)
            {
                case CardKind.GuestDay: return "日租客卡";
                case CardKind.GuestHour: return "钟点客卡";
                case CardKind.Master: return "总控卡";
                case CardKind.Reset: return "复位卡";
                case CardKind.Clear: return "清除卡";
                case CardKind.Install: return "安装卡";
                case CardKind.CheckoutHour: return "退房钟点设置卡";
                case CardKind.TimeSync: return "校时卡";
                case CardKind.Blank: return "空白卡";
                default: return "未知卡型";
            }
        }
    }

    internal static class Hex
    {
        public static string ToHex(byte[] b, int n)
        {
            var sb = new StringBuilder(n * 2);
            for (int i = 0; i < n; i++) sb.Append(b[i].ToString("X2"));
            return sb.ToString();
        }

        public static byte[] FromHex(string h, int n)
        {
            var o = new byte[n];
            for (int i = 0; i < n; i++)
                o[i] = byte.Parse(h.Substring(i * 2, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture);
            return o;
        }

        public static byte[] AsciiZ(string s)
        {
            var b = new byte[s.Length + 2];
            for (int i = 0; i < s.Length; i++) b[i] = (byte)s[i];
            return b;
        }
    }

    /// <summary>卡内数据构造与解析（纯函数，可离线单测）</summary>
    public static class CardBuilder
    {
        public const byte Prefix0 = 0x5B;
        public const byte Prefix1 = 0xEC;
        /// <summary>
        /// 卡内「时间基准」。默认取本机当前时间；测试 / 离线构包可注入固定值，
        /// 这样 selftest 的黄金向量不会随真实日期漂移（TD-155）。
        /// </summary>
        public static DateTime Now = DateTime.Now;

        /// <summary>
        /// 页2[3]：**年（高 4 位）+ 月（低 4 位）**。2026 年 11 月 = `0xAB`。
        /// 读卡侧的还原规则（原厂 `sub_1000260C`）：
        /// `年 = 2000 + 16 × ⌊(当前年 − 2000) / 16⌋ + 高4位`，16 年一循环。
        /// </summary>
        public static byte Suffix
        {
            get
            {
                int y = (Now.Year - 2000) & 0x0F;
                int m = Now.Month & 0x0F;
                return (byte)((y << 4) | m);
            }
        }

        /// <summary>
        /// 按发卡时刻生成卡号（TD-150，2026-11-08 实测破译）。
        /// 16 位 = 日(5) | 时(5) | 小时内序号(6)，与原厂「作业流水号 = 年月日时 + 小时内序号」对应。
        /// 例：11 日 12 点、该小时第 52 张 → (11&lt;&lt;11) | (12&lt;&lt;6) | 52 = <c>0x5B34</c>
        /// （卡内按小端存：N_L=0x34、N_H=0x5B）。
        /// </summary>
        public static int TimestampCardNo(DateTime at, int seq)
        {
            if (seq < 0 || seq > 62)
                throw new ArgumentOutOfRangeException("seq", "小时内序号必须在 0–62");
            return ((at.Day & 0x1F) << 11) | ((at.Hour & 0x1F) << 6) | (seq & 0x3F);
        }

        /// <summary>从卡号还原发卡日期/小时/序号（读卡显示用）。</summary>
        public static void DecodeTimestampCardNo(int cardNo, out int day, out int hour, out int seq)
        {
            day = (cardNo >> 11) & 0x1F;
            hour = (cardNo >> 6) & 0x1F;
            seq = cardNo & 0x3F;
        }

        /// <summary>
        /// 按原厂规则推进「发卡时刻 + 小时内序号」：
        /// 同一小时内序号 +1；满 63 次（序号超过 62）则时刻进位 1 小时、序号归 0；
        /// 当前时刻已经跨小时则直接用当前时刻、序号从 0 开始。
        /// 文档原文：「发卡序号为当前写卡时间的小时内的第几次写卡，超过 63 次则 fksj 小时 +1，序号复位为 0」。
        /// </summary>
        /// <summary>
        /// 按原厂规则推进「发卡时刻 + 本小时第 n 张」。
        /// - 同一小时：n = 上次 + 1；满 63 张后进位到下一小时、n 归 1
        /// - **进位产生的「未来小时」是下一次的基准**，不会退回当前小时（2026-11-08 修正）
        /// - 跨小时/跨日：以当前时刻为基准，n 从 1 开始
        /// <paramref name="nth"/> 取 **1–63**（卡内序号 = nth - 1）。
        /// </summary>
        public static void NextStamp(int currentCardNo, DateTime now, out DateTime at, out int nth)
        {
            int day, hour, lastSeq;
            DecodeTimestampCardNo(currentCardNo, out day, out hour, out lastSeq);

            // 同一「日」且卡号小时 >= 当前小时：沿用卡号的小时作为基准
            // （hour > now.Hour 说明上次已进位到未来小时，这里必须接着往后走，不能回退）
            if (day == now.Day && hour >= now.Hour)
            {
                at = new DateTime(now.Year, now.Month, now.Day, hour, 0, 0);
                nth = lastSeq + 2;              // 上次是 lastSeq+1，本次再 +1
                if (nth > 63)
                {
                    nth = 1;
                    at = at.AddHours(1);        // 满 63 张 → 小时 +1，n 复位
                }
                return;
            }

            // 跨小时 / 跨日：从当前时刻重新计数
            at = new DateTime(now.Year, now.Month, now.Day, now.Hour, 0, 0);
            nth = 1;
        }

        /// <summary>
        /// 天数 → 时长码。1~63 天用 0x80|天数；超过 63 天用 0xC0|月数。
        /// **月按 30 天算**（2026-11-08 实测：锁按日历月执行，90 天→3 月、1095 天→36 月；
        /// 原来用 31 天会算出 35 月，实际少约一个月——见 TD-145）。
        /// </summary>
        public static byte DurationDays(int days)
        {
            if (days < 1) days = 1;
            if (days <= 63) return (byte)(0x80 | (days & 0x3F));
            int months = (int)Math.Round(days / 30.0);
            if (months < 1) months = 1;
            if (months > 63) months = 63;
            return (byte)(0xC0 | (months & 0x3F));
        }

        /// <summary>
        /// 小时 → 时长码。客卡是 0x40|(小时+1)，总控卡是 0x40|小时（实测差异）。
        /// </summary>
        public static byte DurationHours(int hours, bool guest)
        {
            if (hours < 1) hours = 1;
            int v = guest ? hours + 1 : hours;
            if (v > 63) v = 63;
            return (byte)(0x40 | (v & 0x3F));
        }

        /// <summary>日租客卡页1/页2</summary>
        public static void BuildGuestDay(int roomIndex, int days, int cardNo, out byte[] p1, out byte[] p2)
        {
            BuildGuestDay(roomIndex, days, cardNo, false, out p1, out p2);
        }

        /// <summary>日租客卡；lost = 挂失卡（页1[2] 低 4 位 = 1，让该房间在用卡失效）。</summary>
        public static void BuildGuestDay(int roomIndex, int days, int cardNo, bool lost, out byte[] p1, out byte[] p2)
        {
            if (roomIndex < 0 || roomIndex > 0xFF) throw new ArgumentOutOfRangeException("roomIndex");
            if (days < 1 || days > 1095) throw new ArgumentOutOfRangeException("days");
            BuildGuest(roomIndex, (byte)(lost ? 1 : 0), DurationDays(days), cardNo, out p1, out p2);
        }

        /// <summary>钟点客卡</summary>
        public static void BuildGuestHour(int roomIndex, int hours, int cardNo, out byte[] p1, out byte[] p2)
        {
            BuildGuestHour(roomIndex, hours, cardNo, false, out p1, out p2);
        }

        /// <summary>钟点客卡；lost = 挂失卡。</summary>
        public static void BuildGuestHour(int roomIndex, int hours, int cardNo, bool lost, out byte[] p1, out byte[] p2)
        {
            if (roomIndex < 0 || roomIndex > 0xFF) throw new ArgumentOutOfRangeException("roomIndex");
            if (hours < 1 || hours > 62) throw new ArgumentOutOfRangeException("hours");
            BuildGuest(roomIndex, (byte)(lost ? 1 : 0), DurationHours(hours, true), cardNo, out p1, out p2);
        }

        /// <summary>允许手动指定低 4 位（用于对标未解规律）</summary>
        public static void BuildGuestRaw(int roomIndex, int lowNibble, int durationCode, int cardNo, out byte[] p1, out byte[] p2)
        {
            BuildGuest(roomIndex, (byte)(lowNibble & 0x0F), (byte)(durationCode & 0xFF), cardNo, out p1, out p2);
        }

        static void BuildGuest(int roomIndex, byte low, byte duration, int cardNo, out byte[] p1, out byte[] p2)
        {
            p1 = new byte[4];
            p1[0] = Prefix0; p1[1] = Prefix1;
            p1[2] = (byte)(((roomIndex & 0x0F) << 4) | low);
            p1[3] = (byte)((roomIndex >> 4) & 0xFF);
            p2 = new byte[4];
            p2[0] = duration;
            p2[1] = (byte)(cardNo & 0xFF);
            p2[2] = (byte)((cardNo >> 8) & 0xFF);
            p2[3] = Suffix;
        }

        /// <summary>总控卡（默认 1 天）</summary>
        public static void BuildMaster(int cardNo, out byte[] p1, out byte[] p2)
        {
            BuildMaster(1, 0, cardNo, out p1, out p2);
        }

        /// <summary>总控卡（指定有效期：hours&gt;0 用小时，否则用 days）</summary>
        public static void BuildMaster(int days, int hours, int cardNo, out byte[] p1, out byte[] p2)
        {
            p1 = new byte[] { Prefix0, Prefix1, 0xF0, 0xFF };
            byte dur = hours > 0 ? DurationHours(hours, false) : DurationDays(days);
            p2 = new byte[] { dur, (byte)(cardNo & 0xFF), (byte)((cardNo >> 8) & 0xFF), Suffix };
        }

        /// <summary>复位卡</summary>
        public static void BuildReset(int cardNo, out byte[] p1, out byte[] p2)
        {
            p1 = new byte[] { Prefix0, Prefix1, 0x0A, 0x00 };
            p2 = new byte[] { 0x00, (byte)(cardNo & 0xFF), (byte)((cardNo >> 8) & 0xFF), Suffix };
        }

        /// <summary>清除卡</summary>
        public static void BuildClear(int cardNo, out byte[] p1, out byte[] p2)
        {
            p1 = new byte[] { Prefix0, Prefix1, 0x9B, 0xE5 };
            p2 = new byte[] { 0xC7, (byte)(cardNo & 0xFF), (byte)((cardNo >> 8) & 0xFF), Suffix };
        }

        /// <summary>安装卡（只含房间序号，页2 固定 46F20001）</summary>
        public static void BuildInstall(int roomIndex, out byte[] p1, out byte[] p2)
        {
            if (roomIndex < 0 || roomIndex > 0xFF) throw new ArgumentOutOfRangeException("roomIndex");
            p1 = new byte[4];
            p1[0] = Prefix0; p1[1] = Prefix1;
            p1[2] = (byte)(((roomIndex & 0x0F) << 4) | 0x0C);
            p1[3] = (byte)((roomIndex >> 4) & 0xFF);
            p2 = new byte[] { 0x46, 0xF2, 0x00, 0x01 };
        }

        /// <summary>退房钟点设置卡</summary>
        public static void BuildCheckoutHour(int hour, int cardNo, out byte[] p1, out byte[] p2)
        {
            if (hour < 0 || hour > 23) throw new ArgumentOutOfRangeException("hour");
            p1 = new byte[] { Prefix0, Prefix1, 0x7B, (byte)hour };
            p2 = new byte[] { 0xFF, (byte)(cardNo & 0xFF), (byte)((cardNo >> 8) & 0xFF), Suffix };
        }

        /// <summary>
        /// 校时卡。值 = 16 × 当日分钟数 + 953（只含当日时/分）。
        /// 页2[3] 由 <see cref="Suffix"/> 按年月生成；页2 首字节是**明文 `0x06`**
        ///（IDA: `LOBYTE(v14[2]) = 6`，RWCardDemo 实测一致；早期照抄的 `0x14` 是加扰残留，见 TD-154）。
        /// </summary>
        public static void BuildTimeSync(int hour, int minute, int cardNo, out byte[] p1, out byte[] p2)
        {
            BuildTimeSync(hour, minute, cardNo, 0x06, out p1, out p2);
        }

        public static void BuildTimeSync(int hour, int minute, int cardNo, int p2Head, out byte[] p1, out byte[] p2)
        {
            if (hour < 0 || hour > 23) throw new ArgumentOutOfRangeException("hour");
            if (minute < 0 || minute > 59) throw new ArgumentOutOfRangeException("minute");
            // 页1[2..3] 的位域（IDA: tem_writetimercard_sdk @ 0x7154 解出，2026-11-08 七组样本验证）：
            //   p1[2] = 16 * (分 & 0x0F) + 9
            //   p1[3] = (分 >> 4) + ((4 * 时) & 0x7C)
            // 早先按「16 × 当日分钟数 + 953」拟合的公式是错的——它只在 19:47 那组样本下近似成立。
            p1 = new byte[4];
            p1[0] = Prefix0; p1[1] = Prefix1;
            p1[2] = (byte)(16 * (minute & 0x0F) + 9);
            p1[3] = (byte)((minute >> 4) + ((4 * hour) & 0x7C));
            p2 = new byte[4];
            p2[0] = (byte)(p2Head & 0xFF);
            p2[1] = (byte)(cardNo & 0xFF);
            p2[2] = (byte)((cardNo >> 8) & 0xFF);
            p2[3] = Suffix;
        }
        /// <summary>直接按抓包字节写（校时卡等未解卡型）</summary>
        public static void BuildRaw(string p1Hex, string p2Hex, out byte[] p1, out byte[] p2)
        {
            p1 = Hex.FromHex(p1Hex, 4);
            p2 = Hex.FromHex(p2Hex, 4);
        }

        /// <summary>解析 28 字节卡数据</summary>
        public static CardInfo Parse(byte[] raw28)
        {
            var info = new CardInfo();
            info.Raw28 = raw28;
            if (raw28 == null || raw28.Length < 28) { info.Kind = CardKind.Unknown; return info; }

            for (int i = 0; i < 8; i++)
                if (raw28[i] != 0) goto notBlank;
            info.Kind = CardKind.Blank; info.IsBlank = true; return info;

        notBlank:
            byte[] p1 = { raw28[0], raw28[1], raw28[2], raw28[3] };
            byte[] p2 = { raw28[4], raw28[5], raw28[6], raw28[7] };

            if (p1[0] != Prefix0 || p1[1] != Prefix1) { info.Kind = CardKind.Unknown; return info; }

            // 已退房：页2 = 12 12 12 12
            if (p2[0] == 0x12 && p2[1] == 0x12 && p2[2] == 0x12 && p2[3] == 0x12)
            { info.Kind = CardKind.Blank; info.IsBlank = true; return info; }

            info.IsLost = (p1[2] & 0x0F) != 0;   // 页1[2] 低 4 位：1 = 挂失卡（TD-150）
            int roomIndex = ((p1[2] >> 4) & 0x0F) | (p1[3] << 4);

            if (p1[2] == 0x0A && p1[3] == 0x00) info.Kind = CardKind.Reset;
            else if (p1[2] == 0x9B && p1[3] == 0xE5) info.Kind = CardKind.Clear;
            else if (p1[2] == 0xF0 && p1[3] == 0xFF) info.Kind = CardKind.Master;
            else if (p2[0] == 0x46 && p2[3] == 0x01) { info.Kind = CardKind.Install; info.RoomIndex = roomIndex; }
            else if (p1[2] == 0x7B) { info.Kind = CardKind.CheckoutHour; info.CheckoutHour = p1[3]; }
            else if (p2[0] == 0x06 || p2[0] == 0x14) info.Kind = CardKind.TimeSync;   // 校时卡：明文 0x06，含旧卡 0x14
            else if ((p2[0] & 0xC0) == 0xC0)
            {
                // 0xC_ 段：月数编码（实测 90天→0xC3, 120→0xC4, 360→0xCC, 1095→0xE3）
                info.Kind = CardKind.GuestDay;
                info.RoomIndex = roomIndex;
                info.Days = (p2[0] & 0x3F) * 30;   // 30 天/月（TD-145 实测：锁按日历月执行）
                info.IsMonthEncoded = true;
            }
            else if ((p2[0] & 0xC0) == 0x40)
            {
                info.Kind = CardKind.GuestHour;
                info.RoomIndex = roomIndex;
                info.Hours = (p2[0] & 0x3F) - 1;
                if (info.Hours < 0) info.Hours = 0;
            }
            else if ((p2[0] & 0x80) == 0x80)
            {
                info.Kind = CardKind.GuestDay;
                info.RoomIndex = roomIndex;
                info.Days = p2[0] & 0x3F;
            }
            else info.Kind = CardKind.Unknown;

            // 挂失位只在**客卡**上有意义：校时卡 `页1[2]` 的低 4 位是 `16*(分 & 0xF) + 9`（恒为 9），
            // 管理卡（复位/清除/安装/总控/退房钟点）也没有挂失概念 —— 一律不算挂失，
            // 否则每张校时卡都会被读成「挂失卡」（2026-11-08 实测发现）。
            if (info.Kind != CardKind.GuestDay && info.Kind != CardKind.GuestHour)
            {
                info.IsLost = false;
            }

            // 页2[3] = 年(高4位) + 月(低4位)：月份 1–12 有效即认为含卡号
            //（早期拿它和固定 0xAB 比较，改成年月编码后不再适用）
            int ymMonth = p2[3] & 0x0F;
            if (ymMonth >= 1 && ymMonth <= 12)
            {
                info.CardNo = (p2[2] << 8) | p2[1];
                // 卡号 = 日(5) + 时(5) + 小时内序号(6)，解出发卡时刻（TD-150 实测）
                CardBuilder.DecodeTimestampCardNo(info.CardNo, out info.IssueDay, out info.IssueHour, out info.IssueSeq);
            }
            return info;
        }
    }

    /// <summary>房号 ↔ 序号映射（RoomIndex.ini）</summary>
    public sealed class RoomTable
    {
        readonly Dictionary<string, int> _byRoom = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);
        readonly Dictionary<int, string> _byIndex = new Dictionary<int, string>();

        public string SourcePath { get; private set; }
        public int Count { get { return _byRoom.Count; } }

        public bool Load(string path)
        {
            SourcePath = path;
            _byRoom.Clear(); _byIndex.Clear();
            if (!File.Exists(path)) return false;
            foreach (var line in File.ReadAllLines(path))
            {
                var t = line.Trim();
                if (t.Length == 0 || t[0] == ';' || t[0] == '#' || t[0] == '[') continue;
                var kv = t.Split('=');
                if (kv.Length != 2) continue;
                int idx;
                if (!int.TryParse(kv[1].Trim(), out idx)) continue;
                string room = kv[0].Trim();
                _byRoom[room] = idx;
                if (!_byIndex.ContainsKey(idx)) _byIndex[idx] = room;
            }
            return _byRoom.Count > 0;
        }

        public bool TryGetIndex(string room, out int index) { return _byRoom.TryGetValue(room, out index); }
        public bool TryGetRoom(int index, out string room) { return _byIndex.TryGetValue(index, out room); }
        public string RoomOf(int index) { string r; return _byIndex.TryGetValue(index, out r) ? r : null; }
    }


    /// <summary>运行配置（LockCard.ini）</summary>
    public sealed class LockCardOptions
    {
        public string DriverDir = @"IOServer\LC2000";   // 相对本体目录
        public int Port = 6;                 // 6 = COM7（驱动内部 port+1）
        public int Baud = 9600;
        public string Password = "12000206";
        public string RoomIndexPath = "RoomIndex.ini";

        public static LockCardOptions Load(string iniPath, string baseDir)
        {
            var o = new LockCardOptions();
            if (string.IsNullOrEmpty(baseDir)) baseDir = AppDomain.CurrentDomain.BaseDirectory;
            // 相对路径以 ini 所在目录为基准（更符合直觉）
            string refDir = baseDir;
            string fullIni = null;
            if (!string.IsNullOrEmpty(iniPath))
            {
                try
                {
                    fullIni = Path.GetFullPath(Path.IsPathRooted(iniPath) ? iniPath : Path.Combine(baseDir, iniPath));
                    string d = Path.GetDirectoryName(fullIni);
                    if (!string.IsNullOrEmpty(d)) refDir = d;
                }
                catch { fullIni = iniPath; }
            }
            if (fullIni != null && File.Exists(fullIni))
            {
                string section = "";
                foreach (var raw in File.ReadAllLines(fullIni))
                {
                    var line = raw.Trim();
                    if (line.Length == 0 || line[0] == ';' || line[0] == '#') continue;
                    if (line[0] == '[') { section = line.Trim('[', ']').ToLowerInvariant(); continue; }
                    var kv = line.Split(new[] { '=' }, 2);
                    if (kv.Length != 2) continue;
                    string k = kv[0].Trim(), v = kv[1].Trim();
                    switch (k.ToLowerInvariant())
                    {
                        case "driverdir": o.DriverDir = v; break;
                        case "port": int.TryParse(v, out o.Port); break;
                        case "baud": int.TryParse(v, out o.Baud); break;
                        case "password": o.Password = v; break;
                        case "roomindex": o.RoomIndexPath = v; break;
                    }
                }
            }
            o.DriverDir = Resolve(refDir, o.DriverDir);
            o.RoomIndexPath = Resolve(refDir, o.RoomIndexPath);
            return o;
        }

        static string Resolve(string baseDir, string p)
        {
            if (string.IsNullOrEmpty(p)) return p;
            return Path.IsPathRooted(p) ? p : Path.GetFullPath(Path.Combine(baseDir, p));
        }

        static int ParseHexOrDec(string s, int dflt)
        {
            int v;
            if (s.StartsWith("0x", StringComparison.OrdinalIgnoreCase))
                return int.TryParse(s.Substring(2), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out v) ? v : dflt;
            if (int.TryParse(s, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out v)) return v;
            return dflt;
        }
    }

    /// <summary>
    /// TEMIC 读写器。线程不安全，一个实例对应一个串口。
    /// </summary>
    public sealed class LockCardReader : IDisposable
    {
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Fn0();
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Fn2(int a, int b);
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int FnI(int a);
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int FnS(byte[] s);
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int FnWP(int a1, int page, byte[] pwd, byte[] data);
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int FnSP(int a1, int a2, int a3, int a4, byte[] pwd);

        [DllImport("kernel32", CharSet = CharSet.Ansi)] static extern IntPtr LoadLibrary(string p);
        [DllImport("kernel32", CharSet = CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr h, string n);
        [DllImport("kernel32")] static extern bool FreeLibrary(IntPtr h);

        IntPtr _h;
        Fn2 _selecom; Fn0 _shake, _close; FnI _config, _selecard, _beep;
        FnS _readAsc; FnWP _writepwm; FnSP _setuppwm;

        public string DriverPath { get; private set; }
        public string ReaderId { get; private set; }
        public bool IsOpen { get; private set; }
        public LockCardOptions Options { get; private set; }
        public RoomTable Rooms { get; private set; }

        public LockCardReader(LockCardOptions options)
        {
            Options = options;
            DriverPath = Path.Combine(options.DriverDir, "LC32RFRW.dll");
            Rooms = new RoomTable();
            Rooms.Load(options.RoomIndexPath);
        }

        public int Open()
        {
            if (_h != IntPtr.Zero) return LockResult.OK;
            _h = LoadLibrary(DriverPath);
            if (_h == IntPtr.Zero) return LockResult.ErrDriver;

            _selecom = Bind<Fn2>("mif_selecom");
            _shake = Bind<Fn0>("mif_shake");
            _config = Bind<FnI>("config_parameter");
            _selecard = Bind<FnI>("mif_selecard");
            _beep = Bind<FnI>("mif_beep");
            _readAsc = Bind<FnS>("te_read_asc");
            _writepwm = Bind<FnWP>("te_writepwm_asc");
            _setuppwm = Bind<FnSP>("te_setuppwm_asc");
            _close = Bind<Fn0>("mif_closecom");

            if (_selecom == null || _shake == null || _config == null || _selecard == null ||
                _readAsc == null || _writepwm == null || _setuppwm == null)
            {
                CloseHandle();
                return LockResult.ErrDriver;
            }

            if (_selecom(Options.Port, Options.Baud) != 0) { CloseHandle(); return LockResult.ErrPort; }
            Thread.Sleep(150);
            int rc = _shake();
            if (rc != 0) { _config(0); Thread.Sleep(80); rc = _shake(); }
            _config(0);                    // 关键：明文模式
            Thread.Sleep(60);
            _shake();
            rc = _selecard(5);             // 5 = TEMIC
            if (rc != 0) { CloseHandle(); return LockResult.ErrNoCard; }

            var id = new byte[64];
            var getId = Bind<FnS>("get_id_asc");
            if (getId != null && getId(id) == 0) ReaderId = AsciiZ(id);

            IsOpen = true;
            return LockResult.OK;
        }

        T Bind<T>(string name) where T : class
        {
            IntPtr p = GetProcAddress(_h, name);
            return p == IntPtr.Zero ? null : (T)(object)Marshal.GetDelegateForFunctionPointer(p, typeof(T));
        }

        static string AsciiZ(byte[] b)
        {
            int n = Array.IndexOf(b, (byte)0);
            if (n < 0) n = b.Length;
            return Encoding.ASCII.GetString(b, 0, n);
        }

        /// <summary>读 28 字节卡数据</summary>
        public int ReadCardBytes(out byte[] raw28)
        {
            raw28 = null;
            if (!IsOpen) return LockResult.ErrPort;
            var buf = new byte[256];
            int rc = _readAsc(buf);
            if (rc != 0) return LockResult.ErrRead;
            string s = AsciiZ(buf);
            if (s.Length < 56) return LockResult.ErrRead;
            // te_read_asc 返回 29 字节（58 个 HEX 字符）：
            //   前 28 字节是卡数据，最后 1 字节是读卡器状态（现场恒为 0x02）
            raw28 = Hex.FromHex(s.Substring(0, 56), 28);
            return LockResult.OK;
        }

        /// <summary>读卡并解析</summary>
        public int ReadCard(out CardInfo info)
        {
            info = null;
            byte[] raw;
            int rc = ReadCardBytes(out raw);
            if (rc != LockResult.OK) return rc;
            info = CardBuilder.Parse(raw);
            if (info.Kind == CardKind.Blank || info.Kind == CardKind.Unknown) return LockResult.OK;
            if (info.RoomIndex >= 0) info.Room = Rooms.RoomOf(info.RoomIndex);
            return LockResult.OK;
        }

        // ---------------- 发卡 ----------------

        public int IssueGuestDay(string room, int days, out CardInfo written)
        { return IssueGuestDay(room, days, CardNoPolicy.Increment, -1, out written); }

        public int IssueGuestDay(string room, int days, CardNoPolicy policy, int explicitCardNo, out CardInfo written)
        {
            return IssueGuestDay(room, days, policy, explicitCardNo, false, out written);
        }

        /// <summary>日租客卡；lost = 挂失卡（页1[2] 低 4 位 = 1，让该房间在用卡失效）。</summary>
        public int IssueGuestDay(string room, int days, CardNoPolicy policy, int explicitCardNo, bool lost, out CardInfo written)
        {
            int idx;
            if (!ResolveRoom(room, out idx, out written)) return LockResult.ErrParam;
            byte[] p1, p2;
            int no;
            int rc = PrepareCardNo(explicitCardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildGuestDay(idx, days, no, lost, out p1, out p2);
            return WriteAndVerify(p1, p2, idx, out written);
        }

        /// <summary>日租客卡（直接给房间序号 1–254；CLI 不再传房号）。</summary>
        public int IssueGuestDay(int roomIndex, int days, int cardNo, bool lost, out CardInfo written)
        {
            if (roomIndex < 1 || roomIndex > 254) { written = null; return LockResult.ErrParam; }
            byte[] p1, p2;
            int no;
            int rc = PrepareCardNo(cardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildGuestDay(roomIndex, days, no, lost, out p1, out p2);
            return WriteAndVerify(p1, p2, roomIndex, out written);
        }

        public int IssueGuestHour(string room, int hours, out CardInfo written)
        { return IssueGuestHour(room, hours, CardNoPolicy.Increment, -1, out written); }

        public int IssueGuestHour(string room, int hours, CardNoPolicy policy, int explicitCardNo, out CardInfo written)
        {
            return IssueGuestHour(room, hours, policy, explicitCardNo, false, out written);
        }

        /// <summary>钟点客卡；lost = 挂失卡。</summary>
        public int IssueGuestHour(string room, int hours, CardNoPolicy policy, int explicitCardNo, bool lost, out CardInfo written)
        {
            int idx;
            if (!ResolveRoom(room, out idx, out written)) return LockResult.ErrParam;
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(explicitCardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildGuestHour(idx, hours, no, lost, out p1, out p2);
            return WriteAndVerify(p1, p2, idx, out written);
        }

        /// <summary>钟点客卡（直接给房间序号 1–254；CLI 不再传房号）。</summary>
        public int IssueGuestHour(int roomIndex, int hours, int cardNo, bool lost, out CardInfo written)
        {
            if (roomIndex < 1 || roomIndex > 254) { written = null; return LockResult.ErrParam; }
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(cardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildGuestHour(roomIndex, hours, no, lost, out p1, out p2);
            return WriteAndVerify(p1, p2, roomIndex, out written);
        }

        public int IssueMaster(out CardInfo written)
        { return IssueMaster(CardNoPolicy.Increment, -1, out written); }

        public int IssueMaster(CardNoPolicy policy, int explicitCardNo, out CardInfo written)
        {
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(explicitCardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildMaster(no, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }

        public int IssueReset(out CardInfo written) { return IssueReset(CardNoPolicy.Increment, -1, out written); }
        public int IssueReset(CardNoPolicy policy, int explicitCardNo, out CardInfo written)
        {
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(explicitCardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildReset(no, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }

        public int IssueClear(out CardInfo written) { return IssueClear(CardNoPolicy.Increment, -1, out written); }
        public int IssueClear(CardNoPolicy policy, int explicitCardNo, out CardInfo written)
        {
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(explicitCardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildClear(no, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }

        public int IssueInstall(string room, out CardInfo written)
        {
            int idx;
            if (!ResolveRoom(room, out idx, out written)) return LockResult.ErrParam;
            byte[] p1, p2;
            CardBuilder.BuildInstall(idx, out p1, out p2);
            return WriteAndVerify(p1, p2, idx, out written);
        }

        public int IssueCheckoutHour(int hour, out CardInfo written)
        { return IssueCheckoutHour(hour, CardNoPolicy.Increment, -1, out written); }

        public int IssueCheckoutHour(int hour, CardNoPolicy policy, int explicitCardNo, out CardInfo written)
        {
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(explicitCardNo, out no);
            if (rc != LockResult.OK) { written = null; return rc; }
            CardBuilder.BuildCheckoutHour(hour, no, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }

        /// <summary>校时卡：时/分 + 卡号（卡号由 CLI 用「日期时间 + 序号」算好后传入）</summary>
        public int IssueTimeSync(int hour, int minute, int cardNo, out CardInfo written)
        {
            written = null;
            byte[] p1, p2; int no;
            int rc = PrepareCardNo(cardNo, out no);
            if (rc != LockResult.OK) return rc;
            CardBuilder.BuildTimeSync(hour, minute, no, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }
        /// <summary>校时卡：时间字段未解，需给出抓包得到的页1/页2</summary>
        public int IssueTimeSync(string p1Hex, string p2Hex, out CardInfo written)
        {
            written = null;
            if (string.IsNullOrEmpty(p1Hex) || string.IsNullOrEmpty(p2Hex)) return LockResult.ErrUnsupported;
            byte[] p1, p2;
            CardBuilder.BuildRaw(p1Hex, p2Hex, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }

        /// <summary>直接写任意 8 字节</summary>
        public int WriteRaw(string p1Hex, string p2Hex, out CardInfo written)
        {
            byte[] p1, p2;
            CardBuilder.BuildRaw(p1Hex, p2Hex, out p1, out p2);
            return WriteAndVerify(p1, p2, -1, out written);
        }

        // ---------------- 内部 ----------------

        bool ResolveRoom(string room, out int idx, out CardInfo written)
        {
            written = null;
            if (string.IsNullOrEmpty(room)) { idx = -1; return false; }
            int tmp;
            if (int.TryParse(room, out tmp) && !Rooms.TryGetIndex(room, out idx))
            {
                // 纯数字但表里没有：按序号直接使用
                idx = tmp;
                return true;
            }
            if (Rooms.TryGetIndex(room, out idx)) return true;
            idx = -1;
            return false;
        }

        /// <summary>
        /// 卡号由调用方传入（CLI/SDK 不再自增、不碰 `CardNo.state`）。
        /// 1–0xFFFF 之外一律 `ErrCardNo`。
        /// </summary>
        int PrepareCardNo(int explicitCardNo, out int cardNo)
        {
            cardNo = -1;
            if (explicitCardNo <= 0 || explicitCardNo > 0xFFFF) return LockResult.ErrCardNo;
            cardNo = explicitCardNo;
            return LockResult.OK;
        }

        int WriteAndVerify(byte[] p1, byte[] p2, int roomIndex, out CardInfo written)
        {
            written = null;
            if (!IsOpen) return LockResult.ErrPort;

            byte[] pwd = Hex.AsciiZ(Options.Password);

            if (_setuppwm(0, 2, 0, 1, pwd) != 0) return LockResult.ErrWrite;
            Thread.Sleep(60);
            if (_writepwm(0, 1, pwd, Hex.AsciiZ(Hex.ToHex(p1, 4))) != 0) return LockResult.ErrWrite;
            Thread.Sleep(80);
            if (_writepwm(0, 2, pwd, Hex.AsciiZ(Hex.ToHex(p2, 4))) != 0) return LockResult.ErrWrite;
            Thread.Sleep(150);

            // 回读校验（最多 3 次，写完立刻读可能拿到旧值）
            for (int attempt = 0; attempt < 3; attempt++)
            {
                byte[] raw;
                if (ReadCardBytes(out raw) == LockResult.OK &&
                    raw[0] == p1[0] && raw[1] == p1[1] && raw[2] == p1[2] && raw[3] == p1[3] &&
                    raw[4] == p2[0] && raw[5] == p2[1] && raw[6] == p2[2] && raw[7] == p2[3])
                {
                    if (_beep != null) _beep(2);
                    written = CardBuilder.Parse(raw);
                    written.RoomIndex = roomIndex >= 0 ? roomIndex : written.RoomIndex;
                    written.Room = written.RoomIndex >= 0 ? Rooms.RoomOf(written.RoomIndex) : null;
                    return LockResult.OK;
                }
                Thread.Sleep(200);
            }
            return LockResult.ErrVerify;
        }

        public void Close()
        {
            if (_close != null) { try { _close(); } catch { } }
            CloseHandle();
        }

        void CloseHandle()
        {
            IsOpen = false;
            if (_h != IntPtr.Zero) { FreeLibrary(_h); _h = IntPtr.Zero; }
            _selecom = null; _shake = null; _config = null; _selecard = null;
            _readAsc = null; _writepwm = null; _setuppwm = null; _close = null; _beep = null;
        }

        public void Dispose() { Close(); }
    }
}




