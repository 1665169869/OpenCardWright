//! Tauri 命令层。
//!
//! 三条规则：
//! * 所有设备调用走 `spawn_blocking`，不阻塞 UI 线程；
//! * 数据库异常与"查不到记录"必须区分——前者是 `Failed`，后者是 `NotFound`；
//! * 卡片数据（A 区）不受记录查询结果影响，永远返回。

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;
use tauri::State;

use ocw_store::DbConnection as Connection;

use ocw_device::{CardKind, CardSnapshot, DeviceError};
use ocw_store::{self, IssuanceRecord, Room, StoreError};

use crate::rules::{self, CardTier};
use crate::state::AppState;

// ---------------------------------------------------------------- 错误

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: i32,
    pub message: String,
    pub hint: String,
}

impl CommandError {
    pub fn new(code: i32, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(-1, message, "请导出诊断包并反馈")
    }
}

impl From<DeviceError> for CommandError {
    fn from(e: DeviceError) -> Self {
        Self::new(e.exit_code(), e.to_string(), e.hint())
    }
}

impl From<StoreError> for CommandError {
    fn from(e: StoreError) -> Self {
        Self::new(-2, e.to_string(), "数据库异常，请检查磁盘空间或恢复备份")
    }
}

// ---------------------------------------------------------------- DTO

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub db_path: String,
    pub runtime_dir: String,
    pub driver_sha256: Option<String>,
    /// true = 当前是模拟设备（未找到内置 SDK 或强制 OCW_SIM）
    pub simulation: bool,
    /// 串口索引，0 = COM1
    pub port: u8,
    /// SDK 来源：env / resource / dev
    pub sdk_source: Option<String>,
    /// 开发构建（`tauri dev`）为 true
    pub dev: bool,
}

/// 读卡页 B 区：记录查询的四种结果。
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum RecordLookup {
    Found { latest: IssuanceRecord, count: u32 },
    NotFound,
    NotApplicable { reason: String },
    Failed { message: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardReadDto {
    pub card: CardSnapshot,
    pub lookup: RecordLookup,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedDto {
    pub card: CardSnapshot,
    /// 管理卡（复位/清除/总控/校时）没有房间，所以是可选。
    pub room: Option<Room>,
    /// 到期时间（UTC ISO8601 估算值）。无时长的卡型（安装/复位/清除）为 None。
    pub expires_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfTestDto {
    pub passed: i32,
    pub failed: i32,
    pub ok: bool,
}

// ---------------------------------------------------------------- 工具

fn lock_db(db: &Arc<Mutex<Connection>>) -> Result<MutexGuard<'_, Connection>, CommandError> {
    db.lock()
        .map_err(|_| CommandError::internal("数据库连接状态异常，请重启程序"))
}

async fn blocking<T, F>(f: F) -> Result<T, CommandError>
where
    F: FnOnce() -> Result<T, CommandError> + Send + 'static,
    T: Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(result) => result,
        Err(e) => Err(CommandError::internal(format!("后台任务异常: {e}"))),
    }
}

/// 记录查询。**这里绝不能把数据库错误吞成"未查到"。**
fn lookup_record(db: &Arc<Mutex<Connection>>, card: &CardSnapshot) -> RecordLookup {
    let card_no = match card.card_no {
        Some(no) => no,
        None => {
            let reason = if card.kind.is_blank() {
                "空白卡 / 已退房，无卡号"
            } else {
                "该卡型不含卡号（管理卡）"
            };
            return RecordLookup::NotApplicable {
                reason: reason.to_string(),
            };
        }
    };

    let conn = match db.lock() {
        Ok(c) => c,
        Err(_) => {
            return RecordLookup::Failed {
                message: "数据库连接状态异常".into(),
            }
        }
    };

    match ocw_store::latest_issuance_for_card(&conn, card_no) {
        Ok(Some(latest)) => {
            let count = ocw_store::count_issuance_for_card(&conn, card_no).unwrap_or(1);
            RecordLookup::Found { latest, count }
        }
        Ok(None) => RecordLookup::NotFound,
        Err(e) => RecordLookup::Failed {
            message: e.to_string(),
        },
    }
}

// ---------------------------------------------------------------- 命令

fn app_info_of(state: &AppState) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: state.db_path.display().to_string(),
        runtime_dir: state.runtime_dir.display().to_string(),
        driver_sha256: state.driver_sha256.clone(),
        simulation: state.device().is_sim(),
        port: state.port(),
        sdk_source: state.sdk_source.clone(),
        dev: state.dev,
    }
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    app_info_of(&state)
}

/// 切换串口。设备层每次调用都会开关串口，所以重建实例是安全的；
/// 端口写进 setting，下次启动沿用。
#[tauri::command]
pub async fn set_port(state: State<'_, AppState>, port: u8) -> Result<AppInfo, CommandError> {
    require_admin(&state)?;
    if port > 7 {
        return Err(CommandError::new(
            1,
            "串口索引必须在 0–7（0 = COM1，6 = COM7）",
            "用「探测端口」看看读卡器挂在哪一个",
        ));
    }

    {
        let conn = lock_db(&state.db)?;
        ocw_store::set_setting(&conn, "port", &port.to_string())?;
    }
    state
        .rebuild_device(port)
        .map_err(|m| CommandError::new(-3, m, "重启程序后再试"))?;

    Ok(app_info_of(&state))
}

/// 读卡：卡片数据一定返回；记录区给出 Found / NotFound / NotApplicable / Failed。
#[tauri::command]
pub async fn read_card(state: State<'_, AppState>) -> Result<CardReadDto, CommandError> {
    require_login(&state)?;
    let device = state.device();
    let db = state.db.clone();

    blocking(move || {
        let card = device.read_card()?;
        let lookup = lookup_record(&db, &card);
        Ok(CardReadDto { card, lookup })
    })
    .await
}

#[tauri::command]
pub async fn list_rooms(state: State<'_, AppState>) -> Result<Vec<Room>, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_rooms(&conn)?)
    })
    .await
}

/// 新增房间。序号不传则自动分配 0–255 中最小的空闲值。
///
/// 如果传的序号属于一条**已逻辑删除**的房间，会复用那条记录（只改房名）并返回
/// `revivedFrom`，因为序号已随门锁绑定、不能真正插一行新记录。
#[tauri::command]
pub async fn create_room(
    state: State<'_, AppState>,
    room_no: String,
    room_index: Option<u8>,
    room_type_id: Option<i64>,
) -> Result<ocw_store::RoomUpsert, CommandError> {
    let me = require_admin(&state)?;
    let room_no = require_room_no(&room_no)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        // 房型必须真实存在，避免前端传了个已被删掉的 id
        if let Some(type_id) = room_type_id {
            let exists: i64 = conn
                .query_row("SELECT COUNT(*) FROM room_type WHERE id = ?1", [type_id], |r| {
                    r.get(0)
                })
                .map_err(ocw_store::StoreError::from)?;
            if exists == 0 {
                return Err(CommandError::new(1, "房型不存在", "刷新房型列表后重选"));
            }
        }
        let upsert = ocw_store::insert_room_reporting(
            &conn,
            &room_no,
            room_index,
            None,
            None,
            room_type_id,
        )?;
        // 复用老记录这种"看不见的写操作"更需要留痕
        let _ = ocw_store::record_audit(
            &conn,
            "create_room",
            Some(&room_no),
            Some(&format!(
                "roomIndex={},revivedFrom={}",
                upsert.room.room_index,
                upsert.revived_from.as_deref().unwrap_or("-")
            )),
            "ok",
            Some(me.operator_id),
        );
        Ok(upsert)
    })
    .await
}

/// 删除房间 = 逻辑删除：只置 `active = 0`，序号不释放、历史发卡记录保留。
///
/// 场上还有有效卡时拒绝，理由和改房号一样：卡是按房号对账的，
/// 房间一旦从房间图消失，这些卡就没有地方退了。
#[tauri::command]
pub async fn delete_room(state: State<'_, AppState>, room_no: String) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let room_no = require_room_no(&room_no)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let room = ocw_store::find_room(&conn, &room_no)?.ok_or_else(|| {
            CommandError::new(1, format!("房号不存在：{room_no}"), "刷新房间列表后重试")
        })?;
        if !room.active {
            return Err(CommandError::new(1, format!("{room_no} 房已经删除了"), "刷新房间列表"));
        }
        let cards = ocw_store::count_active_cards(&conn, &room_no)?;
        if cards > 0 {
            return Err(CommandError::new(
                2,
                format!("{room_no} 房还有 {cards} 张有效卡，不能删除"),
                "请先退房（读卡退房 / 无卡退房）再删除",
            ));
        }
        ocw_store::deactivate_room(&conn, &room_no)?;
        let _ = ocw_store::record_audit(
            &conn,
            "delete_room",
            Some(&room_no),
            Some(&format!("roomIndex={}", room.room_index)),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 改房号（房间显示名称），序号保持不变。
#[tauri::command]
pub async fn rename_room(
    state: State<'_, AppState>,
    room_no: String,
    new_room_no: String,
) -> Result<ocw_store::Room, CommandError> {
    let me = require_admin(&state)?;
    let old_no = require_room_no(&room_no)?;
    let new_no = require_room_no(&new_room_no)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        if ocw_store::find_room(&conn, &old_no)?.is_none() {
            return Err(CommandError::new(1, format!("房号不存在：{old_no}"), "刷新房间列表后重试"));
        }
        // 退房/发卡都按房号关联，场上还有卡时改名会让这些卡对不上账
        let cards = ocw_store::count_active_cards(&conn, &old_no)?;
        if cards > 0 {
            return Err(CommandError::new(
                2,
                format!("{old_no} 房还有 {cards} 张有效卡，不能改房号"),
                "请先退房（读卡退房 / 无卡退房）再改",
            ));
        }
        ocw_store::rename_room(&conn, &old_no, &new_no)?;
        let _ = ocw_store::record_audit(
            &conn,
            "rename_room",
            Some(&old_no),
            Some(&format!("to={new_no}")),
            "ok",
            Some(me.operator_id),
        );
        ocw_store::find_room(&conn, &new_no)?
            .ok_or_else(|| CommandError::new(1, "改名后读不回该房间", "刷新房间列表"))
    })
    .await
}

/// 新增房号后的闭环：制作安装卡（只含序号），拿到锁上刷一次即完成绑定。
#[tauri::command]
pub async fn issue_install_card(
    state: State<'_, AppState>,
    room_no: String,
) -> Result<IssuedDto, CommandError> {
    let me = require_admin(&state)?;
    let device = state.device();
    let db = state.db.clone();
    let room_no = room_no.trim().to_string();

    // 写卡前统一先读卡：没卡就别进设备层才发现
    pre_read_card(&device)?;

    blocking(move || {
        let room = {
            let conn = lock_db(&db)?;
            let room = ocw_store::find_room(&conn, &room_no)?.ok_or_else(|| {
                CommandError::new(
                    1,
                    format!("房号不存在：{room_no}"),
                    "请先在房间管理里新增该房号",
                )
            })?;
            room
        };

        if !room.active {
            return Err(CommandError::new(
                1,
                format!("房间已停用：{room_no}"),
                "启用后再制作安装卡",
            ));
        }

        // 设备调用期间不持有数据库锁
        let card = device.issue(
            &["--type", "install", "--room", room_no.as_str()],
            Some(room.room_index),
        )?;

        {
            let conn = lock_db(&db)?;
            ocw_store::mark_install_card(&conn, &room_no)?;
            ocw_store::record_issuance(
                &conn,
                &ocw_store::NewIssuance {
                    room_no: Some(room_no.as_str()),
                    kind: "安装卡",
                    days: 0,
                    hours: 0,
                    card_no: None,
                    raw_hex: Some(card.raw_hex.as_str()),
                    operator_id: Some(me.operator_id),
                    reader_id: None,
                    result: "ok",
                    error_code: None,
                    idempotency_key: None,
                    duration_ms: None,
                },
            )?;
        }

        Ok(IssuedDto {
            card,
            room: Some(room),
            expires_at: None,
        })
    })
    .await
}

/// 转发 SDK 的自检（27 条黄金向量）。构建流水线也跑同一个命令。
#[tauri::command]
pub async fn run_selftest(state: State<'_, AppState>) -> Result<SelfTestDto, CommandError> {
    require_login(&state)?;
    let device = state.device();
    blocking(move || {
        let st = device.selftest()?;
        Ok(SelfTestDto {
            passed: st.passed,
            failed: st.failed,
            ok: st.is_ok(),
        })
    })
    .await
}

#[tauri::command]
pub async fn probe_ports(
    state: State<'_, AppState>,
) -> Result<Vec<ocw_device::PortProbe>, CommandError> {
    require_login(&state)?;
    let device = state.device();
    blocking(move || Ok(device.probe_ports()?)).await
}

