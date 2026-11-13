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

/// 可分配给真实房间的最大序号。
///
/// 卡内序号是 **12 bit**（`页1[2]` 的高 4 位 + `页1[3]` 整字节），值域 0–4095。
/// 其中 **4095 与总控卡字节相同**（页1 = `5B EC F0 FF`），格式上无法区分，
/// 读卡侧只能按总控卡处理——所以业务层绝不能把 4095 分配给真实房间，
/// 否则该房间的客卡会被读成总控卡。有效可分配范围是 **0–4094**。
pub const MAX_ROOM_INDEX: u16 = 4094;

/// 总控卡占用的序号（12 位全 1）。只用于判定，**不要**分配给房间。
pub const MASTER_ROOM_INDEX: u16 = 4095;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub room_no: String,
    /// 锁内序号 0–4094（12 bit）。4095 与总控卡字节相同，永不分给房间。
    pub room_index: u16,
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
    /// 当前 **open 登记单**的住客数；没有 open 单时为 0。
    /// 与「有效卡」是两个独立口径：登记不等于发卡，人数也不等于卡数。
    pub guest_count: i64,
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
    pub raw_hex: Option<String>,
    pub operator_id: Option<i64>,
    pub result: String,
    pub error_code: Option<i64>,
    pub created_at: String,
    /// 到期时间（UTC ISO8601）。按发卡时间 + days/hours 推算的估算值
    /// （锁按卡内发卡时间计算；日租另受锁内退房钟点影响）。凡 days/hours>0 的卡型都有值，
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
    apply_v8(conn)?;
    apply_v9(conn)?;
    apply_v10(conn)?;
    apply_v11(conn)?;
    apply_v12(conn)?;
    apply_v13(conn)?;
    apply_v14(conn)?;
    apply_v15(conn)?;
    apply_v16(conn)?;
    Ok(())
}

// ---------------------------------------------------------------- 备份 / 恢复

/// 备份文件的内容摘要。恢复前必须先用它确认「这确实是我们自己的库」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// schema_version 表里的最大版本号（老库可能是 3、5 等）
    pub schema_version: i64,
    /// 业务表数量（`room` / `issuance` / `operator` 等都在才算好库）
    pub tables: i64,
    pub rooms: i64,
    pub issuances: i64,
    pub operators: i64,
}

/// 本程序能读到的最高 schema 版本。恢复时用来拒绝「来自更新版本的备份」。
pub const SCHEMA_VERSION: i64 = 16;

/// 把当前库**在线备份**到指定文件。
///
/// 用 SQLite 的在线备份 API（`rusqlite::backup`），不是文件拷贝：
/// 库是 WAL 模式，直接 `copy` 会漏掉还在 `-wal` 里没 checkpoint 的事务。
/// 备份文件建成后强制 `journal_mode=DELETE`，保证它是**单个自包含文件**，
/// 拷到别的机器上能直接用只读连接打开。
pub fn backup_to(conn: &Connection, path: &Path) -> Result<u64> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }

    let mut dst = Connection::open(path)?;
    {
        let backup = rusqlite::backup::Backup::new(conn, &mut dst)?;
        backup.run_to_completion(128, Duration::from_millis(5), None)?;
    }
    // 备份文件不保留 WAL，避免出现「db + db-wal」两个文件才算完整
    let _: String = dst.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
    drop(dst);

    Ok(std::fs::metadata(path)?.len())
}

/// 只读检查一个备份文件是否可用（不动当前库）。
pub fn inspect_backup(path: &Path) -> Result<BackupInfo> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    for table in ["room", "issuance", "operator", "schema_version"] {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |r| r.get(0),
        )?;
        if n == 0 {
            return Err(StoreError::Invalid(format!(
                "不是 OpenCardWright 的备份：缺少 {table} 表"
            )));
        }
    }

    let schema_version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;
    let tables: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    let count = |table: &str| -> Result<i64> {
        Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
    };

    Ok(BackupInfo {
        schema_version,
        tables,
        rooms: count("room")?,
        issuances: count("issuance")?,
        operators: count("operator")?,
    })
}

/// 用备份文件**覆盖**当前库的内容（同一连接，不换文件）。
///
/// 调用方必须先 `inspect_backup` 校验、并把当前库另存为 `.bak`——
/// 这个函数一旦成功，旧数据就没了。恢复后建议再跑一次 [`migrate`]。
pub fn restore_from(conn: &mut Connection, path: &Path) -> Result<()> {
    let src = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let backup = rusqlite::backup::Backup::new(&src, conn)?;
    backup.run_to_completion(128, Duration::from_millis(5), None)?;
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

/// v7 → v8：预定日期区间。
///
/// 表本身在 schema.sql 里用 `CREATE TABLE IF NOT EXISTS` 建（每次 migrate 都会执行），
/// 这里只登记版本号；不涉及旧列改写，升级无损。
fn apply_v8(conn: &Connection) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (8)", [])?;
    Ok(())
}

/// v8 → v9：预定状态加「checked_in」（已入住，TD-128）——发卡后自动消除预定信息，
/// 保留记录且与「取消」区分。SQLite 不能修改 CHECK 约束，按官方流程建新表迁数据再换名；
/// 幂等：库里的建表 SQL 已含 checked_in（新库）就只登记版本号。
/// v10：身份证读卡扩展字段（有效期起止 / 签发机关 / 设备型号序列）。
/// **头像 base64 不入库**，因此这里只加 5 个文本列；列已存在时跳过（幂等）。
fn apply_v10(conn: &Connection) -> Result<()> {
    let cols: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('issuance') WHERE name = 'id_valid_from'",
        [],
        |r| r.get(0),
    )?;
    if cols == 0 {
        conn.execute_batch(
            "ALTER TABLE issuance ADD COLUMN id_valid_from TEXT;
             ALTER TABLE issuance ADD COLUMN id_valid_to TEXT;
             ALTER TABLE issuance ADD COLUMN id_authority TEXT;
             ALTER TABLE issuance ADD COLUMN id_device_model TEXT;
             ALTER TABLE issuance ADD COLUMN id_device_serial TEXT;",
        )?;
    }
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (10)", [])?;
    Ok(())
}

/// v11 → v12：按卡内原始数据（raw_hex）匹配发卡记录，补查询索引。
fn apply_v12(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_issuance_raw_hex ON issuance (raw_hex);
         INSERT OR IGNORE INTO schema_version (version) VALUES (12);",
    )?;
    Ok(())
}

/// v13 → v14：入住登记单（TD-220 / TD-221）。
///
/// 建 `checkin` / `checkin_guest`（`schema.sql` 已建，这里保证老库也补齐列与索引），
/// 并把历史上「发卡时填过身份信息」的记录迁移成「一单一客」：
/// 每条 `guest_name` / `guest_id_number` 非空的 `issuance` → 一张 `closed` 单 + 一条 `seq = 1` 住客，
/// 再回填 `issuance.checkin_id`。幂等靠 `checkin_id IS NULL`，重复启动不会重复建单。
fn apply_v14(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "issuance",
        "checkin_id",
        "ALTER TABLE issuance ADD COLUMN checkin_id INTEGER REFERENCES checkin (id)",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_checkin_room_status ON checkin (room_no, status);
         CREATE INDEX IF NOT EXISTS idx_checkin_guest_seq ON checkin_guest (checkin_id, seq);",
    )?;

    let legacy: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM issuance
              WHERE checkin_id IS NULL
                AND (   (guest_name IS NOT NULL AND TRIM(guest_name) != '')
                     OR (guest_id_number IS NOT NULL AND TRIM(guest_id_number) != ''))",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        out
    };

    for issuance_id in legacy {
        conn.execute(
            "INSERT INTO checkin
                 (room_no, check_in_at, check_out_at, status, operator_id, note, created_at)
             SELECT COALESCE(room_no, ''), created_at, checked_out_at, 'closed',
                    operator_id, '历史数据迁移（TD-221）', created_at
               FROM issuance WHERE id = ?1",
            params![issuance_id],
        )?;
        let checkin_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO checkin_guest
                 (checkin_id, seq, name, gender, id_type, id_number, nation, address,
                  birth_date, note, id_valid_from, id_valid_to, id_authority,
                  id_device_model, id_device_serial)
             SELECT ?2, 1, guest_name, guest_gender, guest_id_type, guest_id_number,
                    guest_nation, guest_address, guest_birth_date, guest_note,
                    id_valid_from, id_valid_to, id_authority,
                    id_device_model, id_device_serial
               FROM issuance WHERE id = ?1",
            params![issuance_id, checkin_id],
        )?;
        conn.execute(
            "UPDATE issuance SET checkin_id = ?2 WHERE id = ?1",
            params![issuance_id, checkin_id],
        )?;
    }

    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (14)", [])?;
    Ok(())
}

/// v14 → v15：房间序号从 8 位放宽到 12 位（0–4094）。
///
/// SQLite 不支持修改 `CHECK` 约束，只能按官方推荐流程重建表：
/// `CREATE room_v15` → `INSERT ... SELECT` → `DROP room` → `RENAME` → 重建索引。
///
/// room 表**没有被任何表用外键引用**（`issuance` / `checkin` / `reservation` 的
/// `room_no` 都是裸 `TEXT`），所以 `DROP TABLE` 不会触发外键错误，也不会牵连别的表。
///
/// 幂等：靠 `sqlite_master` 里 room 表的 DDL 判断——老库是 `room_index <= 255`，
/// 新库是 `<= 4094`，已经迁过就直接返回。
/// v15 → v16：登记单可以挂上「由哪条预定转正而来」。
///
/// 只加一列（可空），旧库既有 `checkin` 行保持 NULL；不动任何已有数据。
fn apply_v16(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "checkin", "reservation_id")? {
        conn.execute(
            "ALTER TABLE checkin ADD COLUMN reservation_id INTEGER REFERENCES reservation (id)",
            [],
        )?;
    }
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (16)", [])?;
    Ok(())
}

fn apply_v15(conn: &Connection) -> Result<()> {
    let ddl: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'room'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(ddl) = ddl else {
        // 表还没建（正常不会发生：SCHEMA 已经先跑过），没什么可迁的
        conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (15)", [])?;
        return Ok(());
    };
    if !ddl.contains("room_index <= 255") {
        // 已经是 4094 上界（新库，或迁移过）：仍要打版本标记，
        // 否则 `inspect_backup` 看到的库版本会停在 14。
        conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (15)", [])?;
        return Ok(());
    }

    // 列清单必须与 v14 的 room 表完全一致（含 v2 之后 ALTER 加的三列），
    // 少写一列就是静默丢数据。
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE room_v15 (
             room_no         TEXT PRIMARY KEY,
             room_index      INTEGER NOT NULL UNIQUE CHECK (room_index >= 0 AND room_index <= 4094),
             building        TEXT,
             floor           INTEGER,
             room_type       TEXT,
             active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
             install_card_at TEXT,
             created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
             room_type_id    INTEGER REFERENCES room_type (id),
             max_cards       INTEGER,
             status          TEXT NOT NULL DEFAULT 'vacant_clean'
         );
         INSERT INTO room_v15
             (room_no, room_index, building, floor, room_type, active,
              install_card_at, created_at, room_type_id, max_cards, status)
         SELECT room_no, room_index, building, floor, room_type, active,
                install_card_at, created_at, room_type_id, max_cards, status
           FROM room;
         DROP TABLE room;
         ALTER TABLE room_v15 RENAME TO room;
         CREATE INDEX IF NOT EXISTS idx_room_index ON room (room_index);",
    )?;
    tx.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (15)", [])?;
    tx.commit()?;
    Ok(())
}

/// v12 → v13：客卡作废（TD-218）。
///
/// 挂失卡写卡成功后，同房仍有效的旧客卡会在 `invalidated_at` 打时间戳，
/// `invalid_reason` 记原因（当前只有 `lost`）。与 `checked_out_at` 区分语义：
/// 退房是客人离开，作废是卡被挂失顶掉；两者都不计入「有效卡」。
fn apply_v13(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "issuance",
        "invalidated_at",
        "ALTER TABLE issuance ADD COLUMN invalidated_at TEXT",
    )?;
    ensure_column(
        conn,
        "issuance",
        "invalid_reason",
        "ALTER TABLE issuance ADD COLUMN invalid_reason TEXT",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_issuance_invalidated ON issuance (invalidated_at);
         INSERT OR IGNORE INTO schema_version (version) VALUES (13);",
    )?;
    Ok(())
}

/// v10 → v11：预定增加入住/退房时间（HH:mm），默认 12:00 / 12:00。
/// 旧数据按整夜保留原显示；新库 schema.sql 已含列时只登记版本。
fn apply_v11(conn: &Connection) -> Result<()> {
    let cols: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('reservation') WHERE name = 'check_in_time'",
        [],
        |r| r.get(0),
    )?;
    if cols == 0 {
        conn.execute_batch(
            "ALTER TABLE reservation ADD COLUMN check_in_time TEXT NOT NULL DEFAULT '12:00';
             ALTER TABLE reservation ADD COLUMN check_out_time TEXT NOT NULL DEFAULT '12:00';",
        )?;
    }
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (11)", [])?;
    Ok(())
}

fn apply_v9(conn: &Connection) -> Result<()> {
    let ddl: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'reservation'",
            [],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    if ddl.map(|s| s.contains("checked_in")).unwrap_or(false) {
        conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (9)", [])?;
        return Ok(());
    }
    conn.execute_batch(
        "CREATE TABLE reservation_new (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            room_no      TEXT NOT NULL,
            check_in     TEXT NOT NULL,
            check_out    TEXT NOT NULL,
            guest_name   TEXT,
            note         TEXT,
            status       TEXT NOT NULL DEFAULT 'booked' CHECK (status IN ('booked', 'cancelled', 'checked_in')),
            operator_id  INTEGER REFERENCES operator (id),
            created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            cancelled_at TEXT
        );
        INSERT INTO reservation_new
            (id, room_no, check_in, check_out, guest_name, note, status,
             operator_id, created_at, updated_at, cancelled_at)
        SELECT id, room_no, check_in, check_out, guest_name, note, status,
               operator_id, created_at, updated_at, cancelled_at
          FROM reservation;
        DROP TABLE reservation;
        ALTER TABLE reservation_new RENAME TO reservation;
        CREATE INDEX IF NOT EXISTS idx_reservation_room_dates ON reservation (room_no, check_in, check_out);
    ")?;
    conn.execute("INSERT OR IGNORE INTO schema_version (version) VALUES (9)", [])?;
    Ok(())
}

// ---------------------------------------------------------------- 房间

fn row_to_room(r: &Row<'_>) -> rusqlite::Result<Room> {
    Ok(Room {
        room_no: r.get(0)?,
        room_index: room_index_column(r, 1)?,
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
        guest_count: r.get(13)?,
    })
}

/// 读 `room_index` 列（12 bit）。
///
/// **不要用 `as u16` 截断**：那会把 4096 静默变成 0、把负数变成天文数字，
/// 而序号错了等于把卡发到别的房间。越界就是数据损坏，必须报错。
fn room_index_column(r: &Row<'_>, idx: usize) -> rusqlite::Result<u16> {
    let raw: i64 = r.get(idx)?;
    u16::try_from(raw).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(idx, raw))
}

const ROOM_SELECT: &str = "SELECT r.room_no, r.room_index, r.building, r.floor, r.room_type_id, \
                                  t.name, r.max_cards, r.status, r.active, r.install_card_at, r.created_at, \
                                  (SELECT COUNT(*) FROM issuance i \
                                    WHERE i.room_no = r.room_no AND i.result = 'ok' \
                                      AND i.expires_at IS NOT NULL \
                                      AND i.checked_out_at IS NULL \
                                      AND i.invalidated_at IS NULL \
                                      AND i.kind IN ('日租客卡','钟点客卡') \
                                      AND i.expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')), \
                                  (SELECT MAX(i.expires_at) FROM issuance i \
                                    WHERE i.room_no = r.room_no AND i.result = 'ok' \
                                      AND i.expires_at IS NOT NULL \
                                      AND i.checked_out_at IS NULL \
                                      AND i.invalidated_at IS NULL \
                                      AND i.kind IN ('日租客卡','钟点客卡') \
                                      AND i.expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')), \
                                  (SELECT COUNT(*) FROM checkin_guest g \
                                    JOIN checkin c ON c.id = g.checkin_id \
                                    WHERE c.room_no = r.room_no AND c.status = 'open') \
                             FROM room r LEFT JOIN room_type t ON t.id = r.room_type_id";

pub fn list_rooms(conn: &Connection) -> Result<Vec<Room>> {
    let sql = format!("{ROOM_SELECT}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_room)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    // TD-213：房号优先，纯数字按数值排；非数字按字符串排；同房号再按序号。
    out.sort_by(|a, b| {
        let an = a.room_no.parse::<u64>().ok();
        let bn = b.room_no.parse::<u64>().ok();
        match (an, bn) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.room_no.cmp(&b.room_no),
        }
        .then_with(|| a.room_index.cmp(&b.room_index))
    });
    Ok(out)
}

pub fn find_room(conn: &Connection, room_no: &str) -> Result<Option<Room>> {
    let sql = format!("{ROOM_SELECT} WHERE r.room_no = ?1");
    Ok(conn.query_row(&sql, params![room_no], row_to_room).optional()?)
}

pub fn find_room_by_index(conn: &Connection, room_index: u16) -> Result<Option<Room>> {
    let sql = format!("{ROOM_SELECT} WHERE r.room_index = ?1");
    Ok(conn
        .query_row(&sql, params![i64::from(room_index)], row_to_room)
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
    // 按「张」计数（TD-127）：同卡号多张（复用首卡号发给第二位客人）占多个名额，
    // 不再按卡号去重 —— 否则 max_cards 拦不住、房态也不会升到「已满」。
    let n: i64 = conn.query_row(
        "SELECT COUNT(*)
           FROM issuance
          WHERE room_no = ?1
            AND result = 'ok'
            AND expires_at IS NOT NULL
            AND checked_out_at IS NULL
            AND invalidated_at IS NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![room_no],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// 作废某房间仍有效的客卡（TD-218 挂失卡）。
///
/// 由 `issue_guest` 在**挂失卡写卡成功后**调用：
/// - 同房、客卡、未退房、未过期、未作废的旧记录打上 `invalidated_at` + `invalid_reason='lost'`；
/// - 新发的挂失卡本身打 `invalid_reason='lost_card'`（`invalidated_at` 同样置上），
///   这样它保留发卡记录与审计，但**不计入**有效卡数（TD-218 要求 activeCards 归零）。
///
/// 返回被作废的**旧卡**张数（不含挂失卡本人）。
pub fn invalidate_guest_cards(
    conn: &Connection,
    room_no: &str,
    new_card_id: i64,
) -> Result<usize> {
    let n = conn.execute(
        "UPDATE issuance
            SET invalidated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                invalid_reason = 'lost'
          WHERE room_no = ?1
            AND id != ?2
            AND result = 'ok'
            AND checked_out_at IS NULL
            AND invalidated_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![room_no, new_card_id],
    )?;
    conn.execute(
        "UPDATE issuance
            SET invalidated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                invalid_reason = 'lost_card'
          WHERE id = ?1",
        params![new_card_id],
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
            AND invalidated_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          ORDER BY created_at DESC, id DESC LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params![room_no], row_to_issuance)
        .optional()?)
}

// ---------------------------------------------------------------- 房态

/// 按发卡记录 id 退房：只关闭这一条记录；若该房已没有别的有效卡，再把房态置为 `dirty_status`。
///
/// 返回房号与剩余有效卡数（记录不存在 / 不是客卡 / 已退房时返回 `None`）。
pub fn checkout_issuance(
    conn: &Connection,
    id: i64,
    dirty_status: &str,
) -> Result<Option<(String, i64)>> {
    let room_no: Option<String> = conn
        .query_row(
            "SELECT room_no FROM issuance
              WHERE id = ?1 AND result = 'ok' AND room_no IS NOT NULL
                AND checked_out_at IS NULL AND invalidated_at IS NULL
                AND expires_at IS NOT NULL
                AND kind IN ('日租客卡','钟点客卡')",
            params![id],
            |r| r.get(0),
        )
        .optional()?;

    let Some(room_no) = room_no else {
        return Ok(None);
    };

    let n = conn.execute(
        "UPDATE issuance
            SET checked_out_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE id = ?1 AND checked_out_at IS NULL AND invalidated_at IS NULL",
        params![id],
    )?;
    if n == 0 {
        return Ok(None);
    }

    // 关联登记单：判据是「**这张登记单关联的卡**是否都退完」，与同房其它卡无关。
    // 不能等 remaining == 0 才处理——同房可能还有不属于这张登记单的卡。
    let checkin_id: Option<i64> = conn
        .query_row(
            "SELECT checkin_id FROM issuance WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten();
    if let Some(checkin_id) = checkin_id {
        let _ = close_checkin_if_all_cards_closed(conn, checkin_id)?;
    }

    let remaining = count_active_cards(conn, &room_no)?;
    // TD-214：退房后重算房态。
    // - 剩余 0：转退房默认态，但维修 / 其他等人工状态不覆盖；
    // - 仍有卡且当前是「已满房」：回退「未满房」；
    // - 其余（空净 / 预定 / 未满 / 人工态）：不动。
    let current = find_room(conn, &room_no)?.map(|r| r.status);
    let manual = matches!(current.as_deref(), Some("maintenance") | Some("other"));
    if remaining == 0 {
        if !manual {
            set_room_status(conn, &room_no, dirty_status)?;
        }
        // 退房联动（TD-220）：最后一张卡退掉后，关闭该房的 open 登记单
        let _ = close_open_checkin_for_room(conn, &room_no)?;
    } else if current.as_deref() == Some("occupied_full") {
        set_room_status(conn, &room_no, "occupied_partial")?;
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
            AND invalidated_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![room_no],
    )?;
    // 刚过期但还没标记的客卡也要一并关闭：expiring_cards 的提醒窗口是 [-10 分钟, +N 分钟]，
    // 不关掉它们，无卡退房后铃铛还会挂到自然出窗。
    conn.execute(
        "UPDATE issuance
            SET checked_out_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE room_no = ?1
            AND result = 'ok'
            AND checked_out_at IS NULL
            AND invalidated_at IS NULL
            AND expires_at IS NOT NULL
            AND kind IN ('日租客卡','钟点客卡')
            AND expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![room_no],
    )?;
    // TD-214：与单卡退房同一套重算逻辑 —— 批量退完后剩余必为 0，
    // 维修 / 其他等人工状态不覆盖。
    let current = find_room(conn, room_no)?.map(|r| r.status);
    if !matches!(current.as_deref(), Some("maintenance") | Some("other")) {
        set_room_status(conn, room_no, dirty_status)?;
    }
    // 退房联动（TD-220）：批量退房后关闭该房的 open 登记单
    let _ = close_open_checkin_for_room(conn, room_no)?;
    Ok(n)
}

/// 预定记录。日期是本地业务日期（`YYYY-MM-DD`），退房日不含。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reservation {
    pub id: i64,
    pub room_no: String,
    pub check_in: String,
    pub check_in_time: String,
    pub check_out: String,
    pub check_out_time: String,
    pub guest_name: Option<String>,
    pub note: Option<String>,
    pub status: String,
    pub operator_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub cancelled_at: Option<String>,
}

/// 新建 / 改期预定的入参。`check_out` 是退房日（不含）。
#[derive(Debug, Clone)]
pub struct ReservationInput {
    pub room_no: String,
    pub check_in: String,
    pub check_in_time: Option<String>,
    pub check_out: String,
    pub check_out_time: Option<String>,
    pub guest_name: Option<String>,
    pub note: Option<String>,
}

fn row_to_reservation(r: &Row<'_>) -> rusqlite::Result<Reservation> {
    Ok(Reservation {
        id: r.get(0)?,
        room_no: r.get(1)?,
        check_in: r.get(2)?,
        check_in_time: r.get(3)?,
        check_out: r.get(4)?,
        check_out_time: r.get(5)?,
        guest_name: r.get(6)?,
        note: r.get(7)?,
        status: r.get(8)?,
        operator_name: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        cancelled_at: r.get(12)?,
    })
}

const RESERVATION_SELECT: &str = "SELECT r.id, r.room_no, r.check_in, r.check_in_time, r.check_out, \
                                         r.check_out_time, r.guest_name, r.note, r.status, \
                                         o.display_name, r.created_at, r.updated_at, r.cancelled_at \
                                    FROM reservation r LEFT JOIN operator o ON o.id = r.operator_id";

fn is_valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return false;
    }
    let year: i32 = s[0..4].parse().unwrap_or(0);
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    if !(1970..=2200).contains(&year) || !(1..=12).contains(&month) {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    (1..=max_day).contains(&day)
}

fn today_local(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT date('now','localtime')", [], |r| r.get(0))?)
}

fn tomorrow_local(conn: &Connection, today: &str) -> Result<String> {
    Ok(conn.query_row("SELECT date(?1, '+1 day')", params![today], |r| r.get(0))?)
}

/// `HH:mm`，24 小时制，固定 5 字符。
fn is_valid_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return false;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 2 || c.is_ascii_digit())
    {
        return false;
    }
    let hour: u32 = s[0..2].parse().unwrap_or(99);
    let minute: u32 = s[3..5].parse().unwrap_or(99);
    hour < 24 && minute < 60
}

