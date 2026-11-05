// =====================================================================
//  Demo.cs — .NET 调用 PMSLockSDK 的示例
//
//  编译：
//    csc /target:exe /platform:x86 /out:Demo.exe /r:..\PMSLockSDK.dll Demo.cs
//  运行：
//    Demo.exe                 只读卡
//    Demo.exe 209 1           给 209 发 1 天卡
//    Demo.exe 220 h3          给 220 发 3 小时卡
// =====================================================================
using System;
using System.IO;
using PmsLockSdk;

static class Demo
{
    static int Main(string[] args)
    {
        string baseDir = AppDomain.CurrentDomain.BaseDirectory;
        string ini = Path.Combine(baseDir, "..\\LockCard.ini");
        var opt = LockCardOptions.Load(ini, baseDir);

        Console.WriteLine("驱动目录: " + opt.DriverDir);
        Console.WriteLine("串口索引: " + opt.Port + " (COM" + (opt.Port + 1) + ")");
        Console.WriteLine("房号表  : " + opt.RoomIndexPath);

        using (var r = new LockCardReader(opt))
        {
            Console.WriteLine("房号数量: " + r.Rooms.Count);
            int rc = r.Open();
            if (rc != 0) { Console.WriteLine("打开失败: " + LockResult.Describe(rc)); return 2; }
            Console.WriteLine("读卡器  : " + r.ReaderId);

            CardInfo info;
            rc = r.ReadCard(out info);
            if (rc != 0) { Console.WriteLine("读卡失败: " + LockResult.Describe(rc)); return 3; }
            Console.WriteLine("当前卡  : " + info.Describe());
            Console.WriteLine("原始数据: " + info.RawHex);

            if (args.Length >= 2)
            {
                string room = args[0];
                string what = args[1].ToLowerInvariant();
                CardInfo w;
                if (what.StartsWith("h"))
                {
                    int hours = int.Parse(what.Substring(1));
                    rc = r.IssueGuestHour(room, hours, out w);
                    Console.WriteLine("发钟点卡(" + hours + "小时) -> " + LockResult.Describe(rc));
                }
                else
                {
                    int days = int.Parse(what);
                    rc = r.IssueGuestDay(room, days, out w);
                    Console.WriteLine("发日租卡(" + days + "天) -> " + LockResult.Describe(rc));
                }
                if (w != null) Console.WriteLine("写入卡  : " + w.Describe() + "  " + w.RawHex);
                if (rc != 0) return 4;
            }
        }
        return 0;
    }
}
