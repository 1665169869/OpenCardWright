VERSION 5.00
Begin VB.Form Form1 
   BorderStyle     =   3  'Fixed Dialog
   Caption         =   "感应门锁接口函数DemoV4.2_D"
   ClientHeight    =   9570
   ClientLeft      =   45
   ClientTop       =   435
   ClientWidth     =   12255
   LinkTopic       =   "Form1"
   MaxButton       =   0   'False
   MinButton       =   0   'False
   ScaleHeight     =   9570
   ScaleWidth      =   12255
   ShowInTaskbar   =   0   'False
   StartUpPosition =   2  'CenterScreen
   Begin VB.Frame Frame3 
      Caption         =   "读写时钟信息"
      Height          =   4095
      Left            =   7950
      TabIndex        =   47
      Top             =   210
      Width           =   3975
      Begin VB.TextBox txtTime 
         Height          =   405
         Left            =   1320
         TabIndex        =   53
         Top             =   930
         Width           =   2205
      End
      Begin VB.TextBox txtCardID 
         Height          =   405
         Left            =   1320
         TabIndex        =   52
         Top             =   450
         Width           =   2205
      End
      Begin VB.TextBox txtXh 
         Height          =   345
         Left            =   1320
         TabIndex        =   51
         Top             =   1410
         Width           =   1455
      End
      Begin VB.TextBox Text12 
         ForeColor       =   &H000000FF&
         Height          =   1095
         Left            =   150
         Locked          =   -1  'True
         MultiLine       =   -1  'True
         ScrollBars      =   2  'Vertical
         TabIndex        =   50
         Text            =   "Form1.frx":0000
         Top             =   2010
         Width           =   3975
      End
      Begin VB.CommandButton Command8 
         Caption         =   "读时钟"
         Height          =   585
         Left            =   960
         TabIndex        =   49
         Top             =   3240
         Width           =   1155
      End
      Begin VB.CommandButton Command9 
         Caption         =   "写时钟"
         Height          =   585
         Left            =   2280
         TabIndex        =   48
         Top             =   3240
         Width           =   1275
      End
      Begin VB.Label Label13 
         AutoSize        =   -1  'True
         Caption         =   "时钟:"
         Height          =   195
         Left            =   600
         TabIndex        =   56
         Top             =   1050
         Width           =   405
      End
      Begin VB.Label Label14 
         AutoSize        =   -1  'True
         Caption         =   "作业流水号:"
         Height          =   195
         Left            =   150
         TabIndex        =   55
         Top             =   540
         Width           =   945
      End
      Begin VB.Label Label15 
         Caption         =   "发卡序号"
         Height          =   225
         Left            =   330
         TabIndex        =   54
         Top             =   1500
         Width           =   735
      End
   End
   Begin VB.Frame Frame4 
      Caption         =   "读ID卡卡号"
      Height          =   3075
      Left            =   7950
      TabIndex        =   44
      Top             =   4680
      Width           =   3975
      Begin VB.TextBox txtIDCardNo 
         Height          =   375
         Left            =   570
         TabIndex        =   46
         Top             =   720
         Width           =   2895
      End
      Begin VB.CommandButton Command10 
         Caption         =   "读ID卡"
         Height          =   585
         Left            =   1590
         TabIndex        =   45
         Top             =   1380
         Width           =   1155
      End
   End
   Begin VB.TextBox txtLockAP 
      Height          =   375
      Left            =   1320
      TabIndex        =   36
      Text            =   "0"
      Top             =   810
      Width           =   855
   End
   Begin VB.Frame Frame2 
      Caption         =   "已保存的门锁信息"
      Height          =   3345
      Left            =   3960
      TabIndex        =   25
      Top             =   210
      Width           =   3855
      Begin VB.CommandButton Command7 
         Caption         =   "打开文件"
         Height          =   435
         Left            =   2730
         TabIndex        =   31
         Top             =   2610
         Width           =   975
      End
      Begin VB.ListBox List1 
         Height          =   1815
         Left            =   210
         TabIndex        =   30
         Top             =   600
         Width           =   3495
      End
      Begin VB.CommandButton Command6 
         Caption         =   "删除当前"
         Height          =   435
         Left            =   1590
         TabIndex        =   29
         Top             =   2610
         Width           =   975
      End
      Begin VB.CommandButton Command5 
         Caption         =   "全部清空"
         Height          =   435
         Left            =   300
         TabIndex        =   28
         Top             =   2610
         Width           =   1155
      End
      Begin VB.Label Label10 
         AutoSize        =   -1  'True
         Caption         =   "房号:"
         Height          =   195
         Left            =   1440
         TabIndex        =   27
         Top             =   360
         Width           =   405
      End
      Begin VB.Label Label9 
         AutoSize        =   -1  'True
         Caption         =   "编号:"
         Height          =   195
         Left            =   210
         TabIndex        =   26
         Top             =   360
         Width           =   405
      End
   End
   Begin VB.CommandButton Command4 
      Caption         =   "保存编号及房号"
      Height          =   435
      Left            =   4170
      TabIndex        =   24
      Top             =   8940
      Width           =   1635
   End
   Begin VB.TextBox Text10 
      Height          =   435
      Left            =   1560
      TabIndex        =   23
      Top             =   4980
      Width           =   2115
   End
   Begin VB.Frame 写卡数据 
      Caption         =   "写卡数据"
      Height          =   3945
      Left            =   120
      TabIndex        =   11
      Top             =   5520
      Width           =   3615
      Begin VB.TextBox txtCheckOutHour2 
         Height          =   435
         Left            =   1320
         TabIndex        =   60
         Text            =   "12"
         Top             =   3000
         Width           =   2115
      End
      Begin VB.TextBox Text6 
         Height          =   435
         Left            =   1350
         TabIndex        =   59
         Text            =   "1"
         Top             =   2370
         Width           =   2115
      End
      Begin VB.CheckBox chkOpenInner2 
         Caption         =   "是否开启反锁"
         Height          =   225
         Left            =   1800
         TabIndex        =   57
         Top             =   3600
         Width           =   1815
      End
      Begin VB.TextBox Text4 
         Height          =   435
         Left            =   1350
         TabIndex        =   16
         Text            =   "1"
         Top             =   1320
         Width           =   2115
      End
      Begin VB.TextBox Text5 
         Height          =   435
         Left            =   1350
         TabIndex        =   15
         Text            =   "1"
         Top             =   1845
         Width           =   2115
      End
      Begin VB.CheckBox Check1 
         Caption         =   "是否挂失?"
         Height          =   225
         Left            =   240
         TabIndex        =   14
         Top             =   3600
         Value           =   1  'Checked
         Width           =   1425
      End
      Begin VB.TextBox Text8 
         Height          =   435
         Left            =   1350
         TabIndex        =   13
         Top             =   795
         Width           =   2115
      End
      Begin VB.TextBox Text9 
         Height          =   435
         Left            =   1350
         TabIndex        =   12
         Top             =   270
         Width           =   2115
      End
      Begin VB.Label lblCheckOutHour2 
         Caption         =   "退房钟点:"
         Height          =   255
         Left            =   240
         TabIndex        =   58
         Top             =   3120
         Width           =   1095
      End
      Begin VB.Label Label2 
         AutoSize        =   -1  'True
         Caption         =   "一小时内序号:"
         Height          =   195
         Left            =   30
         TabIndex        =   21
         Top             =   1334
         Width           =   1125
      End
      Begin VB.Label Label4 
         AutoSize        =   -1  'True
         Caption         =   "时间单位:"
         Height          =   195
         Left            =   390
         TabIndex        =   20
         Top             =   1851
         Width           =   765
      End
      Begin VB.Label Label5 
         AutoSize        =   -1  'True
         Caption         =   "时间长度:"
         Height          =   195
         Left            =   390
         TabIndex        =   19
         Top             =   2370
         Width           =   765
      End
      Begin VB.Label Label6 
         AutoSize        =   -1  'True
         Caption         =   "起始时间:"
         Height          =   195
         Left            =   390
         TabIndex        =   18
         Top             =   817
         Width           =   765
      End
      Begin VB.Label Label7 
         AutoSize        =   -1  'True
         Caption         =   "门锁编号:"
         Height          =   195
         Left            =   360
         TabIndex        =   17
         Top             =   300
         Width           =   765
      End
   End
   Begin VB.TextBox Text7 
      ForeColor       =   &H000000FF&
      Height          =   4365
      Left            =   3930
      Locked          =   -1  'True
      MultiLine       =   -1  'True
      ScrollBars      =   2  'Vertical
      TabIndex        =   5
      Text            =   "Form1.frx":002E
      Top             =   3960
      Width           =   3975
   End
   Begin VB.CommandButton Command3 
      Caption         =   "关闭"
      Height          =   435
      Left            =   5880
      TabIndex        =   4
      Top             =   8970
      Width           =   1635
   End
   Begin VB.CommandButton Command2 
      Caption         =   "写卡"
      Height          =   435
      Left            =   5880
      TabIndex        =   3
      Top             =   8400
      Width           =   1635
   End
   Begin VB.CommandButton Command1 
      Caption         =   "读卡"
      Height          =   435
      Left            =   4170
      TabIndex        =   2
      Top             =   8400
      Width           =   1635
   End
   Begin VB.TextBox Text1 
      Height          =   375
      Left            =   1320
      TabIndex        =   1
      Text            =   "0"
      Top             =   210
      Width           =   855
   End
   Begin VB.Frame Frame1 
      Caption         =   "读卡数据"
      Height          =   3495
      Left            =   180
      TabIndex        =   6
      Top             =   1350
      Width           =   3615
      Begin VB.CheckBox chkOpenInner1 
         Caption         =   "是否开启反锁"
         Height          =   195
         Left            =   1440
         TabIndex        =   63
         Top             =   3240
         Width           =   1935
      End
      Begin VB.TextBox txtCheckOutHour1 
         Height          =   435
         Left            =   1380
         TabIndex        =   62
         Top             =   2640
         Width           =   2115
      End
      Begin VB.TextBox txtEDT 
         Height          =   435
         Left            =   1380
         Locked          =   -1  'True
         TabIndex        =   42
         Top             =   2160
         Width           =   2115
      End
      Begin VB.TextBox txtSDT 
         Height          =   435
         Left            =   1380
         Locked          =   -1  'True
         TabIndex        =   40
         Top             =   1680
         Width           =   2115
      End
      Begin VB.TextBox Text11 
         Height          =   435
         Left            =   1380
         Locked          =   -1  'True
         TabIndex        =   32
         Top             =   1200
         Width           =   2115
      End
      Begin VB.TextBox Text2 
         Height          =   435
         Left            =   1380
         Locked          =   -1  'True
         TabIndex        =   8
         Top             =   240
         Width           =   2115
      End
      Begin VB.TextBox Text3 
         Height          =   435
         Left            =   1380
         Locked          =   -1  'True
         TabIndex        =   7
         Top             =   720
         Width           =   2115
      End
      Begin VB.Label lblCheckOutHour1 
         Caption         =   "退房钟点:"
         Height          =   255
         Left            =   360
         TabIndex        =   61
         Top             =   2760
         Width           =   975
      End
      Begin VB.Label Label21 
         AutoSize        =   -1  'True
         Caption         =   "结束时间:"
         Height          =   195
         Left            =   390
         TabIndex        =   43
         Top             =   2250
         Width           =   765
      End
      Begin VB.Label Label20 
         AutoSize        =   -1  'True
         Caption         =   "起始时间:"
         Height          =   195
         Left            =   390
         TabIndex        =   41
         Top             =   1755
         Width           =   765
      End
      Begin VB.Label Label11 
         AutoSize        =   -1  'True
         Caption         =   "门锁编号2:"
         Height          =   195
         Left            =   300
         TabIndex        =   33
         Top             =   1260
         Width           =   855
      End
      Begin VB.Label aaa 
         AutoSize        =   -1  'True
         Caption         =   "门锁编号1:"
         Height          =   195
         Left            =   300
         TabIndex        =   10
         Top             =   270
         Width           =   855
      End
      Begin VB.Label Label3 
         AutoSize        =   -1  'True
         Caption         =   "作业流水号:"
         Height          =   195
         Left            =   210
         TabIndex        =   9
         Top             =   765
         Width           =   945
      End
   End
   Begin VB.Label Label19 
      AutoSize        =   -1  'True
      Caption         =   "注:一般为0,具备二级销售商模式才生效"
      Height          =   495
      Left            =   2250
      TabIndex        =   39
      Top             =   780
      Width           =   1665
      WordWrap        =   -1  'True
   End
   Begin VB.Label Label18 
      AutoSize        =   -1  'True
      Caption         =   "注:0=串口1,1=串口2,2=串口3,....."
      Height          =   390
      Left            =   2250
      TabIndex        =   38
      Top             =   300
      Width           =   1575
      WordWrap        =   -1  'True
   End
   Begin VB.Label Label17 
      AutoSize        =   -1  'True
      Caption         =   "系统标识号(LockAP)："
      Height          =   390
      Left            =   180
      TabIndex        =   37
      Top             =   780
      Width           =   1020
      WordWrap        =   -1  'True
   End
   Begin VB.Label Label16 
      Caption         =   "Label16"
      Height          =   30
      Left            =   5370
      TabIndex        =   35
      Top             =   4740
      Width           =   30
   End
   Begin VB.Label Label12 
      AutoSize        =   -1  'True
      Caption         =   "注意："
      ForeColor       =   &H000000FF&
      Height          =   195
      Left            =   3960
      TabIndex        =   34
      Top             =   3750
      Width           =   540
   End
   Begin VB.Label Label8 
      AutoSize        =   -1  'True
      Caption         =   "输入房号名称："
      Height          =   195
      Left            =   240
      TabIndex        =   22
      Top             =   5100
      Width           =   1260
   End
   Begin VB.Label Label1 
      AutoSize        =   -1  'True
      Caption         =   "串口号："
      Height          =   195
      Left            =   450
      TabIndex        =   0
      Top             =   270
      Width           =   720
   End