fn reservation_start(check_in: &str, check_in_time: &str) -> String {
    format!("{check_in}T{check_in_time}")
}

fn reservation_end(check_out: &str, check_out_time: &str) -> String {
    format!("{check_out}T{check_out_time}")
}

fn reservation_covers_today(
    conn: &Connection,
    check_in: &str,
    check_in_time: &str,
    check_out: &str,
    check_out_time: &str,
) -> Result<bool> {
    let today = today_local(conn)?;
    let tomorrow = tomorrow_local(conn, &today)?;
    let start = reservation_start(check_in, check_in_time);
    let end = reservation_end(check_out, check_out_time);
    Ok(start < format!("{tomorrow}T00:00") && end > format!("{today}T00:00"))
}

/// 校验日期、房号与同房区间重叠。`allow_past_check_in` 用于改期：
/// 允许把一条已经开始的预定改成今天退房，但创建时不允许入住日早于今天。
fn validate_reservation(
    conn: &Connection,
    input: &ReservationInput,
    exclude_id: Option<i64>,
    allow_past_check_in: bool,
) -> Result<()> {
    if input.room_no.trim().is_empty() {
        return Err(StoreError::Invalid("房号不能为空".into()));
    }
    let room_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM room WHERE room_no = ?1 AND active = 1",
        params![input.room_no],
        |r| r.get(0),
    )?;
    if room_exists == 0 {
        return Err(StoreError::Invalid(format!("房号不存在或已删除：{}", input.room_no)));
    }
    if !is_valid_date(&input.check_in) || !is_valid_date(&input.check_out) {
        return Err(StoreError::Invalid("日期格式应为 YYYY-MM-DD".into()));
    }
    let check_in_time = input.check_in_time.as_deref().unwrap_or("12:00");
    let check_out_time = input.check_out_time.as_deref().unwrap_or("12:00");
    if !is_valid_time(check_in_time) || !is_valid_time(check_out_time) {
        return Err(StoreError::Invalid("时间格式应为 HH:mm".into()));
    }
    let start = reservation_start(&input.check_in, check_in_time);
    let end = reservation_end(&input.check_out, check_out_time);
    if end <= start {
        return Err(StoreError::Invalid(
            "退房时间必须晚于入住时间（退房时间不占房）".into(),
        ));
    }
    let today = today_local(conn)?;
    if !allow_past_check_in && input.check_in < today {
        return Err(StoreError::Invalid("入住日期不能早于今天".into()));
    }

    let mut sql = String::from(
        "SELECT COUNT(*) FROM reservation
          WHERE room_no = ?1 AND status = 'booked'
            AND (check_in || 'T' || check_in_time) < ?2
            AND (check_out || 'T' || check_out_time) > ?3",
    );
    if exclude_id.is_some() {
        sql.push_str(" AND id != ?4");
    }
    let mut stmt = conn.prepare(&sql)?;
    let count: i64 = if let Some(id) = exclude_id {
        stmt.query_row(
            params![input.room_no, end, start, id],
            |r| r.get(0),
        )?
    } else {
        stmt.query_row(params![input.room_no, end, start], |r| r.get(0))?
    };
    if count > 0 {
        return Err(StoreError::Invalid("该房间在这个日期区间已有预定".into()));
    }
    Ok(())
}

/// 预定变化后同步「今天」的房态。
///
/// * 今天有有效预定且房间是空净 / 空脏 → 标成预定房；
/// * 原本覆盖今天、现在不再覆盖，且没有别的预定 → 只有当前是预定房才回退空净。
///
/// 在住/维修/已满不强行改，避免把在住房变成预定房。**到期不自动改房态**
/// （用户明确要求），这里只在创建/改期/取消这些主动操作后执行。
fn sync_room_status_for_reservation(
    conn: &Connection,
    room_no: &str,
    was_covering_today: bool,
) -> Result<()> {
    let today = today_local(conn)?;
    let tomorrow = tomorrow_local(conn, &today)?;
    let today_start = format!("{today}T00:00");
    let tomorrow_start = format!("{tomorrow}T00:00");
    let covering: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reservation
          WHERE room_no = ?1 AND status = 'booked'
            AND (check_in || 'T' || check_in_time) < ?2
            AND (check_out || 'T' || check_out_time) > ?3",
        params![room_no, tomorrow_start, today_start],
        |r| r.get(0),
    )?;
    let Some(room) = find_room(conn, room_no)? else {
        return Ok(());
    };
    if covering > 0 {
        if matches!(room.status.as_str(), "vacant_clean" | "vacant_dirty") {
            set_room_status(conn, room_no, "reserved")?;
        }
    } else if was_covering_today && room.status == "reserved" {
        set_room_status(conn, room_no, "vacant_clean")?;
    }
    Ok(())
}

pub fn find_reservation(conn: &Connection, id: i64) -> Result<Option<Reservation>> {
    let sql = format!("{RESERVATION_SELECT} WHERE r.id = ?1");
    Ok(conn
        .query_row(&sql, params![id], row_to_reservation)
        .optional()?)
}

/// 查询与 `[from, to)` 相交的有效预定；日期都是 `YYYY-MM-DD`。
pub fn list_reservations(
    conn: &Connection,
    from: &str,
    to: &str,
    room_no: Option<&str>,
) -> Result<Vec<Reservation>> {
    if !is_valid_date(from) || !is_valid_date(to) || to <= from {
        return Err(StoreError::Invalid("查询日期区间不合法".into()));
    }
    let mut sql = format!(
        "{RESERVATION_SELECT} WHERE r.status IN ('booked','checked_in') \
         AND (r.check_in || 'T' || r.check_in_time) < (date(?1, '+1 day') || 'T12:00') \
         AND (r.check_out || 'T' || r.check_out_time) > (date(?2, '-1 day') || 'T12:00')"
    );
    if room_no.is_some() {
        sql.push_str(" AND r.room_no = ?3");
    }
    sql.push_str(" ORDER BY r.check_in, r.room_no, r.id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(room) = room_no {
        stmt.query_map(params![to, from, room], row_to_reservation)?
    } else {
        stmt.query_map(params![to, from], row_to_reservation)?
    };
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// TD-206：客人姓名分词。jieba 切成词后去重、限 8 个；全空时回退原文。
fn guest_name_tokens(name: &str) -> Vec<String> {
    static JIEBA: std::sync::OnceLock<jieba_rs::Jieba> = std::sync::OnceLock::new();
    let jieba = JIEBA.get_or_init(jieba_rs::Jieba::new);
    let mut out: Vec<String> = Vec::new();
    for token in jieba.cut(name, true) {
        let token = token.trim();
        if token.is_empty()
            || token
                .chars()
                .all(|c| c.is_whitespace() || c.is_ascii_punctuation())
        {
            continue;
        }
        if !out.iter().any(|existing| existing == token) {
            out.push(token.to_string());
        }
        if out.len() >= 8 {
            break;
        }
    }
    if out.is_empty() {
        vec![name.to_string()]
    } else {
        out
    }
}

/// 记录页的预定查询条件（TD-167）。与日历用的 `list_reservations` 不同：
/// **包含已取消 / 已入住**，日期按 `check_in` 过滤，且分页。
#[derive(Debug, Clone, Default)]
pub struct ReservationFilter {
    pub room_no: Option<String>,
    pub guest_name: Option<String>,
    /// booked / checked_in / cancelled；None = 全部
    pub status: Option<String>,
    /// check_in >= from（YYYY-MM-DD）
    pub from: Option<String>,
    /// check_in < to（YYYY-MM-DD）
    pub to: Option<String>,
    pub page: i64,
    pub page_size: i64,
}

/// 记录页预定查询：房号前缀 / 客人姓名模糊 / 状态 / 入住日区间 + 分页，按入住日倒序。
pub fn search_reservations(
    conn: &Connection,
    f: &ReservationFilter,
) -> Result<Page<Reservation>> {
    use rusqlite::types::Value;

    let from = f.from.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let to = f.to.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if let Some(d) = from {
        if !is_valid_date(d) {
            return Err(StoreError::Invalid("起始日期不合法".into()));
        }
    }
    if let Some(d) = to {
        if !is_valid_date(d) {
            return Err(StoreError::Invalid("截止日期不合法".into()));
        }
    }
    if let (Some(a), Some(b)) = (from, to) {
        if b <= a {
            return Err(StoreError::Invalid("查询日期区间不合法".into()));
        }
    }

    let mut where_sql = String::from(" WHERE 1 = 1");
    let mut vals: Vec<Value> = Vec::new();
    if let Some(room) = f.room_no.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND r.room_no LIKE ? || '%'");
        vals.push(Value::Text(room.to_string()));
    }
    if let Some(name) = f.guest_name.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        // TD-206：jieba 分词后按词 OR 匹配；分词为空时回退原文整串。
        let tokens = guest_name_tokens(name);
        where_sql.push_str(" AND (");
        for (i, token) in tokens.iter().enumerate() {
            if i > 0 {
                where_sql.push_str(" OR ");
            }
            where_sql.push_str("r.guest_name LIKE '%' || ? || '%'");
            vals.push(Value::Text(token.clone()));
        }
        where_sql.push(')');
    }
    if let Some(status) = f.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !matches!(status, "booked" | "checked_in" | "cancelled") {
            return Err(StoreError::Invalid("预定状态不合法".into()));
        }
        where_sql.push_str(" AND r.status = ?");
        vals.push(Value::Text(status.to_string()));
    }
    if let Some(d) = from {
        where_sql.push_str(" AND r.check_in >= ?");
        vals.push(Value::Text(d.to_string()));
    }
    if let Some(d) = to {
        where_sql.push_str(" AND r.check_in < ?");
        vals.push(Value::Text(d.to_string()));
    }

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM reservation r{where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;

    let (size, offset) = limit_offset(f.page, f.page_size);
    let sql = format!(
        "{RESERVATION_SELECT}{where_sql} ORDER BY r.check_in DESC, r.id DESC LIMIT ? OFFSET ?"
    );
    let mut all = vals;
    all.push(Value::Integer(size));
    all.push(Value::Integer(offset));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(all.iter()), row_to_reservation)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(Page { rows: out, total })
}

pub fn create_reservation(
    conn: &Connection,
    input: &ReservationInput,
    operator_id: Option<i64>,
) -> Result<Reservation> {
    validate_reservation(conn, input, None, false)?;
    let check_in_time = input.check_in_time.as_deref().unwrap_or("12:00");
    let check_out_time = input.check_out_time.as_deref().unwrap_or("12:00");
    conn.execute(
        "INSERT INTO reservation
           (room_no, check_in, check_in_time, check_out, check_out_time, guest_name, note, operator_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.room_no.as_str(),
            input.check_in.as_str(),
            check_in_time,
            input.check_out.as_str(),
            check_out_time,
            input.guest_name.as_deref(),
            input.note.as_deref(),
            operator_id
        ],
    )?;
    let id = conn.last_insert_rowid();
    sync_room_status_for_reservation(conn, &input.room_no, false)?;
    find_reservation(conn, id)?
        .ok_or_else(|| StoreError::Invalid("预定写入后读不到".into()))
}

pub fn update_reservation(
    conn: &Connection,
    id: i64,
    input: &ReservationInput,
) -> Result<Reservation> {
    let old = find_reservation(conn, id)?
        .ok_or_else(|| StoreError::Invalid("预定不存在".into()))?;
    if old.status != "booked" {
        return Err(StoreError::Invalid("已取消的预定不能改期".into()));
    }
    validate_reservation(conn, input, Some(id), true)?;
    let old_covers_today = reservation_covers_today(
        conn,
        &old.check_in,
        &old.check_in_time,
        &old.check_out,
        &old.check_out_time,
    )?;
    let check_in_time = input.check_in_time.as_deref().unwrap_or("12:00");
    let check_out_time = input.check_out_time.as_deref().unwrap_or("12:00");
    conn.execute(
        "UPDATE reservation
            SET room_no = ?2, check_in = ?3, check_in_time = ?4,
                check_out = ?5, check_out_time = ?6,
                guest_name = ?7, note = ?8,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE id = ?1",
        params![
            id,
            input.room_no.as_str(),
            input.check_in.as_str(),
            check_in_time,
            input.check_out.as_str(),
            check_out_time,
            input.guest_name.as_deref(),
            input.note.as_deref()
        ],
    )?;
    sync_room_status_for_reservation(conn, &old.room_no, old_covers_today)?;
    if old.room_no != input.room_no {
        sync_room_status_for_reservation(conn, &input.room_no, false)?;
    }
    find_reservation(conn, id)?
        .ok_or_else(|| StoreError::Invalid("预定更新后读不到".into()))
}

pub fn cancel_reservation(conn: &Connection, id: i64) -> Result<Reservation> {
    let old = find_reservation(conn, id)?
        .ok_or_else(|| StoreError::Invalid("预定不存在".into()))?;
    if old.status != "booked" {
        return Ok(old);
    }
    let was_covering_today = reservation_covers_today(
        conn,
        &old.check_in,
        &old.check_in_time,
        &old.check_out,
        &old.check_out_time,
    )?;
    conn.execute(
        "UPDATE reservation
            SET status = 'cancelled',
                cancelled_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE id = ?1",
        params![id],
    )?;
    sync_room_status_for_reservation(conn, &old.room_no, was_covering_today)?;
    find_reservation(conn, id)?
        .ok_or_else(|| StoreError::Invalid("预定取消后读不到".into()))
}

/// 本地日期 `YYYY-MM-DD`（与预定 / 房态口径一致：SQLite `date('now','localtime')`）。
pub fn local_today(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?)
}

/// 发卡即入住（TD-128）：把该房覆盖 `today` 的有效预定标为「已入住」（checked_in）。
/// 口径与房态同步一致（TD-115）：check_in <= today < check_out。返回标记条数。
pub fn check_in_reservations(conn: &Connection, room_no: &str, today: &str) -> Result<usize> {
    let n = conn.execute(
        "UPDATE reservation
            SET status = 'checked_in', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE room_no = ?1 AND status = 'booked'
            AND (check_in || 'T' || check_in_time) < (date(?2, '+1 day') || 'T00:00')
            AND (check_out || 'T' || check_out_time) > (?2 || 'T00:00')",
        params![room_no, today],
    )?;
    Ok(n)
}

/// 把「该房今天覆盖的 booked 预定」转正：状态改 `checked_in`，并把它的 id 写进入住单。
///
/// 口径与发卡路径的 [`check_in_reservations`] 一致（覆盖今天、状态 `booked`）；
/// 差别是这里**只转一条**（按入住时间取最早），并回写 `checkin.reservation_id`，
/// 从而能回答「这条预定转成了哪张入住单」。同房今天还有别的预定时保持 `booked`，
/// 不会因为一次登记被一起吞掉。
///
/// 返回转正的预定 id；没有可转的预定返回 `None`。
pub fn convert_reservation_for_room(
    conn: &Connection,
    room_no: &str,
    today: &str,
    checkin_id: i64,
) -> Result<Option<i64>> {
    let reservation_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM reservation
              WHERE room_no = ?1 AND status = 'booked'
                AND (check_in || 'T' || check_in_time) < (date(?2, '+1 day') || 'T00:00')
                AND (check_out || 'T' || check_out_time) > (?2 || 'T00:00')
              ORDER BY check_in, check_in_time, id
              LIMIT 1",
            params![room_no, today],
            |r| r.get(0),
        )
        .optional()?;

    let Some(id) = reservation_id else {
        return Ok(None);
    };

    conn.execute(
        "UPDATE reservation
            SET status = 'checked_in', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE id = ?1 AND status = 'booked'",
        params![id],
    )?;
    conn.execute(
        "UPDATE checkin SET reservation_id = ?2 WHERE id = ?1",
        params![checkin_id, id],
    )?;
    Ok(Some(id))
}

/// 序号槽位数：0–4094 共 4095 个（数组长度必须是 const 表达式）。
const ROOM_INDEX_SLOTS: usize = MAX_ROOM_INDEX as usize + 1;

/// 0–4094 中未使用的最小序号。用尽返回 None。
///
/// 扫描上界是 [`MAX_ROOM_INDEX`]（4094），不是 255：卡内序号是 **12 位**，
/// 4095 要留给总控卡（见常量注释）。
pub fn next_free_room_index(conn: &Connection) -> Result<Option<u16>> {
    let mut used = [false; ROOM_INDEX_SLOTS];
    let mut stmt = conn.prepare("SELECT room_index FROM room")?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    for row in rows {
        let idx = row?;
        if (0..=i64::from(MAX_ROOM_INDEX)).contains(&idx) {
            used[idx as usize] = true;
        }
    }
    Ok((0..=MAX_ROOM_INDEX).find(|i| !used[usize::from(*i)]))
}

/// 新增房间。`room_index` 为 None 时自动分配最小可用序号。
pub fn insert_room(
    conn: &Connection,
    room_no: &str,
    room_index: Option<u16>,
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
    room_index: Option<u16>,
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
            .ok_or_else(|| StoreError::Invalid("房间序号已用尽（0–4094），无法再新增房间".into()))?,
    };

    // 数据层的最后一道闸：4095 与总控卡字节相同（页1 = `5B EC F0 FF`），
    // 分配出去会让该房间的客卡被读成总控卡。命令层也拦，但这里必须兜住——
    // 导入、复活、将来的批量接口都走这个函数。
    if index > MAX_ROOM_INDEX {
        return Err(StoreError::Invalid(format!(
            "房间序号 {index} 超出可分配范围（0–{MAX_ROOM_INDEX}）：{MASTER_ROOM_INDEX} 与总控卡字节相同，不能分配给房间"
        )));
    }

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
            params![i64::from(index), room_no, building, floor, room_type_id],
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
        params![room_no, i64::from(index), building, floor, room_type_id],
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
        raw_hex: r.get(6)?,
        operator_id: r.get(7)?,
        result: r.get(8)?,
        error_code: r.get(9)?,
        created_at: r.get(10)?,
        guest_name: r.get(11)?,
        guest_gender: r.get(12)?,
        guest_id_type: r.get(13)?,
        guest_id_number: r.get(14)?,
        guest_nation: r.get(15)?,
        guest_address: r.get(16)?,
        guest_birth_date: r.get(17)?,
        guest_note: r.get(18)?,
        expires_at: r.get(19)?,
    })
}