/// 写卡前统一先读一次卡。
///
/// 发卡 / 续住 / 安装卡 / 管理卡都必须先过这一步：
/// 一是确认读卡器上确实有卡（否则错误会在设备层才冒出来，提示很含糊），
/// 二是把原卡信息打进日志，方便排查"这次到底写的是哪张卡"。
fn pre_read_card(device: &crate::state::Device) -> Result<CardSnapshot, CommandError> {
    match device.read_card() {
        Ok(card) => {
            let no = card
                .card_no
                .map(|v| format!("0x{v:04X}"))
                .unwrap_or_else(|| "无卡号".to_string());
            eprintln!("[write] 写卡前预读：{} · {no}", card.kind_label);
            Ok(card)
        }
        Err(e) => Err(CommandError::new(
            e.exit_code(),
            format!("写卡前读卡失败：{e}"),
            "先把卡放到读卡器上，确认串口没被原门锁软件占用，然后重试",
        )),
    }
}

// ---------------------------------------------------------------- 管理卡

/// 管理卡请求。前端传 `{ kind: "master", days: 1 }` 这种结构。
///
/// `rename_all` 只改**变体名**；变体里的字段要靠 `rename_all_fields`，
/// 否则 `Install { room_no }` 会期望 `room_no` 而不是前端发的 `roomNo`。
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AdminCardRequest {
    Master { days: Option<u16>, hours: Option<u16> },
    Reset,
    Clear,
    Install { room_no: String },
    Checkout { hour: u8 },
    TimeSync { hour: Option<u8>, minute: Option<u8> },
    Raw { p1: String, p2: String },
}

impl AdminCardRequest {
    fn label(&self) -> &'static str {
        match self {
            Self::Master { .. } => "总控卡",
            Self::Reset => "复位卡",
            Self::Clear => "清除卡",
            Self::Install { .. } => "安装卡",
            Self::Checkout { .. } => "退房钟点卡",
            Self::TimeSync { .. } => "校时卡",
            Self::Raw { .. } => "原始直写",
        }
    }

    /// 转成 CLI 参数（不含末尾的 `--json`，由 `Device::issue` 补）。
    fn to_args(&self) -> Result<Vec<String>, CommandError> {
        let mut a: Vec<String> = Vec::new();
        match self {
            Self::Master { days, hours } => {
                a.push("--type".into());
                a.push("master".into());
                if let Some(h) = hours {
                    rules::validate_hours(CardTier::Master, *h)
                        .map_err(|m| CommandError::new(1, m, ""))?;
                    a.push("--hours".into());
                    a.push(h.to_string());
                } else {
                    let d = days.unwrap_or(1);
                    rules::validate_days(CardTier::Master, d)
                        .map_err(|m| CommandError::new(1, m, "总控卡超过 63 天请从固定档位选择"))?;
                    a.push("--days".into());
                    a.push(d.to_string());
                }
            }
            Self::Reset => {
                a.push("--type".into());
                a.push("reset".into());
            }
            Self::Clear => {
                a.push("--type".into());
                a.push("clear".into());
            }
            Self::Install { room_no } => {
                a.push("--type".into());
                a.push("install".into());
                a.push("--room".into());
                a.push(room_no.trim().to_string());
            }
            Self::Checkout { hour } => {
                if *hour > 23 {
                    return Err(CommandError::new(1, "退房钟点必须在 0–23", ""));
                }
                a.push("--type".into());
                a.push("checkout".into());
                a.push("--hour".into());
                a.push(hour.to_string());
            }
            Self::TimeSync { hour, minute } => {
                a.push("--type".into());
                a.push("timesync".into());
                if let (Some(h), Some(m)) = (hour, minute) {
                    if *h > 23 || *m > 59 {
                        return Err(CommandError::new(1, "时间不合法", "小时 0–23，分钟 0–59"));
                    }
                    // CLI 用 --room HH:MM 传时间（历史怪癖）
                    a.push("--room".into());
                    a.push(format!("{h:02}:{m:02}"));
                }
            }
            Self::Raw { p1, p2 } => {
                let valid = |s: &str| s.len() == 8 && s.chars().all(|c| c.is_ascii_hexdigit());
                if !valid(p1) || !valid(p2) {
                    return Err(CommandError::new(1, "p1 / p2 必须是 8 个十六进制字符", ""));
                }
                a.push("--type".into());
                a.push("raw".into());
                a.push("--p1".into());
                a.push(p1.to_uppercase());
                a.push("--p2".into());
                a.push(p2.to_uppercase());
            }
        }
        Ok(a)
    }
}

/// 发管理卡。必须 `confirm: true`，且每次操作都落审计。
#[tauri::command]
pub async fn issue_admin_card(
    state: State<'_, AppState>,
    request: AdminCardRequest,
    confirm: bool,
    // 总控卡长有效期（>63 天）的确认词
    confirm_long: Option<String>,
) -> Result<IssuedDto, CommandError> {
    let me = require_admin(&state)?;
    if let AdminCardRequest::Master { days: Some(d), hours: None } = &request {
        rules::check_long_confirm(CardTier::Master, *d, confirm_long.as_deref())
            .map_err(|m| CommandError::new(1, m, "总控卡长有效期要先确认水位后果"))?;
    }
    if !confirm {
        return Err(CommandError::new(
            1,
            "管理卡需要二次确认",
            "管理卡会改变门锁状态，必须先确认再发卡",
        ));
    }

    let label = request.label().to_string();
    let device = state.device();
    let db = state.db.clone();

    // 写卡前统一先读卡
    pre_read_card(&device)?;

    blocking(move || {
        let args = request.to_args()?;
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        // 安装卡需要房间；其它卡带房号反而会干扰 CLI
        let room_no = match &request {
            AdminCardRequest::Install { room_no } => Some(room_no.trim().to_string()),
            _ => None,
        };

        let room = if let Some(no) = &room_no {
            let conn = lock_db(&db)?;
            let room = ocw_store::find_room(&conn, no)?.ok_or_else(|| {
                CommandError::new(1, format!("房号不存在：{no}"), "请先在房间管理里新增该房号")
            })?;
            // 已逻辑删除的房间不能再做安装卡（与 issue_install_card 的护栏一致）
            if !room.active {
                return Err(CommandError::new(
                    1,
                    format!("房间已删除：{no}"),
                    "先新增该序号的房间再制作安装卡",
                ));
            }
            Some(room)
        } else {
            None
        };

        let started = Instant::now();
        let issued = device.issue(&refs, room.as_ref().map(|r| r.room_index));
        let duration = started.elapsed().as_millis() as i64;

        match issued {
            Ok(card) => {
                let conn = lock_db(&db)?;
                if let Some(no) = &room_no {
                    ocw_store::mark_install_card(&conn, no)?;
                }
                let issuance_id = ocw_store::record_issuance(
                    &conn,
                    &ocw_store::NewIssuance {
                        room_no: room_no.as_deref(),
                        kind: &label,
                        days: card.days as i64,
                        hours: card.hours as i64,
                        card_no: card.card_no,
                        raw_hex: Some(card.raw_hex.as_str()),
                        operator_id: Some(me.operator_id),
                        reader_id: None,
                        result: "ok",
                        error_code: None,
                        idempotency_key: None,
                        duration_ms: Some(duration),
                    },
                )?;
                let expires_at = ocw_store::find_issuance(&conn, issuance_id)?
                    .and_then(|r| r.expires_at);
                ocw_store::record_audit(
                    &conn,
                    "issue_admin_card",
                    Some(&label),
                    Some(&format!("room={room_no:?} raw={}", card.raw_hex)),
                    "ok",
                    Some(me.operator_id),
                )?;
                Ok(IssuedDto {
                    card,
                    room,
                    expires_at,
                })
            }
            Err(e) => {
                // 失败也要留痕：管理卡失败往往是现场事故的第一手线索
                let conn = lock_db(&db)?;
                let detail = format!("room={room_no:?} err={e}");
                let _ = ocw_store::record_audit(
                    &conn,
                    "issue_admin_card",
                    Some(&label),
                    Some(&detail),
                    "error",
                    None,
                );
                Err(e.into())
            }
        }
    })
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WaterLevel {
    /// 正常
    Normal,
    /// >= 警戒线（默认 0xE000，可配置）：提示给全部门锁刷复位卡
    Warn,
    /// >= 0xFF00：阻断客卡，只能发管理卡（去刷复位卡）
    Blocked,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardNoStatus {
    /// 水位 = SDK 的 `CardNo.state`（缺失时退回本地已发最大号）
    pub current: Option<u16>,
    /// 来自运行副本里的 CardNo.state
    pub state_file: Option<u16>,
    /// 来自本地发卡流水
    pub max_issued: Option<u16>,
    pub level: WaterLevel,
    /// 警戒线（设置里可改，默认 0xE000）
    pub warn_at: u16,
    /// 硬阻断线：到这条线只能发管理卡，必须刷复位卡
    pub block_at: u16,
    pub state_path: String,
}

/// 读 `CardNo.state`（内容是 4 位十六进制文本）。
fn read_state_file(path: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(path).ok()?;
    let t = text.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u32::from_str_radix(t, 16).ok().filter(|v| *v <= 0xFFFF).map(|v| v as u16)
}

/// 原子写 `CardNo.state`（先写 tmp 再改名，避免断电留下半个文件）。
fn write_state_file(path: &Path, value: u16) -> std::io::Result<()> {
    let tmp = path.with_extension("state.tmp");
    std::fs::write(&tmp, format!("{value:04X}"))?;
    std::fs::rename(&tmp, path)
}

/// 解析用户输入的十六进制卡号（允许带 0x 前缀）。
fn parse_hex_u16(input: &str) -> Result<u16, CommandError> {
    let t = input.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u32::from_str_radix(t, 16)
        .ok()
        .filter(|v| *v <= 0xFFFF)
        .map(|v| v as u16)
        .ok_or_else(|| CommandError::new(1, "请输入 16 位十六进制数（如 E000）", "范围 0000–FFFF"))
}

const DEFAULT_WARN_AT: u16 = 0xE000;
const BLOCK_AT: u16 = 0xFF00;

/// 读警戒线设置；没设置或非法就用默认值。
fn water_warn_at(conn: &Connection) -> u16 {
    ocw_store::get_setting(conn, "waterWarnAt")
        .ok()
        .flatten()
        .and_then(|v| parse_hex_u16(&v).ok())
        .filter(|v| *v < BLOCK_AT)
        .unwrap_or(DEFAULT_WARN_AT)
}

fn compute_status(db: &Arc<Mutex<Connection>>, state_path: &Path) -> Result<CardNoStatus, CommandError> {
    let (max_issued, warn_at) = {
        let conn = lock_db(db)?;
        (ocw_store::max_issued_card_no(&conn)?, water_warn_at(&conn))
    };
    let state_file = read_state_file(state_path);

    // 水位以 SDK 的计数器文件为准：每发一张卡它就前进一格，和"刚发出的卡号"天然对得上。
    // 数据库里的最大号只作为**历史参考**（可能被导入/手写的异常大号污染），不再参与水位计算。
    let current = state_file.or(max_issued);

    let level = match current {
        Some(v) if v >= BLOCK_AT => WaterLevel::Blocked,
        Some(v) if v >= warn_at => WaterLevel::Warn,
        _ => WaterLevel::Normal,
    };

    Ok(CardNoStatus {
        current,
        state_file,
        max_issued,
        level,
        warn_at,
        block_at: BLOCK_AT,
        state_path: state_path.display().to_string(),
    })
}

/// 设置警戒线（管理员）。到这条线开始提示"给全部门锁刷复位卡"。
#[tauri::command]
pub async fn set_water_warn_at(
    state: State<'_, AppState>,
    hex: String,
) -> Result<CardNoStatus, CommandError> {
    let me = require_admin(&state)?;
    let value = parse_hex_u16(&hex)?;
    if value >= BLOCK_AT {
        return Err(CommandError::new(
            1,
            format!("警戒线必须低于 {BLOCK_AT:04X}"),
            "到了 FF00 就只能发管理卡了",
        ));
    }
    if value < 0x100 {
        return Err(CommandError::new(1, "警戒线太低", "至少要高于 0x0100"));
    }
    let db = state.db.clone();
    let path = state.runtime_dir.join("CardNo.state");
    blocking(move || {
        {
            let conn = lock_db(&db)?;
            ocw_store::set_setting(&conn, "waterWarnAt", &format!("{value:04X}"))?;
            let _ = ocw_store::record_audit(
                &conn,
                "set_water_warn_at",
                Some(&format!("{value:04X}")),
                None,
                "ok",
                Some(me.operator_id),
            );
        }
        compute_status(&db, &path)
    })
    .await
}

/// 重置水位：把卡号计数器拨回一个较低的值。
///
/// **前提**：已经给全部门锁刷过复位卡 —— 锁里的高水位是每房独立的，
/// 没刷复位就拨低，新卡会被那些锁当成"更旧的卡"拒开。
#[tauri::command]
pub async fn reset_water_mark(
    state: State<'_, AppState>,
    start_at: String,
) -> Result<CardNoStatus, CommandError> {
    let me = require_admin(&state)?;
    let value = parse_hex_u16(&start_at)?;
    let db = state.db.clone();
    let path = state.runtime_dir.join("CardNo.state");
    blocking(move || {
        let warn_at = {
            let conn = lock_db(&db)?;
            water_warn_at(&conn)
        };
        if value >= warn_at {
            return Err(CommandError::new(
                1,
                format!("重置值 {value:04X} 不能高于警戒线 {warn_at:04X}"),
                "先调低警戒线，或把重置值调到更低",
            ));
        }
        write_state_file(&path, value)
            .map_err(|e| CommandError::new(2, format!("写 CardNo.state 失败：{e}"), "检查运行目录权限"))?;
        {
            let conn = lock_db(&db)?;
            ocw_store::set_setting(&conn, "startCardNo", &format!("{value:04X}"))?;
            let _ = ocw_store::record_audit(
                &conn,
                "reset_water_mark",
                Some(&format!("{value:04X}")),
                Some(&format!("warnAt={warn_at:04X}")),
                "ok",
                Some(me.operator_id),
            );
        }
        eprintln!("[water] 水位已重置为 {value:04X}（原水位见审计），请确认全部门锁已刷复位卡");
        compute_status(&db, &path)
    })
    .await
}

#[tauri::command]
pub async fn card_no_status(state: State<'_, AppState>) -> Result<CardNoStatus, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    let path = state.runtime_dir.join("CardNo.state");
    blocking(move || compute_status(&db, &path)).await
}

// ---------------------------------------------------------------- 发客卡

fn snapshot_from_record(rec: &IssuanceRecord, room: &Room) -> CardSnapshot {
    CardSnapshot {
        kind: CardKind::from_label(&rec.kind),
        kind_label: rec.kind.clone(),
        room: rec.room_no.clone(),
        room_index: room.room_index as i32,
        days: rec.days as i32,
        hours: rec.hours as i32,
        card_no: rec.card_no.and_then(|v| u16::try_from(v).ok()),
        raw_hex: rec.raw_hex.clone().unwrap_or_default(),
    }
}

/// 通用文本校验：去首尾空白 + 长度区间。前端也有同名规则，两边要一致。
fn require_text(value: &str, field: &str, min: usize, max: usize) -> Result<String, CommandError> {
    let v = value.trim();
    let n = v.chars().count();
    if n < min {
        return Err(CommandError::new(1, format!("{field}不能为空"), format!("至少 {min} 个字符")));
    }
    if n > max {
        return Err(CommandError::new(
            1,
            format!("{field}最多 {max} 个字符"),
            format!("当前 {n} 个字符"),
        ));
    }
    Ok(v.to_string())
}

/// 登录账号：3–32 位，字母/数字/下划线/点/横线，且以字母或数字开头。
fn require_login_name(value: &str) -> Result<String, CommandError> {
    let v = require_text(value, "登录账号", 3, 32)?;
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err(CommandError::new(
            1,
            "登录账号只能包含字母、数字、下划线、点或横线",
            "",
        ));
    }
    if !v.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
        return Err(CommandError::new(1, "登录账号必须以字母或数字开头", ""));
    }
    Ok(v)
}

