// =====================================================================
//  PMSLockSDK.Com.cs — COM 可见封装（供 Delphi / C++ / VB6 / VBS 调用）
//
//  编译：
//    csc /target:library /platform:x86 /out:PMSLockSDK.Com.dll /r:PMSLockSDK.dll PMSLockSDK.Com.cs
//  注册（管理员）：
//    C:\Windows\Microsoft.NET\Framework\v4.0.30319\regasm.exe PMSLockSDK.Com.dll /tlb /codebase
//
//  VBS 示例：
//    Set r = CreateObject("PmsLockSdk.Reader")
//    r.Open "D:\...\IOServer\LC2000", 6
//    WScript.Echo r.Read()
//    WScript.Echo r.IssueGuest("220", 1)
//    r.Close
// =====================================================================
using System;
using System.IO;
using System.Runtime.InteropServices;
using PmsLockSdk;

namespace PmsLockSdkCom
{
    [ComVisible(true)]
    [Guid("8F3B2C41-7A19-4C2E-9D51-2B6E0A4C7F31")]
    [ProgId("PmsLockSdk.Reader")]
    [ClassInterface(ClassInterfaceType.AutoDual)]
    public class Reader
    {
        LockCardReader _r;

        public Reader() { }

        /// <summary>最近一次结果描述</summary>
        public string LastMessage { get; private set; }
        /// <summary>最近一次写入的卡号（十六进制字符串，无则空）</summary>
        public string LastCardNo { get; private set; }
        /// <summary>读卡器 ID</summary>
        public string ReaderId { get { return _r == null ? null : _r.ReaderId; } }
        public bool IsOpen { get { return _r != null && _r.IsOpen; } }

        /// <summary>打开读卡器。driverDir 为 LC32RFRW.dll 所在目录；portIndex 0=COM1</summary>
        public int Open(string driverDir, int portIndex)
        {
            Close();
            var opt = new LockCardOptions();
            if (!string.IsNullOrEmpty(driverDir)) opt.DriverDir = driverDir;
            opt.Port = portIndex;
            string baseDir = AppDomain.CurrentDomain.BaseDirectory;
            string ini = Path.Combine(baseDir, "LockCard.ini");
            if (File.Exists(ini))
            {
                var loaded = LockCardOptions.Load(ini, baseDir);
                if (string.IsNullOrEmpty(driverDir)) loaded.DriverDir = opt.DriverDir;
                loaded.Port = portIndex;
                opt = loaded;
            }
            _r = new LockCardReader(opt);
            int rc = _r.Open();
            LastMessage = LockResult.Describe(rc);
            return rc;
        }

        /// <summary>读卡，返回描述字符串；失败返回 "ERR:&lt;code&gt;:描述"</summary>
        public string Read()
        {
            if (_r == null) return "ERR:3:未打开";
            CardInfo info;
            int rc = _r.ReadCard(out info);
            LastMessage = LockResult.Describe(rc);
            if (rc != 0) return "ERR:" + rc + ":" + LastMessage;
            if (info.CardNo >= 0) LastCardNo = "0x" + info.CardNo.ToString("X4");
            return info.Describe() + "|" + info.RawHex;
        }

        public int IssueGuest(string room, int days) { return Do(delegate (out CardInfo c) { return _r.IssueGuestDay(room, days, out c); }); }
        public int IssueHour(string room, int hours) { return Do(delegate (out CardInfo c) { return _r.IssueGuestHour(room, hours, out c); }); }
        public int IssueMaster() { return Do(delegate (out CardInfo c) { return _r.IssueMaster(out c); }); }
        public int IssueReset() { return Do(delegate (out CardInfo c) { return _r.IssueReset(out c); }); }
        public int IssueClear() { return Do(delegate (out CardInfo c) { return _r.IssueClear(out c); }); }
        public int IssueInstall(string room) { return Do(delegate (out CardInfo c) { return _r.IssueInstall(room, out c); }); }
        public int IssueCheckoutHour(int hour) { return Do(delegate (out CardInfo c) { return _r.IssueCheckoutHour(hour, out c); }); }
        public int WriteRaw(string page1Hex, string page2Hex) { return Do(delegate (out CardInfo c) { return _r.WriteRaw(page1Hex, page2Hex, out c); }); }

        delegate int Op(out CardInfo c);

        int Do(Op op)
        {
            if (_r == null) { LastMessage = "未打开"; return LockResult.ErrPort; }
            CardInfo info;
            int rc = op(out info);
            LastMessage = LockResult.Describe(rc) + (info != null ? "（" + info.Describe() + "）" : "");
            if (info != null && info.CardNo >= 0) LastCardNo = "0x" + info.CardNo.ToString("X4");
            return rc;
        }

        public void Close()
        {
            if (_r != null) { _r.Dispose(); _r = null; }
        }
    }
}