const ISSUANCE_COLUMNS: &str = "id, uuid, room_no, kind, days, hours, raw_hex, \
                                operator_id, result, error_code, created_at, \
                                guest_name, guest_gender, guest_id_type, guest_id_number, \
                                guest_nation, guest_address, guest_birth_date, guest_note, \
                                expires_at";

pub struct NewIssuance<'a> {
    pub room_no: Option<&'a str>,
    pub kind: &'a str,
    pub days: i64,
    pub hours: i64,
    /// 卡内作业流水号（16 位 `DDHHII`）。**没有流水号的卡型写 `None`**：
    /// 安装卡、空白卡、原始直写都不占号，记录里就是 NULL。
    ///
    /// 注意它不含年月——跨月同日同时同序会重复，所以**不要**拿它当唯一键；
    /// 发卡记录之间的关联键仍是 `raw_hex`（28 字节整卡指纹），主键是自增 `id`。
    pub card_no: Option<i64>,
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
    /// 身份证扩展字段（schema v10）。**头像不入库**。
    pub id_valid_from: Option<&'a str>,
    pub id_valid_to: Option<&'a str>,
    pub id_authority: Option<&'a str>,
    pub device_model: Option<&'a str>,
    pub device_serial: Option<&'a str>,
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
            && self.id_valid_from.is_none()
            && self.id_valid_to.is_none()
            && self.id_authority.is_none()
            && self.device_model.is_none()
            && self.device_serial.is_none()
    }

    /// 至少有姓名或证件号之一（TD-224：拒绝空白住客）。
    ///
    /// 只校验这两项：其余字段（性别 / 民族 / 住址 / 有效期…）都允许为空，
    /// 但一条既没姓名也没证件号的记录没有任何业务意义，落库后会变成前台看到的"空白住客"。
    pub fn has_identity(&self) -> bool {
        fn filled(v: Option<&str>) -> bool {
            v.map(str::trim).is_some_and(|s| !s.is_empty())
        }
        filled(self.name) || filled(self.id_number)
    }
}