/// 房号：1–16 位，字母/数字/横线/下划线（不限制纯数字，现场可能有 A201 这类）。
fn require_room_no(value: &str) -> Result<String, CommandError> {
    let v = require_text(value, "房号", 1, 16)?;
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CommandError::new(1, "房号只能包含字母、数字、横线或下划线", ""));
    }
    Ok(v)
}

/// 发卡时录入的客人信息（全部可空）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestInput {
    pub name: Option<String>,
    pub gender: Option<String>,
    pub id_type: Option<String>,
    pub id_number: Option<String>,
    pub nation: Option<String>,
    pub address: Option<String>,
    pub birth_date: Option<String>,
    pub note: Option<String>,
}

/// 去掉首尾空白、空串转 None、限制长度（防止一次粘贴几 MB 进库）。
fn clip(value: &Option<String>, max: usize) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(max).collect())
}

/// 发客卡（日租或钟点）。
///
/// `days` / `hours` 必须二选一。`idempotency_key` 由前端每次提交生成，
/// 同一个键重复提交只会发一张卡，并回放上一次的结果。
#[tauri::command]
pub async fn issue_guest(
    state: State<'_, AppState>,
    room_no: String,
    days: Option<u16>,
    hours: Option<u16>,
    idempotency_key: String,
    guest: Option<GuestInput>,
    // 长有效期卡（>62 天）的确认词，必须原样等于「顶高水位」；普通档位可不传
    confirm_long: Option<String>,
) -> Result<IssuedDto, CommandError> {
    let me = require_login(&state)?;
    let (use_days, amount) = match (days, hours) {
        (Some(d), None) => {
            rules::validate_days(CardTier::Guest, d).map_err(|m| {
                CommandError::new(1, m, "客卡超过 62 天的有效期请从固定档位（90/120/…）选择")
            })?;
            rules::check_long_confirm(CardTier::Guest, d, confirm_long.as_deref())
                .map_err(|m| CommandError::new(1, m, "长有效期卡要先确认水位后果"))?;
            (true, d)
        }
        (None, Some(h)) => {
            rules::validate_hours(CardTier::Guest, h)
                .map_err(|m| CommandError::new(1, m, "钟点卡上限 62 小时"))?;
            (false, h)
        }
        _ => {
            return Err(CommandError::new(
                1,
                "必须且只能指定天数或小时之一",
                "日租卡传 days，钟点卡传 hours",
            ))
        }
    };

    let idempotency_key = idempotency_key.trim().to_string();
    if idempotency_key.is_empty() {
        return Err(CommandError::new(1, "缺少幂等键", "由前端生成，用于防止重复发卡"));
    }

    let device = state.device();
    let db = state.db.clone();
    let state_path = state.runtime_dir.join("CardNo.state");
    let room_no = room_no.trim().to_string();

    // 身份信息只存库，长度做个上限，避免误粘贴
    let g = guest.unwrap_or_default();
    let g_name = clip(&g.name, 64);
    let g_gender = clip(&g.gender, 8);
    let g_id_type = clip(&g.id_type, 32);
    let g_id_number = clip(&g.id_number, 64);
    let g_nation = clip(&g.nation, 32);
    let g_address = clip(&g.address, 200);
    let g_birth_date = clip(&g.birth_date, 10);
    let g_note = clip(&g.note, 500);

    blocking(move || {
        // 1) 幂等回放：同一个键已经发过，直接把上次结果还回去
        {
            let conn = lock_db(&db)?;
            if let Some(rec) = ocw_store::find_issuance_by_idempotency(&conn, &idempotency_key)? {
                if let Some(room) = ocw_store::find_room(&conn, &room_no)? {
                    return Ok(IssuedDto {
                        card: snapshot_from_record(&rec, &room),
                        room: Some(room),
                        expires_at: rec.expires_at.clone(),
                    });
                }
            }
        }

        // 2) 房间校验
        let room = {
            let conn = lock_db(&db)?;
            ocw_store::find_room(&conn, &room_no)?.ok_or_else(|| {
                CommandError::new(1, format!("房号不存在：{room_no}"), "请先在房间管理里新增该房号")
            })?
        };
        if !room.active {
            return Err(CommandError::new(
                1,
                format!("房间已停用：{room_no}"),
                "停用的房间不能再发卡",
            ));
        }

        // 房间级发卡上限：看还剩几个名额，再动卡
        if let Some(max) = room.max_cards {
            let active = {
                let conn = lock_db(&db)?;
                ocw_store::count_active_cards(&conn, &room_no)?
            };
            if active >= max {
                eprintln!(
                    "[guard] {room_no} 房已达最大发卡数 {max}（当前有效 {active} 张），拒绝发卡"
                );
                return Err(CommandError::new(
                    409,
                    format!("{room_no} 房当前有效客卡 {active} 张，已达上限 {max} 张"),
                    "等旧卡过期，或到房间页调大「最大发卡数」",
                ));
            }
        }

        // 3) 水位闸门
        let status = compute_status(&db, &state_path)?;
        if status.level == WaterLevel::Blocked {
            return Err(CommandError::new(
                6,
                "卡号水位已超过 0xFF00，已停止发客卡",
                "请在空房时段按「复位作业向导」逐锁刷复位卡，再继续发卡",
            ));
        }

        // 同房第二张卡复用有效首卡序号（TD-127，已实测：同号刷门会刷新锁内有效期）：
        // 首卡仍在有效期内且未到上限 → 复用它的卡号构包（不推进 CardNo.state），
        // 第一张卡不会被顶掉；上限口径用 count_active_cards（DISTINCT 卡号，同号合并计一张）。
        let reuse_serial: Option<u16> = {
            let conn = lock_db(&db)?;
            match ocw_store::latest_active_guest_card(&conn, &room_no)? {
                Some(rec) => {
                    let active = ocw_store::count_active_cards(&conn, &room_no)?;
                    let within_limit = room.max_cards.map_or(true, |max| active < max);
                    if within_limit {
                        rec.card_no.map(|n| n as u16)
                    } else {
                        None
                    }
                }
                None => None,
            }
        };

        // 4) 写卡前统一先读卡：确认卡在机器上，并记录原卡信息
        pre_read_card(&device)?;

        // 4) 发卡（设备调用期间不持锁）
        let started = Instant::now();
        let amount_text = amount.to_string();
        let flag = if use_days { "--days" } else { "--hours" };
        let serial_text = reuse_serial.map(|n| format!("{n:04X}"));
        let mut args: Vec<&str> = vec!["--room", room_no.as_str(), flag, amount_text.as_str()];
        if let Some(s) = serial_text.as_deref() {
            args.push("--serial");
            args.push(s);
        }
        // 模拟设备会离线构包并推进 CardNo.state；真机返回 None，继续走 CLI
        let card = match device.sim_issue_guest(
            room.room_index,
            &room_no,
            if use_days { amount } else { 0 },
            if use_days { 0 } else { amount },
            reuse_serial,
        ) {
            Some(res) => res?,
            None => device.issue(&args, Some(room.room_index))?,
        };

        // 5) 落库（含客人身份信息；身份信息只存库，不写卡）
        {
            let conn = lock_db(&db)?;
            let issuance_id = ocw_store::record_issuance(
                &conn,
                &ocw_store::NewIssuance {
                    room_no: Some(room_no.as_str()),
                    kind: &card.kind_label,
                    days: card.days as i64,
                    hours: card.hours as i64,
                    card_no: card.card_no,
                    raw_hex: Some(card.raw_hex.as_str()),
                    operator_id: Some(me.operator_id),
                    reader_id: None,
                    result: "ok",
                    error_code: None,
                    idempotency_key: Some(idempotency_key.as_str()),
                    duration_ms: Some(started.elapsed().as_millis() as i64),
                },
            )?;
            ocw_store::update_guest_identity(
                &conn,
                issuance_id,
                &ocw_store::GuestIdentity {
                    name: g_name.as_deref(),
                    gender: g_gender.as_deref(),
                    id_type: g_id_type.as_deref(),
                    id_number: g_id_number.as_deref(),
                    nation: g_nation.as_deref(),
                    address: g_address.as_deref(),
                    birth_date: g_birth_date.as_deref(),
                    note: g_note.as_deref(),
                },
            )?;
            let expires_at = ocw_store::find_issuance(&conn, issuance_id)?
                .and_then(|r| r.expires_at);

            // 6) 发卡成功后同步房态（TD-115）：空净房 / 预定房 → 未满房；
            //    设了上限且有效卡已到顶 → 已满房。维修 / 其他 / 已满 不动。
            //    只在成功分支执行 —— 写卡失败保持原房态。
            if matches!(room.status.as_str(), "vacant_clean" | "reserved") {
                let active_after = ocw_store::count_active_cards(&conn, &room_no)?;
                let new_status = match room.max_cards {
                    Some(max) if active_after >= max => "occupied_full",
                    _ => "occupied_partial",
                };
                ocw_store::set_room_status(&conn, &room_no, new_status)?;
            }

            // 7) 发卡即入住（TD-128）：把该房覆盖今天的有效预定标为「已入住」——
            //    日历 / 房号列的「预定」随即消失，再发第二张卡不会重复弹确认框。
            let today = ocw_store::local_today(&conn)?;
            let _ = ocw_store::check_in_reservations(&conn, &room_no, &today)?;

            Ok(IssuedDto {
                card,
                room: Some(room),
                expires_at,
            })
        }
    })
    .await
}

// ---------------------------------------------------------------- 部门

#[tauri::command]
pub async fn list_departments(
    state: State<'_, AppState>,
) -> Result<Vec<ocw_store::Department>, CommandError> {
    require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_departments(&conn)?)
    })
    .await
}

#[tauri::command]
pub async fn create_department(
    state: State<'_, AppState>,
    name: String,
    note: Option<String>,
) -> Result<ocw_store::Department, CommandError> {
    let me = require_admin(&state)?;
    let name = require_text(&name, "部门名称", 1, 32)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let d = ocw_store::insert_department(&conn, &name, note.as_deref())?;
        let _ = ocw_store::record_audit(
            &conn,
            "create_department",
            Some(&d.name),
            None,
            "ok",
            Some(me.operator_id),
        );
        Ok(d)
    })
    .await
}

