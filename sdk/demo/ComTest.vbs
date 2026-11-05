' ComTest.vbs — 通过 COM 调用 PMSLockSDK 的示例（无需 .NET 编程）
'
' 前提：
'   regasm PMSLockSDK.Com.dll /tlb /codebase   （管理员，32 位 regasm）
'   32 位宿主（cscript //x86 或 32 位程序）
'
' 用法： cscript //nologo ComTest.vbs [房号] [天数]

Option Explicit

Dim room, days
If WScript.Arguments.Count >= 1 Then room = WScript.Arguments(0) Else room = ""
If WScript.Arguments.Count >= 2 Then days = CLng(WScript.Arguments(1)) Else days = 0

Dim r, rc
Set r = CreateObject("PmsLockSdk.Reader")

rc = r.Open("D:\Program Files\智能门锁管理软件\IOServer\LC2000", 6)
WScript.Echo "Open -> " & rc & "  " & r.LastMessage
If rc <> 0 Then
    WScript.Echo "打开失败，检查：驱动目录 / 串口 / 是否被门锁软件占用"
    WScript.Quit 2
End If

WScript.Echo "读卡器: " & r.ReaderId
WScript.Echo "读卡  : " & r.Read()

If room <> "" And days > 0 Then
    rc = r.IssueGuest(room, days)
    WScript.Echo "发卡(" & room & " / " & days & "天) -> " & rc & "  " & r.LastMessage
    WScript.Echo "卡号  : " & r.LastCardNo
End If

r.Close
Set r = Nothing