/// 给一条发卡记录补上身份信息。**历史接口（TD-220 起停用）**：
/// 发卡不再收集身份，登记信息统一写 `checkin_guest`；字段保留只读，老备份仍可查询。
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
                guest_nation = ?6, guest_address = ?7, guest_birth_date = ?8, guest_note = ?9,
                id_valid_from = ?10, id_valid_to = ?11, id_authority = ?12,
                id_device_model = ?13, id_device_serial = ?14
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
            guest.id_valid_from,
            guest.id_valid_to,
            guest.id_authority,
            guest.device_model,
            guest.device_serial,
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
            new.card_no,
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
    // 日租客卡（TD-126）：到期 = 退房日（本地开卡日 + N 天）的「退房钟点」设置项（默认 12），
    // 不再按开卡时刻顺延 —— 18:00 开 1 天卡 → 次日 12:00 到期，而非次日 18:00。
    // 中国无夏令时、localtime 偏移固定（见 logging.rs 注释）；钟点卡 / 管理卡 / 复位仍走
    // expiry_modifier 顺延原逻辑，历史数据（backfill）不回溯，卡内时长码与锁的判定不受影响。
    // 已知差异：若锁内退房钟点与设置项不一致（没刷退房钟点卡、或设置改过但没重刷），
    // 库里的到期会与锁不同；锁内时间未用校时卡校准时，到期判定也会漂移。
    // 设置项只影响产品展示与统计，不改变卡内字节。
    let guest_checkout_hour = if new.kind == "日租客卡" && new.days > 0 {
        Some(
            get_setting(conn, "checkoutHour")?
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| (0..=23).contains(v))
                .unwrap_or(12),
        )
    } else {
        None
    };
    if let Some(hour) = guest_checkout_hour {
        // 本地「归属日 + N 天」的 HH:00，用 'utc' 修饰符转回 UTC 存储。
        // 不要用 unixepoch(...,'localtime')：SQLite 会把已带 'localtime' 的值二次转换
        //（实测本地 6 点变成 22 点），TD-126 回归用例会拦住。
        //
        // TD-225 凌晨房归属：本地开卡时刻早于 `morningCutoff`（默认 06:00）时，
        // 归属日取**前一天** —— 于是凌晨 02:30 开 1 天卡到期是「当天」12:00，
        // 而不是次日 12:00（凌晨房按当晚入住算）。
        let cutoff = get_morning_cutoff_minutes(conn)?;
        conn.execute(
            "UPDATE issuance
                SET expires_at = strftime(
                    '%Y-%m-%dT%H:%M:%SZ',
                    date(created_at, 'localtime',
                         CASE WHEN ( CAST(strftime('%H', created_at, 'localtime') AS INTEGER) * 60
                                   + CAST(strftime('%M', created_at, 'localtime') AS INTEGER) ) < ?2
                              THEN '-1 day' ELSE '+0 days' END,
                         '+' || ?3 || ' days')
                        || printf(' %02d:00:00', ?1),
                    'utc')
              WHERE id = ?4",
            params![hour, cutoff, new.days, id],
        )?;
    } else if let Some(modifier) = expiry_modifier(new.days, new.hours) {
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
/// 注意：这是按**发卡时间**算的估算值 —— 锁按卡内发卡时间计算到期：
/// 日租卡到发卡日 + N 天的锁内退房钟点，钟点卡到发卡时刻 + N 小时
///（见 docs/TEMIC门锁卡技术规格.md §11.1）。
/// 按新的退房钟点重算**仍有效**的日租客卡到期时间（TD-126 补充）。
///
/// 只动未退房、未作废、未过期、有到期时间的日租客卡：
/// - 已结束的记录保持原样 —— 审计用的历史到期时间不能被改写；
/// - 已过期的卡不「复活」—— 把钟点改晚也不会让过期卡重新计入有效卡。
///
/// 公式与 `record_issuance` 完全一致（含 TD-225 的凌晨房归属）。
/// 返回被重算的张数。
pub fn recalc_guest_day_expiry(conn: &Connection, hour: i64) -> Result<usize> {
    let cutoff = get_morning_cutoff_minutes(conn)?;
    let n = conn.execute(
        "UPDATE issuance
            SET expires_at = strftime(
                '%Y-%m-%dT%H:%M:%SZ',
                date(created_at, 'localtime',
                     CASE WHEN ( CAST(strftime('%H', created_at, 'localtime') AS INTEGER) * 60
                               + CAST(strftime('%M', created_at, 'localtime') AS INTEGER) ) < ?2
                          THEN '-1 day' ELSE '+0 days' END,
                     '+' || days || ' days')
                    || printf(' %02d:00:00', ?1),
                'utc')
          WHERE kind = '日租客卡'
            AND days > 0
            AND result = 'ok'
            AND checked_out_at IS NULL
            AND invalidated_at IS NULL
            AND expires_at IS NOT NULL
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![hour, cutoff],
    )?;
    Ok(n)
}

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
/// * 时间窗 `[now - 10 分钟, now + within_minutes]`：既提醒即将到期，也给
///   「过期瞬间没赶上轮询」留 10 分钟补提醒。**过期 10 分钟后必须出窗** ——
///   自然过期的卡没人退房（checked_out_at 为 NULL），窗口放到 1 天会让
///   「房间已经没有有效卡了」的铃铛多挂 24 小时（TD-111）；
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
            AND i.checked_out_at IS NULL
            AND i.invalidated_at IS NULL
            AND i.expires_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 minutes')
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

/// 按卡号里的时间戳（日 / 时 / 小时内序号）反查发卡记录。
///
/// - 客卡：`room_no = Some(...)`，同时按房号过滤；
/// - 总控卡：`room_no = None`，只按卡型 + 时间戳匹配。
///
/// `created_at` 存的是 UTC，这里用 SQLite 的 `localtime` 取本地日/时；
/// 同一小时内第 `seq` 张（0-based）对应按写入顺序的第 `seq + 1` 条。
/// 失败记录也占序号，所以不过滤 `result`，由调用方判断取到的记录是否可用。
pub fn find_issuance_by_stamp(
    conn: &Connection,
    room_no: Option<&str>,
    kind: &str,
    day: u16,
    hour: u16,
    seq: u16,
) -> Result<Option<IssuanceRecord>> {
    use rusqlite::types::Value;

    let mut where_sql = String::from(
        " WHERE kind = ?1 \
          AND CAST(strftime('%d', created_at, 'localtime') AS INTEGER) = ?2 \
          AND CAST(strftime('%H', created_at, 'localtime') AS INTEGER) = ?3",
    );
    let mut vals: Vec<Value> = vec![
        Value::Text(kind.to_string()),
        Value::Integer(day as i64),
        Value::Integer(hour as i64),
    ];
    if let Some(room) = room_no {
        where_sql.push_str(" AND room_no = ?");
        vals.push(Value::Text(room.to_string()));
    }
    let sql = format!(
        "SELECT {ISSUANCE_COLUMNS} FROM issuance{where_sql} \n         ORDER BY created_at, id LIMIT 1 OFFSET ?"
    );
    vals.push(Value::Integer(seq as i64));
    Ok(conn
        .query_row(
            &sql,
            rusqlite::params_from_iter(vals.iter()),
            row_to_issuance,
        )
        .optional()?)
}

/// 卡内原始数据的匹配结果。
#[derive(Debug, Clone)]
pub struct RawIssuanceMatch {
    pub latest: IssuanceRecord,
    /// 同一 raw_hex + 卡型 + 房号下的全部记录数（含已退房/失败）。
    pub count: u32,
    /// latest 是否仍有效（result='ok' 且未退房）。
    pub active: bool,
}

fn normalize_raw_hex(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// 按卡内 28 字节原始数据（raw_hex）匹配发卡记录。
///
/// 这是业务语义上的“卡内容凭据”：同一 raw_hex 视为同一张卡，不区分物理克隆。
/// 排序优先取仍有效的最新记录；没有有效记录时返回最新一条，由调用方判断。
pub fn find_issuance_by_raw(
    conn: &Connection,
    raw_hex: &str,
    kind: &str,
    room_no: Option<&str>,
) -> Result<Option<RawIssuanceMatch>> {
    use rusqlite::types::Value;

    let raw = normalize_raw_hex(raw_hex);
    if raw.is_empty() {
        return Ok(None);
    }
    let mut where_sql = String::from(" WHERE raw_hex = ?1 AND kind = ?2");
    let mut vals: Vec<Value> = vec![Value::Text(raw), Value::Text(kind.to_string())];
    if let Some(room) = room_no {
        where_sql.push_str(" AND room_no = ?");
        vals.push(Value::Text(room.to_string()));
    }

    let count: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM issuance{where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;
    if count == 0 {
        return Ok(None);
    }

    let sql = format!(
        "SELECT {ISSUANCE_COLUMNS} FROM issuance{where_sql} \
         ORDER BY CASE WHEN result = 'ok' AND checked_out_at IS NULL AND invalidated_at IS NULL THEN 0 ELSE 1 END, \
                  created_at DESC, id DESC LIMIT 1"
    );
    let latest = conn.query_row(
        &sql,
        rusqlite::params_from_iter(vals.iter()),
        row_to_issuance,
    )?;
    let active = issuance_is_active(conn, latest.id)?;
    Ok(Some(RawIssuanceMatch {
        latest,
        count: count as u32,
        active,
    }))
}

/// 记录是否仍有效（发卡成功且未退房）。
pub fn issuance_is_active(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT (result = 'ok' AND checked_out_at IS NULL AND invalidated_at IS NULL) FROM issuance WHERE id = ?1",
        params![id],
        |r| r.get::<_, i64>(0).map(|v| v != 0),
    )?)
}

/// 按主键取一条发卡记录。发卡命令刚写入后用它取回 expires_at 等完整字段。
pub fn find_issuance(conn: &Connection, id: i64) -> Result<Option<IssuanceRecord>> {
    let sql = format!("SELECT {ISSUANCE_COLUMNS} FROM issuance WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id], row_to_issuance)
        .optional()?)
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

/// 恢复备份前保存当前账号全字段（含密码哈希，仅供恢复流程内部使用）。
///
/// 恢复后按 `login` 与备份账号合并：当前库已有的账号保留密码 / 角色 / 启用状态。
pub fn snapshot_operators(conn: &Connection) -> Result<Vec<Operator>> {
    let sql = format!("{OPERATOR_SELECT} ORDER BY o.id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_operator)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 恢复备份后账号合并的计数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperatorMergeReport {
    /// 当前已有、备份也有：保留当前密码 / 角色 / 启用状态
    pub preserved: i64,
    /// 备份有、当前没有：导入但停用
    pub imported_disabled: i64,
    /// 当前有、备份没有：补回当前账号（部门置空）
    pub reinserted: i64,
    /// 单条更新 / 插入失败数
    pub failed: i64,
}

/// 恢复整库后，把恢复前的当前账号按 `login` 合并回来。
///
/// - 当前有、备份也有：只覆盖备份行的 `pwd_hash` / `role` / `active`，
///   显示名 / 部门 / `created_at` 用备份值，`operator.id` 沿用备份行（历史外键不变）；
/// - 当前有、备份没有：插入当前账号（新 id，`department_id = NULL`）；
/// - 备份有、当前没有：导入备份账号但 `active = 0`，由管理员手动启用。
///
/// 单条失败只累计 `failed`，不中断整次恢复。
pub fn merge_operators_after_restore(
    conn: &Connection,
    current: &[Operator],
) -> Result<OperatorMergeReport> {
    let after = snapshot_operators(conn)?;
    let mut report = OperatorMergeReport::default();

    for cur in current {
        if after.iter().any(|o| o.login == cur.login) {
            match conn.execute(
                "UPDATE operator SET pwd_hash = ?2, role = ?3, active = ?4 WHERE login = ?1",
                params![
                    cur.login,
                    cur.pwd_hash,
                    cur.role,
                    if cur.active { 1 } else { 0 }
                ],
            ) {
                Ok(n) if n > 0 => report.preserved += 1,
                _ => report.failed += 1,
            }
        } else {
            match conn.execute(
                "INSERT INTO operator
                   (login, display_name, role, pwd_hash, active, department_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![
                    cur.login,
                    cur.display_name,
                    cur.role,
                    cur.pwd_hash,
                    if cur.active { 1 } else { 0 }
                ],
            ) {
                Ok(_) => report.reinserted += 1,
                Err(_) => report.failed += 1,
            }
        }
    }

    for acc in &after {
        if !current.iter().any(|c| c.login == acc.login) {
            match conn.execute(
                "UPDATE operator SET active = 0 WHERE login = ?1",
                params![acc.login],
            ) {
                Ok(n) if n > 0 => report.imported_disabled += 1,
                _ => report.failed += 1,
            }
        }
    }

    Ok(report)
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
                (SELECT COUNT(*) FROM room r WHERE r.room_type_id = t.id AND r.active = 1)
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
        "SELECT COUNT(*) FROM room WHERE room_type_id = ?1 AND active = 1",
        params![id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(StoreError::Invalid(format!(
            "还有 {used} 个房间使用该房型，先把它们改到别的房型再删"
        )));
    }
    // 只剩已逻辑删除的房间在引用它：先摘掉引用再删，否则外键挡住（且这些房不展示，摘掉无副作用）
    conn.execute(
        "UPDATE room SET room_type_id = NULL WHERE room_type_id = ?1",
        params![id],
    )?;
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

/// 全局默认最大发卡数。**找不到 key 时返回 `2`**（产品默认值）。
pub fn get_max_cards_default(conn: &Connection) -> Result<i64> {
    let raw = get_setting(conn, "max_cards_default")?;
    Ok(raw
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(2)
        .max(0))
}

/// `HH:mm` → 一天内的分钟数（0–1439）；非法返回 `None`。
pub fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let h: i64 = h.trim().parse().ok()?;
    let m: i64 = m.trim().parse().ok()?;
    if (0..=23).contains(&h) && (0..=59).contains(&m) {
        Some(h * 60 + m)
    } else {
        None
    }
}

/// 凌晨房归属时间（TD-225）：一天内的分钟数，默认 `06:00` = 360。
///
/// 在截止时间**之前**开卡的日租客卡，归属日算作前一天 —— 于是凌晨 2 点开 1 天卡
/// 到期是「当天」12 点，而不是次日 12 点（凌晨房按当晚入住算）。
pub fn get_morning_cutoff_minutes(conn: &Connection) -> Result<i64> {
    let raw = get_setting(conn, "morningCutoff")?;
    Ok(raw.as_deref().and_then(parse_hhmm).unwrap_or(6 * 60))
}

/// 设置全局默认最大发卡数（`0` = 不限）。
pub fn set_max_cards_default(conn: &Connection, n: i64) -> Result<()> {
    if n < 0 {
        return Err(StoreError::Invalid("最大发卡数不能为负".into()));
    }
    set_setting(conn, "max_cards_default", &n.to_string())
}

/// 把「房间自设值 + 全局默认」合成实际生效的上限：
/// - 房间填 `0` → `None`（不限）
/// - 房间填 `n` → `Some(n)`
/// - 房间留空（`None`）→ `Some(global_default)`
pub fn effective_max_cards(room_value: Option<i64>, global_default: i64) -> Option<i64> {
    match room_value {
        Some(0) => None,
        Some(n) => Some(n),
        None => Some(global_default),
    }
}


// ---------------------------------------------------------------- 发卡记录 / 操作审计

/// 发卡记录筛选条件。全部可选，`page` 从 1 开始。
#[derive(Debug, Clone, Default)]
pub struct IssuanceFilter {
    pub room_no: Option<String>,
    pub kind: Option<String>,
    /// ISO8601 UTC 下界（含）
    pub from: Option<String>,
    /// ISO8601 UTC 上界（不含）
    pub to: Option<String>,
    pub page: i64,
    pub page_size: i64,
    /// 入住记录视图（TD-118）：只看填了身份信息的发卡 —— guest_name 非空且非空白，
    /// 「未输入身份信息不记录入住信息」在查询层落实。
    pub guest_only: bool,
}

/// 列表用的发卡记录行（比 [`IssuanceRecord`] 宽，且**不复用**它 —— 那条被读卡/换房链路共用，
/// 加列会牵动一大片）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuanceRow {
    pub id: i64,
    pub created_at: String,
    pub room_no: Option<String>,
    pub kind: String,
    pub days: i64,
    pub hours: i64,
    pub guest_name: Option<String>,
    pub guest_gender: Option<String>,
    pub guest_id_type: Option<String>,
    pub guest_id_number: Option<String>,
    pub guest_nation: Option<String>,
    pub guest_address: Option<String>,
    pub guest_birth_date: Option<String>,
    pub guest_note: Option<String>,
    pub operator_name: Option<String>,
    pub result: String,
    /// 统一状态（TD-229）：`active` / `expired` / `checked_out` / `voided` / `failed`。
    ///
    /// 由 `list_issuances` 的 SQL 算好，前端只做中文映射 —— 以前前端把过期判断写成
    /// 「仅客卡」(`kind === '日租客卡' || '钟点客卡'`)，总控卡过期后仍显示「有效」。
    pub status: String,
    pub expires_at: Option<String>,
    pub checked_out_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub raw_hex: Option<String>,
    /// 身份证扩展字段（v10）；头像不入库
    pub id_valid_from: Option<String>,
    pub id_valid_to: Option<String>,
    pub id_authority: Option<String>,
    pub id_device_model: Option<String>,
    pub id_device_serial: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: i64,
    pub at: String,
    pub action: String,
    pub target: Option<String>,
    /// `audit_log.detail_json` 里实际存的是自由文本，字段名叫 detail 才不误导
    pub detail: Option<String>,
    pub result: String,
    pub operator_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub total: i64,
}

const MAX_PAGE_SIZE: i64 = 200;

fn limit_offset(page: i64, page_size: i64) -> (i64, i64) {
    // 0 / 负数按默认 50 处理（别把"没传"变成"只取 1 条"）
    let size = if page_size <= 0 {
        50
    } else {
        page_size.min(MAX_PAGE_SIZE)
    };
    let page = page.max(1);
    (size, (page - 1) * size)
}

// ---------------------------------------------------------------- 入住登记（TD-220）

/// 入住登记单：一间房同一时刻最多一张 `open`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkin {
    pub id: i64,
    pub room_no: String,
    pub check_in_at: String,
    pub check_out_at: Option<String>,
    pub status: String,
    pub operator_id: Option<i64>,
    pub note: Option<String>,
    pub created_at: String,
}

/// 同住的客人（一人一行，`seq` 决定「首个登记人」）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinGuest {
    pub id: i64,
    pub checkin_id: i64,
    pub seq: i64,
    pub name: Option<String>,
    pub gender: Option<String>,
    pub id_type: Option<String>,
    pub id_number: Option<String>,
    pub nation: Option<String>,
    pub address: Option<String>,
    pub birth_date: Option<String>,
    pub note: Option<String>,
    pub id_valid_from: Option<String>,
    pub id_valid_to: Option<String>,
    pub id_authority: Option<String>,
    pub device_model: Option<String>,
    pub device_serial: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinDetail {
    pub checkin: Checkin,
    pub guests: Vec<CheckinGuest>,
    /// 这张登记单关联的发卡记录（**登记 → 卡**的反向可见）。
    ///
    /// 退房联动关闭登记单时，判据就是这批卡是否都已失效。
    pub cards: Vec<CheckinCard>,
}

/// 登记单关联的一张卡：轻量视图，够界面展示与判断「是否还挂在住客身上」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinCard {
    pub id: i64,
    /// 16 位作业流水号；安装卡 / 空白卡 / 原始直写为 `None`。
    pub card_no: Option<i64>,
    pub kind: String,
    pub raw_hex: Option<String>,
    pub expires_at: Option<String>,
    pub result: String,
    pub checked_out_at: Option<String>,
    pub invalidated_at: Option<String>,
}

/// 列表行：首个登记人 + 人数，列表展示够用，不必把住客全查出来。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinRow {
    pub id: i64,
    pub room_no: String,
    /// 该房号当前的锁内序号；房间记录不存在（改名/物理缺失）时为 `None`。
    ///
    /// 注意：**同一房间的多次入住会拿到同一个序号**，不能用它区分不同的登记单——
    /// 区分「哪几个人是同一张单」要看 `id`。
    pub room_index: Option<u16>,
    pub check_in_at: String,
    pub check_out_at: Option<String>,
    pub status: String,
    pub guest_count: i64,
    pub first_guest_name: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckinFilter {
    pub room_no: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: i64,
    pub page_size: i64,
}

const CHECKIN_COLUMNS: &str =
    "id, room_no, check_in_at, check_out_at, status, operator_id, note, created_at";

const CHECKIN_GUEST_COLUMNS: &str = "id, checkin_id, seq, name, gender, id_type, id_number, \
                                     nation, address, birth_date, note, id_valid_from, \
                                     id_valid_to, id_authority, id_device_model, id_device_serial";

fn row_to_checkin(r: &Row<'_>) -> rusqlite::Result<Checkin> {
    Ok(Checkin {
        id: r.get(0)?,
        room_no: r.get(1)?,
        check_in_at: r.get(2)?,
        check_out_at: r.get(3)?,
        status: r.get(4)?,
        operator_id: r.get(5)?,
        note: r.get(6)?,
        created_at: r.get(7)?,
    })
}

fn row_to_checkin_guest(r: &Row<'_>) -> rusqlite::Result<CheckinGuest> {
    Ok(CheckinGuest {
        id: r.get(0)?,
        checkin_id: r.get(1)?,
        seq: r.get(2)?,
        name: r.get(3)?,
        gender: r.get(4)?,
        id_type: r.get(5)?,
        id_number: r.get(6)?,
        nation: r.get(7)?,
        address: r.get(8)?,
        birth_date: r.get(9)?,
        note: r.get(10)?,
        id_valid_from: r.get(11)?,
        id_valid_to: r.get(12)?,
        id_authority: r.get(13)?,
        device_model: r.get(14)?,
        device_serial: r.get(15)?,
    })
}

pub fn find_checkin(conn: &Connection, id: i64) -> Result<Option<Checkin>> {
    let sql = format!("SELECT {CHECKIN_COLUMNS} FROM checkin WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], row_to_checkin).optional()?)
}

/// 取该房当前 open 的登记单。
pub fn get_open_checkin(conn: &Connection, room_no: &str) -> Result<Option<Checkin>> {
    let sql = format!(
        "SELECT {CHECKIN_COLUMNS} FROM checkin
          WHERE room_no = ?1 AND status = 'open'
          ORDER BY id DESC LIMIT 1"
    );
    Ok(conn.query_row(&sql, params![room_no], row_to_checkin).optional()?)
}

/// 建登记单：**同一房间已有 open 单就直接复用**，避免重复开单。
pub fn create_checkin(
    conn: &Connection,
    room_no: &str,
    operator_id: Option<i64>,
    note: Option<&str>,
) -> Result<Checkin> {
    if let Some(existing) = get_open_checkin(conn, room_no)? {
        return Ok(existing);
    }
    conn.execute(
        "INSERT INTO checkin (room_no, status, operator_id, note) VALUES (?1, 'open', ?2, ?3)",
        params![room_no, operator_id, note],
    )?;
    let id = conn.last_insert_rowid();
    find_checkin(conn, id)?.ok_or_else(|| StoreError::Invalid("登记单写入后读不到".into()))
}

/// 登记单 + 全部住客（按 `seq` 排序）。
pub fn get_checkin(conn: &Connection, id: i64) -> Result<Option<CheckinDetail>> {
    let Some(checkin) = find_checkin(conn, id)? else {
        return Ok(None);
    };
    let mut guests_stmt = conn.prepare(
        "SELECT {CHECKIN_GUEST_COLUMNS} FROM checkin_guest WHERE checkin_id = ?1 ORDER BY seq, id"
            .replace("{CHECKIN_GUEST_COLUMNS}", CHECKIN_GUEST_COLUMNS)
            .as_str(),
    )?;
    let rows = guests_stmt.query_map(params![id], row_to_checkin_guest)?;
    let mut guests = Vec::new();
    for r in rows {
        guests.push(r?);
    }

    // 反向可见：这张登记单关联了哪些卡（发卡时由 attach_issuance_to_open_checkin 写入）。
    let mut cards_stmt = conn.prepare(
        "SELECT id, card_no, kind, raw_hex, expires_at, result, checked_out_at, invalidated_at
           FROM issuance WHERE checkin_id = ?1 ORDER BY id",
    )?;
    let cards: Vec<CheckinCard> = cards_stmt
        .query_map(params![id], |r| {
            Ok(CheckinCard {
                id: r.get(0)?,
                card_no: r.get(1)?,
                kind: r.get(2)?,
                raw_hex: r.get(3)?,
                expires_at: r.get(4)?,
                result: r.get(5)?,
                checked_out_at: r.get(6)?,
                invalidated_at: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(CheckinDetail {
        checkin,
        guests,
        cards,
    }))
}

/// 列表：房号前缀 / 状态 / 入住日区间 + 分页，按入住时间倒序。
pub fn list_checkins(conn: &Connection, f: &CheckinFilter) -> Result<Page<CheckinRow>> {
    use rusqlite::types::Value;

    let mut where_sql = String::from(" WHERE 1 = 1");
    let mut vals: Vec<Value> = Vec::new();
    if let Some(room) = f.room_no.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND c.room_no LIKE ? || '%'");
        vals.push(Value::Text(room.to_string()));
    }
    if let Some(status) = f.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND c.status = ?");
        vals.push(Value::Text(status.to_string()));
    }
    if let Some(from) = f.from.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !is_valid_date(from) {
            return Err(StoreError::Invalid("起始日期不合法".into()));
        }
        where_sql.push_str(" AND date(c.check_in_at, 'localtime') >= ?");
        vals.push(Value::Text(from.to_string()));
    }
    if let Some(to) = f.to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !is_valid_date(to) {
            return Err(StoreError::Invalid("截止日期不合法".into()));
        }
        where_sql.push_str(" AND date(c.check_in_at, 'localtime') < ?");
        vals.push(Value::Text(to.to_string()));
    }

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM checkin c{where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;

    let (limit, offset) = limit_offset(f.page, f.page_size);
    let sql = format!(
        "SELECT c.id, c.room_no, c.check_in_at, c.check_out_at, c.status, c.note,
                (SELECT COUNT(*) FROM checkin_guest g WHERE g.checkin_id = c.id) AS guest_count,
                (SELECT g.name FROM checkin_guest g WHERE g.checkin_id = c.id
                  ORDER BY g.seq, g.id LIMIT 1) AS first_guest_name,
                r.room_index
           FROM checkin c
           LEFT JOIN room r ON r.room_no = c.room_no{where_sql}
          ORDER BY c.check_in_at DESC, c.id DESC
          LIMIT ? OFFSET ?"
    );
    let mut all = vals.clone();
    all.push(Value::Integer(limit));
    all.push(Value::Integer(offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(all.iter()), |r| {
        Ok(CheckinRow {
            id: r.get(0)?,
            room_no: r.get(1)?,
            room_index: match r.get::<_, Option<i64>>(8)? {
                Some(v) => Some(
                    u16::try_from(v)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(8, v))?,
                ),
                None => None,
            },
            check_in_at: r.get(2)?,
            check_out_at: r.get(3)?,
            status: r.get(4)?,
            note: r.get(5)?,
            guest_count: r.get(6)?,
            first_guest_name: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(Page { rows: out, total })
}

/// 导出用：一张登记单 × 一位住客（LEFT JOIN，空单也会出一行，住客字段为空）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinExportRow {
    /// 登记单号：同一张单的住客共享它，导出后用它分组。
    pub checkin_id: i64,
    pub room_no: String,
    /// 该房号当前的锁内序号；房间记录缺失时为 `None`（同房间多次入住会重复，不能当分组键）。
    pub room_index: Option<u16>,
    pub check_in_at: String,
    pub check_out_at: Option<String>,
    pub status: String,
    pub checkin_note: Option<String>,
    pub seq: Option<i64>,
    pub name: Option<String>,
    pub gender: Option<String>,
    pub id_type: Option<String>,
    pub id_number: Option<String>,
    pub nation: Option<String>,
    pub birth_date: Option<String>,
    pub address: Option<String>,
    pub id_valid_from: Option<String>,
    pub id_valid_to: Option<String>,
    pub id_authority: Option<String>,
    pub guest_note: Option<String>,
}

/// 导出用查询：按筛选条件取登记单 + 全部住客（不分页，上限由调用方检查）。
pub fn export_checkins(conn: &Connection, f: &CheckinFilter) -> Result<Vec<CheckinExportRow>> {
    use rusqlite::types::Value;

    let mut where_sql = String::from(" WHERE 1 = 1");
    let mut vals: Vec<Value> = Vec::new();
    if let Some(room) = f.room_no.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND c.room_no LIKE ? || '%'");
        vals.push(Value::Text(room.to_string()));
    }
    if let Some(status) = f.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND c.status = ?");
        vals.push(Value::Text(status.to_string()));
    }
    if let Some(from) = f.from.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !is_valid_date(from) {
            return Err(StoreError::Invalid("起始日期不合法".into()));
        }
        where_sql.push_str(" AND date(c.check_in_at, 'localtime') >= ?");
        vals.push(Value::Text(from.to_string()));
    }
    if let Some(to) = f.to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !is_valid_date(to) {
            return Err(StoreError::Invalid("截止日期不合法".into()));
        }
        where_sql.push_str(" AND date(c.check_in_at, 'localtime') < ?");
        vals.push(Value::Text(to.to_string()));
    }

    let sql = format!(
        "SELECT c.id, c.room_no, r.room_index, c.check_in_at, c.check_out_at, c.status, c.note,
                g.seq, g.name, g.gender, g.id_type, g.id_number, g.nation, g.birth_date,
                g.address, g.id_valid_from, g.id_valid_to, g.id_authority, g.note
           FROM checkin c
           LEFT JOIN checkin_guest g ON g.checkin_id = c.id
           LEFT JOIN room r ON r.room_no = c.room_no{where_sql}
          ORDER BY c.check_in_at DESC, c.id DESC, g.seq, g.id
          LIMIT 50000"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter()), |r| {
        Ok(CheckinExportRow {
            checkin_id: r.get(0)?,
            room_no: r.get(1)?,
            room_index: match r.get::<_, Option<i64>>(2)? {
                Some(v) => Some(
                    u16::try_from(v)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, v))?,
                ),
                None => None,
            },
            check_in_at: r.get(3)?,
            check_out_at: r.get(4)?,
            status: r.get(5)?,
            checkin_note: r.get(6)?,
            seq: r.get(7)?,
            name: r.get(8)?,
            gender: r.get(9)?,
            id_type: r.get(10)?,
            id_number: r.get(11)?,
            nation: r.get(12)?,
            birth_date: r.get(13)?,
            address: r.get(14)?,
            id_valid_from: r.get(15)?,
            id_valid_to: r.get(16)?,
            id_authority: r.get(17)?,
            guest_note: r.get(18)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 追加一位住客：`seq` 自动取当前最大值 +1（决定「首个登记人」顺序）。
///
/// TD-224：拒绝既无姓名也无证件号的空白住客 —— 以前只有 open 校验，前端任何
/// 漏掉非空判断的调用路径都会落一条「只有证件类型」的空行。
pub fn add_checkin_guest(
    conn: &Connection,
    checkin_id: i64,
    guest: &GuestIdentity<'_>,
) -> Result<i64> {
    if !guest.has_identity() {
        return Err(StoreError::Invalid(
            "住客至少要有姓名或证件号".into(),
        ));
    }
    let checkin =
        find_checkin(conn, checkin_id)?.ok_or_else(|| StoreError::Invalid("登记单不存在".into()))?;
    if checkin.status != "open" {
        return Err(StoreError::Invalid("登记单已关闭，不能再加住客".into()));
    }
    let next_seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM checkin_guest WHERE checkin_id = ?1",
        params![checkin_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO checkin_guest
             (checkin_id, seq, name, gender, id_type, id_number, nation, address,
              birth_date, note, id_valid_from, id_valid_to, id_authority,
              id_device_model, id_device_serial)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            checkin_id,
            next_seq,
            guest.name,
            guest.gender,
            guest.id_type,
            guest.id_number,
            guest.nation,
            guest.address,
            guest.birth_date,
            guest.note,
            guest.id_valid_from,
            guest.id_valid_to,
            guest.id_authority,
            guest.device_model,
            guest.device_serial,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 修改住客资料（TD-224：同样拒绝改成空白住客）。
pub fn update_checkin_guest(conn: &Connection, id: i64, guest: &GuestIdentity<'_>) -> Result<()> {
    if !guest.has_identity() {
        return Err(StoreError::Invalid(
            "住客至少要有姓名或证件号".into(),
        ));
    }
    let n = conn.execute(
        "UPDATE checkin_guest
            SET name = ?2, gender = ?3, id_type = ?4, id_number = ?5, nation = ?6,
                address = ?7, birth_date = ?8, note = ?9, id_valid_from = ?10,
                id_valid_to = ?11, id_authority = ?12, id_device_model = ?13,
                id_device_serial = ?14
          WHERE id = ?1",
        params![
            id,
            guest.name,
            guest.gender,
            guest.id_type,
            guest.id_number,
            guest.nation,
            guest.address,
            guest.birth_date,
            guest.note,
            guest.id_valid_from,
            guest.id_valid_to,
            guest.id_authority,
            guest.device_model,
            guest.device_serial,
        ],
    )?;
    if n == 0 {
        return Err(StoreError::Invalid("住客不存在".into()));
    }
    Ok(())
}

pub fn delete_checkin_guest(conn: &Connection, id: i64) -> Result<()> {
    let n = conn.execute("DELETE FROM checkin_guest WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(StoreError::Invalid("住客不存在".into()));
    }
    Ok(())
}

/// 关闭登记单（退房 / 换房 / 手动）。
pub fn close_checkin(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE checkin
            SET status = 'closed',
                check_out_at = COALESCE(check_out_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
          WHERE id = ?1 AND status = 'open'",
        params![id],
    )?;
    Ok(())
}

/// 退房联动：关闭该房当前 open 的登记单，返回关闭数量（0 或 1）。
/// 若这张登记单关联的客卡已全部失效（退房 / 作废 / 过期），把登记单关掉。
///
/// 判据是**这张登记单关联的卡**，不是房号：同房可能还有别的、不属于这张登记单的卡，
/// 按房号判断会把它误当成「还没退完」，登记单就再也关不掉了。
/// （对应「所有关联卡都退完才关单」。）
///
/// 返回是否真的关闭了登记单；登记单本来就是 closed 时返回 `false`。
pub fn close_checkin_if_all_cards_closed(conn: &Connection, checkin_id: i64) -> Result<bool> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM checkin WHERE id = ?1",
            params![checkin_id],
            |r| r.get(0),
        )
        .optional()?;
    if status.as_deref() != Some("open") {
        return Ok(false);
    }

    // 口径与「有效卡」一致：已退房 / 已作废 / 已过期都不算仍在用。
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM issuance
          WHERE checkin_id = ?1
            AND result = 'ok'
            AND kind IN ('日租客卡','钟点客卡')
            AND checked_out_at IS NULL
            AND invalidated_at IS NULL
            AND expires_at IS NOT NULL
            AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        params![checkin_id],
        |r| r.get(0),
    )?;
    if active > 0 {
        return Ok(false);
    }

    let n = conn.execute(
        "UPDATE checkin
            SET status = 'closed',
                check_out_at = COALESCE(check_out_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
          WHERE id = ?1 AND status = 'open'",
        params![checkin_id],
    )?;
    Ok(n > 0)
}

/// 关闭某房间所有「关联卡都已失效」的 open 登记单，返回关闭的张数。
pub fn close_open_checkin_for_room(conn: &Connection, room_no: &str) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id FROM checkin WHERE room_no = ?1 AND status = 'open'")?;
    let ids: Vec<i64> = stmt
        .query_map(params![room_no], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;

    let mut closed = 0;
    for id in ids {
        if close_checkin_if_all_cards_closed(conn, id)? {
            closed += 1;
        }
    }
    Ok(closed)
}

/// 强制关闭该房所有 open 登记单（**不看关联卡**）。
///
/// 用于退房时用户勾选「同时清除入住登记信息」：此时即使该房还留着别的有效卡，
/// 也把登记单关掉。语义仍是**关闭**（`status='closed'`），住客档案保留，记录页可回溯。
pub fn close_open_checkin_for_room_force(conn: &Connection, room_no: &str) -> Result<usize> {
    let n = conn.execute(
        "UPDATE checkin
            SET status = 'closed',
                check_out_at = COALESCE(check_out_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
          WHERE room_no = ?1 AND status = 'open'",
        params![room_no],
    )?;
    Ok(n)
}

/// 发卡后把这条卡关联到该房当前的 open 登记单（没有就返回 `None`，不影响发卡）。
pub fn attach_issuance_to_open_checkin(
    conn: &Connection,
    issuance_id: i64,
    room_no: &str,
) -> Result<Option<i64>> {
    let Some(checkin) = get_open_checkin(conn, room_no)? else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE issuance SET checkin_id = ?2 WHERE id = ?1",
        params![issuance_id, checkin.id],
    )?;
    Ok(Some(checkin.id))
}

/// 日历入住时段（含已退房历史）：按本地日期区间返回所有相交客卡记录。
///
/// 只画**发卡区间**：只登记没发卡的入住不会出现在日历上（TD-220 起登记与发卡解耦）。
/// 姓名取关联登记单的首个住客（`seq` 最小），没有登记单时回退 `issuance.guest_name`。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestStay {
    pub id: i64,
    pub room_no: String,
    pub guest_name: Option<String>,
    pub kind: String,
    pub days: i64,
    pub hours: i64,
    pub start: String,
    pub end: String,
    pub planned_end: String,
    /// 入住状态（TD-215 补充）：`active` / `checked_out` / `invalidated` / `expired`。
    /// 过期未退房的记录不能再显示成「入住中」。
    pub status: String,
    /// 关联的入住登记单（TD-221）：有的话姓名取首个登记人、详情可展开全部住客。
    pub checkin_id: Option<i64>,
    /// 该登记单的住客人数（没有登记单时为 0）。
    pub guest_count: i64,
}

pub fn list_guest_stays(conn: &Connection, from: &str, to: &str) -> Result<Vec<GuestStay>> {
    if !is_valid_date(from) || !is_valid_date(to) || to <= from {
        return Err(StoreError::Invalid("入住时段日期区间不合法".into()));
    }
    // TD-225：把凌晨开卡归到前一晚，边界取设置项（默认 06:00 = 360 分钟）。
    // 以前这里硬编码 `-6 hours`，与可配置的凌晨归属时间会脱节。
    let cutoff = get_morning_cutoff_minutes(conn)?;
    let mut stmt = conn.prepare(
        "SELECT i.id, i.room_no,
                COALESCE(
                    (SELECT g.name FROM checkin_guest g
                      WHERE g.checkin_id = i.checkin_id
                      ORDER BY g.seq, g.id LIMIT 1),
                    i.guest_name
                ) AS guest_name,
                i.kind, i.days, i.hours,
                date(i.created_at, ?3, 'localtime') AS start,
                date(COALESCE(i.checked_out_at, i.invalidated_at, i.expires_at), 'localtime') AS end,
                date(i.expires_at, 'localtime') AS planned_end,
                CASE
                    WHEN i.checked_out_at IS NOT NULL THEN 'checked_out'
                    WHEN i.invalidated_at IS NOT NULL THEN 'invalidated'
                    WHEN i.expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now') THEN 'expired'
                    ELSE 'active'
                END AS status,
                i.checkin_id,
                (SELECT COUNT(*) FROM checkin_guest g WHERE g.checkin_id = i.checkin_id) AS guest_count
           FROM issuance i
          WHERE i.result = 'ok'
            AND i.room_no IS NOT NULL
            AND i.expires_at IS NOT NULL
            AND i.kind IN ('日租客卡','钟点客卡')
            AND date(i.created_at, ?3, 'localtime') < ?2
            AND date(COALESCE(i.checked_out_at, i.invalidated_at, i.expires_at), 'localtime') >= ?1
          ORDER BY i.created_at, i.id",
    )?;
    let offset = format!("-{cutoff} minutes");
    let rows = stmt.query_map(params![from, to, offset], |r| {
        Ok(GuestStay {
            id: r.get(0)?,
            room_no: r.get(1)?,
            guest_name: r.get(2)?,
            kind: r.get(3)?,
            days: r.get(4)?,
            hours: r.get(5)?,
            start: r.get(6)?,
            end: r.get(7)?,
            planned_end: r.get(8)?,
            status: r.get(9)?,
            checkin_id: r.get(10)?,
            guest_count: r.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 发卡记录列表：房号前缀 / 卡号 / 卡型 / 时间范围 + 分页，按时间倒序。
pub fn list_issuances(conn: &Connection, f: &IssuanceFilter) -> Result<Page<IssuanceRow>> {
    use rusqlite::types::Value;

    let mut where_sql = String::from(" WHERE 1 = 1");
    let mut vals: Vec<Value> = Vec::new();
    if let Some(room) = f.room_no.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND i.room_no LIKE ? || '%'");
        vals.push(Value::Text(room.to_string()));
    }
    if let Some(kind) = f.kind.as_ref().filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND i.kind = ?");
        vals.push(Value::Text(kind.to_string()));
    }
    if let Some(from) = f.from.as_ref().filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND i.created_at >= ?");
        vals.push(Value::Text(from.to_string()));
    }
    if let Some(to) = f.to.as_ref().filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND i.created_at < ?");
        vals.push(Value::Text(to.to_string()));
    }
    // 入住记录视图（TD-118）：未输入身份信息（NULL / 空白）的发卡不进入入住记录
    if f.guest_only {
        where_sql.push_str(" AND i.guest_name IS NOT NULL AND TRIM(i.guest_name) != ''");
    }

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM issuance i{where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;

    let (size, offset) = limit_offset(f.page, f.page_size);
    let sql = format!(
        "SELECT i.id, i.created_at, i.room_no, i.kind, i.days, i.hours,
                i.guest_name, i.guest_gender, i.guest_id_type, i.guest_id_number,
                i.guest_nation, i.guest_address, i.guest_birth_date, i.guest_note,
                o.display_name,
                i.result, i.expires_at, i.checked_out_at, i.duration_ms, i.raw_hex,
                i.id_valid_from, i.id_valid_to, i.id_authority, i.id_device_model, i.id_device_serial,
                CASE
                  WHEN i.result != 'ok' THEN 'failed'
                  WHEN i.checked_out_at IS NOT NULL THEN 'checked_out'
                  WHEN i.invalidated_at IS NOT NULL THEN 'voided'
                  WHEN i.expires_at IS NOT NULL
                       AND i.expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now') THEN 'expired'
                  ELSE 'active'
                END AS status
           FROM issuance i
           LEFT JOIN operator o ON o.id = i.operator_id
           {where_sql}
          ORDER BY i.created_at DESC, i.id DESC
          LIMIT ? OFFSET ?"
    );
    let mut all = vals;
    all.push(Value::Integer(size));
    all.push(Value::Integer(offset));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(all.iter()), |r| {
        Ok(IssuanceRow {
            id: r.get(0)?,
            created_at: r.get(1)?,
            room_no: r.get(2)?,
            kind: r.get(3)?,
            days: r.get(4)?,
            hours: r.get(5)?,
            guest_name: r.get(6)?,
            guest_gender: r.get(7)?,
            guest_id_type: r.get(8)?,
            guest_id_number: r.get(9)?,
            guest_nation: r.get(10)?,
            guest_address: r.get(11)?,
            guest_birth_date: r.get(12)?,
            guest_note: r.get(13)?,
            operator_name: r.get(14)?,
            result: r.get(15)?,
            status: r.get(25)?,
            expires_at: r.get(16)?,
            checked_out_at: r.get(17)?,
            duration_ms: r.get(18)?,
            raw_hex: r.get(19)?,
            id_valid_from: r.get(20)?,
            id_valid_to: r.get(21)?,
            id_authority: r.get(22)?,
            id_device_model: r.get(23)?,
            id_device_serial: r.get(24)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(Page { rows: out, total })
}

/// 操作审计列表：动作前缀 / 时间范围 + 分页。
pub fn list_audits(
    conn: &Connection,
    action: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Page<AuditRow>> {
    use rusqlite::types::Value;
    let mut where_sql = String::from(" WHERE 1 = 1");
    let mut vals: Vec<Value> = Vec::new();
    if let Some(a) = action.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND a.action LIKE ? || '%'");
        vals.push(Value::Text(a.to_string()));
    }
    if let Some(f) = from.filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND a.at >= ?");
        vals.push(Value::Text(f.to_string()));
    }
    if let Some(t) = to.filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND a.at < ?");
        vals.push(Value::Text(t.to_string()));
    }
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM audit_log a{where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;

    let (size, offset) = limit_offset(page, page_size);
    let sql = format!(
        "SELECT a.id, a.at, a.action, a.target, a.detail_json, a.result, o.display_name
           FROM audit_log a LEFT JOIN operator o ON o.id = a.operator_id
           {where_sql}
          ORDER BY a.at DESC, a.id DESC
          LIMIT ? OFFSET ?"
    );
    let mut all = vals;
    all.push(Value::Integer(size));
    all.push(Value::Integer(offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(all.iter()), |r| {
        Ok(AuditRow {
            id: r.get(0)?,
            at: r.get(1)?,
            action: r.get(2)?,
            target: r.get(3)?,
            detail: r.get(4)?,
            result: r.get(5)?,
            operator_name: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(Page { rows: out, total })
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
    fn max_cards_default_and_room_override() {
        let (conn, path) = temp_db();

        // 缺省就是 2（产品默认值），不需要初始化
        assert_eq!(get_max_cards_default(&conn).unwrap(), 2, "缺省应为 2");

        // 改全局 → 读回新值
        set_max_cards_default(&conn, 5).unwrap();
        assert_eq!(get_max_cards_default(&conn).unwrap(), 5);

        // 生效值三态：留空跟随全局 / 0 不限 / n 覆盖
        assert_eq!(effective_max_cards(None, 5), Some(5), "留空跟随全局");
        assert_eq!(effective_max_cards(Some(0), 5), None, "0 = 不限");
        assert_eq!(effective_max_cards(Some(3), 5), Some(3), "房间覆盖优先");

        // 负数拒绝
        assert!(set_max_cards_default(&conn, -1).is_err(), "负数应拒绝");

        cleanup(conn, path);
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
    fn list_rooms_sorts_by_room_no_then_index() {
        let (conn, path) = temp_db();
        insert_room(&conn, "10", Some(2), None, None, None).unwrap();
        insert_room(&conn, "2", Some(0), None, None, None).unwrap();
        insert_room(&conn, "A10", Some(3), None, None, None).unwrap();
        insert_room(&conn, "A2", Some(1), None, None, None).unwrap();
        let nos: Vec<String> = list_rooms(&conn)
            .unwrap()
            .into_iter()
            .map(|r| r.room_no)
            .collect();
        assert_eq!(nos, vec!["2", "10", "A10", "A2"], "数字房号按数值，非数字按字符串");
        cleanup(conn, path);
    }

    #[test]
    fn jieba_name_tokens_keep_chinese_words() {
        let tokens = guest_name_tokens("张三 丰");
        assert!(tokens.iter().any(|t| t == "丰"), "分词应保留姓名片段: {tokens:?}");
        assert!(!tokens.is_empty());
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
    fn find_issuance_by_stamp_uses_local_day_hour_and_seq() {
        let (conn, path) = temp_db();
        let mk = |room: &str, _seq: u16| {
            record_issuance(
                &conn,
                &NewIssuance {
                    room_no: Some(room),
                    kind: "日租客卡",
                    days: 1,
                    hours: 0,
                    card_no: None,
                    raw_hex: None,
                    operator_id: None,
                    reader_id: None,
                    result: "ok",
                    error_code: None,
                    idempotency_key: None,
                    duration_ms: None,
                },
            )
            .unwrap()
        };
        let a = mk("801", 0);
        let b = mk("801", 1);
        let c = mk("802", 2);
        // 统一到同一小时，避免测试跨小时边界时 seq 偏移错位
        conn.execute(
            "UPDATE issuance SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            [],
        )
        .unwrap();
        let (day, hour): (i64, i64) = conn
            .query_row(
                "SELECT CAST(strftime('%d','now','localtime') AS INTEGER), \
                        CAST(strftime('%H','now','localtime') AS INTEGER)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        let hit = find_issuance_by_stamp(&conn, Some("801"), "日租客卡", day as u16, hour as u16, 1)
            .unwrap()
            .unwrap();
        assert_eq!(hit.id, b, "同房同小时第 2 张应命中 b");
        assert_eq!(hit.room_no.as_deref(), Some("801"));

        // 房号过滤：801 只有 2 条，seq=2 必须为空
        assert!(find_issuance_by_stamp(
            &conn,
            Some("801"),
            "日租客卡",
            day as u16,
            hour as u16,
            2
        )
        .unwrap()
        .is_none());
        // 总控卡没有记录
        assert!(find_issuance_by_stamp(&conn, None, "总控卡", day as u16, hour as u16, 0)
            .unwrap()
            .is_none());
        // 不同小时不命中
        assert!(find_issuance_by_stamp(
            &conn,
            Some("801"),
            "日租客卡",
            day as u16,
            ((hour + 1) % 24) as u16,
            0
        )
        .unwrap()
        .is_none());

        let _ = (a, c);
        cleanup(conn, path);
    }

    #[test]
    fn idempotency_key_is_enforced() {
        let (conn, path) = temp_db();
        let a = NewIssuance {
            room_no: Some("220"),
            kind: "日租客卡",
            days: 1,
            hours: 0,
            card_no: None,
            raw_hex: None,
            operator_id: None,
            reader_id: None,
            result: "ok",
            error_code: None,
            idempotency_key: Some("same"),
            duration_ms: None,
        };
        record_issuance(&conn, &a).unwrap();
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
            card_no: None,
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
        assert_eq!(hit.unwrap().room_no.as_deref(), Some("220"));
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
            card_no: None,
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
    fn search_reservations_includes_all_statuses_and_paginates() {
        let (conn, path) = temp_db();
        insert_room(&conn, "801", None, None, None, None).unwrap();
        insert_room(&conn, "802", None, None, None, None).unwrap();
        let day = |offset: &str| -> String {
            conn.query_row(
                "SELECT date('now','localtime', ?1)",
                params![offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        let mk = |room: &str, ci: &str, co: &str, name: &str| {
            create_reservation(
                &conn,
                &ReservationInput {
                    room_no: room.into(),
                    check_in: ci.into(),
                    check_in_time: None,
                    check_out: co.into(),
                    check_out_time: None,
                    guest_name: Some(name.into()),
                    note: None,
                },
                None,
            )
            .unwrap()
        };
        let a = mk("801", &day("+1 day"), &day("+3 days"), "张三");
        let b = mk("802", &day("+5 days"), &day("+7 days"), "李四");
        let c = mk("801", &day("+10 days"), &day("+12 days"), "张三");
        cancel_reservation(&conn, b.id).unwrap();

        let all = search_reservations(
            &conn,
            &ReservationFilter {
                page: 1,
                page_size: 50,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(all.total, 3, "已取消的预定也要能查到");
        assert_eq!(all.rows[0].id, c.id, "按入住日倒序");

        let cancelled = search_reservations(
            &conn,
            &ReservationFilter {
                status: Some("cancelled".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((cancelled.total, cancelled.rows[0].id), (1, b.id));

        let filtered = search_reservations(
            &conn,
            &ReservationFilter {
                room_no: Some("80".into()),
                guest_name: Some("张".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered.total, 2);
        assert!(filtered.rows.iter().all(|r| r.room_no == "801"));

        let by_date = search_reservations(
            &conn,
            &ReservationFilter {
                from: Some(b.check_in.clone()),
                to: Some(c.check_in.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((by_date.total, by_date.rows[0].id), (1, b.id));

        let p1 = search_reservations(
            &conn,
            &ReservationFilter {
                page: 1,
                page_size: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let p2 = search_reservations(
            &conn,
            &ReservationFilter {
                page: 2,
                page_size: 2,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((p1.total, p1.rows.len(), p2.rows.len()), (3, 2, 1));

        assert!(search_reservations(
            &conn,
            &ReservationFilter {
                status: Some("bogus".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(search_reservations(
            &conn,
            &ReservationFilter {
                from: Some(day("+3 days")),
                to: Some(day("+1 day")),
                ..Default::default()
            }
        )
        .is_err());

        // 未使用的变量仅用于确保三条记录的 id 稳定
        let _ = a;
        cleanup(conn, path);
    }

    #[test]
    fn check_in_reservations_marks_tonight_only() {
        let (conn, path) = temp_db();
        insert_room(&conn, "801", None, None, None, None).unwrap();
        let day = |offset: &str| -> String {
            conn.query_row(
                "SELECT date('now','localtime', ?1)",
                params![offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        let today = day("+0 day");
        let tomorrow = day("+1 day");
        let day3 = day("+2 days");
        let day4 = day("+3 days");
        // 覆盖 today 的预定 → 标记 checked_in；未来预定不动（TD-128）
        let a = create_reservation(
            &conn,
            &ReservationInput {
                room_no: "801".into(),
                check_in: today.clone(),
                check_in_time: None,
                check_out: tomorrow.clone(),
                check_out_time: None,
                guest_name: Some("张三".into()),
                note: None,
            },
            None,
        )
        .unwrap();
        let b = create_reservation(
            &conn,
            &ReservationInput {
                room_no: "801".into(),
                check_in: day3.clone(),
                check_in_time: None,
                check_out: day4.clone(),
                check_out_time: None,
                guest_name: None,
                note: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(
            check_in_reservations(&conn, "801", &today).unwrap(),
            1,
            "只标记覆盖 today 的那一条"
        );
        assert_eq!(
            find_reservation(&conn, a.id).unwrap().unwrap().status,
            "checked_in"
        );
        assert_eq!(
            find_reservation(&conn, b.id).unwrap().unwrap().status,
            "booked",
            "未来预定不受影响"
        );
        assert_eq!(
            check_in_reservations(&conn, "801", &today).unwrap(),
            0,
            "再标记幂等：无 booked 可标"
        );
        cleanup(conn, path);
    }

    #[test]
    fn list_rooms_active_cards_counts_same_no_separately() {
        let (conn, path) = temp_db();
        insert_room(&conn, "222", None, None, None, None).unwrap();
        // 同号两条有效记录（TD-127）：房间列表的 active_cards 子查询也要按「张」计数
        for offset in ["+1 day", "+2 days"] {
            conn.execute(
                "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
                 VALUES (?1, '222', '日租客卡', 1, 0, 11, 'ok',
                         strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?2))",
                params![uuid::Uuid::new_v4().to_string(), offset],
            )
            .unwrap();
        }
        let rooms = list_rooms(&conn).unwrap();
        let room = rooms.iter().find(|r| r.room_no == "222").unwrap();
        assert_eq!(room.active_cards, 2, "房间卡片「有效卡 N」应按张计数");
        cleanup(conn, path);
    }

    #[test]
    fn guest_day_expiry_uses_checkout_hour_setting() {
        let (conn, path) = temp_db();
        // 默认退房钟点 12：日租 1 天 → 到期 = 本地「开卡日 + 1 天」的 12:00（TD-126）
        let id = record_issuance(&conn, &sample_issuance(Some("220"), "日租客卡", 1, 0)).unwrap();
        // 期望值必须复刻产品的「凌晨归属日」规则（TD-225）：本地开卡时刻早于
        // morningCutoff（默认 06:00）时归属日取前一天。少了这段 CASE，
        // 在本地凌晨跑测试就会与实现差一天。
        let cutoff = get_morning_cutoff_minutes(&conn).unwrap();
        let local_day: String = conn
            .query_row(
                "SELECT strftime('%Y-%m-%d',
                           date(created_at, 'localtime',
                                CASE WHEN ( CAST(strftime('%H', created_at, 'localtime') AS INTEGER) * 60
                                          + CAST(strftime('%M', created_at, 'localtime') AS INTEGER) ) < ?2
                                     THEN '-1 day' ELSE '+0 days' END,
                                '+1 days'))
                   FROM issuance WHERE id = ?1",
                params![id, cutoff],
                |r| r.get(0),
            )
            .unwrap();
        let (expires_day, expires_hour): (String, String) = conn
            .query_row(
                "SELECT strftime('%Y-%m-%d', expires_at, 'localtime'),
                        strftime('%H', expires_at, 'localtime')
                   FROM issuance WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(expires_day, local_day, "到期日 = 本地开卡日 + 天数");
        assert_eq!(expires_hour, "12", "默认退房钟点 12");

        // 改设置项为 6 点 → 新发的卡到期时刻跟着变
        set_setting(&conn, "checkoutHour", "6").unwrap();
        let id2 = record_issuance(&conn, &sample_issuance(Some("221"), "日租客卡", 2, 0)).unwrap();
        let hour: String = conn
            .query_row(
                "SELECT strftime('%H', expires_at, 'localtime') FROM issuance WHERE id = ?1",
                params![id2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hour, "06", "到期时刻 = 设置项钟点");

        // 钟点卡不受影响：仍按发卡时刻顺延小时
        let id3 = record_issuance(&conn, &sample_issuance(Some("222"), "钟点客卡", 0, 3)).unwrap();
        let expected3: String = conn
            .query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+3 hours')
                   FROM issuance WHERE id = ?1",
                params![id3],
                |r| r.get(0),
            )
            .unwrap();
        let actual3: String = conn
            .query_row("SELECT expires_at FROM issuance WHERE id = ?1", params![id3], |r| r.get(0))
            .unwrap();
        assert_eq!(actual3, expected3, "钟点卡仍按发卡时刻顺延");

        cleanup(conn, path);
    }

    /// TD-225：凌晨房归属 —— 早于 cutoff 开卡的日租卡到期算「当天」退房钟点。
    ///
    /// 基准取「后天 02:30」而不是当天：`recalc_guest_day_expiry` 只重算**未过期**的卡，
    /// 用当天 12:00 会让用例依赖运行时刻（下午跑就会因过期而命中不到）。
    #[test]
    fn morning_cutoff_moves_early_morning_cards_to_same_day() {
        let (conn, path) = temp_db();
        insert_room(&conn, "951", None, None, None, None).unwrap();
        set_setting(&conn, "checkoutHour", "12").unwrap();
        set_setting(&conn, "morningCutoff", "00:00").unwrap();

        let day_of = |id: i64| -> (String, String) {
            conn.query_row(
                "SELECT strftime('%Y-%m-%d', expires_at, 'localtime'),
                        strftime('%H', expires_at, 'localtime')
                   FROM issuance WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let rel_day = |modifier: &str| -> String {
            conn.query_row(
                &format!("SELECT date('now', '{modifier}', 'localtime')"),
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        // 基准日：后天（保证到期时间在未来，recalc 才会命中）
        let base = rel_day("+2 days");

        // 把 created_at 挪到「基准日 HH:MM」
        let place_at = |id: i64, hhmm: &str| {
            conn.execute(
                "UPDATE issuance
                    SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?2, 'utc')
                  WHERE id = ?1",
                params![id, format!("{base} {hhmm}:00")],
            )
            .unwrap();
        };

        // ① cutoff=00:00 等同于「无凌晨归属」：凌晨卡仍按次日
        let early = record_issuance(&conn, &sample_issuance(Some("951"), "日租客卡", 1, 0)).unwrap();
        place_at(early, "02:30");
        recalc_guest_day_expiry(&conn, 12).unwrap();
        assert_eq!(
            day_of(early).0,
            rel_day("+3 days"),
            "cutoff=00:00 时凌晨卡不动，到期仍是「开卡日 + 1 天」"
        );
        assert_eq!(day_of(early).1, "12");

        // ② cutoff=06:00 → 02:30 属于「前一晚」，到期提前到基准日当天 12:00
        set_setting(&conn, "morningCutoff", "06:00").unwrap();
        assert!(recalc_guest_day_expiry(&conn, 12).unwrap() >= 1);
        assert_eq!(day_of(early).0, base, "凌晨 02:30 开 1 天卡应到「当天」12:00，而不是次日");
        assert_eq!(day_of(early).1, "12");

        // ③ 06:01 之后开卡不受影响：仍按次日
        let late = record_issuance(&conn, &sample_issuance(Some("951"), "日租客卡", 1, 0)).unwrap();
        place_at(late, "06:30");
        recalc_guest_day_expiry(&conn, 12).unwrap();
        assert_eq!(day_of(late).0, rel_day("+3 days"), "06:30 开卡属于当天，到期是次日");

        // ④ 归属时间可配：截止设到 02:00 后，02:30 已过截止 → 回到次日
        set_setting(&conn, "morningCutoff", "02:00").unwrap();
        recalc_guest_day_expiry(&conn, 12).unwrap();
        assert_eq!(
            day_of(early).0,
            rel_day("+3 days"),
            "cutoff=02:00 时 02:30 已过截止，不再按凌晨房归属"
        );

        // ⑤ 反过来：截止设到 05:00 仍是凌晨房（当天）
        set_setting(&conn, "morningCutoff", "05:00").unwrap();
        recalc_guest_day_expiry(&conn, 12).unwrap();
        assert_eq!(day_of(early).0, base, "cutoff=05:00 时 02:30 早于截止，仍是凌晨房");

        cleanup(conn, path);
    }

    /// TD-225：日历起始日按设置项的凌晨归属时间，不再硬编码 6 小时。
    #[test]
    fn guest_stays_start_follows_morning_cutoff() {
        let (conn, path) = temp_db();
        insert_room(&conn, "952", None, None, None, None).unwrap();
        set_setting(&conn, "checkoutHour", "12").unwrap();
        set_setting(&conn, "morningCutoff", "06:00").unwrap();

        let id = record_issuance(&conn, &sample_issuance(Some("952"), "日租客卡", 1, 0)).unwrap();
        conn.execute(
            "UPDATE issuance
                SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ',
                                          strftime('%Y-%m-%d', 'now', 'localtime') || ' 02:30:00', 'utc')
              WHERE id = ?1",
            params![id],
        )
        .unwrap();

        let start_of = |from: &str, to: &str| -> String {
            list_guest_stays(&conn, from, to)
                .unwrap()
                .into_iter()
                .find(|s| s.id == id)
                .map(|s| s.start)
                .unwrap()
        };

        // 区间取前后各一天，保证两种归属都能命中
        let from: String = conn
            .query_row("SELECT date('now', '-2 days', 'localtime')", [], |r| r.get(0))
            .unwrap();
        let to: String = conn
            .query_row("SELECT date('now', '+3 days', 'localtime')", [], |r| r.get(0))
            .unwrap();
        let yesterday: String = conn
            .query_row("SELECT date('now', '-1 day', 'localtime')", [], |r| r.get(0))
            .unwrap();
        let today: String = conn
            .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))
            .unwrap();

        // cutoff 06:00：凌晨 02:30 归到前一晚
        assert_eq!(start_of(&from, &to), yesterday, "凌晨开卡应画在前一晚");

        // cutoff 00:00：不再前移
        set_setting(&conn, "morningCutoff", "00:00").unwrap();
        assert_eq!(start_of(&from, &to), today, "关闭凌晨归属后应画在当天");

        cleanup(conn, path);
    }

    /// TD-126 补充：改退房钟点后，只重算仍有效的日租客卡；已退房记录保持原样。
    #[test]
    fn recalc_guest_day_expiry_moves_active_cards_only() {
        let (conn, path) = temp_db();
        insert_room(&conn, "701", None, None, None, None).unwrap();
        set_setting(&conn, "checkoutHour", "12").unwrap();
        let active = record_issuance(&conn, &sample_issuance(Some("701"), "日租客卡", 2, 0)).unwrap();
        let gone = record_issuance(&conn, &sample_issuance(Some("701"), "日租客卡", 2, 0)).unwrap();
        checkout_issuance(&conn, gone, "vacant_dirty").unwrap();

        let expiry = |id: i64| -> String {
            conn.query_row(
                "SELECT expires_at FROM issuance WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        let active_before = expiry(active);
        let gone_before = expiry(gone);

        let n = recalc_guest_day_expiry(&conn, 14).unwrap();
        assert_eq!(n, 1, "只应重算仍有效的那张");
        let active_after = expiry(active);
        assert_ne!(active_after, active_before, "在用卡的到期时间应随钟点变化");
        assert_eq!(expiry(gone), gone_before, "已退房记录的到期时间不能被改写");

        // 与发卡同一公式（含 TD-225 凌晨归属日）：本地「归属日 + 2 天」的 14:00 转 UTC
        let cutoff = get_morning_cutoff_minutes(&conn).unwrap();
        let expected: String = conn
            .query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%SZ',
                           date(created_at, 'localtime',
                                CASE WHEN ( CAST(strftime('%H', created_at, 'localtime') AS INTEGER) * 60
                                          + CAST(strftime('%M', created_at, 'localtime') AS INTEGER) ) < ?2
                                     THEN '-1 day' ELSE '+0 days' END,
                                '+2 days') || ' 14:00:00',
                           'utc')
                   FROM issuance WHERE id = ?1",
                params![active, cutoff],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active_after, expected);

        // 改回 12 点：仍只命中 1 张，且结果回到最初的值
        assert_eq!(recalc_guest_day_expiry(&conn, 12).unwrap(), 1);
        assert_eq!(expiry(active), active_before);

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

        // 卡号不再落库：换房核对改按时间戳匹配，active_guest_card_nos 在后续步骤删除。
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

        insert("-2 days", "301", "日租客卡", 10); // 过期超宽限，出窗
        insert("-30 minutes", "307", "日租客卡", 17); // 过期超 10 分钟宽限：TD-111 回归，不出窗
        insert("-5 minutes", "302", "日租客卡", 11); // 刚过期（宽限内），要提醒
        insert("+30 minutes", "303", "日租客卡", 12); // 即将到期，要提醒
        insert("+2 hours", "304", "日租客卡", 13); // 还早，出窗

        // 同房间同卡型重发：只保留最新一条
        insert("-20 minutes", "305", "日租客卡", 14);
        insert("-5 minutes", "305", "日租客卡", 15);

        let list = expiring_cards(&conn, 60).unwrap();
        let rooms: Vec<&str> = list.iter().filter_map(|c| c.room_no.as_deref()).collect();
        assert!(!rooms.contains(&"301"));
        assert!(!rooms.contains(&"307"), "过期超 10 分钟宽限不得再提醒（TD-111）");
        assert!(!rooms.contains(&"304"));
        assert!(rooms.contains(&"302"));
        assert!(rooms.contains(&"303"));
        assert_eq!(rooms.iter().filter(|r| **r == "305").count(), 1, "同房同卡型应去重");
        assert_eq!(list.last().unwrap().room_no.as_deref(), Some("303"), "按到期时间升序");

        cleanup(conn, path);
    }

    #[test]
    fn expiring_cards_excludes_checked_out() {
        let (conn, path) = temp_db();
        insert_room(&conn, "306", None, None, None, None).unwrap();
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at)
             VALUES (?1, '306', '日租客卡', 1, 0, 16, 'ok',
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+30 minutes'))",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();

        assert_eq!(expiring_cards(&conn, 60).unwrap().len(), 1, "退房前应提醒");
        checkout_room(&conn, "306", "vacant_dirty").unwrap();
        assert!(
            expiring_cards(&conn, 60).unwrap().is_empty(),
            "已退房的卡不应再出现在到期提醒里"
        );

        cleanup(conn, path);
    }

    /// 预定转正：只转该房今天覆盖的那一条，回写 `checkin.reservation_id`；重复调用幂等。
    #[test]
    fn convert_reservation_marks_checked_in_and_links_checkin() {
        let (conn, path) = temp_db();
        insert_room(&conn, "260", Some(50), None, None, None).unwrap();
        let day = |offset: &str| -> String {
            conn.query_row(
                "SELECT date('now','localtime', ?1)",
                params![offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        let today = day("+0 day");
        let tomorrow = day("+1 day");

        let res = create_reservation(
            &conn,
            &ReservationInput {
                room_no: "260".into(),
                check_in: today.clone(),
                check_in_time: None,
                check_out: tomorrow,
                check_out_time: None,
                guest_name: Some("张三".into()),
                note: Some("到店晚".into()),
            },
            None,
        )
        .unwrap();
        let checkin = create_checkin(&conn, "260", None, None).unwrap();

        // 第一次：转正 + 关联
        assert_eq!(
            convert_reservation_for_room(&conn, "260", &today, checkin.id).unwrap(),
            Some(res.id)
        );
        let status: String = conn
            .query_row(
                "SELECT status FROM reservation WHERE id = ?1",
                params![res.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "checked_in");
        let linked: Option<i64> = conn
            .query_row(
                "SELECT reservation_id FROM checkin WHERE id = ?1",
                params![checkin.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, Some(res.id), "入住单要记住由哪条预定转正而来");

        // 第二次：已经 checked_in，不再转正
        assert_eq!(
            convert_reservation_for_room(&conn, "260", &today, checkin.id).unwrap(),
            None
        );

        cleanup(conn, path);
    }

    /// 没有预定、或预定已取消时，转正返回 None 且不在入住单上留关联。
    #[test]
    fn convert_reservation_ignores_cancelled_and_missing() {
        let (conn, path) = temp_db();
        insert_room(&conn, "261", Some(51), None, None, None).unwrap();
        let day = |offset: &str| -> String {
            conn.query_row(
                "SELECT date('now','localtime', ?1)",
                params![offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        let today = day("+0 day");
        let tomorrow = day("+1 day");
        let checkin = create_checkin(&conn, "261", None, None).unwrap();

        // 没有任何预定
        assert_eq!(
            convert_reservation_for_room(&conn, "261", &today, checkin.id).unwrap(),
            None
        );

        // 有预定但已取消
        let res = create_reservation(
            &conn,
            &ReservationInput {
                room_no: "261".into(),
                check_in: today.clone(),
                check_in_time: None,
                check_out: tomorrow,
                check_out_time: None,
                guest_name: Some("李四".into()),
                note: None,
            },
            None,
        )
        .unwrap();
        cancel_reservation(&conn, res.id).unwrap();
        assert_eq!(
            convert_reservation_for_room(&conn, "261", &today, checkin.id).unwrap(),
            None
        );
        let linked: Option<i64> = conn
            .query_row(
                "SELECT reservation_id FROM checkin WHERE id = ?1",
                params![checkin.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, None, "没转正就不能留关联");

        cleanup(conn, path);
    }

    #[test]
    fn reservation_overlap_room_status_and_cancel() {
        let (conn, path) = temp_db();
        insert_room(&conn, "801", None, None, None, None).unwrap();
        let day = |offset: &str| -> String {
            conn.query_row(
                "SELECT date('now','localtime', ?1)",
                params![offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        let today = day("+0 day");
        let tomorrow = day("+1 day");
        let day_after = day("+2 days");
        let make = |check_in: &str, check_out: &str| ReservationInput {
            room_no: "801".into(),
            check_in: check_in.into(),
            check_in_time: None,
            check_out: check_out.into(),
            check_out_time: None,
            guest_name: Some("张三".into()),
            note: None,
        };

        let a = create_reservation(&conn, &make(&today, &tomorrow), None).unwrap();
        assert_eq!(
            find_room(&conn, "801").unwrap().unwrap().status,
            "reserved",
            "覆盖今天的预定应把空房标成预定房"
        );
        assert!(
            create_reservation(&conn, &make(&today, &day_after), None).is_err(),
            "同房重叠日期必须拒绝"
        );
        assert!(
            create_reservation(&conn, &make(&day("-1 day"), &tomorrow), None).is_err(),
            "入住日不能早于今天"
        );

        let b = create_reservation(&conn, &make(&tomorrow, &day_after), None).unwrap();
        assert_eq!(b.check_in, tomorrow, "紧邻的下一段不算重叠");

        // 取消 A：今天已无有效预定，房态从 reserved 回退
        let cancelled = cancel_reservation(&conn, a.id).unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(
            find_room(&conn, "801").unwrap().unwrap().status,
            "vacant_clean"
        );
        assert_eq!(
            cancel_reservation(&conn, a.id).unwrap().status,
            "cancelled",
            "重复取消幂等"
        );

        // 取消的预定不参与重叠：C 可以占用同一段
        let c = create_reservation(&conn, &make(&today, &tomorrow), None).unwrap();
        assert_eq!(
            find_room(&conn, "801").unwrap().unwrap().status,
            "reserved"
        );

        // 改期到明天（B 仍占着明天，先取消 B 再改）
        cancel_reservation(&conn, b.id).unwrap();
        let moved = update_reservation(&conn, c.id, &make(&tomorrow, &day_after)).unwrap();
        assert_eq!(moved.check_in, tomorrow);
        assert_eq!(
            find_room(&conn, "801").unwrap().unwrap().status,
            "vacant_clean",
            "改到未来后今天不再覆盖，房态回退"
        );

        let list = list_reservations(&conn, &today, &day_after, Some("801")).unwrap();
        assert_eq!(list.len(), 1, "取消的 A / B 不在列表里，只剩改期后的 C");
        assert_eq!(list[0].id, c.id);

        cleanup(conn, path);
    }

    #[test]
    fn reservation_time_interval_edges() {
        let (conn, path) = temp_db();
        insert_room(&conn, "801", None, None, None, None).unwrap();
        insert_room(&conn, "802", None, None, None, None).unwrap();
        insert_room(&conn, "803", None, None, None, None).unwrap();
        insert_room(&conn, "804", None, None, None, None).unwrap();
        let day = |offset: &str| -> String {
            conn.query_row(
                "SELECT date('now','localtime', ?1)",
                params![offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        let mk = |room: &str, ci: &str, cit: Option<&str>, co: &str, cot: Option<&str>| {
            ReservationInput {
                room_no: room.into(),
                check_in: ci.into(),
                check_in_time: cit.map(str::to_string),
                check_out: co.into(),
                check_out_time: cot.map(str::to_string),
                guest_name: None,
                note: None,
            }
        };

        let a = create_reservation(
            &conn,
            &mk("801", &day("+5 days"), None, &day("+7 days"), None),
            None,
        )
        .unwrap();
        assert_eq!((a.check_in_time.as_str(), a.check_out_time.as_str()), ("12:00", "12:00"));

        let early = create_reservation(
            &conn,
            &mk("802", &day("+0 day"), Some("02:00"), &day("+0 day"), Some("12:00")),
            None,
        )
        .unwrap();
        assert_eq!((early.check_in_time.as_str(), early.check_out_time.as_str()), ("02:00", "12:00"));

        assert!(
            create_reservation(
                &conn,
                &mk("803", &day("+0 day"), Some("12:00"), &day("+0 day"), Some("12:00")),
                None
            )
            .is_err(),
            "同日期同时间的零时长预定必须拒绝"
        );

        let boundary_a = create_reservation(
            &conn,
            &mk("804", &day("+1 day"), Some("12:00"), &day("+2 days"), Some("12:00")),
            None,
        )
        .unwrap();
        let boundary_b = create_reservation(
            &conn,
            &mk("804", &day("+2 days"), Some("12:00"), &day("+3 days"), Some("12:00")),
            None,
        )
        .unwrap();
        assert_eq!(boundary_b.check_in, day("+2 days"), "12:00 边界可紧邻续住");
        assert!(
            create_reservation(
                &conn,
                &mk("804", &day("+2 days"), Some("11:00"), &day("+3 days"), Some("12:00")),
                None
            )
            .is_err(),
            "11:00 入住与 12:00 边界有重叠"
        );
        let _ = (a.id, boundary_a.id);

        let multi = create_reservation(
            &conn,
            &mk("801", &day("+10 days"), Some("14:00"), &day("+12 days"), Some("12:00")),
            None,
        )
        .unwrap();
        let listed = list_reservations(&conn, &day("+10 days"), &day("+12 days"), Some("801"))
            .unwrap();
        assert!(listed.iter().any(|r| r.id == multi.id), "多晚预定应按日期区间返回");

        cleanup(conn, path);
    }

    #[test]
    fn v11_adds_reservation_times_with_default_12() {
        let (conn, path) = temp_db();
        // 模拟 v10：reservation 表已存在但没有时间列，且已有业务数据。
        conn.execute_batch(
            "DROP TABLE reservation;
             CREATE TABLE reservation (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                room_no      TEXT NOT NULL,
                check_in     TEXT NOT NULL,
                check_out    TEXT NOT NULL,
                guest_name   TEXT,
                note         TEXT,
                status       TEXT NOT NULL DEFAULT 'booked' CHECK (status IN ('booked', 'cancelled', 'checked_in')),
                operator_id  INTEGER REFERENCES operator (id),
                created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                cancelled_at TEXT
             );
             INSERT INTO reservation (room_no, check_in, check_out, guest_name)
             VALUES ('801', '2026-11-10', '2026-11-12', '旧预定');",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let (ci, ci_time, co, co_time, name): (String, String, String, String, String) = conn
            .query_row(
                "SELECT check_in, check_in_time, check_out, check_out_time, guest_name
                   FROM reservation",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!((ci.as_str(), ci_time.as_str()), ("2026-11-10", "12:00"));
        assert_eq!((co.as_str(), co_time.as_str()), ("2026-11-12", "12:00"));
        assert_eq!(name, "旧预定");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('reservation') WHERE name = 'check_in_time'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );

        cleanup(conn, path);
    }

    /// 发卡记录恢复落库卡号：带流水号的卡写 16 位卡号，不占号的卡型记 NULL。
    ///
    /// 关联键仍是 `raw_hex`（整卡指纹），主键是自增 `id`——卡号**不**当唯一键，
    /// 因为它不含年月，跨月同日同时同序会重复。
    #[test]
    fn card_no_is_recorded_and_null_when_absent() {
        let (conn, path) = temp_db();

        let guest = record_issuance(
            &conn,
            &NewIssuance {
                room_no: Some("220"),
                kind: "日租客卡",
                days: 1,
                hours: 0,
                card_no: Some(0x2404),
                raw_hex: Some("5BECF000810424AB"),
                operator_id: None,
                reader_id: None,
                result: "ok",
                error_code: None,
                idempotency_key: None,
                duration_ms: None,
            },
        )
        .unwrap();

        let install = record_issuance(
            &conn,
            &NewIssuance {
                room_no: Some("221"),
                kind: "安装卡",
                days: 0,
                hours: 0,
                card_no: None,
                raw_hex: Some("5BECFC0046F20001"),
                operator_id: None,
                reader_id: None,
                result: "ok",
                error_code: None,
                idempotency_key: None,
                duration_ms: None,
            },
        )
        .unwrap();

        let card_no_of = |id: i64| -> Option<i64> {
            conn.query_row(
                "SELECT card_no FROM issuance WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(card_no_of(guest), Some(0x2404), "客卡必须落库卡号");
        assert_eq!(card_no_of(install), None, "安装卡没有流水号，记 NULL");

        // 关联仍按 raw_hex：同一张卡用整卡指纹能查回来
        assert!(find_issuance_by_raw(&conn, "5BECF000810424AB", "日租客卡", Some("220"))
            .unwrap()
            .is_some());

        cleanup(conn, path);
    }

    #[test]
    fn find_issuance_by_raw_prefers_latest_active_and_counts() {
        let (conn, path) = temp_db();
        let mk = |raw: &str, result: &str| {
            record_issuance(
                &conn,
                &NewIssuance {
                    room_no: Some("801"),
                    kind: "日租客卡",
                    days: 1,
                    hours: 0,
                    card_no: None,
                    raw_hex: Some(raw),
                    operator_id: None,
                    reader_id: None,
                    result,
                    error_code: None,
                    idempotency_key: None,
                    duration_ms: None,
                },
            )
            .unwrap()
        };
        let raw = "5BECF100810424AB";
        let a = mk(raw, "ok");
        let b = mk(raw, "ok");
        let hit = find_issuance_by_raw(&conn, raw, "日租客卡", Some("801"))
            .unwrap()
            .unwrap();
        assert_eq!(hit.latest.id, b, "同 raw 的多条记录取最新一条");
        assert_eq!(hit.count, 2);
        assert!(hit.active);

        conn.execute(
            "UPDATE issuance SET checked_out_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1",
            params![b],
        )
        .unwrap();
        let hit = find_issuance_by_raw(&conn, raw, "日租客卡", Some("801"))
            .unwrap()
            .unwrap();
        assert_eq!(hit.latest.id, a, "最新记录退房后回退到上一条有效记录");
        assert_eq!(hit.count, 2);
        assert!(hit.active);

        assert!(find_issuance_by_raw(&conn, " ", "日租客卡", Some("801"))
            .unwrap()
            .is_none());
        assert!(find_issuance_by_raw(&conn, raw, "日租客卡", Some("802"))
            .unwrap()
            .is_none());

        cleanup(conn, path);
    }

    #[test]
    fn date_validation_rejects_impossible_dates() {
        assert!(is_valid_date("2026-02-28"));
        assert!(!is_valid_date("2026-02-29"));
        assert!(is_valid_date("2028-02-29"));
        assert!(!is_valid_date("2026-13-01"));
        assert!(!is_valid_date("2026-00-10"));
        assert!(!is_valid_date("2026-1-01"));
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
    fn snapshot_operators_keeps_password_and_flags() {
        let (conn, path) = temp_db();
        let admin = insert_operator(&conn, "admin", "管理员", "admin", "h1", None).unwrap();
        insert_operator(&conn, "front", "前台", "frontdesk", "h2", None).unwrap();
        set_operator_active(&conn, admin.id, false).unwrap();

        let snap = snapshot_operators(&conn).unwrap();
        assert_eq!(snap.len(), 2);
        let admin = snap.iter().find(|o| o.login == "admin").unwrap();
        assert_eq!(admin.pwd_hash, "h1");
        assert_eq!(admin.role, "admin");
        assert!(!admin.active);
        let front = snap.iter().find(|o| o.login == "front").unwrap();
        assert_eq!(front.pwd_hash, "h2");
        assert!(front.active);
        cleanup(conn, path);
    }

    #[test]
    fn merge_operators_after_restore_preserves_current_accounts() {
        let (conn, path) = temp_db();
        // 模拟恢复后的备份库
        insert_operator(&conn, "admin", "备份管理员", "frontdesk", "backup-admin", None).unwrap();
        insert_operator(&conn, "new", "备份新增", "frontdesk", "backup-new", None).unwrap();

        let current = vec![
            Operator {
                id: 10,
                login: "admin".into(),
                display_name: "当前管理员".into(),
                role: "admin".into(),
                pwd_hash: "current-admin".into(),
                active: true,
                department_id: None,
                department_name: None,
            },
            Operator {
                id: 11,
                login: "only-current".into(),
                display_name: "当前独有".into(),
                role: "admin".into(),
                pwd_hash: "current-only".into(),
                active: true,
                department_id: Some(7),
                department_name: None,
            },
        ];
        let report = merge_operators_after_restore(&conn, &current).unwrap();
        assert_eq!(report.preserved, 1);
        assert_eq!(report.imported_disabled, 1);
        assert_eq!(report.reinserted, 1);
        assert_eq!(report.failed, 0);

        let admin = find_operator(&conn, "admin").unwrap().unwrap();
        assert_eq!(admin.pwd_hash, "current-admin", "密码优先保留当前");
        assert_eq!(admin.role, "admin", "角色优先保留当前");
        assert!(admin.active, "启用状态优先保留当前");
        assert_eq!(admin.display_name, "备份管理员", "显示名用备份值");

        let new = find_operator(&conn, "new").unwrap().unwrap();
        assert!(!new.active, "备份新增账号导入停用");

        let only = find_operator(&conn, "only-current").unwrap().unwrap();
        assert_eq!(only.pwd_hash, "current-only");
        assert!(only.active);
        assert_eq!(only.department_id, None, "补回账号部门置空");
        cleanup(conn, path);
    }

    #[test]
    fn merge_operators_after_restore_counts_failures() {
        let (conn, path) = temp_db();
        insert_operator(&conn, "admin", "备份管理员", "admin", "backup-admin", None).unwrap();
        // 非法角色触发 CHECK，验证失败计数且不中断
        let current = vec![Operator {
            id: 1,
            login: "admin".into(),
            display_name: "当前".into(),
            role: "bad-role".into(),
            pwd_hash: "h".into(),
            active: true,
            department_id: None,
            department_name: None,
        }];
        let report = merge_operators_after_restore(&conn, &current).unwrap();
        assert_eq!(report.preserved, 0);
        assert_eq!(report.failed, 1);
        let admin = find_operator(&conn, "admin").unwrap().unwrap();
        assert_eq!(admin.pwd_hash, "backup-admin", "失败时备份账号保持原值");
        cleanup(conn, path);
    }

    #[test]
    fn restore_backup_preserves_current_operators() {
        let (mut conn, path) = temp_db();
        insert_operator(&conn, "admin", "当前管理员", "admin", "current-hash", None).unwrap();
        insert_room(&conn, "701", None, None, None, None).unwrap();
        let current = snapshot_operators(&conn).unwrap();

        // 制作一份「备份库」：admin 密码不同，另有新增账号与另一间房
        let (backup, bpath) = temp_db();
        insert_operator(&backup, "admin", "备份管理员", "frontdesk", "backup-hash", None).unwrap();
        insert_operator(&backup, "new", "备份新增", "frontdesk", "new-hash", None).unwrap();
        insert_room(&backup, "702", None, None, None, None).unwrap();
        let file = std::env::temp_dir().join(format!("ocw-restore-{}.db", uuid::Uuid::new_v4()));
        backup_to(&backup, &file).unwrap();

        restore_from(&mut conn, &file).unwrap();
        migrate(&conn).unwrap();
        let report = merge_operators_after_restore(&conn, &current).unwrap();

        assert_eq!(report.preserved, 1);
        assert_eq!(report.imported_disabled, 1);
        assert_eq!(report.failed, 0);
        let admin = find_operator(&conn, "admin").unwrap().unwrap();
        assert_eq!(admin.pwd_hash, "current-hash", "恢复后当前密码不被备份覆盖");
        assert_eq!(admin.role, "admin");
        assert!(admin.active);
        let new = find_operator(&conn, "new").unwrap().unwrap();
        assert!(!new.active, "备份新增账号默认停用");
        // 业务数据按备份覆盖
        assert!(find_room(&conn, "701").unwrap().is_none());
        assert!(find_room(&conn, "702").unwrap().is_some());

        let _ = std::fs::remove_file(&file);
        cleanup(conn, path);
        cleanup(backup, bpath);
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
    fn active_card_count_counts_same_card_no_separately() {
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
        mk(1, "+3 days"); // 同号多张（TD-127 复用卡号）：按「张」计数，不再按卡号去重

        assert_eq!(count_active_cards(&conn, "220").unwrap(), 3);
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
        mk(3, "-1 hour"); // 已过期：不计入 closed，但退房时要一并标记，避免到期提醒残留

        assert_eq!(count_active_cards(&conn, "220").unwrap(), 2);
        assert_eq!(find_room(&conn, "220").unwrap().unwrap().status, "vacant_clean");

        let closed = checkout_room(&conn, "220", "vacant_dirty").unwrap();
        assert_eq!(closed, 2, "closed 只数仍有效的卡");
        let expired_checked_out: Option<String> = conn
            .query_row(
                "SELECT checked_out_at FROM issuance WHERE room_no = '220' AND card_no = 3",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            expired_checked_out.is_some(),
            "已过期客卡也要标记退房，避免到期提醒残留"
        );
        assert_eq!(count_active_cards(&conn, "220").unwrap(), 0);
        assert_eq!(
            find_room(&conn, "220").unwrap().unwrap().status,
            "vacant_dirty"
        );

        cleanup(conn, path);
    }

    /// 「所有关联卡都退完才关单」：同一张登记单挂两张卡，退第一张不关，退第二张才关。
    #[test]
    fn checkin_closes_only_after_all_linked_cards_checked_out() {
        let (conn, path) = temp_db();
        insert_room(&conn, "220", Some(15), None, None, None).unwrap();
        let checkin = create_checkin(&conn, "220", None, None).unwrap();

        let mk = |card_no: i64, raw: &'static str| {
            let id = record_issuance(
                &conn,
                &NewIssuance {
                    room_no: Some("220"),
                    kind: "日租客卡",
                    days: 1,
                    hours: 0,
                    card_no: Some(card_no),
                    raw_hex: Some(raw),
                    operator_id: None,
                    reader_id: None,
                    result: "ok",
                    error_code: None,
                    idempotency_key: None,
                    duration_ms: None,
                },
            )
            .unwrap();
            attach_issuance_to_open_checkin(&conn, id, "220").unwrap();
            id
        };
        let a = mk(0x2401, "5BECF000810424AB");
        let b = mk(0x2402, "5BECF001810424AB");

        assert!(checkout_issuance(&conn, a, "vacant_dirty").unwrap().is_some());
        assert_eq!(
            find_checkin(&conn, checkin.id).unwrap().unwrap().status,
            "open",
            "同单还有一张有效卡，不能关单"
        );

        assert!(checkout_issuance(&conn, b, "vacant_dirty").unwrap().is_some());
        assert_eq!(
            find_checkin(&conn, checkin.id).unwrap().unwrap().status,
            "closed",
            "关联卡全部退完，登记单要一起关"
        );
        cleanup(conn, path);
    }

    /// 同房还有别的、**不属于这张登记单**的卡时，退掉关联卡也应该关单——
    /// 判据是关联卡，而不是「该房剩余卡数」。
    #[test]
    fn checkin_ignores_cards_not_linked_to_it() {
        let (conn, path) = temp_db();
        insert_room(&conn, "221", Some(16), None, None, None).unwrap();

        // 先发一张没挂登记单的卡（此时还没有 open 单）
        let _stray = record_issuance(
            &conn,
            &NewIssuance {
                room_no: Some("221"),
                kind: "日租客卡",
                days: 1,
                hours: 0,
                card_no: Some(0x2403),
                raw_hex: Some("5BECF002810424AB"),
                operator_id: None,
                reader_id: None,
                result: "ok",
                error_code: None,
                idempotency_key: None,
                duration_ms: None,
            },
        )
        .unwrap();

        let checkin = create_checkin(&conn, "221", None, None).unwrap();
        let linked = record_issuance(
            &conn,
            &NewIssuance {
                room_no: Some("221"),
                kind: "日租客卡",
                days: 1,
                hours: 0,
                card_no: Some(0x2404),
                raw_hex: Some("5BECF003810424AB"),
                operator_id: None,
                reader_id: None,
                result: "ok",
                error_code: None,
                idempotency_key: None,
                duration_ms: None,
            },
        )
        .unwrap();
        attach_issuance_to_open_checkin(&conn, linked, "221").unwrap();

        assert!(checkout_issuance(&conn, linked, "vacant_dirty").unwrap().is_some());
        assert_eq!(
            find_checkin(&conn, checkin.id).unwrap().unwrap().status,
            "closed",
            "关联卡已退完，尽管同房还有不属于该单的卡"
        );
        cleanup(conn, path);
    }

    /// 退房时勾了「同时清除入住登记」：即使该单还有有效卡，也强制关单（只关不删，档案保留）。
    #[test]
    fn force_close_checkin_ignores_remaining_cards() {
        let (conn, path) = temp_db();
        insert_room(&conn, "222", Some(17), None, None, None).unwrap();
        let checkin = create_checkin(&conn, "222", None, None).unwrap();

        let mk = |card_no: i64, raw: &'static str| {
            let id = record_issuance(
                &conn,
                &NewIssuance {
                    room_no: Some("222"),
                    kind: "日租客卡",
                    days: 1,
                    hours: 0,
                    card_no: Some(card_no),
                    raw_hex: Some(raw),
                    operator_id: None,
                    reader_id: None,
                    result: "ok",
                    error_code: None,
                    idempotency_key: None,
                    duration_ms: None,
                },
            )
            .unwrap();
            attach_issuance_to_open_checkin(&conn, id, "222").unwrap();
            id
        };
        let _first = mk(0x2501, "5BECF004810424AB");
        let second = mk(0x2502, "5BECF005810424AB");

        // 只退一张：关联卡还剩一张，按自动判据登记单仍是 open
        assert!(checkout_issuance(&conn, second, "vacant_dirty").unwrap().is_some());
        assert_eq!(
            find_checkin(&conn, checkin.id).unwrap().unwrap().status,
            "open"
        );

        // 勾了「同时清除」→ 强制关单，即使该单还有有效卡
        assert_eq!(close_open_checkin_for_room_force(&conn, "222").unwrap(), 1);
        assert_eq!(
            find_checkin(&conn, checkin.id).unwrap().unwrap().status,
            "closed"
        );
        // 是关闭而不是删除：登记单与住客档案都还在
        assert!(get_checkin(&conn, checkin.id).unwrap().is_some());

        cleanup(conn, path);
    }

    /// `clearCheckin` 的 true / false 差别：同样是「还剩一张有效卡」的现场，
    /// 不强制时登记单保持 open，强制时立即 closed。
    #[test]
    fn clear_checkin_flag_decides_whether_checkin_closes() {
        let (conn, path) = temp_db();
        let add_card = |room: &'static str, card_no: i64, raw: &'static str| {
            let id = record_issuance(
                &conn,
                &NewIssuance {
                    room_no: Some(room),
                    kind: "日租客卡",
                    days: 1,
                    hours: 0,
                    card_no: Some(card_no),
                    raw_hex: Some(raw),
                    operator_id: None,
                    reader_id: None,
                    result: "ok",
                    error_code: None,
                    idempotency_key: None,
                    duration_ms: None,
                },
            )
            .unwrap();
            attach_issuance_to_open_checkin(&conn, id, room).unwrap();
            id
        };

        // false：正常退一张，还有一张有效卡 → 登记单保持 open
        insert_room(&conn, "230", Some(20), None, None, None).unwrap();
        let keep = create_checkin(&conn, "230", None, None).unwrap();
        let _k1 = add_card("230", 0x2601, "5BECF006810424AB");
        let k2 = add_card("230", 0x2602, "5BECF007810424AB");
        assert!(checkout_issuance(&conn, k2, "vacant_dirty").unwrap().is_some());
        assert_eq!(
            find_checkin(&conn, keep.id).unwrap().unwrap().status,
            "open",
            "clearCheckin=false：关联卡还有有效卡，不关单"
        );

        // true：同样还剩一张有效卡，强制关单
        insert_room(&conn, "231", Some(21), None, None, None).unwrap();
        let drop_it = create_checkin(&conn, "231", None, None).unwrap();
        let _d1 = add_card("231", 0x2603, "5BECF008810424AB");
        let d2 = add_card("231", 0x2604, "5BECF009810424AB");
        assert!(checkout_issuance(&conn, d2, "vacant_dirty").unwrap().is_some());
        assert_eq!(close_open_checkin_for_room_force(&conn, "231").unwrap(), 1);
        assert_eq!(
            find_checkin(&conn, drop_it.id).unwrap().unwrap().status,
            "closed",
            "clearCheckin=true：即使还剩有效卡也关单"
        );

        cleanup(conn, path);
    }

    /// 房间上的「登记人数」只看 **open 单**的住客：有 2 人、有单没人、单已关闭三种情况。
    #[test]
    fn room_guest_count_only_counts_open_checkin() {
        let (conn, path) = temp_db();
        insert_room(&conn, "240", Some(30), None, None, None).unwrap();
        insert_room(&conn, "241", Some(31), None, None, None).unwrap();
        insert_room(&conn, "242", Some(32), None, None, None).unwrap();

        let add_guest = |checkin_id: i64, name: &'static str| {
            add_checkin_guest(
                &conn,
                checkin_id,
                &GuestIdentity {
                    name: Some(name),
                    gender: None,
                    id_type: None,
                    id_number: None,
                    nation: None,
                    address: None,
                    birth_date: None,
                    note: None,
                    id_valid_from: None,
                    id_valid_to: None,
                    id_authority: None,
                    device_model: None,
                    device_serial: None,
                },
            )
            .unwrap();
        };

        // 240：open 单 + 2 位住客
        let a = create_checkin(&conn, "240", None, None).unwrap();
        add_guest(a.id, "张三");
        add_guest(a.id, "李四");

        // 241：open 单但没录住客
        let _empty = create_checkin(&conn, "241", None, None).unwrap();

        // 242：单已关闭（2 位住客也不该计入）
        let closed = create_checkin(&conn, "242", None, None).unwrap();
        add_guest(closed.id, "王五");
        close_checkin(&conn, closed.id).unwrap();

        let rooms = list_rooms(&conn).unwrap();
        let of = |no: &str| {
            rooms
                .iter()
                .find(|r| r.room_no == no)
                .unwrap_or_else(|| panic!("房间 {no} 不存在"))
                .guest_count
        };
        assert_eq!(of("240"), 2, "open 单的住客数");
        assert_eq!(of("241"), 0, "open 单但没录住客");
        assert_eq!(of("242"), 0, "单已关闭，不再计入");

        cleanup(conn, path);
    }

    /// 登记单列表带出房间序号：正常房间有值；房号在 room 表里查不到时为 None。
    #[test]
    fn list_checkins_carries_room_index() {
        let (conn, path) = temp_db();
        insert_room(&conn, "250", Some(40), None, None, None).unwrap();
        let known = create_checkin(&conn, "250", None, None).unwrap();
        // room 表里没有这个房号（历史数据 / 改名后残留）
        let orphan = create_checkin(&conn, "999", None, None).unwrap();

        let filter = CheckinFilter {
            room_no: None,
            status: None,
            from: None,
            to: None,
            page: 1,
            page_size: 50,
        };
        let page = list_checkins(&conn, &filter).unwrap();
        let of = |id: i64| page.rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(of(known.id).room_index, Some(40));
        assert_eq!(of(orphan.id).room_index, None, "房号查不到序号时留空");

        cleanup(conn, path);
    }

    #[test]
    fn checkout_issuance_closes_only_that_record() {
        let (conn, path) = temp_db();
        insert_room(&conn, "205", None, None, None, None).unwrap();
        let id1 = record_issuance(&conn, &sample_issuance(Some("205"), "日租客卡", 1, 0)).unwrap();
        let id2 = record_issuance(&conn, &sample_issuance(Some("205"), "日租客卡", 2, 0)).unwrap();

        assert_eq!(count_active_cards(&conn, "205").unwrap(), 2);

        // 退掉其中一条：房间还剩 1 张，房态不应变脏
        let (room, remaining) = checkout_issuance(&conn, id1, "vacant_dirty").unwrap().unwrap();
        assert_eq!(room, "205");
        assert_eq!(remaining, 1);
        assert_eq!(count_active_cards(&conn, "205").unwrap(), 1);
        assert_eq!(find_room(&conn, "205").unwrap().unwrap().status, "vacant_clean");

        // 再退最后一条：房态转空脏
        let (_, remaining) = checkout_issuance(&conn, id2, "vacant_dirty").unwrap().unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(find_room(&conn, "205").unwrap().unwrap().status, "vacant_dirty");

        // 不存在的记录
        assert!(checkout_issuance(&conn, 99999, "vacant_dirty").unwrap().is_none());

        cleanup(conn, path);
    }

    /// TD-220：建单复用、多人按 seq 排序、关单后可新开、列表首客与人数。
    #[test]
    fn checkin_create_reuses_open_and_orders_guests() {
        let (conn, path) = temp_db();
        insert_room(&conn, "801", None, None, None, None).unwrap();

        let a = create_checkin(&conn, "801", None, None).unwrap();
        let b = create_checkin(&conn, "801", None, None).unwrap();
        assert_eq!(a.id, b.id, "同房已有 open 单应复用");

        let guest = |name: &'static str, id_number: &'static str| GuestIdentity {
            name: Some(name),
            gender: Some("男"),
            id_type: Some("身份证"),
            id_number: Some(id_number),
            nation: None,
            address: None,
            birth_date: None,
            note: None,
            id_valid_from: None,
            id_valid_to: None,
            id_authority: None,
            device_model: None,
            device_serial: None,
        };
        add_checkin_guest(&conn, a.id, &guest("张三", "1101")).unwrap();
        add_checkin_guest(&conn, a.id, &guest("李四", "1102")).unwrap();

        let detail = get_checkin(&conn, a.id).unwrap().unwrap();
        assert_eq!(detail.guests.len(), 2);
        assert_eq!(detail.guests[0].seq, 1);
        assert_eq!(detail.guests[0].name.as_deref(), Some("张三"));
        assert_eq!(detail.guests[1].seq, 2);
        assert_eq!(detail.guests[1].name.as_deref(), Some("李四"));

        let page = list_checkins(
            &conn,
            &CheckinFilter {
                room_no: Some("801".into()),
                status: None,
                from: None,
                to: None,
                page: 1,
                page_size: 20,
            },
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].guest_count, 2);
        assert_eq!(page.rows[0].first_guest_name.as_deref(), Some("张三"), "首个登记人取 seq 最小");

        // 关闭后不再复用；退房联动也走同一条 SQL
        close_checkin(&conn, a.id).unwrap();
        assert!(get_open_checkin(&conn, "801").unwrap().is_none());
        let c = create_checkin(&conn, "801", None, None).unwrap();
        assert_ne!(c.id, a.id, "关闭后再建应是新单");
        assert_eq!(close_open_checkin_for_room(&conn, "801").unwrap(), 1);

        cleanup(conn, path);
    }

    /// TD-224：既无姓名也无证件号的空白住客必须被拒（前端任何漏判的路径都不能落库）。
    #[test]
    fn checkin_rejects_blank_guest() {
        let (conn, path) = temp_db();
        insert_room(&conn, "803", None, None, None, None).unwrap();
        let c = create_checkin(&conn, "803", None, None).unwrap();

        // 只有证件类型 —— 就是 bug 里看到的那种空白住客
        let blank = GuestIdentity {
            name: None,
            gender: None,
            id_type: Some("身份证"),
            id_number: None,
            nation: None,
            address: None,
            birth_date: None,
            note: None,
            id_valid_from: None,
            id_valid_to: None,
            id_authority: None,
            device_model: None,
            device_serial: None,
        };
        assert!(!blank.has_identity());
        assert!(add_checkin_guest(&conn, c.id, &blank).is_err(), "空白住客应被拒");

        // 纯空格也算空
        let spaces = GuestIdentity {
            name: Some("   "),
            id_number: Some("  "),
            ..blank
        };
        assert!(!spaces.has_identity());
        assert!(add_checkin_guest(&conn, c.id, &spaces).is_err(), "全空格应被拒");
        assert_eq!(get_checkin(&conn, c.id).unwrap().unwrap().guests.len(), 0);

        // 只有姓名（无证件号）是合法的
        let name_only = GuestIdentity {
            name: Some("张三"),
            ..blank
        };
        assert!(name_only.has_identity());
        let id = add_checkin_guest(&conn, c.id, &name_only).unwrap();

        // 改成空白也要拒，原来的资料不能丢
        assert!(update_checkin_guest(&conn, id, &blank).is_err(), "改成空白住客应被拒");
        let detail = get_checkin(&conn, c.id).unwrap().unwrap();
        assert_eq!(detail.guests[0].name.as_deref(), Some("张三"), "被拒后原资料应保留");

        cleanup(conn, path);
    }

    /// TD-221：v14 把历史身份信息迁成「一单一客」，重复迁移不重复建单。
    #[test]
    fn v14_migrates_legacy_guest_identity_once() {
        let (conn, path) = temp_db();
        insert_room(&conn, "802", None, None, None, None).unwrap();
        conn.execute(
            "INSERT INTO issuance
                 (uuid, room_no, kind, days, hours, result, guest_name, guest_id_number, guest_gender)
             VALUES (?1, '802', '日租客卡', 1, 0, 'ok', '王五', '3301', '男')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        let issuance_id = conn.last_insert_rowid();

        migrate(&conn).unwrap();
        let checkin_id: Option<i64> = conn
            .query_row(
                "SELECT checkin_id FROM issuance WHERE id = ?1",
                params![issuance_id],
                |r| r.get(0),
            )
            .unwrap();
        let checkin_id = checkin_id.expect("迁移后应回填 checkin_id");

        let detail = get_checkin(&conn, checkin_id).unwrap().unwrap();
        assert_eq!(detail.checkin.status, "closed");
        assert_eq!(detail.checkin.room_no, "802");
        assert_eq!(detail.guests.len(), 1);
        assert_eq!(detail.guests[0].seq, 1);
        assert_eq!(detail.guests[0].name.as_deref(), Some("王五"));
        assert_eq!(detail.guests[0].id_number.as_deref(), Some("3301"));

        // 再跑一次迁移：不能重复建单
        migrate(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM checkin", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "迁移必须幂等");

        cleanup(conn, path);
    }

    #[test]
    fn guest_stays_expose_all_four_states() {
        let (conn, path) = temp_db();
        insert_room(&conn, "601", None, None, None, None).unwrap();
        let active = record_issuance(&conn, &sample_issuance(Some("601"), "日租客卡", 2, 0)).unwrap();
        let checked = record_issuance(&conn, &sample_issuance(Some("601"), "日租客卡", 2, 0)).unwrap();
        let invalid = record_issuance(&conn, &sample_issuance(Some("601"), "日租客卡", 2, 0)).unwrap();
        let expired = record_issuance(&conn, &sample_issuance(Some("601"), "日租客卡", 2, 0)).unwrap();

        checkout_issuance(&conn, checked, "vacant_dirty").unwrap();
        conn.execute(
            "UPDATE issuance
                SET invalidated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                    invalid_reason = 'lost'
              WHERE id = ?1",
            params![invalid],
        )
        .unwrap();
        // 模拟「8 号入住、9 号到期、今天 10 号都没退房」：只让 expires_at 过期
        conn.execute(
            "UPDATE issuance
                SET expires_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 day')
              WHERE id = ?1",
            params![expired],
        )
        .unwrap();

        let from: String = conn
            .query_row("SELECT date('now','localtime','-7 days')", [], |r| r.get(0))
            .unwrap();
        let to: String = conn
            .query_row("SELECT date('now','localtime','+7 days')", [], |r| r.get(0))
            .unwrap();
        let stays = list_guest_stays(&conn, &from, &to).unwrap();
        let state = |id: i64| -> Option<String> {
            stays.iter().find(|s| s.id == id).map(|s| s.status.clone())
        };

        assert_eq!(state(active).as_deref(), Some("active"));
        assert_eq!(state(checked).as_deref(), Some("checked_out"));
        assert_eq!(state(invalid).as_deref(), Some("invalidated"));
        assert_eq!(state(expired).as_deref(), Some("expired"), "过期未退房不能算在住");

        cleanup(conn, path);
    }

    /// TD-221：日历只画发卡区间 —— 只登记没发卡的不会出现在 list_guest_stays。
    #[test]
    fn guest_stays_only_include_issued_cards() {
        let (conn, path) = temp_db();
        insert_room(&conn, "901", None, None, None, None).unwrap();
        let checkin = create_checkin(&conn, "901", None, None).unwrap();
        add_checkin_guest(
            &conn,
            checkin.id,
            &GuestIdentity {
                name: Some("赵六"),
                gender: None,
                id_type: None,
                id_number: Some("4401"),
                nation: None,
                address: None,
                birth_date: None,
                note: None,
                id_valid_from: None,
                id_valid_to: None,
                id_authority: None,
                device_model: None,
                device_serial: None,
            },
        )
        .unwrap();

        let from: String = conn
            .query_row("SELECT date('now','localtime','-7 days')", [], |r| r.get(0))
            .unwrap();
        let to: String = conn
            .query_row("SELECT date('now','localtime','+7 days')", [], |r| r.get(0))
            .unwrap();
        assert!(
            list_guest_stays(&conn, &from, &to).unwrap().is_empty(),
            "只登记未发卡不应画到日历"
        );

        let id = record_issuance(&conn, &sample_issuance(Some("901"), "日租客卡", 1, 0)).unwrap();
        attach_issuance_to_open_checkin(&conn, id, "901").unwrap();
        let stays = list_guest_stays(&conn, &from, &to).unwrap();
        assert_eq!(stays.len(), 1);
        assert_eq!(stays[0].guest_name.as_deref(), Some("赵六"), "姓名取首个登记人");
        assert_eq!(stays[0].guest_count, 1);
        assert_eq!(stays[0].checkin_id, Some(checkin.id));

        cleanup(conn, path);
    }

    /// TD-218：挂失卡作废同房旧客卡。已退房记录不被二次作废，挂失卡本身不计入有效卡。
    #[test]
    fn invalidate_guest_cards_skips_closed_records() {
        let (conn, path) = temp_db();
        insert_room(&conn, "512", None, None, None, None).unwrap();
        let old1 = record_issuance(&conn, &sample_issuance(Some("512"), "日租客卡", 1, 0)).unwrap();
        let old2 = record_issuance(&conn, &sample_issuance(Some("512"), "日租客卡", 2, 0)).unwrap();
        let gone = record_issuance(&conn, &sample_issuance(Some("512"), "日租客卡", 1, 0)).unwrap();
        assert!(checkout_issuance(&conn, gone, "vacant_dirty").unwrap().is_some());
        assert_eq!(count_active_cards(&conn, "512").unwrap(), 2);
        assert!(latest_active_guest_card(&conn, "512").unwrap().is_some());

        // 发一张挂失卡 → 作废仍有效的两张旧卡
        let lost_card =
            record_issuance(&conn, &sample_issuance(Some("512"), "日租客卡", 1, 0)).unwrap();
        let n = invalidate_guest_cards(&conn, "512", lost_card).unwrap();
        assert_eq!(n, 2, "两张仍有效的旧卡应被作废");
        assert_eq!(count_active_cards(&conn, "512").unwrap(), 0, "挂失卡本身不计入有效卡");
        assert!(latest_active_guest_card(&conn, "512").unwrap().is_none());

        let reason = |id: i64| -> Option<String> {
            conn.query_row(
                "SELECT invalid_reason FROM issuance WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(reason(old1).as_deref(), Some("lost"));
        assert_eq!(reason(old2).as_deref(), Some("lost"));
        assert_eq!(reason(lost_card).as_deref(), Some("lost_card"));
        assert!(reason(gone).is_none(), "已退房记录不应被二次作废");
        assert!(!issuance_is_active(&conn, lost_card).unwrap());

        cleanup(conn, path);
    }

    /// TD-214：已满房退掉一张 → 未满房；退最后一张 → 退房默认态。
    #[test]
    fn checkout_downgrades_full_room_to_partial() {
        let (conn, path) = temp_db();
        insert_room(&conn, "311", None, None, None, None).unwrap();
        let id1 = record_issuance(&conn, &sample_issuance(Some("311"), "日租客卡", 1, 0)).unwrap();
        let id2 = record_issuance(&conn, &sample_issuance(Some("311"), "日租客卡", 1, 0)).unwrap();
        set_room_status(&conn, "311", "occupied_full").unwrap();

        let (_, remaining) = checkout_issuance(&conn, id1, "vacant_dirty").unwrap().unwrap();
        assert_eq!(remaining, 1);
        assert_eq!(
            find_room(&conn, "311").unwrap().unwrap().status,
            "occupied_partial",
            "还剩 1 张有效卡，已满房应回退未满房"
        );

        let (_, remaining) = checkout_issuance(&conn, id2, "vacant_dirty").unwrap().unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(
            find_room(&conn, "311").unwrap().unwrap().status,
            "vacant_dirty",
            "最后一张退掉后转退房默认态"
        );

        cleanup(conn, path);
    }

    /// TD-214：维修 / 其他等人工房态不被退房覆盖（单卡与批量两条路径）。
    #[test]
    fn checkout_keeps_manual_room_status() {
        let (conn, path) = temp_db();
        insert_room(&conn, "312", None, None, None, None).unwrap();
        let id1 = record_issuance(&conn, &sample_issuance(Some("312"), "日租客卡", 1, 0)).unwrap();
        set_room_status(&conn, "312", "maintenance").unwrap();
        checkout_issuance(&conn, id1, "vacant_dirty").unwrap().unwrap();
        assert_eq!(
            find_room(&conn, "312").unwrap().unwrap().status,
            "maintenance",
            "维修房退房后仍是维修"
        );

        let _id2 = record_issuance(&conn, &sample_issuance(Some("312"), "日租客卡", 1, 0)).unwrap();
        set_room_status(&conn, "312", "other").unwrap();
        checkout_room(&conn, "312", "vacant_dirty").unwrap();
        assert_eq!(
            find_room(&conn, "312").unwrap().unwrap().status,
            "other",
            "其他房批量退房后仍是其他"
        );

        cleanup(conn, path);
    }
    // ---------------- 备份 / 恢复 ----------------

    /// 备份 → 改库 → 恢复：数据要能完整回来，且备份文件必须自包含（可只读打开）。
    #[test]
    fn backup_and_restore_roundtrip() {
        let (mut conn, path) = temp_db();
        insert_room(&conn, "601", Some(61), None, None, None).unwrap();
        record_issuance(&conn, &sample_issuance(Some("601"), "日租客卡", 1, 0)).unwrap();

        let dir = std::env::temp_dir().join(format!("ocw-bk-{}", uuid::Uuid::new_v4()));
        let file = dir.join("ocw-test.db");
        let bytes = backup_to(&conn, &file).expect("备份应成功");
        assert!(bytes > 0);
        assert!(file.is_file());
        // 备份必须是「单个自包含文件」：不能留下 -wal / -shm 兄弟文件
        let wal = std::path::PathBuf::from(format!("{}-wal", file.display()));
        let shm = std::path::PathBuf::from(format!("{}-shm", file.display()));
        assert!(!wal.exists(), "备份不该带 -wal：{}", wal.display());
        assert!(!shm.exists(), "备份不该带 -shm：{}", shm.display());

        // 备份文件自包含：只读打开也能读到完整结构
        let info = inspect_backup(&file).expect("应识别为自己的库");
        assert_eq!(info.schema_version, SCHEMA_VERSION);
        assert_eq!(info.rooms, 1);
        assert_eq!(info.issuances, 1);
        assert!(info.tables >= 8, "表数量异常：{}", info.tables);

        // 破坏当前库，再恢复
        conn.execute("DELETE FROM room", []).unwrap();
        conn.execute("DELETE FROM issuance", []).unwrap();
        assert!(find_room(&conn, "601").unwrap().is_none());

        restore_from(&mut conn, &file).expect("恢复应成功");
        migrate(&conn).expect("恢复后迁移应幂等");

        assert_eq!(find_room(&conn, "601").unwrap().map(|r| r.room_index), Some(61));
        assert_eq!(count_active_cards(&conn, "601").unwrap(), 1, "发卡记录也应回来");

        let _ = std::fs::remove_dir_all(&dir);
        cleanup(conn, path);
    }

    /// v14 → v15：`room_index` 的 CHECK 从 255 放宽到 4094。
    ///
    /// 这是**重建表**式迁移，老库的每一列（含 v2 之后 ALTER 加的 `room_type_id` /
    /// `max_cards` / `status`）都必须原样带过来；迁完要能插 4094，
    /// v16：checkin 多出 reservation_id 列；迁移幂等，已有登记单与住客原样保留。
    #[test]
    fn v16_adds_reservation_id_and_keeps_existing_checkins() {
        let (conn, path) = temp_db();
        assert!(
            column_exists(&conn, "checkin", "reservation_id").unwrap(),
            "v16 必须给 checkin 加上 reservation_id"
        );

        insert_room(&conn, "270", Some(60), None, None, None).unwrap();
        let checkin = create_checkin(&conn, "270", None, None).unwrap();
        add_checkin_guest(
            &conn,
            checkin.id,
            &GuestIdentity {
                name: Some("老客人"),
                gender: None,
                id_type: None,
                id_number: None,
                nation: None,
                address: None,
                birth_date: None,
                note: None,
                id_valid_from: None,
                id_valid_to: None,
                id_authority: None,
                device_model: None,
                device_serial: None,
            },
        )
        .unwrap();

        // 再跑一次迁移：幂等，且老数据原样保留
        migrate(&conn).unwrap();
        let d = get_checkin(&conn, checkin.id).unwrap().unwrap();
        assert_eq!(d.guests.len(), 1);
        assert_eq!(d.guests[0].name.as_deref(), Some("老客人"));
        let linked: Option<i64> = conn
            .query_row(
                "SELECT reservation_id FROM checkin WHERE id = ?1",
                params![checkin.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, None, "迁移前的登记单没有预定关联");

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        cleanup(conn, path);
    }

    /// 而 4095（与总控卡页1 字节相同）仍被数据层拒绝。
    #[test]
    fn v15_widens_room_index_to_4094() {
        let (conn, path) = temp_db();

        // 把库还原成 v14 形态：room 表仍是 255 上界
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_room_index;
             DROP TABLE room;
             CREATE TABLE room (
                 room_no         TEXT PRIMARY KEY,
                 room_index      INTEGER NOT NULL UNIQUE CHECK (room_index >= 0 AND room_index <= 255),
                 building        TEXT,
                 floor           INTEGER,
                 room_type       TEXT,
                 active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
                 install_card_at TEXT,
                 created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                 room_type_id    INTEGER REFERENCES room_type (id),
                 max_cards       INTEGER,
                 status          TEXT NOT NULL DEFAULT 'vacant_clean'
             );
             CREATE INDEX idx_room_index ON room (room_index);
             INSERT INTO room
                 (room_no, room_index, building, floor, room_type, active,
                  install_card_at, created_at, room_type_id, max_cards, status)
             VALUES ('220', 15, 'A', 2, '大床房', 1, NULL, '2026-11-01T00:00:00Z', NULL, 2, 'occupied_partial'),
                    ('221', 16, NULL, NULL, NULL, 0, '2026-11-02T00:00:00Z',
                     '2026-11-02T00:00:00Z', NULL, NULL, 'vacant_clean');
             DELETE FROM schema_version WHERE version >= 15;",
        )
        .unwrap();

        migrate(&conn).expect("v15 迁移应成功");

        // 老数据一列不丢
        let room = find_room(&conn, "220").unwrap().unwrap();
        assert_eq!(room.room_index, 15);
        assert_eq!(room.building.as_deref(), Some("A"));
        assert_eq!(room.floor, Some(2));
        assert_eq!(room.max_cards, Some(2));
        assert_eq!(room.status, "occupied_partial");
        assert!(room.active);
        assert!(!find_room(&conn, "221").unwrap().unwrap().active);
        let created: String = conn
            .query_row(
                "SELECT created_at FROM room WHERE room_no = '220'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(created, "2026-11-01T00:00:00Z", "created_at 不能被改写成当前时间");

        // 新上界生效：4094 可分配；4095 与总控卡字节相同，必须被拒
        let high = insert_room(&conn, "9001", Some(4094), None, None, None).unwrap();
        assert_eq!(high.room_index, 4094);
        let err = insert_room(&conn, "9002", Some(4095), None, None, None)
            .expect_err("4095 与总控卡冲突，不能分配给房间");
        assert!(format!("{err}").contains("4095"), "错误文案要点明 4095：{err}");

        // 索引仍在（按序号查房间不该退化成全表扫）
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_room_index'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "重建表后要补回 idx_room_index");

        // 幂等：再迁一次不报错，数据不变
        migrate(&conn).expect("重复迁移应幂等");
        assert_eq!(
            find_room_by_index(&conn, 4094).unwrap().unwrap().room_no,
            "9001"
        );

        cleanup(conn, path);
    }

    /// 不是本程序的库（缺 room/issuance/schema_version）必须被拒绝，而不是恢复出半截数据。
    #[test]
    fn inspect_backup_rejects_foreign_file() {
        let dir = std::env::temp_dir().join(format!("ocw-bk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("junk.db");
        let other = Connection::open(&file).unwrap();
        other.execute_batch("CREATE TABLE foo (x INTEGER);").unwrap();
        drop(other);

        let err = inspect_backup(&file).expect_err("外部库必须拒绝");
        assert!(err.to_string().contains("不是 OpenCardWright"), "错误文案要指明原因：{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 房型的「N 间」必须排除已逻辑删除的房间（否则删过房的酒店数字永远对不上）。
    #[test]
    fn room_type_count_excludes_deleted_room() {
        let (conn, path) = temp_db();
        let t = insert_room_type(&conn, "大床房", 0).unwrap();
        insert_room(&conn, "701", Some(71), None, None, Some(t.id)).unwrap();
        assert_eq!(list_room_types(&conn).unwrap()[0].room_count, 1);

        deactivate_room(&conn, "701").unwrap();
        assert_eq!(
            list_room_types(&conn).unwrap()[0].room_count,
            0,
            "逻辑删除的房间不该再算进房型间数"
        );
        cleanup(conn, path);
    }

    /// 房间全删光后，该房型应当可以删除（占用检查与展示口径一致）。
    #[test]
    fn deleted_room_frees_room_type() {
        let (conn, path) = temp_db();
        let t = insert_room_type(&conn, "标间", 0).unwrap();
        insert_room(&conn, "702", Some(72), None, None, Some(t.id)).unwrap();
        assert!(delete_room_type(&conn, t.id).is_err(), "在用房型不该能删");

        deactivate_room(&conn, "702").unwrap();
        delete_room_type(&conn, t.id).expect("房间已删，房型应可删");
        cleanup(conn, path);
    }

    /// 入住记录视图（TD-118）：只有填了身份信息（guest_name 非空且非空白）的发卡才出现。
    #[test]
    fn guest_only_excludes_blank_names() {
        let (conn, path) = temp_db();
        let insert = |name: Option<&str>| {
            conn.execute(
                "INSERT INTO issuance (uuid, room_no, kind, days, hours, card_no, result, expires_at, guest_name)
                 VALUES (?1, '803', '日租客卡', 1, 0, 1, 'ok',
                         strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 day'), ?2)",
                params![uuid::Uuid::new_v4().to_string(), name],
            )
            .unwrap();
        };
        insert(Some("张三"));
        insert(None);
        insert(Some("   ")); // 空白姓名也不算入住信息

        let all = list_issuances(&conn, &IssuanceFilter::default()).unwrap();
        assert_eq!(all.total, 3, "不过滤时三条都在");

        let guests = list_issuances(
            &conn,
            &IssuanceFilter {
                guest_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(guests.total, 1, "未输入身份信息的两条不进入入住记录");
        assert_eq!(guests.rows[0].guest_name.as_deref(), Some("张三"));

        cleanup(conn, path);
    }

    /// 记录页的筛选与分页：条数要与直查一致，超上限的 page_size 要被夹住。
    /// TD-229：发卡记录的状态由 SQL 统一算，总控卡过期也必须判成 expired。
    #[test]
    fn list_issuances_status_covers_all_kinds() {
        let (conn, path) = temp_db();
        insert_room(&conn, "901", None, None, None, None).unwrap();

        // 1) 有效：未来到期
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, result, expires_at)
             VALUES (?1, '901', '总控卡', 30, 0, 'ok', '2999-01-01T00:00:00Z')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        // 2) 已过期：**总控卡**（bug 里就是这种卡被显示成「有效」）
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, result, expires_at)
             VALUES (?1, '901', '总控卡', 30, 0, 'ok', '2000-01-01T00:00:00Z')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        // 3) 已过期客卡
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, result, expires_at)
             VALUES (?1, '901', '日租客卡', 1, 0, 'ok', '2000-01-01T00:00:00Z')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        // 4) 已退房
        conn.execute(
            "INSERT INTO issuance
                 (uuid, room_no, kind, days, hours, result, expires_at, checked_out_at)
             VALUES (?1, '901', '日租客卡', 1, 0, 'ok', '2999-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        // 5) 已作废（挂失卡让旧卡失效）
        conn.execute(
            "INSERT INTO issuance
                 (uuid, room_no, kind, days, hours, result, expires_at, invalidated_at)
             VALUES (?1, '901', '日租客卡', 1, 0, 'ok', '2999-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        // 6) 失败
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, result)
             VALUES (?1, '901', '日租客卡', 1, 0, 'failed')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        // 7) 安装卡：无到期，永远 active（不参与过期判断）
        conn.execute(
            "INSERT INTO issuance (uuid, room_no, kind, days, hours, result)
             VALUES (?1, '901', '安装卡', 0, 0, 'ok')",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();

        let page = list_issuances(
            &conn,
            &IssuanceFilter {
                room_no: Some("901".into()),
                page: 1,
                page_size: 20,
                ..Default::default()
            },
        )
        .unwrap();

        let status_of = |kind: &str, expired: bool| -> Vec<String> {
            page.rows
                .iter()
                .filter(|r| r.kind == kind && (r.status == "expired") == expired)
                .map(|r| r.status.clone())
                .collect()
        };

        // 总控卡：一张 active、一张 expired —— 这正是 bug 的核心
        let mut controller: Vec<_> = page
            .rows
            .iter()
            .filter(|r| r.kind == "总控卡")
            .map(|r| r.status.as_str())
            .collect();
        controller.sort_unstable();
        assert_eq!(controller, vec!["active", "expired"], "总控卡过期必须判成 expired");

        assert_eq!(status_of("日租客卡", true), vec!["expired"], "过期客卡");
        assert!(
            page.rows.iter().any(|r| r.kind == "安装卡" && r.status == "active"),
            "安装卡无到期应保持 active"
        );
        for want in ["checked_out", "voided", "failed"] {
            assert!(
                page.rows.iter().any(|r| r.status == want),
                "应存在状态 {want}"
            );
        }

        cleanup(conn, path);
    }

    #[test]
    fn list_issuances_filters_and_paginates() {
        let (conn, path) = temp_db();
        insert_room(&conn, "801", Some(80), None, None, None).unwrap();
        insert_room(&conn, "802", Some(81), None, None, None).unwrap();
        for i in 0..5 {
            let room = if i % 2 == 0 { Some("801") } else { Some("802") };
            record_issuance(&conn, &sample_issuance(room, "日租客卡", 1, 0)).unwrap();
        }

        let all = list_issuances(&conn, &IssuanceFilter::default()).unwrap();
        assert_eq!(all.total, 5);
        assert_eq!(all.rows.len(), 5);

        // 带时间范围的查询会 JOIN operator；created_at 必须用 i. 限定。
        let ranged = list_issuances(
            &conn,
            &IssuanceFilter {
                from: Some("1970-01-01T00:00:00Z".into()),
                to: Some("2999-01-01T00:00:00Z".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ranged.total, 5);

        let only = list_issuances(
            &conn,
            &IssuanceFilter {
                room_no: Some("801".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(only.total, 3, "801 房应有 3 条");

        let paged = list_issuances(
            &conn,
            &IssuanceFilter {
                page: 2,
                page_size: 2,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(paged.total, 5);
        assert_eq!(paged.rows.len(), 2);

        cleanup(conn, path);
    }

    /// 审计列表能查到刚写的动作，且带操作员显示名。
    #[test]
    fn list_audits_finds_recent_action() {
        let (conn, path) = temp_db();
        let op = insert_operator(&conn, "u1", "值班", "frontdesk", "hash", None).unwrap();
        record_audit(&conn, "checkout_room", Some("801"), Some("cardsClosed=1"), "ok", Some(op.id))
            .unwrap();

        let page = list_audits(&conn, Some("checkout"), None, None, 1, 20).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].action, "checkout_room");
        assert_eq!(page.rows[0].detail.as_deref(), Some("cardsClosed=1"));
        assert_eq!(page.rows[0].operator_name.as_deref(), Some("值班"));

        cleanup(conn, path);
    }

}
