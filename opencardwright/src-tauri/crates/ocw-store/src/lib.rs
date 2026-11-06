//! 持久化层：SQLite（rusqlite 的 bundled 特性把 sqlite3 静态编进二进制，
//! 现场机器不需要装任何 SQLite 组件）。
//!
//! 领域约束在这里变成硬约束：
//! * `room.room_index` 唯一 —— 序号与门锁绑定，删房后不可复用；
//! * `issuance` 只追加 —— 这里只提供 insert 与查询，没有 update/delete。

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// 让上层（命令层）不必自己声明 rusqlite 依赖。
pub use rusqlite::Connection as DbConnection;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("文件错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub room_no: String,
    pub room_index: u8,
    pub building: Option<String>,
    pub floor: Option<i64>,
    /// 房型分类（v2 起使用）；旧 `room_type` 文本列保留但停用。
    pub room_type_id: Option<i64>,
    pub room_type_name: Option<String>,
    /// 该房间最多同时有效的客卡数；`None` = 不限。
    pub max_cards: Option<i64>,
    /// 房态 key（见 `room_status` 表），默认 `vacant_clean`。
    pub status: String,
    pub active: bool,
    pub install_card_at: Option<String>,
    pub created_at: String,
    /// 当前仍有效的客卡张数（按卡号去重、已退房不算），供房间格子展示 `1/2` 这种进度。
    pub active_cards: i64,
    /// 当前有效卡里最晚的到期时间（ISO UTC）；没有有效卡时为 `None`。
    /// 续住写卡用它算默认天数。
    pub active_until: Option<String>,
}

/// 新增房间的结果。
///
/// `revived_from` 非空表示这次没有插新行，而是把一条已逻辑删除的房间记录
/// 改名后重新启用（序号复用），值是那条记录的旧房号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomUpsert {
    pub room: Room,
    pub revived_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuanceRecord {
    pub id: i64,
    pub uuid: String,
    pub room_no: Option<String>,
    pub kind: String,
    pub days: i64,
    pub hours: i64,
    pub card_no: Option<i64>,
    pub raw_hex: Option<String>,
    pub operator_id: Option<i64>,
    pub result: String,
    pub error_code: Option<i64>,
    pub created_at: String,
    /// 到期时间（UTC ISO8601）。按发卡时间 + days/hours 推算的估算值
    /// （卡内只有时长码，锁实际从首次刷卡起算）。凡 days/hours>0 的卡型都有值，
    /// 复位/清除/安装卡等无时长卡型为 NULL。
    pub expires_at: Option<String>,
    // 客人身份信息（只存库，不写卡；全部可空）
    pub guest_name: Option<String>,
    pub guest_gender: Option<String>,
    pub guest_id_type: Option<String>,
    pub guest_id_number: Option<String>,
    pub guest_nation: Option<String>,
    pub guest_address: Option<String>,
    pub guest_birth_date: Option<String>,
    pub guest_note: Option<String>,
}

/// 打开（并初始化）数据库。
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let conn = Connection::open(path)?;
    // WAL：单写入者 + 读不阻塞写，掉电后由 SQLite 自己恢复。
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    migrate(&conn)?;
    Ok(conn)
}

/// 建表 + 列级迁移。全程幂等，可重复执行。
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    apply_v2(conn)?;
    apply_v3(conn)?;
    apply_v4(conn)?;
    apply_v5(conn)?;
    apply_v6(conn)?;
    apply_v7(conn)?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    if !column_exists(conn, table, column)? {
        conn.execute_batch(ddl)?;
    }
    Ok(())
}

/// v1 → v2：部门、房型、发卡到期时间。
///
/// 只加列/加表，不删不改既有列，所以老库升级零风险；回填失败最多让历史卡没有到期时间。
fn apply_v2(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "operator",
        "department_id",
        "ALTER TABLE operator ADD COLUMN department_id INTEGER REFERENCES department(id)",
    )?;
    ensure_column(
        conn,
        "room",
        "room_type_id",
        "ALTER TABLE room ADD COLUMN room_type_id INTEGER REFERENCES room_type(id)",
    )?;
    ensure_column(
        conn,
        "issuance",
        "expires_at",
        "ALTER TABLE issuance ADD COLUMN expires_at TEXT",
    )?;

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_issuance_expires ON issuance (expires_at);",
    )?;

    // 回填历史客卡：只有日租/钟点客卡有时长，管理卡一律 NULL
    conn.execute(
        "UPDATE issuance
            SET expires_at = strftime(
                  '%Y-%m-%dT%H:%M:%SZ', created_at,
                  CASE WHEN kind = '日租客卡' AND days > 0 THEN '+' || days || ' days'
                       WHEN kind = '钟点客卡' AND hours > 0 THEN '+' || hours || ' hours'
                  END)
          WHERE expires_at IS NULL
            AND result = 'ok'
            AND ((kind = '日租客卡' AND days > 0) OR (kind = '钟点客卡' AND hours > 0))",
        [],
    )?;

    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (2)", [])?;
    Ok(())
}

/// v2 → v3：发卡时录入的客人身份信息。
///
/// 身份信息**只存数据库，不写卡**（卡里只有 8 字节，放不下也没必要）。
/// 全部可空 —— 前台可以先发卡后补录，不能因为身份信息把发卡卡住。
fn apply_v3(conn: &Connection) -> Result<()> {
    for (col, ddl) in [
        ("guest_name", "ALTER TABLE issuance ADD COLUMN guest_name TEXT"),
        ("guest_gender", "ALTER TABLE issuance ADD COLUMN guest_gender TEXT"),
        ("guest_id_type", "ALTER TABLE issuance ADD COLUMN guest_id_type TEXT"),
        ("guest_id_number", "ALTER TABLE issuance ADD COLUMN guest_id_number TEXT"),
        ("guest_nation", "ALTER TABLE issuance ADD COLUMN guest_nation TEXT"),
        ("guest_address", "ALTER TABLE issuance ADD COLUMN guest_address TEXT"),
        ("guest_birth_date", "ALTER TABLE issuance ADD COLUMN guest_birth_date TEXT"),
        ("guest_note", "ALTER TABLE issuance ADD COLUMN guest_note TEXT"),
    ] {
        ensure_column(conn, "issuance", col, ddl)?;
    }

    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (3)", [])?;
    Ok(())
}

/// v3 → v4：房间级最大发卡数量（NULL = 不限）。
fn apply_v4(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "room",
        "max_cards",
        "ALTER TABLE room ADD COLUMN max_cards INTEGER",
    )?;
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (4)", [])?;
    Ok(())
}

/// v4 → v5：房态（每个房间一个状态）+ 退房标记。
fn apply_v5(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "room",
        "status",
        "ALTER TABLE room ADD COLUMN status TEXT NOT NULL DEFAULT 'vacant_clean'",
    )?;
    // 退房时间：置上之后这张卡的"有效"就不再计入
    ensure_column(
        conn,
        "issuance",
        "checked_out_at",
        "ALTER TABLE issuance ADD COLUMN checked_out_at TEXT",
    )?;
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (5)", [])?;
    Ok(())
}

/// v5 → v6：房型自定义颜色（NULL = 用中性灰）。
fn apply_v6(conn: &Connection) -> Result<()> {
    ensure_column(conn, "room_type", "color", "ALTER TABLE room_type ADD COLUMN color TEXT")?;
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (6)", [])?;
    Ok(())
}

/// v6 → v7：到期时间统一口径。
///
/// v2 只给客卡回填了 expires_at，管理卡（总控卡等）虽然有 days/hours 但一直是 NULL。
/// 现在改为「凡带时长的卡型都有到期时间」，这里把历史 NULL 记录按同一规则补上。
///
/// 幂等靠 `expires_at IS NULL`：已回填过的行（含 v2 回填的客卡）不会被覆写。
/// 失效/失败记录（result != 'ok'）不动 —— 那类行没有"到期"语义。
fn apply_v7(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE issuance
            SET expires_at = strftime(
                  '%Y-%m-%dT%H:%M:%SZ', created_at,
                  CASE WHEN days > 0 THEN '+' || days || ' days'
                       WHEN hours > 0 THEN '+' || hours || ' hours'
                  END)
          WHERE expires_at IS NULL
            AND result = 'ok'
            AND (days > 0 OR hours > 0)",
        [],
    )?;
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (7)", [])?;
    Ok(())
}

// ---------------------------------------------------------------- 房间

fn row_to_room(r: &Row<'_>) -> rusqlite::Result<Room> {
    Ok(Room {
        room_no: r.get(0)?,
        room_index: r.get::<_, i64>(1)? as u8,
        building: r.get(2)?,
        floor: r.get(3)?,
        room_type_id: r.get(4)?,
        room_type_name: r.get(5)?,
        max_cards: r.get(6)?,
        status: r.get(7)?,
        active: r.get::<_, i64>(8)? != 0,
        install_card_at: r.get(9)?,
        created_at: r.get(10)?,
        active_cards: r.get(11)?,
        active_until: r.get(12)?,
    })
}

