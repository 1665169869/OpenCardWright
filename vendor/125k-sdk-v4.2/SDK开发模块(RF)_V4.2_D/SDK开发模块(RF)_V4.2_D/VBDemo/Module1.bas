Attribute VB_Name = "Module1"
Option Explicit

'*************************************************************************
'*  Á´½Ó¿¨Æ¬²Ù×÷º¯Êý¿â
'*************************************************************************

Public Declare Function mif_selecom Lib "LCRFRW_SDK.dll" (ByVal com As Long, ByVal baud As Long) As Long

Public Declare Function mif_selecard Lib "LCRFRW_SDK.dll" (ByVal ncardtype As Long) As Long

Public Declare Function mif_closecom Lib "LCRFRW_SDK.dll" () As Long

Public Declare Function tem_writedoorcard13_sdk Lib "LCRFRW_SDK.dll" (ByVal buffer1$, ByVal buffer2$, ByVal xh As Long, ByVal sjdw As Long, ByVal sjlenght As Long, ByVal gs As Long, ByVal p_nLockAP As Long, ByVal tfzd As Long, ByVal gs As Boolean) As Long
Public Declare Function tem_readdoorcard_sdk Lib "LCRFRW_SDK.dll" (ByVal buffer1$, ByVal buffer2$, ByVal p_nLockAP As Long) As Long

Public Declare Function tem_readtimercard_sdk Lib "LCRFRW_SDK.dll" (ByVal fksj$, ByVal time$, ByVal p_nLockAP As Long) As Long
Public Declare Function tem_writetimercard_sdk Lib "LCRFRW_SDK.dll" (ByVal fksj$, ByVal nxh As Long, ByVal time$, ByVal p_nLockAP As Long) As Long

Public Declare Function tem_readidcard_sdk Lib "LCRFRW_SDK.dll" (ByVal idbuff$) As Long

Public Declare Function tem_readdatetime13_sdk Lib "LCRFRW_SDK.dll" (ByVal p_csStartTime$, ByVal p_nUnit$, ByVal p_nTimes$, ByVal p_CheckOutHour$, ByVal p_OpenInner$, ByVal p_nLockAP As Long) As Long