#[tauri::command]
pub async fn rename_department(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let name = require_text(&name, "部门名称", 1, 32)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::rename_department(&conn, id, &name)?;
        let _ = ocw_store::record_audit(
            &conn,
            "rename_department",
            Some(&name),
            Some(&format!("id={id}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_department_active(
    state: State<'_, AppState>,
    id: i64,
    active: bool,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_department_active(&conn, id, active)?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_department_active",
            Some(&format!("id={id}")),
            Some(&format!("active={active}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------- 房型

#[tauri::command]
pub async fn list_room_types(
    state: State<'_, AppState>,
) -> Result<Vec<ocw_store::RoomType>, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_room_types(&conn)?)
    })
    .await
}

#[tauri::command]
pub async fn create_room_type(
    state: State<'_, AppState>,
    name: String,
    sort_order: Option<i64>,
) -> Result<ocw_store::RoomType, CommandError> {
    let me = require_admin(&state)?;
    let name = require_text(&name, "房型名称", 1, 32)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let t = ocw_store::insert_room_type(&conn, &name, sort_order.unwrap_or(0))?;
        let _ = ocw_store::record_audit(
            &conn,
            "create_room_type",
            Some(&t.name),
            None,
            "ok",
            Some(me.operator_id),
        );
        Ok(t)
    })
    .await
}

#[tauri::command]
pub async fn rename_room_type(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let name = require_text(&name, "房型名称", 1, 32)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::rename_room_type(&conn, id, &name)?;
        let _ = ocw_store::record_audit(
            &conn,
            "rename_room_type",
            Some(&name),
            Some(&format!("id={id}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn delete_room_type(state: State<'_, AppState>, id: i64) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::delete_room_type(&conn, id)?;
        let _ = ocw_store::record_audit(
            &conn,
            "delete_room_type",
            Some(&format!("id={id}")),
            None,
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_room_type(
    state: State<'_, AppState>,
    room_no: String,
    room_type_id: Option<i64>,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_room_type(&conn, &room_no, room_type_id)?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_room_type",
            Some(&room_no),
            Some(&format!("roomTypeId={room_type_id:?}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 修改房间参数：最多同时有效的客卡数（`None` = 不限）。
#[tauri::command]
pub async fn set_room_max_cards(
    state: State<'_, AppState>,
    room_no: String,
    max_cards: Option<i64>,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_room_max_cards(&conn, &room_no, max_cards)?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_room_max_cards",
            Some(&room_no),
            Some(&format!("maxCards={max_cards:?}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 房态列表（含颜色/图标，可自定义）。
#[tauri::command]
pub async fn list_room_statuses(
    state: State<'_, AppState>,
) -> Result<Vec<ocw_store::RoomStatus>, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_room_statuses(&conn)?)
    })
    .await
}

/// 改房态的图标/颜色（管理员）。
#[tauri::command]
pub async fn update_room_status_style(
    state: State<'_, AppState>,
    key: String,
    icon: String,
    color: String,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::update_room_status_style(&conn, &key, &icon, &color)?;
        let _ = ocw_store::record_audit(
            &conn,
            "update_room_status_style",
            Some(&key),
            Some(&format!("icon={icon} color={color}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 改某个房间的房态。前台也会用（入住/退房都改房态）。
#[tauri::command]
pub async fn set_room_status(
    state: State<'_, AppState>,
    room_no: String,
    status: String,
) -> Result<(), CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_room_status(&conn, &room_no, &status)?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_room_status",
            Some(&room_no),
            Some(&format!("status={status}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 列出与 `[from, to)` 相交的有效预定（日期均为 `YYYY-MM-DD`）。
#[tauri::command]
pub async fn list_reservations(
    state: State<'_, AppState>,
    from: String,
    to: String,
    room_no: Option<String>,
) -> Result<Vec<ocw_store::Reservation>, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_reservations(
            &conn,
            &from,
            &to,
            room_no.as_deref(),
        )?)
    })
    .await
}

/// 新建预定。`check_out` 是退房日（不含）。
#[tauri::command]
pub async fn create_reservation(
    state: State<'_, AppState>,
    room_no: String,
    check_in: String,
    check_out: String,
    guest_name: Option<String>,
    note: Option<String>,
) -> Result<ocw_store::Reservation, CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let input = ocw_store::ReservationInput {
            room_no,
            check_in,
            check_out,
            guest_name,
            note,
        };
        let r = ocw_store::create_reservation(&conn, &input, Some(me.operator_id))?;
        let _ = ocw_store::record_audit(
            &conn,
            "create_reservation",
            Some(&r.room_no),
            Some(&format!("{}→{}", r.check_in, r.check_out)),
            "ok",
            Some(me.operator_id),
        );
        Ok(r)
    })
    .await
}

/// 改期 / 改预定信息。已取消的预定不能改。
#[tauri::command]
pub async fn update_reservation(
    state: State<'_, AppState>,
    id: i64,
    room_no: String,
    check_in: String,
    check_out: String,
    guest_name: Option<String>,
    note: Option<String>,
) -> Result<ocw_store::Reservation, CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let input = ocw_store::ReservationInput {
            room_no,
            check_in,
            check_out,
            guest_name,
            note,
        };
        let r = ocw_store::update_reservation(&conn, id, &input)?;
        let _ = ocw_store::record_audit(
            &conn,
            "update_reservation",
            Some(&r.room_no),
            Some(&format!("{}→{}", r.check_in, r.check_out)),
            "ok",
            Some(me.operator_id),
        );
        Ok(r)
    })
    .await
}

/// 取消预定（保留记录，重复取消幂等）。
#[tauri::command]
pub async fn cancel_reservation(
    state: State<'_, AppState>,
    id: i64,
) -> Result<ocw_store::Reservation, CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let r = ocw_store::cancel_reservation(&conn, id)?;
        let _ = ocw_store::record_audit(
            &conn,
            "cancel_reservation",
            Some(&r.room_no),
            Some(&format!("{}→{}", r.check_in, r.check_out)),
            "ok",
            Some(me.operator_id),
        );
        Ok(r)
    })
    .await
}

/// 缩小到系统托盘：关窗确认框的「缩小」选项（TD-109）。窗口隐藏，进程继续跑。
#[tauri::command]
pub async fn hide_to_tray(
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), CommandError> {
    require_login(&state)?;
    let _ = window.hide();
    Ok(())
}

/// 真正退出程序（确认框的「退出」与托盘菜单共用），绕过关窗拦截。
#[tauri::command]
pub async fn quit_app(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), CommandError> {
    require_login(&state)?;
    app.exit(0);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutResult {
    pub room_no: String,
    /// 本次被标记为已退房的卡数
    pub cards_closed: i64,
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMovePreview {
    pub room_no: String,
    /// 源房当前有效客卡张数（换房就是把它们全部退掉）
    pub cards: i64,
    /// 源房最晚到期时间（UTC），无有效卡时为 None
    pub active_until: Option<String>,
    /// 源房最近一张有效客卡记录：用来把客人信息、时长带到新房
    pub latest: Option<ocw_store::IssuanceRecord>,
    /// 源房当前所有有效客卡的卡号（去重）。换房前要读卡核对"是不是本房原卡"。
    pub active_card_nos: Vec<u16>,
}

/// 换房第一步：查源房"能带走什么"，**不改任何数据**。
///
/// 前台要先看到「几张有效卡 / 客人是谁 / 几天卡 / 到期时间」再决定换去哪个房间，
/// 所以把预览与执行拆成两个命令；预览里不写库，用户点取消就是零影响。
#[tauri::command]
pub async fn room_move_preview(
    state: State<'_, AppState>,
    room_no: String,
) -> Result<RoomMovePreview, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let room = ocw_store::find_room(&conn, &room_no)?.ok_or_else(|| {
            CommandError::new(1, format!("房号不存在：{room_no}"), "刷新房间列表后重试")
        })?;
        let latest = ocw_store::latest_active_guest_card(&conn, &room_no)?;
        let active_card_nos = ocw_store::active_guest_card_nos(&conn, &room_no)?;
        Ok(RoomMovePreview {
            room_no,
            cards: room.active_cards,
            active_until: room.active_until,
            latest,
            active_card_nos,
        })
    })
    .await
}

/// 换房第二步：把源房的全部有效客卡登记退房，并记审计。
///
/// 只做业务登记 —— 卡内的房间序号是写死在卡里的，旧卡仍能开**源房**的门，
/// 直到它自己过期；新房必须重新写一张卡（前端接着打开目标房的发卡表单）。
///
/// `card_no` 是换房前读到的卡号，只用于审计留痕（证明当时核对的是哪张卡）；
/// 不是本房原卡时前端会二次确认，确认后仍带上实际读到的卡号。
#[tauri::command]
pub async fn room_move_out(
    state: State<'_, AppState>,
    room_no: String,
    to_room_no: String,
    card_no: Option<u16>,
) -> Result<CheckoutResult, CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let target = ocw_store::find_room(&conn, &to_room_no)?.ok_or_else(|| {
            CommandError::new(1, format!("目标房号不存在：{to_room_no}"), "刷新房间列表后重试")
        })?;
        if !target.active {
            return Err(CommandError::new(
                1,
                format!("目标房间已删除：{to_room_no}"),
                "换到其它房间",
            ));
        }
        if target.room_no == room_no {
            return Err(CommandError::new(1, "不能换到同一间房", "选择另一个房号"));
        }

        let dirty = ocw_store::checkout_status(&conn)?;
        let closed = ocw_store::checkout_room(&conn, &room_no, &dirty)? as i64;
        let _ = ocw_store::record_audit(
            &conn,
            "move_room",
            Some(&room_no),
            Some(&format!(
                "to={to_room_no} cardsClosed={closed} readCard={}",
                match card_no {
                    Some(n) => format!("0x{n:04X}"),
                    None => "未读卡".to_string(),
                }
            )),
            "ok",
            Some(me.operator_id),
        );
        Ok(CheckoutResult {
            room_no,
            cards_closed: closed,
            status: dirty,
        })
    })
    .await
}

// ---------------------------------------------------------------- 备份 / 恢复

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub path: String,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    /// 本地时间 `YYYY-MM-DD HH:mm:ss`，取不到时为 None
    pub modified_at: Option<String>,
    /// 恢复前自动留的 `.bak`，界面上单独标注、避免误当正式备份
    pub is_safety_copy: bool,
    /// 自动备份（`auto-` 前缀）：保留份数只对这类生效
    pub is_auto: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// 恢复前自动另存的当前库副本
    pub before: String,
    pub schema_version: i64,
    pub rooms: i64,
    pub issuances: i64,
    pub operators: i64,
}

/// 备份文件放哪：与数据库同级的 `backups\` 目录（实现放在 backup_job，自动备份也用同一处）。
fn backups_dir(db_path: &Path) -> std::path::PathBuf {
    crate::backup_job::backups_dir(db_path)
}

/// 用 SQLite 自己的 `strftime` 取时间，省掉 chrono 依赖（库用的就是本地时区）。
fn local_stamp(conn: &Connection) -> String {
    conn.query_row(
        "SELECT strftime('%Y%m%d-%H%M%S', 'now', 'localtime')",
        [],
        |r| r.get(0),
    )
    .unwrap_or_else(|_| "unknown".to_string())
}

fn file_modified(conn: &Connection, path: &Path) -> Option<String> {
    let secs = std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    conn.query_row(
        "SELECT strftime('%Y-%m-%d %H:%M:%S', ?1, 'unixepoch', 'localtime')",
        [secs],
        |r| r.get(0),
    )
    .ok()
}

/// 立即备份数据库（管理员）。默认写到与库同级的 `backups\ocw-时间戳.db`。
#[tauri::command]
pub async fn backup_database(
    state: State<'_, AppState>,
    dir: Option<String>,
) -> Result<BackupResult, CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    let db_path = state.db_path.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let dir = match dir {
            Some(d) if !d.trim().is_empty() => std::path::PathBuf::from(d.trim()),
            _ => backups_dir(&db_path),
        };
        let target = dir.join(format!("ocw-{}.db", local_stamp(&conn)));
        // 在线备份：WAL 里未 checkpoint 的事务也会一起带走
        let bytes = ocw_store::backup_to(&conn, &target)?;
        // 手动备份同样推进"上次备份时间"：刚备完不该马上再自动备一次
        let now: i64 = conn
            .query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0))
            .map_err(|e| CommandError::internal(format!("读取系统时间失败：{e}")))?;
        let _ = ocw_store::set_setting(&conn, crate::backup_job::SETTING_LAST_AT, &now.to_string());
        let _ = ocw_store::record_audit(
            &conn,
            "backup_database",
            None,
            Some(&format!("path={} bytes={bytes}", target.display())),
            "ok",
            Some(me.operator_id),
        );
        Ok(BackupResult {
            path: target.display().to_string(),
            bytes,
        })
    })
    .await
}