const ROOM_SELECT: &str = "SELECT r.room_no, r.room_index, r.building, r.floor, r.room_type_id, \
                                  t.name, r.max_cards, r.status, r.active, r.install_card_at, r.created_at, \
                                  (SELECT COUNT(DISTINCT i.card_no) FROM issuance i \
                                    WHERE i.room_no = r.room_no AND i.result = 'ok' \
                                      AND i.card_no IS NOT NULL AND i.expires_at IS NOT NULL \
                                      AND i.checked_out_at IS NULL \
                                      AND i.kind IN ('日租客卡','钟点客卡') \
                                      AND i.expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')), \
                                  (SELECT MAX(i.expires_at) FROM issuance i \
                                    WHERE i.room_no = r.room_no AND i.result = 'ok' \
                                      AND i.expires_at IS NOT NULL \
                                      AND i.checked_out_at IS NULL \
                                      AND i.kind IN ('日租客卡','钟点客卡') \
                                      AND i.expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
                             FROM room r LEFT JOIN room_type t ON t.id = r.room_type_id";

pub fn list_rooms(conn: &Connection) -> Result<Vec<Room>> {
    let sql = format!("{ROOM_SELECT} ORDER BY r.room_index");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_room)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn find_room(conn: &Connection, room_no: &str) -> Result<Option<Room>> {
    let sql = format!("{ROOM_SELECT} WHERE r.room_no = ?1");
    Ok(conn.query_row(&sql, params![room_no], row_to_room).optional()?)
}

pub fn find_room_by_index(conn: &Connection, room_index: u8) -> Result<Option<Room>> {
    let sql = format!("{ROOM_SELECT} WHERE r.room_index = ?1");
    Ok(conn
        .query_row(&sql, params![room_index as i64], row_to_room)
        .optional()?)
}

/// 给房间挂房型（`None` = 取消房型）。
pub fn set_room_type(conn: &Connection, room_no: &str, room_type_id: Option<i64>) -> Result<()> {
    let n = conn.execute(
        "UPDATE room SET room_type_id = ?2 WHERE room_no = ?1",
        params![room_no, room_type_id],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("房号不存在：{room_no}")));
    }
    Ok(())
}

/// 设置该房间最多同时有效的客卡数。`None` = 不限。
pub fn set_room_max_cards(conn: &Connection, room_no: &str, max_cards: Option<i64>) -> Result<()> {
    if let Some(n) = max_cards {
        if !(1..=99).contains(&n) {
            return Err(StoreError::Invalid("最大发卡数量需在 1–99 之间，留空表示不限".into()));
        }
    }
    let n = conn.execute(
        "UPDATE room SET max_cards = ?2 WHERE room_no = ?1",
        params![room_no, max_cards],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("房号不存在：{room_no}")));
    }
    Ok(())
}

/// 该房间当前仍有效的客卡张数（按卡号去重，已过期的算作无效）。
///
/// 到期时间是按发卡时间估算的，所以这里的"有效"也是估算口径。
/// 显式只算客卡：管理卡现在也有 expires_at，但不应计入房间占用。
pub fn count_active_cards(conn: &Connection, room_no: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT card_no)
           FROM issuance
          WHERE room_no = ?1
            AND result = 'ok'
            AND card_no IS NOT NULL
            AND expires_at IS NOT NULL
            AND checked_out_at IS NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![room_no],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// 该房间最近一张**仍有效**的客卡记录。换房时用它把客人信息/时长带到新房。
///
/// "有效"口径与 [`count_active_cards`] 完全一致（客卡、未退房、未过期），
/// 否则会出现"格子显示有卡、换房却查不到客人"的矛盾。
pub fn latest_active_guest_card(
    conn: &Connection,
    room_no: &str,
) -> Result<Option<IssuanceRecord>> {
    let sql = format!(
        "SELECT {ISSUANCE_COLUMNS} FROM issuance
          WHERE room_no = ?1
            AND result = 'ok'
            AND checked_out_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          ORDER BY created_at DESC, id DESC LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params![room_no], row_to_issuance)
        .optional()?)
}

/// 该房间当前所有**有效客卡的卡号**（去重）。
///
/// 换房要先读卡核对"这张卡是不是本房的原卡"，判据就是读到的卡号在不在这个集合里。
/// 口径与 [`count_active_cards`] / [`latest_active_guest_card`] 完全一致。
pub fn active_guest_card_nos(conn: &Connection, room_no: &str) -> Result<Vec<u16>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT card_no FROM issuance
          WHERE room_no = ?1
            AND result = 'ok'
            AND card_no IS NOT NULL
            AND checked_out_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          ORDER BY card_no",
    )?;
    let rows = stmt.query_map(params![room_no], |r| r.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r? as u16);
    }
    Ok(out)
}

// ---------------------------------------------------------------- 房态

/// 按卡号退房：只关闭这一张卡；若该房已没有别的有效卡，再把房态置为 `dirty_status`。
///
/// 返回房号与剩余有效卡数（卡号查不到记录时返回 `None`）。
pub fn checkout_card(
    conn: &Connection,
    card_no: u16,
    dirty_status: &str,
) -> Result<Option<(String, i64)>> {
    let room_no: Option<String> = conn
        .query_row(
            "SELECT room_no FROM issuance
              WHERE card_no = ?1 AND result = 'ok' AND room_no IS NOT NULL
              ORDER BY created_at DESC, id DESC LIMIT 1",
            params![card_no as i64],
            |r| r.get(0),
        )
        .optional()?;

    let Some(room_no) = room_no else {
        return Ok(None);
    };

    conn.execute(
        "UPDATE issuance
            SET checked_out_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE card_no = ?1
            AND result = 'ok'
            AND checked_out_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![card_no as i64],
    )?;

    let remaining = count_active_cards(conn, &room_no)?;
    if remaining == 0 {
        set_room_status(conn, &room_no, dirty_status)?;
    }
    Ok(Some((room_no, remaining)))
}

/// 退房后要转成的房态（设置里可改，默认空脏房）。
///
/// 配置存 `setting.checkoutStatus`；若存的是个已不存在的房态 key，回退到默认值。
pub fn checkout_status(conn: &Connection) -> Result<String> {
    const DEFAULT: &str = "vacant_dirty";
    let Some(key) = get_setting(conn, "checkoutStatus")? else {
        return Ok(DEFAULT.to_string());
    };
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM room_status WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )?;
    Ok(if exists > 0 { key } else { DEFAULT.to_string() })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomStatus {
    pub key: String,
    pub label: String,
    /// 图标名，前端按名字映射到具体图标组件
    pub icon: String,
    /// 十六进制颜色，可在设置里自定义
    pub color: String,
    pub sort_order: i64,
}

fn row_to_status(r: &Row<'_>) -> rusqlite::Result<RoomStatus> {
    Ok(RoomStatus {
        key: r.get(0)?,
        label: r.get(1)?,
        icon: r.get(2)?,
        color: r.get(3)?,
        sort_order: r.get(4)?,
    })
}

pub fn list_room_statuses(conn: &Connection) -> Result<Vec<RoomStatus>> {
    let mut stmt = conn
        .prepare("SELECT key, label, icon, color, sort_order FROM room_status ORDER BY sort_order, key")?;
    let rows = stmt.query_map([], row_to_status)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 改某个房态的图标/颜色。key 是固定用途，不允许改。
pub fn update_room_status_style(conn: &Connection, key: &str, icon: &str, color: &str) -> Result<()> {
    if !color.starts_with('#') || (color.len() != 7 && color.len() != 4) {
        return Err(StoreError::Invalid(format!("颜色格式不对：{color}")));
    }
    let n = conn.execute(
        "UPDATE room_status SET icon = ?2, color = ?3 WHERE key = ?1",
        params![key, icon, color],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("未知房态：{key}")));
    }
    Ok(())
}

/// 设置某个房间的房态。
pub fn set_room_status(conn: &Connection, room_no: &str, status: &str) -> Result<()> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM room_status WHERE key = ?1",
        params![status],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Err(StoreError::Invalid(format!("未知房态：{status}")));
    }
    let n = conn.execute(
        "UPDATE room SET status = ?2 WHERE room_no = ?1",
        params![room_no, status],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("房号不存在：{room_no}")));
    }
    Ok(())
}