End
Attribute VB_Name = "Form1"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit
Dim nCom As Long
Dim MsTable(2 * 1024) As String
Dim nCount As Long
Dim pubBh As String
Private Declare Function CopyFile Lib "kernel32" Alias "CopyFileA" (ByVal lpExistingFileName As String, ByVal lpNewFileName As String, ByVal bFailIfExists As Long) As Long


Private Sub Command1_Click()
'On Error Resume Next
Dim nerr As Long
Dim bh1bit As String
Dim bh2bit As String
Dim bh3bit As String
Dim bh4bit As String

Dim strBh As String


nCom = Val(Text1.Text)
If nCom < 0 Or nCom > 255 Then
   Text1.Text = 0
   nCom = 0
End If


Call mif_closecom

'打开串口
nerr = mif_selecom(nCom, 9600)

Dim buf1 As String * 20
Dim buf2 As String * 20
Dim l_nLockAP As Long

Dim l_csSDT As String * 8
Dim l_csUnit As String * 8
Dim l_csTimes As String * 8
Dim l_csCheckOutHour As String * 8
Dim l_csOpenInner As String * 8


If nerr <> 0 Then
    '关闭串口
   Call mif_closecom
   MsgBox "初始化串口错误！", vbOKOnly + vbInformation
   Exit Sub