/// 列出已有的备份文件（管理员）：`backups\` 目录 + 库同级的安全副本。
#[tauri::command]
pub async fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupEntry>, CommandError> {
    require_admin(&state)?;
    let db = state.db.clone();
    let db_path = state.db_path.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let mut out: Vec<BackupEntry> = Vec::new();

        let mut dirs = vec![(backups_dir(&db_path), false)];
        if let Some(parent) = db_path.parent() {
            dirs.push((parent.to_path_buf(), true));
        }

        for (dir, safety_only) in dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let is_safety = name.contains(".before-restore-") || name.ends_with(".bak");
                // 数据库自身目录只列安全副本，别把正在用的 db 当备份
                if safety_only && !is_safety {
                    continue;
                }
                if !(is_safety || name.starts_with("ocw-")) {
                    continue;
                }
                let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let is_auto = name.starts_with("auto-");
                out.push(BackupEntry {
                    path: path.display().to_string(),
                    name,
                    bytes,
                    modified_at: file_modified(&conn, &path),
                    is_safety_copy: is_safety,
                    is_auto,
                });
            }
        }

        // 新的排前面；同时间按名字倒序，保证稳定
        out.sort_by(|a, b| {
            b.modified_at
                .cmp(&a.modified_at)
                .then_with(|| b.name.cmp(&a.name))
        });
        out.truncate(100);
        Ok(out)
    })
    .await
}

/// 用备份覆盖当前库（管理员）。
///
/// 顺序刻意收紧：**先只读校验备份**（是不是我们的库、版本是否比本程序新）→
/// 再把当前库另存为 `.before-restore-*.bak` → 最后才覆盖。
/// 任何一步失败都不会动到正在用的数据。
#[tauri::command]
pub async fn restore_database(
    state: State<'_, AppState>,
    path: String,
) -> Result<RestoreResult, CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    let db_path = state.db_path.clone();
    blocking(move || {
        let src = std::path::PathBuf::from(path.trim());
        if !src.is_file() {
            return Err(CommandError::new(
                1,
                format!("备份文件不存在：{}", src.display()),
                "刷新备份列表后重试",
            ));
        }

        // 1) 只读校验：不是我们的库 / 来自更新版本 → 直接拒绝，当前数据不动
        let info = ocw_store::inspect_backup(&src).map_err(|e| {
            CommandError::new(1, format!("备份文件不可用：{e}"), "换一个备份文件")
        })?;
        if info.schema_version > ocw_store::SCHEMA_VERSION {
            return Err(CommandError::new(
                1,
                format!(
                    "备份来自更新的版本（schema v{}），本程序只支持到 v{}",
                    info.schema_version,
                    ocw_store::SCHEMA_VERSION
                ),
                "升级程序后再恢复",
            ));
        }

        let mut conn = lock_db(&db)?;

        // 2) 先把 WAL 落盘，再给当前库留一份安全副本
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| CommandError::internal(format!("WAL 落盘失败：{e}")))?;
        let before = db_path.with_file_name(format!(
            "opencardwright.db.before-restore-{}.bak",
            local_stamp(&conn)
        ));
        ocw_store::backup_to(&conn, &before)?;

        // 3) 覆盖 + 迁移（备份可能来自老 schema）
        ocw_store::restore_from(&mut conn, &src)?;
        ocw_store::migrate(&conn)?;

        let after = ocw_store::inspect_backup(&src)?;
        let _ = ocw_store::record_audit(
            &conn,
            "restore_database",
            None,
            Some(&format!(
                "from={} before={} rooms={} issuances={}",
                src.display(),
                before.display(),
                after.rooms,
                after.issuances
            )),
            "ok",
            Some(me.operator_id),
        );

        Ok(RestoreResult {
            before: before.display().to_string(),
            schema_version: after.schema_version,
            rooms: after.rooms,
            issuances: after.issuances,
            operators: after.operators,
        })
    })
    .await
}

/// 退房：登记退房 + 房态置为空脏房。
///
/// 只做业务登记；要让门锁里的卡立刻失效，还需用退房钟点卡去刷锁。
#[tauri::command]
pub async fn checkout_room(
    state: State<'_, AppState>,
    room_no: String,
) -> Result<CheckoutResult, CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let dirty = ocw_store::checkout_status(&conn)?;
        let closed = ocw_store::checkout_room(&conn, &room_no, &dirty)? as i64;
        let _ = ocw_store::record_audit(
            &conn,
            "checkout_room",
            Some(&room_no),
            Some(&format!("cardsClosed={closed}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(CheckoutResult {
            room_no,
            cards_closed: closed,
            status: dirty,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutCardResult {
    pub room_no: String,
    /// 该房还剩几张有效卡（0 表示房间已转空脏）
    pub remaining: i64,
}

/// 读卡退房 / 持卡退房：按卡号关闭这一张卡。
#[tauri::command]
pub async fn checkout_card(
    state: State<'_, AppState>,
    card_no: u16,
    // 无卡退房（TD-121 的选卡弹窗）：true = 用户手上没有卡，跳过读卡器（不读卡、不擦卡），
    // 只做数据库标记；缺省 = 持卡退房（TD-124：先擦卡后改库）。
    no_device: Option<bool>,
) -> Result<CheckoutCardResult, CommandError> {
    let me = require_login(&state)?;
    let skip_device = no_device.unwrap_or(false);
    let device = state.device();

    // 持卡退房（TD-124）：先擦卡、后改库 —— 卡要在读卡器上、擦卡成功才登记退房。
    // 模拟设备没有真实读写闭环：只做「尽力擦卡」，跳过读卡确认与读回校验。
    if !skip_device {
        if device.is_sim() {
            let _ = device.erase_card();
        } else {
            let on_card = device.read_card()?;
            if on_card.card_no != Some(card_no) {
                let on_card_text = on_card
                    .card_no
                    .map(|n| format!("0x{n:04X}"))
                    .unwrap_or_else(|| "空白 / 无法识别".into());
                return Err(CommandError::new(
                    1,
                    format!("读卡器上的卡号与要退的不一致（卡上 {on_card_text}，要退 0x{card_no:04X}）"),
                    "请确认放在读卡器上的是这张卡；手上没卡请改用无卡退房",
                ));
            }
            // 写零擦卡 → 读回确认已是空白卡
            device.erase_card()?;
            let after = device.read_card()?;
            if !after.kind.is_blank() {
                return Err(CommandError::new(
                    1,
                    "擦卡失败：卡内数据没有写成空白卡",
                    "请把卡重新放正后重试；卡片损坏时可用无卡退房",
                ));
            }
        }
    }

    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let dirty = ocw_store::checkout_status(&conn)?;
        let hit = ocw_store::checkout_card(&conn, card_no, &dirty)?;
        let Some((room_no, remaining)) = hit else {
            return Err(CommandError::new(
                1,
                format!("卡号 0x{card_no:04X} 没有发卡记录"),
                "这张卡可能不是本系统发的，只能用无卡退房按房间处理",
            ));
        };
        let _ = ocw_store::record_audit(
            &conn,
            "checkout_card",
            Some(&room_no),
            Some(&format!(
                "cardNo={card_no} remaining={remaining} erased={}",
                if skip_device { 0 } else { 1 }
            )),
            "ok",
            Some(me.operator_id),
        );
        Ok(CheckoutCardResult { room_no, remaining })
    })
    .await
}

/// 强制退房：卡不是本系统发的（本地没有发卡记录），按卡号找不到房间，
/// 只能按前端从卡片数据里读出的房号，**直接写入退房数据**。
///
/// 语义等同该房间的「无卡退房」：关闭该房全部有效卡 + 房态置为退房默认态，
/// 并在审计里记下是强制退房、来自哪个卡号。调用方必须先让用户二次确认。
#[tauri::command]
pub async fn force_checkout_card(
    state: State<'_, AppState>,
    room_no: String,
    card_no: u16,
) -> Result<CheckoutResult, CommandError> {
    let me = require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let room_no = room_no.trim().to_string();
        if room_no.is_empty() {
            return Err(CommandError::new(
                1,
                "没有确定要退的房间".to_string(),
                "这张卡的数据里没有房号，本机也没有对应序号的房间；请改用右键菜单的「无卡退房」",
            ));
        }
        let conn = lock_db(&db)?;
        let dirty = ocw_store::checkout_status(&conn)?;
        let closed = ocw_store::checkout_room(&conn, &room_no, &dirty)? as i64;
        let _ = ocw_store::record_audit(
            &conn,
            "force_checkout_card",
            Some(&room_no),
            Some(&format!("cardNo={card_no} cardsClosed={closed} foreignCard=true")),
            "ok",
            Some(me.operator_id),
        );
        Ok(CheckoutResult {
            room_no,
            cards_closed: closed,
            status: dirty,
        })
    })
    .await
}

/// 设置房型标签颜色（`None` = 中性灰）。
#[tauri::command]
pub async fn set_room_type_color(
    state: State<'_, AppState>,
    id: i64,
    color: Option<String>,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::update_room_type_color(&conn, id, color.as_deref())?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_room_type_color",
            Some(&format!("id={id}")),
            Some(&format!("color={color:?}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 退房后要转成的房态（设置项，默认空脏房）。
#[tauri::command]
pub async fn get_checkout_status(state: State<'_, AppState>) -> Result<String, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::checkout_status(&conn)?)
    })
    .await
}

#[tauri::command]
pub async fn set_checkout_status(
    state: State<'_, AppState>,
    status: String,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM room_status WHERE key = ?1",
                [&status],
                |r| r.get(0),
            )
            .map_err(ocw_store::StoreError::from)?;
        if exists == 0 {
            return Err(CommandError::new(1, "未知房态", "刷新房态列表后重选"));
        }
        ocw_store::set_setting(&conn, "checkoutStatus", &status)?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_checkout_status",
            Some(&status),
            None,
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------- 用户管理

#[tauri::command]
pub async fn list_operators(
    state: State<'_, AppState>,
) -> Result<Vec<ocw_store::OperatorInfo>, CommandError> {
    require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_operators(&conn)?)
    })
    .await
}

#[tauri::command]
pub async fn create_operator(
    state: State<'_, AppState>,
    login: String,
    display_name: String,
    role: String,
    department_id: Option<i64>,
    password: String,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let role = match role.as_str() {
        "admin" => crate::auth::Role::Admin,
        "frontdesk" => crate::auth::Role::Frontdesk,
        _ => return Err(CommandError::new(1, "角色只能是 admin 或 frontdesk", "")),
    };
    let login = require_login_name(&login)?;
    // 显示名留空就用登录名，省得前台纠结
    let display_name = if display_name.trim().is_empty() {
        login.clone()
    } else {
        require_text(&display_name, "显示名", 1, 32)?
    };
    let hash = crate::auth::hash_password(&password, &login)
        .map_err(|m| CommandError::new(1, m, "初始密码需至少 8 位且含字母和数字"))?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        if let Some(dep_id) = department_id {
            let exists: i64 = conn
                .query_row("SELECT COUNT(*) FROM department WHERE id = ?1", [dep_id], |r| {
                    r.get(0)
                })
                .map_err(ocw_store::StoreError::from)?;
            if exists == 0 {
                return Err(CommandError::new(1, "部门不存在", "刷新部门列表后重选"));
            }
        }
        let op = ocw_store::insert_operator(
            &conn,
            &login,
            &display_name,
            role.as_str(),
            &hash,
            department_id,
        )?;
        let _ = ocw_store::record_audit(
            &conn,
            "create_operator",
            Some(&op.login),
            Some(&format!("role={}", role.as_str())),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn reset_operator_password(
    state: State<'_, AppState>,
    operator_id: i64,
    new_password: String,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    // 密码不能与登录名相同，所以先把目标用户查出来
    let target_login = {
        let conn = lock_db(&state.db)?;
        ocw_store::find_operator_by_id(&conn, operator_id)?
            .ok_or_else(|| CommandError::new(1, "用户不存在", "刷新列表后重试"))?
            .login
    };
    let hash = crate::auth::hash_password(&new_password, &target_login)
        .map_err(|m| CommandError::new(1, m, "新密码需至少 8 位且含字母和数字"))?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::update_operator_password(&conn, operator_id, &hash)?;
        let _ = ocw_store::record_audit(
            &conn,
            "reset_operator_password",
            Some(&format!("id={operator_id}")),
            None,
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_operator_active(
    state: State<'_, AppState>,
    operator_id: i64,
    active: bool,
) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        if !active {
            // 别把最后一个能登录的管理员锁死
            let target = ocw_store::find_operator_by_id(&conn, operator_id)?;
            if let Some(t) = target {
                if t.role == "admin" && t.active && ocw_store::count_active_admins(&conn)? <= 1 {
                    return Err(CommandError::new(
                        1,
                        "这是最后一个可用的管理员，不能停用",
                        "先建一个管理员账号再停用这个",
                    ));
                }
            }
        }
        ocw_store::set_operator_active(&conn, operator_id, active)?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_operator_active",
            Some(&format!("id={operator_id}")),
            Some(&format!("active={active}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

/// 改自己的密码。任何已登录用户都可以，需要验证旧密码。
#[tauri::command]
pub async fn change_own_password(
    state: State<'_, AppState>,
    old_password: String,
    new_password: String,
) -> Result<(), CommandError> {
    let me = require_login(&state)?;
    if !crate::auth::verify_password(
        &old_password,
        &{
            let conn = lock_db(&state.db)?;
            ocw_store::find_operator_by_id(&conn, me.operator_id)?
                .ok_or_else(|| CommandError::new(401, "账号不存在", "请重新登录"))?
                .pwd_hash
        },
    ) {
        return Err(CommandError::new(403, "旧密码不正确", "重新输入，或让管理员重置"));
    }

    let hash = crate::auth::hash_password(&new_password, &me.login)
        .map_err(|m| CommandError::new(1, m, "新密码需至少 8 位且含字母和数字"))?;
    let db = state.db.clone();
    let id = me.operator_id;
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::update_operator_password(&conn, id, &hash)?;
        let _ = ocw_store::record_audit(
            &conn,
            "change_own_password",
            Some(&format!("id={id}")),
            None,
            "ok",
            Some(id),
        );
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------- 日志

/// 前端把 `console.error` / 未捕获异常转发到这里，统一打到控制台。
///
/// 排查时直接读终端日志，不必反复截图 OCR。
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    let lvl = match level.to_ascii_lowercase().as_str() {
        "error" => "ERROR",
        "warn" | "warning" => "WARN",
        _ => "INFO",
    };
    // 截断 + 去换行：前端能把任意文本写进来，不设限等于开了个日志注入口
    let safe: String = message.chars().take(2000).collect();
    let safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::logging::line(lvl, "frontend", &safe);
    eprintln!("[frontend][{lvl}] {safe}");
}

// ---------------------------------------------------------------- 到期提醒

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderSettings {
    pub enabled: bool,
    /// 提前多少分钟开始提醒
    pub minutes: i64,
    pub sound: bool,
    /// 语音播报（系统 TTS）。旧调用方不传时保持库里现值，不重置。
    pub voice: bool,
}

/// 退房钟点（TD-126）：日租卡「预计退房 / 写库到期」用的钟点（0–23），默认 12。
/// 读 = 登录即可（前台发卡页要显示），写 = 管理员（配置是管理行为）。
#[tauri::command]
pub async fn get_checkout_hour(state: State<'_, AppState>) -> Result<u32, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let hour = ocw_store::get_setting(&conn, "checkoutHour")?
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v <= 23)
            .unwrap_or(12);
        Ok(hour)
    })
    .await
}

#[tauri::command]
pub async fn set_checkout_hour(state: State<'_, AppState>, hour: u32) -> Result<u32, CommandError> {
    let me = require_admin(&state)?;
    if hour > 23 {
        return Err(CommandError::new(1, "退房钟点必须在 0–23", ""));
    }
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_setting(&conn, "checkoutHour", &hour.to_string())?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_checkout_hour",
            None,
            Some(&format!("hour={hour}")),
            "ok",
            Some(me.operator_id),
        );
        Ok(hour)
    })
    .await
}

