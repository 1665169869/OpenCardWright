unit Unit1;

interface

uses
  Windows, Messages, SysUtils, Classes, Graphics, Controls, Forms, Dialogs,
  StdCtrls, ComCtrls, ExtCtrls;

type
  TForm1 = class(TForm)
    Label1: TLabel;
    ComboBox1: TComboBox;
    Button1: TButton;
    Button4: TButton;
    Button5: TButton;
    Button7: TButton;
    StatusBar1: TStatusBar;
    Panel2: TPanel;
    Edit2: TEdit;
    Edit1: TEdit;
    Label2: TLabel;
    Label3: TLabel;
    Panel1: TPanel;
    Label4: TLabel;
    Label5: TLabel;
    Edit4: TEdit;
    Edit3: TEdit;
    Label6: TLabel;
    Label7: TLabel;
    Label8: TLabel;
    Edit5: TEdit;
    Label9: TLabel;
    Edit6: TEdit;
    Label10: TLabel;
    Label11: TLabel;
    Label12: TLabel;
    Label13: TLabel;
    Edit7: TEdit;
    Label14: TLabel;
    Edit8: TEdit;
    Label15: TLabel;
    Label16: TLabel;
    txtTimeUnit: TEdit;
    Label17: TLabel;
    txtTimeLen: TEdit;
    Label18: TLabel;
    Label19: TLabel;
    txtCheckOutHour: TEdit;
    txtOpenInner: TEdit;
    Label20: TLabel;
    Label21: TLabel;
    txtCheckOutHour2: TEdit;
    txtOpenInner2: TEdit;
    Label22: TLabel;
    Label23: TLabel;
    txtLockAp: TEdit;
    Label24: TLabel;
    Label25: TLabel;
    txtStartTime: TEdit;
    procedure FormCreate(Sender: TObject);
    procedure Button1Click(Sender: TObject);
    procedure Button7Click(Sender: TObject);
    procedure Button4Click(Sender: TObject);
    procedure Button5Click(Sender: TObject);
  private
    { Private declarations }
  public
    { Public declarations }
  end;

var
  Form1: TForm1;

implementation

uses dllunit;

{$R *.DFM}

procedure TForm1.FormCreate(Sender: TObject);
var
  curtime :string;

begin

  Combobox1.ItemIndex := 0;
  curtime :=DateTostr(date);

  edit4.text:=curtime;


end;

procedure TForm1.Button1Click(Sender: TObject);
var
  rt : integer;
  nrt: integer;
begin
  nrt:=9600;
  rt := dllunit.mif_selecom(Combobox1.itemindex,nrt);
  showmessage('返回值: '+inttostr(rt));
  if rt=0 then begin
     button5.Enabled := true;
     end;
  if rt<>0 then begin
    button7.Enabled := false;
    end;
  button7.Enabled := true;
  Statusbar1.SimpleText := '初始化反回值:'+Inttostr(rt);

end;

procedure TForm1.Button7Click(Sender: TObject);
var
  rt : integer;
begin
  rt := dllunit.mif_closecom;
  Button4.Enabled := False;
  Button5.Enabled := False;
  showmessage('返回值: '+inttostr(rt));

  Statusbar1.SimpleText := '关串口反回值:'+Inttostr(rt);


end;

procedure TForm1.Button4Click(Sender: TObject);
var
  rt : integer;
  xh : integer;
  sjdw : integer;
  sjlength : integer;
  nCheckOutHour: integer;
  nOpenInner :integer;
  nLockAp :integer;
  gs    : integer;
  buff1 : array [0..192] of char;
  buff2 : array [0..192] of char;
begin
  strpcopy(buff1,edit3.Text);
  strpcopy(buff2,edit4.Text);
  nCheckOutHour:= strtoint(txtCheckOutHour2.text);
  nOpenInner:=strtoint(txtOpenInner2.text);
  nLockAp:= strtoint(txtLockAp.text);

  xh := strtoint(edit5.text);
  sjdw := strtoint(edit6.text);
  sjlength := strtoint(edit7.text);
  gs := strtoint(edit8.text);

  if (nCheckOutHour<11) or (nCheckOutHour>18) then
      begin
      showmessage('退房钟点只能在11点到18');
      exit;
  end;

  rt := dllunit.tem_writedoorcard13_sdk(buff1,buff2,xh,sjdw,sjlength,gs,nLockAp,nCheckOutHour,nOpenInner);
  showmessage('返回值: '+inttostr(rt));
  Statusbar1.SimpleText := '写卡返回值:'+Inttostr(rt);

end;

procedure TForm1.Button5Click(Sender: TObject);
var
  rt : integer;
  nLockAp: integer ;
  szLockId : array [0..10] of char;
  szCardId : array [0..10] of char;
  szStartTime : array [0..10] of char;
  szTimeUnit : array [0..10] of char;
  szTimeLen : array [0..10] of char;
  szCheckOutHour : array [0..10] of char;
  szOpenInner : array [0..10] of char;
begin
  nLockAp:= strtoint(txtLockAp.text) ;
  rt := dllunit.tem_readdoorcard_sdk(szLockId,szCardId,nLockAp);
  if rt <>0 then
      begin
      Statusbar1.SimpleText := '读卡返回值:'+Inttostr(rt);
      showmessage(Statusbar1.SimpleText);
      exit;
  end;

  rt := dllunit.tem_readdatetime13_sdk(szStartTime,szTimeUnit,szTimeLen,szCheckOutHour,szOpenInner,nLockAp);
  if rt <>0 then
      begin
      Statusbar1.SimpleText := '读卡返回值:'+Inttostr(rt);
      showmessage(Statusbar1.SimpleText);
      exit;
  end;

  edit1.text := szLockId;
  edit3.text := szLockId;
  edit2.text := szCardId;
  edit4.text := szCardId ;
  txtStartTime.Text:= szStartTime;
  txtTimeUnit.text:=szTimeUnit ;
  txtTimeLen.text:=szTimeLen;
  txtCheckOutHour.text:= szCheckOutHour;
  txtOpenInner.text:=szOpenInner;

  {strCardId:=Copy(buff3,1,10);
  edit2.text := buff4;

  Buff4:=Copy(buff3,1,10);
  edit4.text := buff4; }

  showmessage('返回值: '+inttostr(rt));
  if rt=0 then
     Button4.Enabled :=True
  ELSE
     Button4.Enabled := False
  end;

end.