End If

l_nLockAP = Int(Val(txtLockAP.Text))

If l_nLockAP > 255 Then
    l_nLockAP = 0
End If

'开始读数据
nerr = tem_readdoorcard_sdk(buf1, buf2, l_nLockAP)
If nerr <> 0 Then
   '关闭串口
   Call mif_closecom
   MsgBox "读卡错误！(" & Str(nerr) & ")", vbOKOnly + vbInformation
   Exit Sub
End If

'开始读卡起始结束时间
nerr = tem_readdatetime13_sdk(l_csSDT, l_csUnit, l_csTimes, l_csCheckOutHour, l_csOpenInner, l_nLockAP)
If nerr <> 0 Then
   '关闭串口
   Call mif_closecom
   MsgBox "读卡错误！(" & Str(nerr) & ")", vbOKOnly + vbInformation
   Exit Sub
End If


strBh = Mid(buf1, 1, 10)
Text2.Text = strBh
Text11.Text = strBh
Text9.Text = strBh
Text3.Text = buf2
txtCheckOutHour1.Text = l_csCheckOutHour
l_csOpenInner = Left(l_csOpenInner, 1)
chkOpenInner1.Value = Val(l_csOpenInner)


Dim l_csCOTime As String

Dim l_csSDT_All As String
Dim l_csEDT_All As String