/// 退房：把该房间当前所有仍有效的客卡标记为「已退房」，房态置为 `dirty_status`。
///
/// 返回被标记的卡数。注意这只是业务上的退房登记 —— 门锁里的卡仍然有效，
/// 要让门锁立刻失效，还需要到管理卡页写一张退房钟点卡去刷。
pub fn checkout_room(conn: &Connection, room_no: &str, dirty_status: &str) -> Result<usize> {
    let n = conn.execute(
        "UPDATE issuance
            SET checked_out_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE room_no = ?1
            AND result = 'ok'
            AND checked_out_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![room_no],
    )?;
    set_room_status(conn, room_no, dirty_status)?;
    Ok(n)
}

/// 0..=255 中未使用的最小序号。用尽返回 None。
pub fn next_free_room_index(conn: &Connection) -> Result<Option<u8>> {
    let mut used = [false; 256];
    let mut stmt = conn.prepare("SELECT room_index FROM room")?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    for row in rows {
        let idx = row?;
        if (0..256).contains(&idx) {
            used[idx as usize] = true;
        }
    }
    Ok((0..256u16)
        .find(|i| !used[*i as usize])
        .map(|i| i as u8))
}

/// 新增房间。`room_index` 为 None 时自动分配最小可用序号。
pub fn insert_room(
    conn: &Connection,
    room_no: &str,
    room_index: Option<u8>,
    building: Option<&str>,
    floor: Option<i64>,
    room_type_id: Option<i64>,
) -> Result<Room> {
    Ok(insert_room_reporting(conn, room_no, room_index, building, floor, room_type_id)?.room)
}

/// 新增房间，并告诉调用方这次是不是"复活"了一条已删除的记录。
///
/// 序号一旦被门锁绑定就永不复用，所以删房只置 `active = 0`。将来要用这个序号，
/// 不能插新行（会撞 `room_index` UNIQUE），只能把那行改名 + 重新启用。
/// 自动分配（`room_index` 为 None）只看真正空闲的序号，不会顺手复活已删房。
pub fn insert_room_reporting(
    conn: &Connection,
    room_no: &str,
    room_index: Option<u8>,
    building: Option<&str>,
    floor: Option<i64>,
    room_type_id: Option<i64>,
) -> Result<RoomUpsert> {
    let room_no = room_no.trim();
    if room_no.is_empty() {
        return Err(StoreError::Invalid("房号不能为空".into()));
    }
    let index = match room_index {
        Some(i) => i,
        None => next_free_room_index(conn)?
            .ok_or_else(|| StoreError::Invalid("房间序号已用尽（0–255），无法再新增房间".into()))?,
    };

    // 序号已被占用：还在用的直接拒绝，已逻辑删除的就复用这一行
    if let Some(existing) = find_room_by_index(conn, index)? {
        if existing.active {
            return Err(StoreError::Invalid(format!(
                "序号 {index} 已被 {} 房占用",
                existing.room_no
            )));
        }
        // 新房号不能撞别的房间（撞到自己这行不算）
        if existing.room_no != room_no && find_room(conn, room_no)?.is_some() {
            return Err(StoreError::Invalid(format!("房号已存在：{room_no}")));
        }
        // 这一行代表"一间新房间"：房态回空净房、发卡上限回不限、
        // 安装卡记录清空（那张卡是给旧房号做的，对新房号没有意义）
        conn.execute(
            "UPDATE room SET room_no = ?2, active = 1, status = 'vacant_clean', \
                    install_card_at = NULL, max_cards = NULL, \
                    building = ?3, floor = ?4, room_type_id = ?5 \
             WHERE room_index = ?1",
            params![index as i64, room_no, building, floor, room_type_id],
        )?;
        let room = find_room(conn, room_no)?
            .ok_or_else(|| StoreError::Invalid("复用房间后读不回该房间".into()))?;
        return Ok(RoomUpsert {
            room,
            revived_from: Some(existing.room_no),
        });
    }

    conn.execute(
        "INSERT INTO room (room_no, room_index, building, floor, room_type_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![room_no, index as i64, building, floor, room_type_id],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(_, Some(ref m)) if m.contains("UNIQUE") => {
            StoreError::Invalid(format!("房号或序号已存在：{m}"))
        }
        other => StoreError::Sqlite(other),
    })?;

    let room = find_room(conn, room_no)?
        .ok_or_else(|| StoreError::Invalid("插入后读不回该房间".into()))?;
    Ok(RoomUpsert {
        room,
        revived_from: None,
    })
}

/// 记录"已为该房间制作安装卡"的时间。
pub fn mark_install_card(conn: &Connection, room_no: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE room SET install_card_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE room_no = ?1",
        params![room_no],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("房号不存在：{room_no}")));
    }
    Ok(())
}

/// 改房号（房间显示名称）。序号不变 —— 序号才是门锁里的身份。
///
/// 房号是主键，新名称必须没被占用；发卡/退房链路都按房号关联，
/// 所以调用方需要自己保证该房没有在场的有效卡（见 `count_active_cards`）。
pub fn rename_room(conn: &Connection, room_no: &str, new_room_no: &str) -> Result<()> {
    let new_no = new_room_no.trim();
    if new_no.is_empty() {
        return Err(StoreError::Invalid("房号不能为空".into()));
    }
    if new_no == room_no {
        return Ok(());
    }
    let taken: i64 = conn.query_row(
        "SELECT COUNT(*) FROM room WHERE room_no = ?1",
        params![new_no],
        |r| r.get(0),
    )?;
    if taken > 0 {
        return Err(StoreError::Invalid(format!("房号已存在：{new_no}")));
    }
    let n = conn.execute(
        "UPDATE room SET room_no = ?2 WHERE room_no = ?1",
        params![room_no, new_no],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("房号不存在：{room_no}")));
    }
    Ok(())
}

/// 停用房间。**不释放序号**——序号已随门锁绑定。
pub fn deactivate_room(conn: &Connection, room_no: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE room SET active = 0 WHERE room_no = ?1",
        params![room_no],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid(format!("房号不存在：{room_no}")));
    }
    Ok(())
}

// ---------------------------------------------------------------- 发卡流水

fn row_to_issuance(r: &Row<'_>) -> rusqlite::Result<IssuanceRecord> {
    Ok(IssuanceRecord {
        id: r.get(0)?,
        uuid: r.get(1)?,
        room_no: r.get(2)?,
        kind: r.get(3)?,
        days: r.get(4)?,
        hours: r.get(5)?,
        card_no: r.get(6)?,
        raw_hex: r.get(7)?,
        operator_id: r.get(8)?,
        result: r.get(9)?,
        error_code: r.get(10)?,
        created_at: r.get(11)?,
        guest_name: r.get(12)?,
        guest_gender: r.get(13)?,
        guest_id_type: r.get(14)?,
        guest_id_number: r.get(15)?,
        guest_nation: r.get(16)?,
        guest_address: r.get(17)?,
        guest_birth_date: r.get(18)?,
        guest_note: r.get(19)?,
        expires_at: r.get(20)?,
    })
}

const ISSUANCE_COLUMNS: &str = "id, uuid, room_no, kind, days, hours, card_no, raw_hex, \
                                operator_id, result, error_code, created_at, \
                                guest_name, guest_gender, guest_id_type, guest_id_number, \
                                guest_nation, guest_address, guest_birth_date, guest_note, \
                                expires_at";

pub struct NewIssuance<'a> {
    pub room_no: Option<&'a str>,
    pub kind: &'a str,
    pub days: i64,
    pub hours: i64,
    pub card_no: Option<u16>,
    pub raw_hex: Option<&'a str>,
    pub operator_id: Option<i64>,
    pub reader_id: Option<&'a str>,
    pub result: &'a str,
    pub error_code: Option<i64>,
    pub idempotency_key: Option<&'a str>,
    pub duration_ms: Option<i64>,
}

/// 发卡时录入的客人身份信息。全部可空，只存库、不写卡。
#[derive(Debug, Clone, Default)]
pub struct GuestIdentity<'a> {
    pub name: Option<&'a str>,
    pub gender: Option<&'a str>,
    pub id_type: Option<&'a str>,
    pub id_number: Option<&'a str>,
    pub nation: Option<&'a str>,
    pub address: Option<&'a str>,
    pub birth_date: Option<&'a str>,
    pub note: Option<&'a str>,
}

impl GuestIdentity<'_> {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.gender.is_none()
            && self.id_type.is_none()
            && self.id_number.is_none()
            && self.nation.is_none()
            && self.address.is_none()
            && self.birth_date.is_none()
            && self.note.is_none()
    }
}

/// 给一条发卡记录补上身份信息。发卡成功后再调用，避免主流程被可选字段拖慢。
pub fn update_guest_identity(
    conn: &Connection,
    issuance_id: i64,
    guest: &GuestIdentity<'_>,
) -> Result<()> {
    if guest.is_empty() {
        return Ok(());
    }
    conn.execute(
        "UPDATE issuance
            SET guest_name = ?2, guest_gender = ?3, guest_id_type = ?4, guest_id_number = ?5,
                guest_nation = ?6, guest_address = ?7, guest_birth_date = ?8, guest_note = ?9
          WHERE id = ?1",
        params![
            issuance_id,
            guest.name,
            guest.gender,
            guest.id_type,
            guest.id_number,
            guest.nation,
            guest.address,
            guest.birth_date,
            guest.note,
        ],
    )?;
    Ok(())
}

