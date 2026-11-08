object Form1: TForm1
  Left = 330
  Top = 120
  Width = 462
  Height = 718
  Caption = 'Form1'
  Color = clBtnFace
  Font.Charset = GB2312_CHARSET
  Font.Color = clMaroon
  Font.Height = -12
  Font.Name = #23435#20307
  Font.Style = [fsBold]
  OldCreateOrder = False
  OnCreate = FormCreate
  PixelsPerInch = 96
  TextHeight = 12
  object Label1: TLabel
    Left = 17
    Top = 28
    Width = 52
    Height = 12
    Caption = #20018#21475#21495#65306
  end
  object Label23: TLabel
    Left = 17
    Top = 60
    Width = 78
    Height = 12
    Caption = #31995#32479#26631#35782#21495#65306
  end
  object Label24: TLabel
    Left = 153
    Top = 60
    Width = 137
    Height = 12
    Caption = #20108#32423#20195#29702#21830#21495#65292#19968#33324#20026'0'
  end
  object ComboBox1: TComboBox
    Left = 72
    Top = 24
    Width = 241
    Height = 20
    ItemHeight = 12
    TabOrder = 0
    Text = 'ComboBox1'
    Items.Strings = (
      'COM1'
      'COM2'
      'COM3'
      'COM4'
      'COM5'
      'COM6'
      'COM7'
      'COM8'
      'COM9'
      'COM10'
      'COM11'
      'COM12'
      'COM13'
      'COM14'
      'COM15'
      'COM16')
  end
  object Button1: TButton
    Left = 329
    Top = 20
    Width = 90
    Height = 25
    Caption = #20018#21475#21021#22987#21270
    TabOrder = 1
    OnClick = Button1Click
  end
  object Button4: TButton
    Left = 329
    Top = 116
    Width = 90
    Height = 25
    Caption = #20889#21345
    Enabled = False
    TabOrder = 2
    OnClick = Button4Click
  end
  object Button5: TButton
    Left = 329
    Top = 68
    Width = 90
    Height = 25
    Caption = #35835#21345
    Enabled = False
    TabOrder = 3
    OnClick = Button5Click
  end
  object Button7: TButton
    Left = 329
    Top = 161
    Width = 90
    Height = 25
    Caption = #37322#25918#20018#21475
    TabOrder = 4
    OnClick = Button7Click
  end
  object StatusBar1: TStatusBar
    Left = 0
    Top = 665
    Width = 454
    Height = 19
    Panels = <>
  end
  object Panel2: TPanel
    Left = 16
    Top = 96
    Width = 300
    Height = 249
    Align = alCustom
    TabOrder = 6
    object Label2: TLabel
      Left = 44
      Top = 38
      Width = 65
      Height = 12
      Caption = #38376#38145#32534#21495#65306
    end
    object Label3: TLabel
      Left = 36
      Top = 65
      Width = 78
      Height = 12
      Caption = #20316#19994#27969#27700#21495#65306
    end
    object Label6: TLabel
      Left = 108
      Top = 4
      Width = 52
      Height = 12
      Caption = #35835#21345#25968#25454
    end
    object Label16: TLabel
      Left = 48
      Top = 128
      Width = 59
      Height = 12
      Caption = #26102#38388#21333#20301':'
    end
    object Label17: TLabel
      Left = 48
      Top = 160
      Width = 59
      Height = 12
      Caption = #26377#25928#26102#38271':'
    end
    object Label18: TLabel
      Left = 48
      Top = 224
      Width = 59
      Height = 12
      Caption = #24320#21551#21453#38145':'
    end
    object Label19: TLabel
      Left = 48
      Top = 192
      Width = 59
      Height = 12
      Caption = #36864#25151#38047#28857':'
    end
    object Label25: TLabel
      Left = 48
      Top = 96
      Width = 59
      Height = 12
      Caption = #24320#22987#26102#38388':'
    end
    object Edit2: TEdit
      Left = 123
      Top = 61
      Width = 121
      Height = 20
      TabOrder = 0
    end
    object Edit1: TEdit
      Left = 123
      Top = 33
      Width = 121
      Height = 20
      TabOrder = 1
    end
    object txtTimeUnit: TEdit
      Left = 123
      Top = 125
      Width = 121
      Height = 20
      TabOrder = 2
    end
    object txtTimeLen: TEdit
      Left = 123
      Top = 157
      Width = 121
      Height = 20
      TabOrder = 3
    end
    object txtCheckOutHour: TEdit
      Left = 123
      Top = 189
      Width = 121
      Height = 20
      TabOrder = 4
    end
    object txtOpenInner: TEdit
      Left = 123
      Top = 221
      Width = 38
      Height = 20
      TabOrder = 5
    end
    object txtStartTime: TEdit
      Left = 123
      Top = 93
      Width = 121
      Height = 20
      TabOrder = 6
    end
  end
  object Panel1: TPanel
    Left = 16
    Top = 360
    Width = 300
    Height = 297
    Align = alCustom
    TabOrder = 7
    object Label4: TLabel
      Left = 76
      Top = 38
      Width = 65
      Height = 12
      Caption = #38376#38145#32534#21495#65306
    end
    object Label5: TLabel
      Left = 92
      Top = 73
      Width = 39
      Height = 12
      Caption = #26102#38388#65306
    end
    object Label7: TLabel
      Left = 108
      Top = 4
      Width = 52
      Height = 12
      Caption = #20889#21345#25968#25454
    end
    object Label8: TLabel
      Left = 12
      Top = 105
      Width = 130
      Height = 12
      Caption = #19968#23567#26102#20869#30340#21457#21345#24207#21495#65306
    end
    object Label9: TLabel
      Left = 68
      Top = 137
      Width = 65
      Height = 12
      Caption = #26102#38388#21333#20301#65306
    end
    object Label10: TLabel
      Left = 183
      Top = 105
      Width = 28
      Height = 12
      Caption = '0-62'
    end
    object Label11: TLabel
      Left = 191
      Top = 169
      Width = 80
      Height = 12
      Caption = '3-'#24180','#20854#23427'-'#22825
    end
    object Label12: TLabel
      Left = 186
      Top = 154
      Width = 102
      Height = 12
      Caption = '0-'#26102',1-'#22825',2-'#26376','
    end
    object Label13: TLabel
      Left = 68
      Top = 168
      Width = 65
      Height = 12
      Caption = #26102#38388#38271#24230#65306
    end
    object Label14: TLabel
      Left = 68
      Top = 200
      Width = 65
      Height = 12
      Caption = #26159#21542#25346#22833#65306
    end
    object Label15: TLabel
      Left = 183
      Top = 201
      Width = 100
      Height = 12
      Caption = '0-'#19981#25346#22833',1-'#25346#22833
    end
    object Label20: TLabel
      Left = 64
      Top = 264
      Width = 59
      Height = 12
      Caption = #24320#21551#21453#38145':'
    end
    object Label21: TLabel
      Left = 64
      Top = 232
      Width = 59
      Height = 12
      Caption = #36864#25151#38047#28857':'
    end
    object Label22: TLabel
      Left = 191
      Top = 233
      Width = 61
      Height = 12
      Caption = '11'#28857'~18'#28857
    end
    object Edit4: TEdit
      Left = 142
      Top = 69
      Width = 121
      Height = 20
      TabOrder = 0
      Text = '06080917'
    end
    object Edit3: TEdit
      Left = 142
      Top = 33
      Width = 121
      Height = 20
      TabOrder = 1
      Text = 'abcd'
    end
    object Edit5: TEdit
      Left = 142
      Top = 101
      Width = 30
      Height = 20
      TabOrder = 2
      Text = '1'
    end
    object Edit6: TEdit
      Left = 142
      Top = 133
      Width = 38
      Height = 20
      TabOrder = 3
      Text = '1'
    end
    object Edit7: TEdit
      Left = 142
      Top = 165
      Width = 30
      Height = 20
      TabOrder = 4
      Text = '1'
    end
    object Edit8: TEdit
      Left = 142
      Top = 197
      Width = 30
      Height = 20
      TabOrder = 5
      Text = '1'
    end
    object txtCheckOutHour2: TEdit
      Left = 139
      Top = 229
      Width = 46
      Height = 20
      TabOrder = 6
      Text = '12'
    end
    object txtOpenInner2: TEdit
      Left = 139
      Top = 261
      Width = 38
      Height = 20
      TabOrder = 7
      Text = '0'
    end
  end
  object txtLockAp: TEdit
    Left = 99
    Top = 53
    Width = 46
    Height = 20
    TabOrder = 8
    Text = '0'
  end
end