l_csCOTime = "12"

l_csSDT_All = "20" & Mid(l_csSDT, 1, 2) & "-" & Mid(l_csSDT, 3, 2) & "-" & Mid(l_csSDT, 5, 2) & " " & Mid(l_csSDT, 7, 2)
l_csEDT_All = "20" & Mid(l_csSDT, 1, 2) & "-" & Mid(l_csSDT, 3, 2) & "-" & Mid(l_csSDT, 5, 2) & " " & l_csCOTime

l_csSDT_All = l_csSDT_All + ":00:00"
l_csEDT_All = l_csEDT_All + ":00:00"

Select Case Val(l_csUnit)
    
    Case 0      '以时为单位
        l_csEDT_All = CDate(l_csSDT_All) + Val(l_csTimes) / 24
    Case 1      '以日为单位
        l_csEDT_All = CDate(l_csEDT_All) + Val(l_csTimes)
    Case 2      '以月为单位
        l_csEDT_All = CDate(l_csEDT_All) + Val(l_csTimes) * 30
    Case 3      '以年为单位
        l_csEDT_All = CDate(l_csEDT_All) + Val(l_csTimes) * 365
    Case Else   '无效的时间单位
        l_csEDT_All = "无效"
    
End Select

txtSDT.Text = l_csSDT_All