/// 追加一条发卡记录，返回新行 id。
pub fn record_issuance(conn: &Connection, new: &NewIssuance<'_>) -> Result<i64> {
    let uuid = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO issuance
           (uuid, room_no, kind, days, hours, card_no, raw_hex, operator_id, reader_id,
            result, error_code, idempotency_key, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            uuid,
            new.room_no,
            new.kind,
            new.days,
            new.hours,
            new.card_no.map(|v| v as i64),
            new.raw_hex,
            new.operator_id,
            new.reader_id,
            new.result,
            new.error_code,
            new.idempotency_key,
            new.duration_ms,
        ],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(_, Some(ref m)) if m.contains("UNIQUE") => {
            StoreError::Invalid("重复的幂等键或 uuid".into())
        }
        other => StoreError::Sqlite(other),
    })?;

    let id = conn.last_insert_rowid();

    // 凡带时长的卡型（客卡、总控卡等）都写到期时间。用 SQLite 的日期运算，
    // 保证和 created_at 同格式（ISO UTC）。
    if let Some(modifier) = expiry_modifier(new.days, new.hours) {
        conn.execute(
            "UPDATE issuance
                SET expires_at = strftime('%Y-%m-%dT%H:%M:%SZ', created_at, ?1)
              WHERE id = ?2",
            params![modifier, id],
        )?;
    }

    Ok(id)
}

/// 带时长的卡型的过期偏移量（`+90 days` / `+3 hours`）。无时长（0/0）的卡型一律 None。
///
/// 注意：这是按**发卡时间**算的估算值 —— 卡内只有时长码，没有绝对到期时间，
/// 而锁实际是从"首次刷卡"起算（见 docs/TEMIC门锁卡技术规格.md §11.1）。
pub fn expiry_modifier(days: i64, hours: i64) -> Option<String> {
    if days > 0 {
        Some(format!("+{days} days"))
    } else if hours > 0 {
        Some(format!("+{hours} hours"))
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiringCard {
    pub issuance_id: i64,
    pub room_no: Option<String>,
    pub room_index: Option<i64>,
    pub card_no: Option<i64>,
    pub kind: String,
    pub expires_at: String,
    pub days: i64,
    pub hours: i64,
}

/// 即将到期 / 刚过期的客卡。
///
/// * 每个 `(房间, 卡型)` 只取最新一条 —— 同房重发不会重复提醒；
/// * 时间窗 `[now - 1 天, now + within_minutes]`：既提醒即将到期，也把刚过期没来得及提醒的补回来；
/// * 用 ISO UTC 字符串比较，字典序即时间序。
pub fn expiring_cards(conn: &Connection, within_minutes: i64) -> Result<Vec<ExpiringCard>> {
    let within = within_minutes.clamp(0, 24 * 60);
    let mut stmt = conn.prepare(
        "WITH latest AS (
             SELECT MAX(id) AS id
               FROM issuance
              WHERE room_no IS NOT NULL
                AND expires_at IS NOT NULL
                AND result = 'ok'
                AND kind IN ('日租客卡','钟点客卡')
              GROUP BY room_no, kind
         )
         SELECT i.id, i.room_no, r.room_index, i.card_no, i.kind, i.expires_at, i.days, i.hours
           FROM issuance i
           LEFT JOIN room r ON r.room_no = i.room_no
          WHERE i.id IN (SELECT id FROM latest)
            AND i.expires_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 day')
            AND i.expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+' || ?1 || ' minutes')
          ORDER BY i.expires_at",
    )?;

    let rows = stmt.query_map(params![within], |r| {
        Ok(ExpiringCard {
            issuance_id: r.get(0)?,
            room_no: r.get(1)?,
            room_index: r.get(2)?,
            card_no: r.get(3)?,
            kind: r.get(4)?,
            expires_at: r.get(5)?,
            days: r.get(6)?,
            hours: r.get(7)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 按卡号查最近一条记录。读卡页的"有没有发卡记录"就靠它。
pub fn latest_issuance_for_card(conn: &Connection, card_no: u16) -> Result<Option<IssuanceRecord>> {
    let sql = format!(
        "SELECT {ISSUANCE_COLUMNS} FROM issuance WHERE card_no = ?1 ORDER BY created_at DESC, id DESC LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params![card_no as i64], row_to_issuance)
        .optional()?)
}

/// 按主键取一条发卡记录。发卡命令刚写入后用它取回 expires_at 等完整字段。
pub fn find_issuance(conn: &Connection, id: i64) -> Result<Option<IssuanceRecord>> {
    let sql = format!("SELECT {ISSUANCE_COLUMNS} FROM issuance WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id], row_to_issuance)
        .optional()?)
}

pub fn count_issuance_for_card(conn: &Connection, card_no: u16) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM issuance WHERE card_no = ?1",
        params![card_no as i64],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u32)
}

/// 按幂等键取既有记录。UI 连点两次时用它做"只发一张"的回放。
pub fn find_issuance_by_idempotency(
    conn: &Connection,
    key: &str,
) -> Result<Option<IssuanceRecord>> {
    let sql = format!(
        "SELECT {ISSUANCE_COLUMNS} FROM issuance WHERE idempotency_key = ?1 LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params![key], row_to_issuance)
        .optional()?)
}

/// 已发出过的最大卡号（启动时与 `CardNo.state` 比对用）。
pub fn max_issued_card_no(conn: &Connection) -> Result<Option<u16>> {
    // MAX() 在空表上返回一行 NULL，所以外层是 Option<Option<i64>>：
    // 外层 = "查询有没有成行"，内层 = "值是不是 NULL"。
    let raw: Option<Option<i64>> = conn
        .query_row("SELECT MAX(card_no) FROM issuance", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .optional()?;

    Ok(raw
        .flatten()
        .filter(|x| (0..=0xFFFF).contains(x))
        .map(|x| x as u16))
}

// ---------------------------------------------------------------- 操作员

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    pub id: i64,
    pub login: String,
    pub display_name: String,
    pub role: String,
    pub pwd_hash: String,
    pub active: bool,
    pub department_id: Option<i64>,
    pub department_name: Option<String>,
}

fn row_to_operator(r: &Row<'_>) -> rusqlite::Result<Operator> {
    Ok(Operator {
        id: r.get(0)?,
        login: r.get(1)?,
        display_name: r.get(2)?,
        role: r.get(3)?,
        pwd_hash: r.get(4)?,
        active: r.get::<_, i64>(5)? != 0,
        department_id: r.get(6)?,
        department_name: r.get(7)?,
    })
}

const OPERATOR_SELECT: &str =
    "SELECT o.id, o.login, o.display_name, o.role, o.pwd_hash, o.active, o.department_id, d.name
       FROM operator o LEFT JOIN department d ON d.id = o.department_id";

/// 是否已经初始化过（有任何一个操作员）。
pub fn count_operators(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM operator", [], |r| r.get(0))?;
    Ok(n)
}

pub fn find_operator(conn: &Connection, login: &str) -> Result<Option<Operator>> {
    let sql = format!("{OPERATOR_SELECT} WHERE o.login = ?1");
    Ok(conn
        .query_row(&sql, params![login], row_to_operator)
        .optional()?)
}

// ---------------------------------------------------------------- 部门

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Department {
    pub id: i64,
    pub name: String,
    pub note: Option<String>,
    pub active: bool,
    pub created_at: String,
    /// 该部门下的用户数（删除/停用前给管理员看一眼）
    pub member_count: i64,
}

fn row_to_department(r: &Row<'_>) -> rusqlite::Result<Department> {
    Ok(Department {
        id: r.get(0)?,
        name: r.get(1)?,
        note: r.get(2)?,
        active: r.get::<_, i64>(3)? != 0,
        created_at: r.get(4)?,
        member_count: r.get(5)?,
    })
}

pub fn list_departments(conn: &Connection) -> Result<Vec<Department>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.name, d.note, d.active, d.created_at,
                (SELECT COUNT(*) FROM operator o WHERE o.department_id = d.id)
           FROM department d ORDER BY d.id",
    )?;
    let rows = stmt.query_map([], row_to_department)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn insert_department(conn: &Connection, name: &str, note: Option<&str>) -> Result<Department> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::Invalid("部门名称不能为空".into()));
    }
    conn.execute(
        "INSERT INTO department (name, note) VALUES (?1, ?2)",
        params![name, note],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(_, Some(ref m)) if m.contains("UNIQUE") => {
            StoreError::Invalid(format!("部门已存在：{m}"))
        }
        other => StoreError::Sqlite(other),
    })?;

    let id = conn.last_insert_rowid();
    Ok(conn.query_row(
        "SELECT d.id, d.name, d.note, d.active, d.created_at, 0 FROM department d WHERE d.id = ?1",
        params![id],
        row_to_department,
    )?)
}