#[tauri::command]
pub async fn get_reminder_settings(
    state: State<'_, AppState>,
) -> Result<ReminderSettings, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let enabled = ocw_store::get_setting(&conn, "reminderEnabled")?
            .map(|v| v != "0")
            .unwrap_or(true);
        let minutes = ocw_store::get_setting(&conn, "reminderMinutes")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(30)
            .clamp(1, 1440);
        let sound = ocw_store::get_setting(&conn, "reminderSound")?
            .map(|v| v != "0")
            .unwrap_or(true);
        let voice = ocw_store::get_setting(&conn, "reminderVoice")?
            .map(|v| v != "0")
            .unwrap_or(true);
        Ok(ReminderSettings {
            enabled,
            minutes,
            sound,
            voice,
        })
    })
    .await
}

#[tauri::command]
pub async fn set_reminder_settings(
    state: State<'_, AppState>,
    enabled: bool,
    minutes: i64,
    sound: bool,
    // 语音开关：None = 保持库里现值（旧前端三个参数调用时不会把用户关掉的语音又打开）
    voice: Option<bool>,
) -> Result<ReminderSettings, CommandError> {
    let me = require_admin(&state)?;
    if !(1..=1440).contains(&minutes) {
        return Err(CommandError::new(1, "提前分钟数需在 1–1440 之间", ""));
    }
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_setting(&conn, "reminderEnabled", if enabled { "1" } else { "0" })?;
        ocw_store::set_setting(&conn, "reminderMinutes", &minutes.to_string())?;
        ocw_store::set_setting(&conn, "reminderSound", if sound { "1" } else { "0" })?;
        let voice = match voice {
            Some(v) => v,
            None => ocw_store::get_setting(&conn, "reminderVoice")?
                .map(|v| v != "0")
                .unwrap_or(true),
        };
        ocw_store::set_setting(&conn, "reminderVoice", if voice { "1" } else { "0" })?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_reminder_settings",
            None,
            Some(&format!(
                "enabled={enabled} minutes={minutes} sound={sound} voice={voice}"
            )),
            "ok",
            Some(me.operator_id),
        );
        Ok(ReminderSettings {
            enabled,
            minutes,
            sound,
            voice,
        })
    })
    .await
}

#[tauri::command]
pub async fn expiring_cards(
    state: State<'_, AppState>,
    within_minutes: i64,
) -> Result<Vec<ocw_store::ExpiringCard>, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::expiring_cards(&conn, within_minutes)?)
    })
    .await
}

// ---------------------------------------------------------------- 账号与鉴权

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    /// 是否已经初始化过（存在任意操作员）
    pub initialized: bool,
    pub logged_in: bool,
    pub login: Option<String>,
    pub display_name: Option<String>,
    pub role: Option<crate::auth::Role>,
    pub department_name: Option<String>,
    pub min_password_len: usize,
}

fn build_auth_status(state: &AppState) -> Result<AuthStatus, CommandError> {
    let initialized = {
        let conn = lock_db(&state.db)?;
        ocw_store::count_operators(&conn)? > 0
    };
    let s = state.session();
    Ok(AuthStatus {
        initialized,
        logged_in: s.is_some(),
        login: s.as_ref().map(|x| x.login.clone()),
        display_name: s.as_ref().map(|x| x.display_name.clone()),
        role: s.as_ref().map(|x| x.role),
        department_name: s.as_ref().and_then(|x| x.department_name.clone()),
        min_password_len: crate::auth::MIN_PASSWORD_LEN,
    })
}

fn require_login(state: &AppState) -> Result<crate::auth::Session, CommandError> {
    state
        .session()
        .ok_or_else(|| CommandError::new(401, "未登录", "请先登录再操作"))
}

fn require_admin(state: &AppState) -> Result<crate::auth::Session, CommandError> {
    match state.session() {
        Some(s) if s.role == crate::auth::Role::Admin => Ok(s),
        Some(s) => Err(CommandError::new(
            403,
            format!("账号 {} 没有管理员权限", s.login),
            "管理卡、房间、设备设置需要管理员账号",
        )),
        None => Err(CommandError::new(401, "未登录", "请先登录再操作")),
    }
}

#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> Result<AuthStatus, CommandError> {
    build_auth_status(&state)
}

/// 首次启动：创建超管账号 `admin`。没有默认口令，必须由人当场设置。
#[tauri::command]
pub async fn initialize_admin(
    state: State<'_, AppState>,
    display_name: Option<String>,
    password: String,
    confirm_password: String,
) -> Result<AuthStatus, CommandError> {
    if password != confirm_password {
        return Err(CommandError::new(1, "两次输入的密码不一致", "请重新输入"));
    }
    let hash = crate::auth::hash_password(&password, crate::auth::SUPER_ADMIN_LOGIN)
        .map_err(|m| CommandError::new(1, m, "密码需至少 8 位，且同时包含字母和数字"))?;

    let op = {
        let conn = lock_db(&state.db)?;
        if ocw_store::count_operators(&conn)? > 0 {
            return Err(CommandError::new(
                409,
                "已经初始化过了，请直接登录",
                "确实要重来请先备份并删除数据目录下的 opencardwright.db",
            ));
        }
        let name = display_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "管理员".to_string());
        let op = ocw_store::insert_operator(
            &conn,
            crate::auth::SUPER_ADMIN_LOGIN,
            &name,
            crate::auth::Role::Admin.as_str(),
            &hash,
            None,
        )?;
        let _ = ocw_store::record_audit(&conn, "initialize_admin", Some(&op.login), None, "ok", Some(op.id));
        op
    };

    state.set_session(Some(crate::auth::Session {
        operator_id: op.id,
        login: op.login.clone(),
        display_name: op.display_name.clone(),
        role: crate::auth::Role::Admin,
        department_name: op.department_name.clone(),
    }));

    build_auth_status(&state)
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    login: String,
    password: String,
) -> Result<AuthStatus, CommandError> {
    let login = login.trim().to_string();
    if login.is_empty() {
        return Err(CommandError::new(1, "请输入账号", "默认管理员账号是 admin"));
    }

    let found = {
        let conn = lock_db(&state.db)?;
        ocw_store::find_operator(&conn, &login)?
    };

    // 账号不存在与密码错误给同一句提示，避免探测账号
    let op = match found {
        Some(op) if op.active && crate::auth::verify_password(&password, &op.pwd_hash) => op,
        _ => {
            return Err(CommandError::new(401, "账号或密码错误", "请确认大小写，或联系管理员重置"));
        }
    };

    let role = crate::auth::Role::parse(&op.role).unwrap_or(crate::auth::Role::Frontdesk);
    {
        let conn = lock_db(&state.db)?;
        let _ = ocw_store::record_audit(&conn, "login", Some(&op.login), None, "ok", Some(op.id));
    }

    state.set_session(Some(crate::auth::Session {
        operator_id: op.id,
        login: op.login.clone(),
        display_name: op.display_name.clone(),
        role,
        department_name: op.department_name.clone(),
    }));

    build_auth_status(&state)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<AuthStatus, CommandError> {
    state.set_session(None);
    build_auth_status(&state)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettingsDto {
    /// 自动备份间隔（分钟），0 = 关闭
    pub interval_minutes: i64,
    /// 自动备份保留份数（只对 auto-*.db 生效）
    pub keep: i64,
    /// 上次成功备份的本地时间（手动/自动都算），没有则 None
    pub last_at: Option<String>,
    /// 备份目录
    pub dir: String,
}

/// 读自动备份设置（管理员）。
#[tauri::command]
pub async fn get_backup_settings(
    state: State<'_, AppState>,
) -> Result<BackupSettingsDto, CommandError> {
    require_admin(&state)?;
    let db = state.db.clone();
    let db_path = state.db_path.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let interval = ocw_store::get_setting(&conn, crate::backup_job::SETTING_INTERVAL)?
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(crate::backup_job::DEFAULT_INTERVAL_MINUTES);
        let keep = ocw_store::get_setting(&conn, crate::backup_job::SETTING_KEEP)?
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(crate::backup_job::DEFAULT_KEEP);
        let last_at = ocw_store::get_setting(&conn, crate::backup_job::SETTING_LAST_AT)?
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|secs| *secs > 0)
            .and_then(|secs| {
                conn.query_row(
                    "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch', 'localtime')",
                    [secs],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            });
        Ok(BackupSettingsDto {
            interval_minutes: interval,
            keep,
            last_at,
            dir: backups_dir(&db_path).display().to_string(),
        })
    })
    .await
}

/// 保存自动备份设置（管理员）。间隔 0 = 关闭；保留份数只影响 `auto-*.db`。
#[tauri::command]
pub async fn set_backup_settings(
    state: State<'_, AppState>,
    interval_minutes: i64,
    keep: i64,
) -> Result<BackupSettingsDto, CommandError> {
    let me = require_admin(&state)?;
    if interval_minutes != 0 && !(5..=1440).contains(&interval_minutes) {
        return Err(CommandError::new(
            1,
            "自动备份间隔必须在 5 分钟到 24 小时之间（0 表示关闭）",
            "默认 60 分钟",
        ));
    }
    if !(1..=500).contains(&keep) {
        return Err(CommandError::new(1, "保留份数必须在 1–500 之间", "默认 24 份"));
    }
    let db = state.db.clone();
    let db_path = state.db_path.clone();
    let interval = interval_minutes;
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_setting(
            &conn,
            crate::backup_job::SETTING_INTERVAL,
            &interval.to_string(),
        )?;
        ocw_store::set_setting(&conn, crate::backup_job::SETTING_KEEP, &keep.to_string())?;
        let _ = ocw_store::record_audit(
            &conn,
            "set_backup_settings",
            None,
            Some(&format!("intervalMinutes={interval} keep={keep}")),
            "ok",
            Some(me.operator_id),
        );
        // 后台线程每 30 秒读一次设置，这里不需要通知
        let last_at = ocw_store::get_setting(&conn, crate::backup_job::SETTING_LAST_AT)?
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|secs| *secs > 0)
            .and_then(|secs| {
                conn.query_row(
                    "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch', 'localtime')",
                    [secs],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            });
        Ok(BackupSettingsDto {
            interval_minutes: interval,
            keep,
            last_at,
            dir: backups_dir(&db_path).display().to_string(),
        })
    })
    .await
}