txtEDT.Text = l_csEDT_All

pubBh = Text2.Text

'关闭串口
Call mif_closecom
MsgBox "读卡成功！", vbOKOnly + vbInformation

End Sub

Private Sub Command10_Click()
    
    On Error Resume Next
    Dim nerr As Long
    
    nCom = Val(Text1.Text)
    If nCom < 0 Or nCom > 255 Then
        Text1.Text = 0
       nCom = 0
    End If
    
    Call mif_closecom
    
    '打开串口
    nerr = mif_selecom(nCom, 9600)
    
    Dim buf1 As String * 10
   
    '开始读ID卡卡号
    nerr = tem_readidcard_sdk(buf1$)
    If nerr <> 0 Then
        '关闭串口
        txtIDCardNo.Text = ""
       Call mif_closecom
       MsgBox "读卡错误！(" & Str(nerr) & ")", vbOKOnly + vbInformation
       Exit Sub
    End If
    
    txtIDCardNo.Text = buf1
    
    '关闭串口
    Call mif_closecom
    MsgBox "读卡成功！", vbOKOnly + vbInformation

End Sub

Private Sub Command2_Click()
On Error Resume Next
Dim nerr As Long

nCom = Val(Text1.Text)
If nCom < 0 Or nCom > 255 Then
    Text1.Text = 0
   nCom = 0
End If

Call mif_closecom

'打开串口
nerr = mif_selecom(nCom, 9600)

Dim buf1 As String * 4
Dim buf2 As String * 10
If nerr <> 0 Then
    '关闭串口
   Call mif_closecom
   MsgBox "初始化串口错误！", vbOKOnly + vbInformation
   Exit Sub
End If

Dim bh1bit As String
Dim bh2bit As String
Dim bh3bit As String
Dim bh4bit As String
Dim l_nLockAP As Long
Dim strBh As String
Dim bOpenInner As Boolean
Dim nCheckOutHour As Integer
nCheckOutHour = Val(txtCheckOutHour2.Text)
If nCheckOutHour < 11 Or nCheckOutHour > 18 Then
    MsgBox "退房钟点只能在11点到18点之间", vbOKOnly + vbInformation
    Exit Sub