pub fn rename_department(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::Invalid("部门名称不能为空".into()));
    }
    let n = conn.execute(
        "UPDATE department SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("部门不存在".into()));
    }
    Ok(())
}

pub fn set_department_active(conn: &Connection, id: i64, active: bool) -> Result<()> {
    let n = conn.execute(
        "UPDATE department SET active = ?2 WHERE id = ?1",
        params![id, if active { 1 } else { 0 }],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("部门不存在".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------- 房型

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomType {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
    /// 房型标签颜色（`#RRGGBB`）；`None` = 中性灰
    pub color: Option<String>,
    /// 使用该房型的房间数
    pub room_count: i64,
}

fn row_to_room_type(r: &Row<'_>) -> rusqlite::Result<RoomType> {
    Ok(RoomType {
        id: r.get(0)?,
        name: r.get(1)?,
        sort_order: r.get(2)?,
        created_at: r.get(3)?,
        color: r.get(4)?,
        room_count: r.get(5)?,
    })
}

pub fn list_room_types(conn: &Connection) -> Result<Vec<RoomType>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.sort_order, t.created_at, t.color,
                (SELECT COUNT(*) FROM room r WHERE r.room_type_id = t.id)
           FROM room_type t ORDER BY t.sort_order, t.id",
    )?;
    let rows = stmt.query_map([], row_to_room_type)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 设置房型的标签颜色。`None` 表示恢复中性灰。
pub fn update_room_type_color(conn: &Connection, id: i64, color: Option<&str>) -> Result<()> {
    if let Some(c) = color {
        let ok = c.starts_with('#') && (c.len() == 7 || c.len() == 4);
        if !ok {
            return Err(StoreError::Invalid(format!("颜色格式不对：{c}")));
        }
    }
    let n = conn.execute(
        "UPDATE room_type SET color = ?2 WHERE id = ?1",
        params![id, color],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("房型不存在".into()));
    }
    Ok(())
}

/// 新建房型自动配的标签颜色。
///
/// 取 antd 色板里白字对比度够用的中深色，避免新建房型一律灰色。
const ROOM_TYPE_PALETTE: &[&str] = &[
    "#1677ff", "#13c2c2", "#52c41a", "#722ed1", "#eb2f96", "#fa8c16", "#2f54eb", "#f5222d",
    "#08979c", "#d4380d", "#531dab", "#237804", "#ad6800", "#c41d7f",
];

/// 0..bound 的随机下标。借 uuid v4 的随机字节，不为这点需求单独引 rand 依赖。
fn random_index(bound: usize) -> usize {
    (uuid::Uuid::new_v4().into_bytes()[0] as usize) % bound
}

/// 挑一个还没被其它房型占用的颜色；调色板用尽了就允许重复。
fn pick_room_type_color(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare("SELECT color FROM room_type WHERE color IS NOT NULL")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut used: Vec<String> = Vec::new();
    for c in rows {
        used.push(c?.to_ascii_lowercase());
    }
    let free: Vec<&str> = ROOM_TYPE_PALETTE
        .iter()
        .copied()
        .filter(|c| !used.iter().any(|u| u == c))
        .collect();
    let pool: &[&str] = if free.is_empty() {
        ROOM_TYPE_PALETTE
    } else {
        &free
    };
    Ok(pool[random_index(pool.len())].to_string())
}

pub fn insert_room_type(conn: &Connection, name: &str, sort_order: i64) -> Result<RoomType> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::Invalid("房型名称不能为空".into()));
    }
    let color = pick_room_type_color(conn)?;
    conn.execute(
        "INSERT INTO room_type (name, sort_order, color) VALUES (?1, ?2, ?3)",
        params![name, sort_order, color],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(_, Some(ref m)) if m.contains("UNIQUE") => {
            StoreError::Invalid(format!("房型已存在：{m}"))
        }
        other => StoreError::Sqlite(other),
    })?;

    let id = conn.last_insert_rowid();
    Ok(conn.query_row(
        "SELECT t.id, t.name, t.sort_order, t.created_at, t.color, 0 FROM room_type t WHERE t.id = ?1",
        params![id],
        row_to_room_type,
    )?)
}

pub fn rename_room_type(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::Invalid("房型名称不能为空".into()));
    }
    let n = conn.execute(
        "UPDATE room_type SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("房型不存在".into()));
    }
    Ok(())
}

/// 删除房型。仍有房间在用就拒绝 —— 避免房间悄无声息地掉分类。
pub fn delete_room_type(conn: &Connection, id: i64) -> Result<()> {
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM room WHERE room_type_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(StoreError::Invalid(format!(
            "还有 {used} 个房间使用该房型，先把它们改到别的房型再删"
        )));
    }
    conn.execute("DELETE FROM room_type WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn insert_operator(
    conn: &Connection,
    login: &str,
    display_name: &str,
    role: &str,
    pwd_hash: &str,
    department_id: Option<i64>,
) -> Result<Operator> {
    let login = login.trim();
    if login.is_empty() {
        return Err(StoreError::Invalid("登录名不能为空".into()));
    }
    conn.execute(
        "INSERT INTO operator (login, display_name, role, pwd_hash, department_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![login, display_name, role, pwd_hash, department_id],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(_, Some(ref m)) if m.contains("UNIQUE") => {
            StoreError::Invalid(format!("登录名已存在：{m}"))
        }
        other => StoreError::Sqlite(other),
    })?;

    find_operator(conn, login)?.ok_or_else(|| StoreError::Invalid("插入后读不回该操作员".into()))
}

/// 用户列表项 —— **永远不含密码哈希**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorInfo {
    pub id: i64,
    pub login: String,
    pub display_name: String,
    pub role: String,
    pub department_id: Option<i64>,
    pub department_name: Option<String>,
    pub active: bool,
    pub created_at: String,
}

pub fn list_operators(conn: &Connection) -> Result<Vec<OperatorInfo>> {
    let mut stmt = conn.prepare(
        "SELECT o.id, o.login, o.display_name, o.role, o.department_id, d.name, o.active, o.created_at
           FROM operator o LEFT JOIN department d ON d.id = o.department_id
          ORDER BY o.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(OperatorInfo {
            id: r.get(0)?,
            login: r.get(1)?,
            display_name: r.get(2)?,
            role: r.get(3)?,
            department_id: r.get(4)?,
            department_name: r.get(5)?,
            active: r.get::<_, i64>(6)? != 0,
            created_at: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn find_operator_by_id(conn: &Connection, id: i64) -> Result<Option<Operator>> {
    let sql = format!("{OPERATOR_SELECT} WHERE o.id = ?1");
    Ok(conn.query_row(&sql, params![id], row_to_operator).optional()?)
}

pub fn update_operator_password(conn: &Connection, id: i64, pwd_hash: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE operator SET pwd_hash = ?2 WHERE id = ?1",
        params![id, pwd_hash],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("用户不存在".into()));
    }
    Ok(())
}

pub fn set_operator_active(conn: &Connection, id: i64, active: bool) -> Result<()> {
    let n = conn.execute(
        "UPDATE operator SET active = ?2 WHERE id = ?1",
        params![id, if active { 1 } else { 0 }],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("用户不存在".into()));
    }
    Ok(())
}

pub fn set_operator_department(conn: &Connection, id: i64, department_id: Option<i64>) -> Result<()> {
    let n = conn.execute(
        "UPDATE operator SET department_id = ?2 WHERE id = ?1",
        params![id, department_id],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("用户不存在".into()));
    }
    Ok(())
}

/// 还能登录的管理员数量。停用/降级前用它兜底，避免把最后一个管理员锁死。
pub fn count_active_admins(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM operator WHERE role = 'admin' AND active = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

// ---------------------------------------------------------------- 审计

/// 写一条审计。管理卡、改配置、导入房号都必须走这里。
///
/// 审计是账，只增不改：这里不提供 update/delete。
pub fn record_audit(
    conn: &Connection,
    action: &str,
    target: Option<&str>,
    detail_json: Option<&str>,
    result: &str,
    operator_id: Option<i64>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO audit_log (operator_id, action, target, detail_json, result)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![operator_id, action, target, detail_json, result],
    )?;
    Ok(conn.last_insert_rowid())
}