/// 导入外部备份文件（管理员）。
///
/// 前端用 `<input type="file">` 读到字节后走这里：本机没有文件选择插件，
/// 这样既能选任意路径，又不用为一个功能引入 dialog 插件。
/// 落盘前先看文件头，落盘后再用 `inspect_backup` 确认是**我们自己的库**，
/// 不合格的文件会立刻删掉，不会留在备份列表里误导人。
#[tauri::command]
pub async fn import_backup_file(
    state: State<'_, AppState>,
    name: String,
    bytes: Vec<u8>,
) -> Result<BackupEntry, CommandError> {
    const MAX_BYTES: usize = 256 * 1024 * 1024;
    let me = require_admin(&state)?;
    if bytes.is_empty() {
        return Err(CommandError::new(1, "文件是空的", "换一个备份文件"));
    }
    if bytes.len() > MAX_BYTES {
        return Err(CommandError::new(
            1,
            format!("文件过大（{} MB），上限 256 MB", bytes.len() / 1024 / 1024),
            "备份文件通常只有几 MB，确认选对了文件",
        ));
    }
    if !bytes.starts_with(b"SQLite format 3\0") {
        return Err(CommandError::new(
            1,
            "这不是 SQLite 数据库文件",
            "备份文件应是 .db（OpenCardWright 的备份）",
        ));
    }

    let db = state.db.clone();
    let db_path = state.db_path.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let dir = backups_dir(&db_path);
        std::fs::create_dir_all(&dir)
            .map_err(|e| CommandError::internal(format!("创建备份目录失败：{e}")))?;

        // 文件名只留安全字符，避免路径穿越（"../x.db" 之类）
        let safe: String = name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("imported.db")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
            .collect();
        let stamp = local_stamp(&conn);
        let file_name = if safe.to_lowercase().ends_with(".db") {
            format!("imported-{stamp}-{safe}")
        } else {
            format!("imported-{stamp}-{safe}.db")
        };
        let target = dir.join(&file_name);
        std::fs::write(&target, &bytes)
            .map_err(|e| CommandError::internal(format!("写入导入文件失败：{e}")))?;

        // 落盘后立刻验：不是我们的库就删掉，别让它在列表里冒充可用备份
        if let Err(e) = ocw_store::inspect_backup(&target) {
            let _ = std::fs::remove_file(&target);
            return Err(CommandError::new(
                1,
                format!("导入失败：{e}"),
                "请选择 OpenCardWright 生成的 .db 备份",
            ));
        }

        let bytes_len = std::fs::metadata(&target)
            .map_err(|e| CommandError::internal(format!("读取导入文件失败：{e}")))?
            .len();
        let _ = ocw_store::record_audit(
            &conn,
            "import_backup_file",
            None,
            Some(&format!("path={} bytes={bytes_len}", target.display())),
            "ok",
            Some(me.operator_id),
        );
        Ok(BackupEntry {
            path: target.display().to_string(),
            name: file_name,
            bytes: bytes_len,
            modified_at: file_modified(&conn, &target),
            is_safety_copy: false,
            is_auto: false,
        })
    })
    .await
}

// ---------------------------------------------------------------- 诊断包

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResult {
    pub path: String,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

/// 导出诊断包（管理员）：清单 + 脱敏库副本 + 运行状态 + 日志尾，打包成 zip。
///
/// 没有读卡器也能导出（不碰串口）；库副本是在线备份，导出期间正常发卡不受影响。
#[tauri::command]
pub async fn export_diagnostics(
    state: State<'_, AppState>,
) -> Result<DiagnosticsResult, CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    let db_path = state.db_path.clone();
    let runtime = state.runtime_dir.clone();
    let driver = state.driver_sha256.clone();
    let sdk_source = state.sdk_source.clone();
    let port = state.port();
    let sim = state.device().is_sim();
    blocking(move || {
        // 先查计数再把锁放开 —— 诊断模块自己会加锁做在线备份，Mutex 不可重入
        let (schema, rooms, issuances, operators) = {
            let conn = lock_db(&db)?;
            let schema: i64 = conn
                .query_row("SELECT COALESCE(MAX(version),0) FROM schema_version", [], |r| r.get(0))
                .unwrap_or(0);
            let count = |t: &str| -> i64 {
                conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                    .unwrap_or(0)
            };
            (schema, count("room"), count("issuance"), count("operator"))
        };

        let meta = serde_json::json!({
            "app": "OpenCardWright",
            "version": env!("CARGO_PKG_VERSION"),
            "schemaVersion": schema,
            "dbPath": db_path.display().to_string(),
            "runtimeDir": runtime.display().to_string(),
            "driverSha256": driver,
            "sdkSource": sdk_source,
            "simulation": sim,
            "port": port,
            "os": std::env::var("OS").unwrap_or_default(),
            "counts": { "rooms": rooms, "issuances": issuances, "operators": operators },
        });

        let res = crate::diagnostics::export(&db, &db_path, &runtime, meta)
            .map_err(|m| CommandError::new(1, m, "把诊断包路径发给开发者"))?;

        if let Ok(conn) = db.lock() {
            let _ = ocw_store::record_audit(
                &conn,
                "export_diagnostics",
                None,
                Some(&format!("path={} warnings={}", res.path.display(), res.warnings.len())),
                "ok",
                Some(me.operator_id),
            );
        }
        Ok(DiagnosticsResult {
            path: res.path.display().to_string(),
            bytes: res.bytes,
            warnings: res.warnings,
        })
    })
    .await
}

// ---------------------------------------------------------------- 发卡记录 / 操作审计

type IssuancePage = ocw_store::Page<ocw_store::IssuanceRow>;
type AuditPage = ocw_store::Page<ocw_store::AuditRow>;

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_issuances(
    state: State<'_, AppState>,
    room_no: Option<String>,
    card_no: Option<u16>,
    kind: Option<String>,
    from: Option<String>,
    to: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
    // 入住记录视图（TD-118）：true 时只返回填了身份信息的发卡
    guest_only: Option<bool>,
) -> Result<IssuancePage, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let filter = ocw_store::IssuanceFilter {
            room_no,
            card_no,
            kind,
            from,
            to,
            page: page.unwrap_or(1),
            page_size: page_size.unwrap_or(50),
            guest_only: guest_only.unwrap_or(false),
        };
        Ok(ocw_store::list_issuances(&conn, &filter)?)
    })
    .await
}

#[tauri::command]
pub async fn list_audits(
    state: State<'_, AppState>,
    action: Option<String>,
    from: Option<String>,
    to: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
) -> Result<AuditPage, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::list_audits(
            &conn,
            action.as_deref(),
            from.as_deref(),
            to.as_deref(),
            page.unwrap_or(1),
            page_size.unwrap_or(50),
        )?)
    })
    .await
}

#[tauri::command]
pub async fn card_history(
    state: State<'_, AppState>,
    card_no: u16,
) -> Result<Vec<ocw_store::IssuanceRow>, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        Ok(ocw_store::card_history(&conn, card_no, 200)?)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvResult {
    pub path: String,
    pub rows: i64,
    pub bytes: u64,
}

/// CSV 单元格转义：含逗号/引号/换行就加引号并把内部引号翻倍；
/// 以 `= + - @` 开头的前置单引号，避免 Excel 把内容当公式执行。
fn csv_cell(value: &str) -> String {
    let needs_prefix = value.starts_with(['=', '+', '-', '@']);
    let escaped = value.replace('"', "\"\"");
    let cell = if escaped.contains([',', '"', '\n', '\r']) {
        format!("\"{escaped}\"")
    } else {
        escaped
    };
    if needs_prefix {
        format!("'{cell}")
    } else {
        cell
    }
}

/// 导出 CSV 文本（纯函数，便于单测）：UTF-8 BOM + CRLF，Excel 双击不乱码。
pub fn issuances_to_csv(rows: &[ocw_store::IssuanceRow]) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str("记录ID,发卡时间,房号,卡型,天数,小时,卡号,客人,证件类型,证件号码,操作员,结果,到期时间,退房时间,耗时ms,原始数据\r\n");
    for r in rows {
        let cells = [
            r.id.to_string(),
            r.created_at.clone(),
            r.room_no.clone().unwrap_or_default(),
            r.kind.clone(),
            r.days.to_string(),
            r.hours.to_string(),
            r.card_no.map(|n| format!("0x{n:04X}")).unwrap_or_default(),
            r.guest_name.clone().unwrap_or_default(),
            r.guest_id_type.clone().unwrap_or_default(),
            r.guest_id_number.clone().unwrap_or_default(),
            r.operator_name.clone().unwrap_or_default(),
            r.result.clone(),
            r.expires_at.clone().unwrap_or_default(),
            r.checked_out_at.clone().unwrap_or_default(),
            r.duration_ms.map(|v| v.to_string()).unwrap_or_default(),
            r.raw_hex.clone().unwrap_or_default(),
        ];
        let line: Vec<String> = cells.iter().map(|c| csv_cell(c)).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

/// 导出目录：桌面 → OneDrive 桌面 → 下载 → 数据目录 exports（取第一个存在的）。
fn export_dir() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    if !home.is_empty() {
        let desktop = std::path::PathBuf::from(&home).join("Desktop");
        if desktop.is_dir() {
            return desktop;
        }
        if let Ok(one) = std::env::var("OneDrive") {
            let d = std::path::PathBuf::from(one).join("Desktop");
            if d.is_dir() {
                return d;
            }
        }
        let downloads = std::path::PathBuf::from(&home).join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
    }
    crate::provision::data_root().join("exports")
}

/// 导出当前筛选结果为 CSV（管理员）。上限 5 万行：超了让用户缩范围，不静默截断。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_issuances_csv(
    state: State<'_, AppState>,
    room_no: Option<String>,
    card_no: Option<u16>,
    kind: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<CsvResult, CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let filter = ocw_store::IssuanceFilter {
            room_no,
            card_no,
            kind,
            from,
            to,
            page: 1,
            page_size: 50_000,
            guest_only: false,
        };
        let page = ocw_store::list_issuances(&conn, &filter)?;
        if page.rows.len() >= 50_000 {
            return Err(CommandError::new(
                1,
                "结果超过 5 万行，请缩小日期范围再导出",
                "按月份分批导出",
            ));
        }
        let csv = issuances_to_csv(&page.rows);
        let dir = export_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| CommandError::internal(format!("创建导出目录失败：{e}")))?;
        let name = format!("发卡记录-{}.csv", local_stamp(&conn));
        let path = dir.join(name);
        std::fs::write(&path, csv.as_bytes())
            .map_err(|e| CommandError::internal(format!("写 CSV 失败：{e}")))?;
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let _ = ocw_store::record_audit(
            &conn,
            "export_issuances_csv",
            None,
            Some(&format!("rows={} path={}", page.rows.len(), path.display())),
            "ok",
            Some(me.operator_id),
        );
        Ok(CsvResult {
            path: path.display().to_string(),
            rows: page.rows.len() as i64,
            bytes,
        })
    })
    .await
}

// ---------------------------------------------------------------- 房间导入

/// 导入房间（管理员）：前端读文件字节传进来（本机没有文件选择插件）。
#[tauri::command]
pub async fn import_rooms(
    state: State<'_, AppState>,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<crate::room_import::ImportReport, CommandError> {
    const MAX_BYTES: usize = 2 * 1024 * 1024;
    let me = require_admin(&state)?;
    if bytes.is_empty() {
        return Err(CommandError::new(1, "文件是空的", "选择 CSV 或 ini 文件"));
    }
    if bytes.len() > MAX_BYTES {
        return Err(CommandError::new(
            1,
            "文件过大（房间表通常只有几 KB）",
            "确认选对了文件",
        ));
    }
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let report = crate::room_import::import_bytes(&conn, &file_name, &bytes)
            .map_err(|m| CommandError::new(1, m, "确认文件是 CSV（房号,序号,房型,楼层）或 ini（房号=序号）"))?;
        let _ = ocw_store::record_audit(
            &conn,
            "import_rooms",
            None,
            Some(&format!(
                "added={} revived={} skipped={} failed={}",
                report.added,
                report.revived,
                report.skipped,
                report.failed.len()
            )),
            "ok",
            Some(me.operator_id),
        );
        Ok(report)
    })
    .await
}

// ---------------------------------------------------------------- 首启向导

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    pub done: bool,
    pub rooms: i64,
}

/// 是否需要跑首启向导。
///
/// 判定带兼容分支：没有 `onboardingDone` 这个 setting 但库里已有房间 → 视为已完成，
/// 免得升级用户一开机就被向导拦住。
#[tauri::command]
pub async fn get_onboarding_status(
    state: State<'_, AppState>,
) -> Result<OnboardingStatus, CommandError> {
    require_login(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        let flag = ocw_store::get_setting(&conn, "onboardingDone")?;
        let rooms = ocw_store::list_rooms(&conn)?.len() as i64;
        let done = match flag.as_deref() {
            Some(v) => v == "1",
            None => rooms > 0,
        };
        Ok(OnboardingStatus { done, rooms })
    })
    .await
}