End If

strBh = Trim(Text9.Text)

If Len(strBh) <> 10 Then
    Call mif_closecom
   MsgBox "门锁编号的长度不足10位！", vbOKOnly + vbInformation
   Exit Sub
End If

bh1bit = ChrB(Val("&H" & Mid(strBh, 1, 2)))
bh2bit = ChrB(Val("&H" & Mid(strBh, 3, 2)))
bh3bit = ChrB(Val("&H" & Mid(strBh, 5, 2)))
bh4bit = ChrB(Val("&H" & Mid(strBh, 7, 2)))

strBh = Text9.Text

buf1 = Text9.Text
buf2 = Text8.Text

l_nLockAP = Val(txtLockAP.Text)

If l_nLockAP > 255 Then
    l_nLockAP = 0
End If

bOpenInner = chkOpenInner2.Value


'开始写数据
nerr = tem_writedoorcard13_sdk(strBh$, buf2$, Val(Text4.Text), Val(Text5.Text), Val(Text6.Text), Check1.Value, l_nLockAP, nCheckOutHour, bOpenInner)
If nerr <> 0 Then
    '关闭串口
   Call mif_closecom
   MsgBox "写卡错误！(" & Str(nerr) & ")", vbOKOnly + vbInformation
   Exit Sub
End If

'关闭串口
Call mif_closecom
MsgBox "写卡成功！", vbOKOnly + vbInformation
End Sub

Private Sub Command3_Click()
    End
End Sub

Private Sub Command4_Click()
    If Text2.Text = "" Or Text10.Text = "" Then
        MsgBox "无门锁编号或房号名称!", vbOKOnly + vbInformation
        Exit Sub
    End If
    Dim curStr As String
    Dim strCount As String
    Dim i As Long
    Dim curBh As String * 10
    
    curBh = Text11.Text
    
    curStr = curBh & vbTab & Text10.Text
    
    For i = 0 To nCount - 1
        If MsTable(i) = curStr Then
           Exit Sub
        End If
    Next
    
    nCount = nCount + 1
    
    strCount = Str(nCount)
    
    MsTable(nCount - 1) = curStr
    
    Open App.Path & "\BhFh.txt" For Output As #1
    Print #1, strCount
    For i = 0 To nCount - 1
        Print #1, MsTable(i)
    Next
    
    Close #1
    
    Call OpenMsTabeFile
    
'    MsgBox "保存完成！", vbOKOnly + vbInformation

End Sub

Private Sub Command5_Click()
    If MsgBox("确实全部清除吗？", vbYesNo + vbQuestion) = vbNo Then Exit Sub
    CopyFile App.Path & "\bhfh.txt", App.Path & "\bhfh" + Format(Now, "mmddHHMMSS") + ".txt", False
    
    Open App.Path & "\BhFh.txt" For Output As #1
    Print #1, 0
    Close #1
    Call OpenMsTabeFile
End Sub

Private Sub Command6_Click()
    Dim curStr As String
    Dim strCount As String
    Dim i As Long
    
    If List1.ListIndex = -1 Then Exit Sub
    
    If MsgBox("确实清除当前内容吗？", vbYesNo + vbQuestion) = vbNo Then Exit Sub
    
    List1.RemoveItem List1.ListIndex
    
    For i = 0 To List1.ListCount - 1
       MsTable(i) = List1.List(i)
    Next
    
    
    nCount = List1.ListCount
    
    strCount = Str(nCount)
    
    
    Open App.Path & "\BhFh.txt" For Output As #1
    Print #1, strCount
    For i = 0 To nCount - 1
        Print #1, MsTable(i)
    Next
    
    Close #1
    
    Call OpenMsTabeFile
End Sub

Private Sub Command7_Click()
    Dim id As Long
    id = Shell("notepad.exe " + App.Path + "\BhFh.txt", vbNormalFocus)
End Sub