// ---------------------------------------------------------------- 设置

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM setting WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO setting (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (Connection, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("ocw-test-{}.db", uuid::Uuid::new_v4()));
        let conn = open(&path).expect("建库");
        (conn, path)
    }

    fn cleanup(conn: Connection, path: std::path::PathBuf) {
        drop(conn);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn schema_is_idempotent() {
        let (conn, path) = temp_db();
        migrate(&conn).expect("重复迁移应无副作用");
        cleanup(conn, path);
    }

    #[test]
    fn allocates_smallest_free_index() {
        let (conn, path) = temp_db();
        assert_eq!(insert_room(&conn, "201", None, None, None, None).unwrap().room_index, 0);
        assert_eq!(insert_room(&conn, "202", None, None, None, None).unwrap().room_index, 1);
        assert_eq!(insert_room(&conn, "203", None, None, None, None).unwrap().room_index, 2);
        cleanup(conn, path);
    }

    #[test]
    fn explicit_index_is_honoured_and_skipped() {
        let (conn, path) = temp_db();
        insert_room(&conn, "201", None, None, None, None).unwrap();
        insert_room(&conn, "259", Some(38), None, None, None).unwrap();
        // 自动分配应跳过已用的 0 和 38
        assert_eq!(insert_room(&conn, "202", None, None, None, None).unwrap().room_index, 1);
        cleanup(conn, path);
    }

    #[test]
    fn duplicate_index_is_rejected() {
        let (conn, path) = temp_db();
        insert_room(&conn, "201", Some(5), None, None, None).unwrap();
        let err = insert_room(&conn, "202", Some(5), None, None, None).unwrap_err();
        assert!(matches!(err, StoreError::Invalid(_)));
        cleanup(conn, path);
    }

    #[test]
    fn deactivated_room_keeps_its_index() {
        let (conn, path) = temp_db();
        insert_room(&conn, "201", None, None, None, None).unwrap();
        deactivate_room(&conn, "201").unwrap();
        // 停用不等于释放：新房间不得拿到 0 号
        let next = insert_room(&conn, "202", None, None, None, None).unwrap();
        assert_eq!(next.room_index, 1);
        assert!(!find_room(&conn, "201").unwrap().unwrap().active);
        cleanup(conn, path);
    }

    #[test]
    fn deactivated_room_can_be_revived() {
        let (conn, path) = temp_db();
        let first = insert_room(&conn, "201", None, None, None, None).unwrap();
        assert_eq!(first.room_index, 0);
        set_room_status(&conn, "201", "vacant_dirty").unwrap();
        set_room_max_cards(&conn, "201", Some(2)).unwrap();
        mark_install_card(&conn, "201").unwrap();
        deactivate_room(&conn, "201").unwrap();

        // 手动填已删房的序号 = 复用那条记录，只改房名
        let upsert = insert_room_reporting(&conn, "260", Some(0), None, None, None).unwrap();
        assert_eq!(upsert.revived_from.as_deref(), Some("201"));
        assert_eq!(upsert.room.room_no, "260");
        assert_eq!(upsert.room.room_index, 0);
        assert!(upsert.room.active);
        // 复用的行代表"一间新房间"：房态回空净房、上限回不限、安装卡记录清空
        assert_eq!(upsert.room.status, "vacant_clean");
        assert_eq!(upsert.room.max_cards, None);
        assert_eq!(upsert.room.install_card_at, None);
        // 旧房号已经不存在，全表仍只有一行
        assert!(find_room(&conn, "201").unwrap().is_none());
        assert_eq!(list_rooms(&conn).unwrap().len(), 1);
        cleanup(conn, path);
    }

    #[test]
    fn auto_index_skips_deactivated_room() {
        let (conn, path) = temp_db();
        insert_room(&conn, "201", None, None, None, None).unwrap();
        deactivate_room(&conn, "201").unwrap();
        // 自动分配不复活已删房，只会拿真正空闲的序号
        let upsert = insert_room_reporting(&conn, "202", None, None, None, None).unwrap();
        assert_eq!(upsert.revived_from, None);
        assert_eq!(upsert.room.room_index, 1);
        cleanup(conn, path);
    }

    #[test]
    fn live_room_index_is_rejected() {
        let (conn, path) = temp_db();
        insert_room(&conn, "201", Some(3), None, None, None).unwrap();
        let err = insert_room_reporting(&conn, "202", Some(3), None, None, None).unwrap_err();
        assert!(format!("{err}").contains("已被 201 房占用"), "实际：{err}");
        cleanup(conn, path);
    }

    #[test]
    fn revive_with_taken_room_no_is_rejected() {
        let (conn, path) = temp_db();
        insert_room(&conn, "201", None, None, None, None).unwrap(); // 序号 0
        deactivate_room(&conn, "201").unwrap();
        insert_room(&conn, "202", None, None, None, None).unwrap(); // 序号 1，在用
        // 序号 0 已删，但房号 202 被别的房间占着 → 不能复用成 202
        let err = insert_room_reporting(&conn, "202", Some(0), None, None, None).unwrap_err();
        assert!(format!("{err}").contains("房号已存在"), "实际：{err}");
        cleanup(conn, path);
    }

    #[test]
    fn issuance_lookup_by_card_no() {
        let (conn, path) = temp_db();
        let id = record_issuance(
            &conn,
            &NewIssuance {
                room_no: Some("220"),
                kind: "日租客卡",
                days: 1,
                hours: 0,
                card_no: Some(0x2404),
                raw_hex: Some("5BECF100810424AB"),
                operator_id: None,
                reader_id: Some("4E57"),
                result: "ok",
                error_code: None,
                idempotency_key: Some("k1"),
                duration_ms: Some(1200),
            },
        )
        .unwrap();
        assert!(id > 0);

        let rec = latest_issuance_for_card(&conn, 0x2404).unwrap().unwrap();
        assert_eq!(rec.room_no.as_deref(), Some("220"));
        assert_eq!(count_issuance_for_card(&conn, 0x2404).unwrap(), 1);
        assert_eq!(max_issued_card_no(&conn).unwrap(), Some(0x2404));

        // 没有记录的卡号必须返回 None，而不是报错——读卡页据此显示"未查到"
        assert!(latest_issuance_for_card(&conn, 0x1111).unwrap().is_none());
        assert_eq!(count_issuance_for_card(&conn, 0x1111).unwrap(), 0);

        cleanup(conn, path);
    }

    #[test]
    fn idempotency_key_is_enforced() {
        let (conn, path) = temp_db();
        let mut a = NewIssuance {
            room_no: Some("220"),
            kind: "日租客卡",
            days: 1,
            hours: 0,
            card_no: Some(1),
            raw_hex: None,
            operator_id: None,
            reader_id: None,
            result: "ok",
            error_code: None,
            idempotency_key: Some("same"),
            duration_ms: None,
        };
        record_issuance(&conn, &a).unwrap();
        a.card_no = Some(2);
        assert!(matches!(
            record_issuance(&conn, &a).unwrap_err(),
            StoreError::Invalid(_)
        ));
        cleanup(conn, path);
    }

    #[test]
    fn idempotency_lookup_returns_existing_record() {
        let (conn, path) = temp_db();
        let new = NewIssuance {
            room_no: Some("220"),
            kind: "日租客卡",
            days: 1,
            hours: 0,
            card_no: Some(0x2404),
            raw_hex: Some("5BECF100810424AB"),
            operator_id: None,
            reader_id: None,
            result: "ok",
            error_code: None,
            idempotency_key: Some("click-1"),
            duration_ms: Some(900),
        };
        record_issuance(&conn, &new).unwrap();

        let hit = find_issuance_by_idempotency(&conn, "click-1").unwrap();
        assert_eq!(hit.unwrap().card_no, Some(0x2404));
        assert!(find_issuance_by_idempotency(&conn, "click-2").unwrap().is_none());

        cleanup(conn, path);
    }

    #[test]
    fn settings_roundtrip() {
        let (conn, path) = temp_db();
        assert!(get_setting(&conn, "port").unwrap().is_none());
        set_setting(&conn, "port", "6").unwrap();
        set_setting(&conn, "port", "7").unwrap();
        assert_eq!(get_setting(&conn, "port").unwrap().as_deref(), Some("7"));
        cleanup(conn, path);
    }

    // ---------------- v2：迁移 / 到期 / 部门 / 房型 ----------------

    fn sample_issuance(
        room: Option<&'static str>,
        kind: &'static str,
        days: i64,
        hours: i64,
    ) -> NewIssuance<'static> {
        NewIssuance {
            room_no: room,
            kind,
            days,
            hours,
            card_no: Some(1),
            raw_hex: None,
            operator_id: None,
            reader_id: None,
            result: "ok",
            error_code: None,
            idempotency_key: None,
            duration_ms: None,
        }
    }

    #[test]
    fn v2_columns_exist_and_migrate_is_idempotent() {
        let (conn, path) = temp_db();
        assert!(column_exists(&conn, "operator", "department_id").unwrap());
        assert!(column_exists(&conn, "room", "room_type_id").unwrap());
        assert!(column_exists(&conn, "issuance", "expires_at").unwrap());
        migrate(&conn).expect("重复迁移不应报错");
        cleanup(conn, path);
    }

    #[test]
    fn any_card_kind_with_duration_gets_expiry() {
        let (conn, path) = temp_db();
        let guest = record_issuance(&conn, &sample_issuance(Some("220"), "日租客卡", 1, 0)).unwrap();
        let expires: Option<String> = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![guest], |r| r.get(0))
            .unwrap();
        assert!(expires.is_some(), "客卡必须有到期时间");

        // 总控卡也带时长 → 也要有到期时间（读卡页/管理卡结果统一显示）
        let master = record_issuance(&conn, &sample_issuance(None, "总控卡", 1, 0)).unwrap();
        let expires: Option<String> = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![master], |r| r.get(0))
            .unwrap();
        assert!(expires.is_some(), "总控卡带时长就该有到期时间");

        // 无时长的管理卡仍然没有
        let reset = record_issuance(&conn, &sample_issuance(None, "复位卡", 0, 0)).unwrap();
        let expires: Option<String> = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![reset], |r| r.get(0))
            .unwrap();
        assert!(expires.is_none(), "无时长的管理卡不该有到期时间");

        cleanup(conn, path);
    }

    #[test]
    fn master_card_expiry_equals_created_plus_days() {
        let (conn, path) = temp_db();
        let id = record_issuance(&conn, &sample_issuance(None, "总控卡", 30, 0)).unwrap();
        let (created, expires): (String, String) = conn
            .query_row(
                "SELECT created_at, expires_at FROM issuance WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let expected: String = conn
            .query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', ?1, '+30 days')",
                params![created],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(expires, expected, "总控卡到期 = 发卡时间 + 天数");
        cleanup(conn, path);
    }

    #[test]
    fn expiry_modifier_is_kind_agnostic() {
        assert_eq!(expiry_modifier(1, 0).as_deref(), Some("+1 days"));
        assert_eq!(expiry_modifier(0, 3).as_deref(), Some("+3 hours"));
        assert_eq!(expiry_modifier(0, 0), None, "时长为 0 不产生到期时间");
        assert_eq!(expiry_modifier(30, 2).as_deref(), Some("+30 days"), "天优先于小时");
    }

    #[test]
    fn v7_backfill_fills_null_expiry_and_is_idempotent() {
        let (conn, path) = temp_db();
        // 直接插一条「v2 时代留下」的总控卡：有天数但没有 expires_at
        conn.execute(
            "INSERT INTO issuance (uuid, kind, days, hours, card_no, result, created_at)
             VALUES (?1, '总控卡', 30, 0, 900, 'ok', '2026-01-01T00:00:00Z')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        let id = conn.last_insert_rowid();

        migrate(&conn).expect("迁移不应报错");
        let first: Option<String> = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap();
        assert_eq!(first.as_deref(), Some("2026-01-31T00:00:00Z"), "v7 要补上历史总控卡");

        // 再跑一次：已有值不被改写（这里把值改成明显不同的时间，确认迁移不覆盖）
        conn.execute(
            "UPDATE issuance SET expires_at = '2026-02-02T00:00:00Z' WHERE id = ?1",
            params![id],
        )
        .unwrap();
        migrate(&conn).expect("重复迁移不应报错");
        let second: Option<String> = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap();
        assert_eq!(second.as_deref(), Some("2026-02-02T00:00:00Z"), "已有到期时间不得被覆写");

        cleanup(conn, path);
    }

    #[test]
    fn active_cards_and_room_stats_ignore_admin_cards() {
        let (conn, path) = temp_db();
        let room = insert_room(&conn, "401", Some(41), None, None, None).unwrap();
        // 一张客卡
        record_issuance(&conn, &sample_issuance(Some("401"), "日租客卡", 1, 0)).unwrap();
        // 一张挂在房间上的总控卡（极端情况：将来管理卡带房号）
        let master = record_issuance(&conn, &sample_issuance(Some("401"), "总控卡", 30, 0)).unwrap();
        // 确保它有 expires_at，证明过滤不是靠 NULL 侥幸
        let master_expires: Option<String> = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![master], |r| r.get(0))
            .unwrap();
        assert!(master_expires.is_some());

        assert_eq!(count_active_cards(&conn, "401").unwrap(), 1, "总控卡不计入房间有效卡");
        assert_eq!(room.active_cards, 0, "新建房间初始为 0");

        let rooms = list_rooms(&conn).unwrap();
        let r = rooms.iter().find(|r| r.room_no == "401").unwrap();
        assert_eq!(r.active_cards, 1, "房间格子只数客卡");

        cleanup(conn, path);
    }

    #[test]
    fn expiring_cards_excludes_admin_kinds() {
        let (conn, path) = temp_db();
        // 一条 30 分钟后到期的总控卡（带房号，模拟最坏情况）
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
             VALUES (?1, '402', '总控卡', 1, 0, 901, 'ok',
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+30 minutes'))",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();

        let cards = expiring_cards(&conn, 60).unwrap();
        assert!(cards.is_empty(), "总控卡不该出现在到期提醒里");
        cleanup(conn, path);
    }

    /// 换房取源房"能带走什么"：只认仍有效的客卡，过期卡/管理卡都不能带。
    #[test]
    fn latest_active_guest_card_skips_expired_and_admin() {
        let (conn, path) = temp_db();
        insert_room(&conn, "501", Some(51), None, None, None).unwrap();

        // 一张已过期的客卡
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
             VALUES (?1, '501', '日租客卡', 1, 0, 801, 'ok',
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 hours'))",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        assert!(
            latest_active_guest_card(&conn, "501").unwrap().is_none(),
            "已过期的卡不能带入换房"
        );

        // 一张有效客卡（带客人信息）
        let id = record_issuance(&conn, &sample_issuance(Some("501"), "日租客卡", 1, 0)).unwrap();
        update_guest_identity(
            &conn,
            id,
            &GuestIdentity {
                name: Some("张三"),
                ..Default::default()
            },
        )
        .unwrap();
        // 一张挂在该房的总控卡：不能盖过客卡
        record_issuance(&conn, &sample_issuance(Some("501"), "总控卡", 30, 0)).unwrap();

        let latest = latest_active_guest_card(&conn, "501").unwrap().expect("应查到有效客卡");
        assert_eq!(latest.guest_name.as_deref(), Some("张三"));
        assert_eq!(latest.kind, "日租客卡");

        // 换房核对用的卡号集合：只含有效客卡（过期卡 801 与总控卡都不能进来）
        let nos = active_guest_card_nos(&conn, "501").unwrap();
        assert_eq!(nos, vec![1], "只应有那张有效客卡（sample_issuance 的卡号是 1）");

        cleanup(conn, path);
    }

    #[test]
    fn expiring_cards_window_and_dedupe() {
        let (conn, path) = temp_db();
        let insert = |offset: &str, room: &str, kind: &str, card: i64| {
            conn.execute(
                "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
                 VALUES (?1, ?2, ?3, 1, 0, ?4, 'ok', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?5))",
                params![uuid::Uuid::new_v4().to_string(), room, kind, card, offset],
            )
            .unwrap();
        };

        insert("-2 days", "301", "日租客卡", 10); // 太旧，出窗
        insert("-30 minutes", "302", "日租客卡", 11); // 刚过期，要提醒
        insert("+30 minutes", "303", "日租客卡", 12); // 即将到期，要提醒
        insert("+2 hours", "304", "日租客卡", 13); // 还早，出窗

        // 同房间同卡型重发：只保留最新一条
        insert("-20 minutes", "305", "日租客卡", 14);
        insert("-5 minutes", "305", "日租客卡", 15);

        let list = expiring_cards(&conn, 60).unwrap();
        let rooms: Vec<&str> = list.iter().filter_map(|c| c.room_no.as_deref()).collect();
        assert!(!rooms.contains(&"301"));
        assert!(!rooms.contains(&"304"));
        assert!(rooms.contains(&"302"));
        assert!(rooms.contains(&"303"));
        assert_eq!(rooms.iter().filter(|r| **r == "305").count(), 1, "同房同卡型应去重");
        assert_eq!(list.last().unwrap().room_no.as_deref(), Some("303"), "按到期时间升序");

        cleanup(conn, path);
    }

    #[test]
    fn department_crud_and_member_count() {
        let (conn, path) = temp_db();
        let d = insert_department(&conn, "客房部", Some("前台")).unwrap();
        assert_eq!(d.member_count, 0);

        insert_operator(&conn, "alice", "爱丽丝", "frontdesk", "hash", Some(d.id)).unwrap();
        assert_eq!(list_departments(&conn).unwrap()[0].member_count, 1);

        rename_department(&conn, d.id, "前厅部").unwrap();
        assert_eq!(list_departments(&conn).unwrap()[0].name, "前厅部");

        set_department_active(&conn, d.id, false).unwrap();
        assert!(!list_departments(&conn).unwrap()[0].active);

        cleanup(conn, path);
    }

    /// 改房号：序号不变，重名必须拒绝。
    #[test]
    fn rename_room_keeps_index_and_rejects_duplicate() {
        let (conn, path) = temp_db();
        let a = insert_room(&conn, "220", None, None, None, None).unwrap();
        insert_room(&conn, "221", None, None, None, None).unwrap();

        rename_room(&conn, "220", "230").unwrap();
        let renamed = find_room(&conn, "230").unwrap().expect("改名后应能按新房号查到");
        assert_eq!(renamed.room_index, a.room_index, "序号不应变化");
        assert!(find_room(&conn, "220").unwrap().is_none());

        assert!(rename_room(&conn, "230", "221").is_err(), "重名应被拒绝");
        assert!(rename_room(&conn, "999", "231").is_err(), "不存在的房号应报错");

        cleanup(conn, path);
    }

    #[test]
    fn room_type_assign_and_delete_guard() {
        let (conn, path) = temp_db();
        let t = insert_room_type(&conn, "大床房", 10).unwrap();
        insert_room(&conn, "220", None, None, None, Some(t.id)).unwrap();

        let rooms = list_rooms(&conn).unwrap();
        assert_eq!(rooms[0].room_type_id, Some(t.id));
        assert_eq!(rooms[0].room_type_name.as_deref(), Some("大床房"));
        assert_eq!(list_room_types(&conn).unwrap()[0].room_count, 1);

        assert!(delete_room_type(&conn, t.id).is_err(), "在用房型不能删");
        set_room_type(&conn, "220", None).unwrap();
        delete_room_type(&conn, t.id).expect("解绑后应可删除");

        cleanup(conn, path);
    }

    /// 新房型必须自带颜色，且不要和已有房型撞成同一个（调色板够用时）。
    #[test]
    fn new_room_type_gets_unused_palette_color() {
        let (conn, path) = temp_db();
        let mut colors = Vec::new();
        for name in ["大床房", "标间", "亲子房", "套房", "特价房"] {
            let t = insert_room_type(&conn, name, 0).unwrap();
            let c = t.color.expect("新房型应有颜色");
            assert!(ROOM_TYPE_PALETTE.contains(&c.as_str()), "颜色应取自调色板：{c}");
            assert!(!colors.contains(&c), "调色板够用时不应撞色：{c}");
            colors.push(c);
        }
        cleanup(conn, path);
    }

    #[test]
    fn operator_management_and_admin_count() {
        let (conn, path) = temp_db();
        assert_eq!(count_operators(&conn).unwrap(), 0);

        let admin = insert_operator(&conn, "admin", "管理员", "admin", "h1", None).unwrap();
        insert_operator(&conn, "front", "前台", "frontdesk", "h2", None).unwrap();
        assert_eq!(count_operators(&conn).unwrap(), 2);
        assert_eq!(count_active_admins(&conn).unwrap(), 1);

        update_operator_password(&conn, admin.id, "h3").unwrap();
        assert_eq!(find_operator(&conn, "admin").unwrap().unwrap().pwd_hash, "h3");

        set_operator_active(&conn, admin.id, false).unwrap();
        assert_eq!(count_active_admins(&conn).unwrap(), 0);

        let list = list_operators(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|o| o.login == "front" && o.active));
        assert!(list.iter().any(|o| o.login == "admin" && !o.active));

        cleanup(conn, path);
    }

    #[test]
    fn room_max_cards_set_and_validate() {
        let (conn, path) = temp_db();
        insert_room(&conn, "220", None, None, None, None).unwrap();
        assert_eq!(find_room(&conn, "220").unwrap().unwrap().max_cards, None);

        set_room_max_cards(&conn, "220", Some(2)).unwrap();
        assert_eq!(find_room(&conn, "220").unwrap().unwrap().max_cards, Some(2));

        // 0 和 100 都越界；None 表示不限
        assert!(set_room_max_cards(&conn, "220", Some(0)).is_err());
        assert!(set_room_max_cards(&conn, "220", Some(100)).is_err());
        set_room_max_cards(&conn, "220", None).unwrap();
        assert_eq!(find_room(&conn, "220").unwrap().unwrap().max_cards, None);

        cleanup(conn, path);
    }

    #[test]
    fn active_card_count_ignores_expired_and_dedupes() {
        let (conn, path) = temp_db();
        insert_room(&conn, "220", None, None, None, None).unwrap();
        let mk = |card: i64, offset: &str| {
            conn.execute(
                "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
                 VALUES (?1, '220', '日租客卡', 1, 0, ?2, 'ok',
                         strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?3))",
                params![uuid::Uuid::new_v4().to_string(), card, offset],
            )
            .unwrap();
        };
        mk(1, "+1 day");
        mk(2, "+2 hours");
        mk(3, "-1 hour"); // 已过期，不算
        mk(1, "+3 days"); // 同一张卡重复发，按卡号去重

        assert_eq!(count_active_cards(&conn, "220").unwrap(), 2);
        cleanup(conn, path);
    }

    #[test]
    fn room_status_seeded_and_customizable() {
        let (conn, path) = temp_db();
        let list = list_room_statuses(&conn).unwrap();
        assert_eq!(list.len(), 7, "七种房态应已内置");
        assert!(list.iter().any(|s| s.key == "vacant_clean" && s.label == "空净房"));
        assert!(list.iter().any(|s| s.key == "maintenance" && s.label == "维修房"));

        update_room_status_style(&conn, "vacant_clean", "Tool", "#123456").unwrap();
        let s = list_room_statuses(&conn)
            .unwrap()
            .into_iter()
            .find(|s| s.key == "vacant_clean")
            .unwrap();
        assert_eq!(s.icon, "Tool");
        assert_eq!(s.color, "#123456");

        assert!(update_room_status_style(&conn, "nope", "Tool", "#ffffff").is_err());
        assert!(
            update_room_status_style(&conn, "vacant_clean", "Tool", "red").is_err(),
            "颜色必须是 #RRGGBB"
        );

        cleanup(conn, path);
    }

    #[test]
    fn checkout_closes_cards_and_marks_room_dirty() {
        let (conn, path) = temp_db();
        insert_room(&conn, "220", None, None, None, None).unwrap();

        let mk = |card: i64, offset: &str| {
            conn.execute(
                "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
                 VALUES (?1, '220', '日租客卡', 1, 0, ?2, 'ok',
                         strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?3))",
                params![uuid::Uuid::new_v4().to_string(), card, offset],
            )
            .unwrap();
        };
        mk(1, "+1 day");
        mk(2, "+2 hours");
        mk(3, "-1 hour"); // 已过期，不该被算作"退房关闭"

        assert_eq!(count_active_cards(&conn, "220").unwrap(), 2);
        assert_eq!(find_room(&conn, "220").unwrap().unwrap().status, "vacant_clean");

        let closed = checkout_room(&conn, "220", "vacant_dirty").unwrap();
        assert_eq!(closed, 2, "只关闭仍有效的卡");
        assert_eq!(count_active_cards(&conn, "220").unwrap(), 0);
        assert_eq!(
            find_room(&conn, "220").unwrap().unwrap().status,
            "vacant_dirty"
        );

        cleanup(conn, path);
    }

    #[test]
    fn checkout_card_closes_only_that_card() {
        let (conn, path) = temp_db();
        insert_room(&conn, "205", None, None, None, None).unwrap();

        let mk = |card: i64, offset: &str| {
            conn.execute(
                "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
                 VALUES (?1, '205', '日租客卡', 1, 0, ?2, 'ok',
                         strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?3))",
                params![uuid::Uuid::new_v4().to_string(), card, offset],
            )
            .unwrap();
        };
        mk(0x2BCE, "+1 day");
        mk(0x2405, "+1 day");

        assert_eq!(count_active_cards(&conn, "205").unwrap(), 2);

        // 退掉其中一张：房间还剩 1 张，房态不应变脏
        let (room, remaining) = checkout_card(&conn, 0x2BCE, "vacant_dirty").unwrap().unwrap();
        assert_eq!(room, "205");
        assert_eq!(remaining, 1);
        assert_eq!(count_active_cards(&conn, "205").unwrap(), 1);
        assert_eq!(find_room(&conn, "205").unwrap().unwrap().status, "vacant_clean");

        // 再退最后一张：房态转空脏
        let (_, remaining) = checkout_card(&conn, 0x2405, "vacant_dirty").unwrap().unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(find_room(&conn, "205").unwrap().unwrap().status, "vacant_dirty");

        // 没有发卡记录的卡号
        assert!(checkout_card(&conn, 0x9999, "vacant_dirty").unwrap().is_none());

        cleanup(conn, path);
    }
}