#[tauri::command]
pub async fn complete_onboarding(state: State<'_, AppState>) -> Result<(), CommandError> {
    let me = require_admin(&state)?;
    let db = state.db.clone();
    blocking(move || {
        let conn = lock_db(&db)?;
        ocw_store::set_setting(&conn, "onboardingDone", "1")?;
        let _ = ocw_store::record_audit(
            &conn,
            "complete_onboarding",
            None,
            None,
            "ok",
            Some(me.operator_id),
        );
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Role, Session};
    use crate::state::{AppState, Device, DeviceSlot};
    use std::sync::{Arc, Mutex};
    use super::AdminCardRequest;

    fn args(req: AdminCardRequest) -> Vec<String> {
        req.to_args().expect("参数应合法")
    }

    #[test]
    fn master_defaults_to_one_day() {
        let a = args(AdminCardRequest::Master {
            days: None,
            hours: None,
        });
        assert_eq!(a, vec!["--type", "master", "--days", "1"]);
    }

    #[test]
    fn master_prefers_hours_when_given() {
        let a = args(AdminCardRequest::Master {
            days: Some(3),
            hours: Some(2),
        });
        assert_eq!(a, vec!["--type", "master", "--hours", "2"]);
    }

    #[test]
    fn install_carries_room() {
        let a = args(AdminCardRequest::Install {
            room_no: " 220 ".into(),
        });
        assert_eq!(a, vec!["--type", "install", "--room", "220"]);
    }

    #[test]
    fn timesync_without_time_omits_room() {
        let a = args(AdminCardRequest::TimeSync {
            hour: None,
            minute: None,
        });
        assert_eq!(a, vec!["--type", "timesync"]);
    }

    #[test]
    fn timesync_with_time_uses_hh_mm_quirk() {
        let a = args(AdminCardRequest::TimeSync {
            hour: Some(9),
            minute: Some(5),
        });
        assert_eq!(a, vec!["--type", "timesync", "--room", "09:05"]);
    }

    #[test]
    fn sim_erase_card_returns_blank_snapshot() {
        // 模拟设备没有真实写入闭环：擦卡直接回放空白卡快照（真实设备写 28 字节全零）
        let dev = Device::Sim(ocw_device::SimDevice::default());
        let card = dev.erase_card().expect("模拟擦卡应成功");
        assert!(card.kind.is_blank(), "擦卡结果必须是空白卡");
        assert!(card.card_no.is_none(), "空白卡不含卡号");
    }

    #[test]
    fn checkout_and_raw_are_formatted() {
        assert_eq!(
            args(AdminCardRequest::Checkout { hour: 12 }),
            vec!["--type", "checkout", "--hour", "12"]
        );
        assert_eq!(
            args(AdminCardRequest::Raw {
                p1: "5bec492b".into(),
                p2: "13bf22ab".into()
            }),
            vec!["--type", "raw", "--p1", "5BEC492B", "--p2", "13BF22AB"]
        );
    }

    #[test]
    fn invalid_inputs_are_rejected_before_touching_device() {        assert!(AdminCardRequest::Checkout { hour: 24 }.to_args().is_err());
        assert!(AdminCardRequest::Raw {
            p1: "xyz".into(),
            p2: "13BF22AB".into()
        }
        .to_args()
        .is_err());
        assert!(AdminCardRequest::Master {
            days: None,
            hours: Some(0)
        }
        .to_args()
        .is_err());
    }

    /// 前端发的是 `{kind:"install", roomNo:"220"}`。`rename_all` 只改变体名，
    /// 变体字段要 filename_fields，否则这里会反序列化失败、安装卡永远发不出去。
    #[test]
    fn admin_request_accepts_camel_case_fields() {
        let req: AdminCardRequest =
            serde_json::from_str(r#"{"kind":"install","roomNo":" 220 "}"#).unwrap();
        match req {
            AdminCardRequest::Install { room_no } => assert_eq!(room_no.trim(), "220"),
            other => panic!("应解析为 Install，实际 {other:?}"),
        }

        let req: AdminCardRequest = serde_json::from_str(r#"{"kind":"master","days":63}"#).unwrap();
        assert_eq!(args(req), vec!["--type", "master", "--days", "63"]);
    }
    // ---------------- 角色鉴权：命令 × 角色矩阵 ----------------

    /// 命令应具备的校验级别。
    #[derive(PartialEq, Eq, Debug, Clone, Copy)]
    enum Level {
        Admin,
        Login,
        Public,
    }

    /// 全量对照表。**新增命令必须在这里登记**，否则矩阵测试会报「未登记」。
    /// 三档数量：35 / 28 / 6（改动数量时同步改 `auth_matrix_counts`）。
    const MATRIX: &[(&str, Level)] = &[
        // 公开（启动即调用 / 认证入口）
        ("app_info", Level::Public),
        ("auth_status", Level::Public),
        ("initialize_admin", Level::Public),
        ("login", Level::Public),
        ("logout", Level::Public),
        ("log_frontend", Level::Public),
        // 登录即可（前台日常操作 + 只读查询）
        ("read_card", Level::Login),
        ("list_rooms", Level::Login),
        ("run_selftest", Level::Login),
        ("probe_ports", Level::Login),
        ("card_no_status", Level::Login),
        ("issue_guest", Level::Login),
        ("list_room_types", Level::Login),
        ("list_room_statuses", Level::Login),
        ("set_room_status", Level::Login),
        ("room_move_preview", Level::Login),
        ("room_move_out", Level::Login),
        ("checkout_room", Level::Login),
        ("checkout_card", Level::Login),
        ("force_checkout_card", Level::Login),
        ("get_checkout_status", Level::Login),
        ("change_own_password", Level::Login),
        ("get_reminder_settings", Level::Login),
        ("get_checkout_hour", Level::Login),
        ("expiring_cards", Level::Login),
        ("list_issuances", Level::Login),
        ("list_audits", Level::Login),
        ("card_history", Level::Login),
        ("get_onboarding_status", Level::Login),
        ("list_reservations", Level::Login),
        ("create_reservation", Level::Login),
        ("update_reservation", Level::Login),
        ("cancel_reservation", Level::Login),
        ("hide_to_tray", Level::Login),
        ("quit_app", Level::Login),
        // 管理员
        ("set_port", Level::Admin),
        ("create_room", Level::Admin),
        ("delete_room", Level::Admin),
        ("rename_room", Level::Admin),
        ("issue_install_card", Level::Admin),
        ("issue_admin_card", Level::Admin),
        ("set_water_warn_at", Level::Admin),
        ("reset_water_mark", Level::Admin),
        ("list_departments", Level::Admin),
        ("create_department", Level::Admin),
        ("rename_department", Level::Admin),
        ("set_department_active", Level::Admin),
        ("create_room_type", Level::Admin),
        ("rename_room_type", Level::Admin),
        ("delete_room_type", Level::Admin),
        ("set_room_type", Level::Admin),
        ("set_room_max_cards", Level::Admin),
        ("update_room_status_style", Level::Admin),
        ("backup_database", Level::Admin),
        ("list_backups", Level::Admin),
        ("restore_database", Level::Admin),
        ("set_room_type_color", Level::Admin),
        ("set_checkout_status", Level::Admin),
        ("list_operators", Level::Admin),
        ("create_operator", Level::Admin),
        ("reset_operator_password", Level::Admin),
        ("set_operator_active", Level::Admin),
        ("set_reminder_settings", Level::Admin),
        ("set_checkout_hour", Level::Admin),
        ("get_backup_settings", Level::Admin),
        ("set_backup_settings", Level::Admin),
        ("import_backup_file", Level::Admin),
        ("export_diagnostics", Level::Admin),
        ("export_issuances_csv", Level::Admin),
        ("import_rooms", Level::Admin),
        ("complete_onboarding", Level::Admin),
    ];

    /// 从源码里按 `#[tauri::command]` 切出每个命令的函数名与函数体（到顶格 `}` 为止）。
    fn command_bodies() -> Vec<(String, String)> {
        let src = include_str!("commands.rs");
        let head = &src[..src.find("#[cfg(test)]").expect("找不到测试模块")];
        let mut out = Vec::new();
        for chunk in head.split("#[tauri::command]").skip(1) {
            let Some(fn_at) = chunk.find("fn ") else {
                continue;
            };
            let after = &chunk[fn_at + 3..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            // 函数体到下一个顶格 "}"（命令都是顶层函数，内部闭包都带缩进）
            let body = match chunk.find("\n}\n") {
                Some(end) => &chunk[..end],
                None => chunk,
            };
            out.push((name, body.to_string()));
        }
        out
    }

    /// 矩阵必须覆盖全部命令，且每个命令的校验级别与代码一致。
    ///
    /// 这条测试的价值在于「删掉某个 require_admin 会立刻变红」——
    /// 比人工逐条 review 可靠，也让新加的命令无处可逃。
    #[test]
    fn auth_matrix_matches_every_command() {
        let bodies = command_bodies();
        assert!(!bodies.is_empty(), "没切出任何命令，正则/切片逻辑坏了");

        let mut problems = Vec::new();
        for (name, body) in &bodies {
            let expected = MATRIX.iter().find(|(n, _)| n == name).map(|(_, l)| *l);
            let has_admin = body.contains("require_admin");
            let has_login = body.contains("require_login");
            match expected {
                None => problems.push(format!("{name}: 未在 MATRIX 登记")),
                Some(Level::Admin) => {
                    if !has_admin {
                        problems.push(format!("{name}: 应 require_admin，实际没有"));
                    }
                }
                Some(Level::Login) => {
                    if !has_login && !has_admin {
                        problems.push(format!("{name}: 应 require_login/require_admin，实际没有"));
                    }
                }
                Some(Level::Public) => {
                    if has_admin || has_login {
                        problems.push(format!("{name}: 标为公开却有校验（表需要更新）"));
                    }
                }
            }
        }
        // 反向：表里登记了但源码里没有（改了名字没改表）
        for (name, _) in MATRIX {
            if !bodies.iter().any(|(n, _)| n == name) {
                problems.push(format!("{name}: 表里有、源码里找不到"));
            }
        }
        assert!(problems.is_empty(), "鉴权矩阵不一致：\n{}", problems.join("\n"));
    }

    #[test]
    fn auth_matrix_counts() {
        let admin = MATRIX.iter().filter(|(_, l)| *l == Level::Admin).count();
        let login = MATRIX.iter().filter(|(_, l)| *l == Level::Login).count();
        let public = MATRIX.iter().filter(|(_, l)| *l == Level::Public).count();
        assert_eq!((admin, login, public), (36, 29, 6));
        assert_eq!(command_bodies().len(), 71, "命令数量变了，矩阵要同步");
    }

    fn test_state(role: Option<Role>) -> AppState {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ocw-auth-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("t.db");
        let conn = ocw_store::open(&db_path).unwrap();
        AppState {
            db: Arc::new(Mutex::new(conn)),
            db_path,
            slot: Arc::new(Mutex::new(DeviceSlot {
                device: Device::Sim(ocw_device::SimDevice::default()),
                port: 0,
            })),
            runtime_dir: dir,
            driver_sha256: None,
            sdk_source: None,
            dev: true,
            session: Arc::new(Mutex::new(role.map(|role| Session {
                operator_id: 1,
                login: "tester".into(),
                display_name: "测试".into(),
                role,
                department_name: None,
            }))),
        }
    }

    #[test]
    fn frontdesk_is_rejected_by_admin_guard_with_403() {
        let st = test_state(Some(Role::Frontdesk));
        let err = require_admin(&st).expect_err("前台不该通过管理员校验");
        assert_eq!(err.code, 403);
        assert!(err.message.contains("没有管理员权限"));
    }

    #[test]
    fn anonymous_gets_401_from_both_guards() {
        let st = test_state(None);
        assert_eq!(require_login(&st).unwrap_err().code, 401);
        assert_eq!(require_admin(&st).unwrap_err().code, 401);
    }

    #[test]
    fn frontdesk_passes_login_guard() {
        let st = test_state(Some(Role::Frontdesk));
        let s = require_login(&st).expect("前台应能通过登录校验");
        assert_eq!(s.role, Role::Frontdesk);
        // 管理员命令仍然拒绝
        assert_eq!(require_admin(&st).unwrap_err().code, 403);
    }

    /// CSV：BOM 开头、CRLF 换行、逗号/引号转义、公式注入防护。
    #[test]
    fn csv_escapes_and_has_bom() {
        let row = ocw_store::IssuanceRow {
            id: 7,
            created_at: "2026-11-06T03:00:00Z".into(),
            room_no: Some("220".into()),
            kind: "日租客卡".into(),
            days: 1,
            hours: 0,
            card_no: Some(0x2404),
            guest_name: Some("张三,李四".into()),
            guest_gender: None,
            guest_id_type: Some("身份证".into()),
            guest_id_number: Some("=1+1".into()),
            guest_nation: None,
            guest_address: None,
            guest_birth_date: None,
            guest_note: None,
            operator_name: Some("值班\"甲".into()),
            result: "ok".into(),
            expires_at: None,
            checked_out_at: None,
            duration_ms: Some(120),
            raw_hex: Some("5BECF100810424AB".into()),
        };
        let csv = issuances_to_csv(&[row]);
        assert!(csv.starts_with('\u{feff}'), "必须以 BOM 开头");
        assert!(csv.contains("\r\n"), "必须用 CRLF");
        assert!(csv.contains("\"张三,李四\""), "逗号字段要加引号");
        assert!(csv.contains("'=1+1"), "公式注入要前置单引号");
        assert!(csv.contains("\"值班\"\"甲\""), "内部引号要翻倍");
        assert!(csv.contains("0x2404"));
    }

}