Private Sub Command8_Click()

    'On Error Resume Next
    Dim nerr As Long
    Dim bh1bit As String
    Dim bh2bit As String
    Dim bh3bit As String
    Dim bh4bit As String
    
    Dim strFksj As String
    
    Dim strTime As String
    
    Dim buf1 As String * 10
    Dim buf2 As String * 10
    
    Dim l_nLockAP As Long
    
    nCom = Val(Text1.Text)
    If nCom < 0 Or nCom > 255 Then
       Text1.Text = 0
       nCom = 0
    End If
    
    Call mif_closecom
    
    '打开串口
    nerr = mif_selecom(nCom, 9600)

    If nerr <> 0 Then
        '关闭串口
       Call mif_closecom
       MsgBox "初始化串口错误！", vbOKOnly + vbInformation
       Exit Sub
    End If
    
    l_nLockAP = Int(Val(txtLockAP.Text))

    If l_nLockAP > 255 Then
        l_nLockAP = 0
    End If

    '开始读数据
    nerr = tem_readtimercard_sdk(buf1, buf2, l_nLockAP)
    If nerr <> 0 Then
        '关闭串口
       Call mif_closecom
       MsgBox "读卡错误！(" & Str(nerr) & ")", vbOKOnly + vbInformation
       Exit Sub
    End If
    
    strFksj = Mid(buf1, 1, 10)
    strTime = Mid(buf2, 1, 10)
    
    txtCardID.Text = strFksj
    txtTime.Text = strTime
    
    txtXh.Text = Val("&H" + Right(strFksj, 2))
   
    '关闭串口
    Call mif_closecom
    MsgBox "读卡成功！", vbOKOnly + vbInformation

End Sub

Private Sub Command9_Click()
'On Error Resume Next
    Dim nerr As Long
    Dim bh1bit As String
    Dim bh2bit As String
    Dim bh3bit As String
    Dim bh4bit As String
    
    Dim nxh As Long
    
    Dim strFksj As String
    
    Dim strTime As String
    
    Dim l_nLockAP As Long
    
    
    nCom = Val(Text1.Text)
    If nCom < 0 Or nCom > 255 Then
       Text1.Text = 0
       nCom = 0
    End If
    
    
    Call mif_closecom
    
    '打开串口
    nerr = mif_selecom(nCom, 9600)
    If nerr <> 0 Then
        '关闭串口
       Call mif_closecom
       MsgBox "初始化串口错误！", vbOKOnly + vbInformation
       Exit Sub
    End If
    
    Dim buf1 As String
    Dim buf2 As String
    
    buf1 = Right(String(8, "0") + Left(Trim(txtCardID.Text), 8), 8)
    buf2 = Right(String(10, "0") + Trim(txtTime.Text), 10)
    nxh = Val(txtXh.Text)
  
    l_nLockAP = Int(Val(txtLockAP.Text))

    If l_nLockAP > 255 Then
        l_nLockAP = 0
    End If
    
    '开始读数据
    nerr = tem_writetimercard_sdk(buf1, nxh, buf2, l_nLockAP)
    If nerr <> 0 Then
        '关闭串口
       Call mif_closecom
       MsgBox "写卡错误！(" & Str(nerr) & ")", vbOKOnly + vbInformation
       Exit Sub
    End If
    
    '关闭串口
    Call mif_closecom
    MsgBox "写卡成功！", vbOKOnly + vbInformation
End Sub

Private Sub Form_Load()
    Dim curStr As String
    Dim i As Long
    Text8.Text = Right(Year(Date), 2) + Format(Month(Date), "00") + Format(Day(Date), "00") + Format(Hour(time), "00")
    Call OpenMsTabeFile
    
End Sub

Private Sub OpenMsTabeFile()
    Dim i As Long
    Dim curStr As String
    Dim curTotalStr As String
    Dim strCount As String
    
    List1.Clear
    
    If Dir$(App.Path & "\BhFh.txt") <> "" Then
        Open App.Path & "\BhFh.txt" For Input As #1
        Input #1, strCount
        nCount = strCount
        For i = 0 To nCount - 1
            Input #1, curStr
            MsTable(i) = curStr
            List1.AddItem curStr
        Next
        Close #1
    End If
End Sub

