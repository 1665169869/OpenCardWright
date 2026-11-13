//! 初始房间导入（CSV / ini）。
//!
//! 现场第一次装完是空库，房号得一次性导进来。数据源两种：
//! * CSV：`房号,序号,房型,楼层`（序号/房型/楼层可空；Excel 另存为 CSV 也能用）
//! * ini：`房号=序号`（沿用旧版 `RoomIndex.ini` 的格式，仅作导入模板）
//!
//! 运行期**不读任何 ini**：房号 ↔ 序号的对应关系只存在 SQLite 的 `room` 表里；
//! 门锁认的是**卡内序号**（安装卡写进去的那个），不是房号文本。
//!
//! 两条工程约束：
//! * 不引 csv crate：字段就四列，手写状态机处理引号与 `""` 转义足够；
//! * 解码走本文件的 [`decode`]（UTF-8 优先、失败回落 GBK）—— Excel 默认存 GBK，
//!   这一步漏了中文房型就会变乱码。这跟设备层无关，别再往下沉。
//!
//! 落库统一走 `insert_room_reporting`，所以「序号命中已删除的房间」会按既有规则**复活**它。

use ocw_store::DbConnection as Connection;

/// 把导入文件解码成字符串。
///
/// Excel「另存为 CSV」在中文 Windows 上写出的是 GBK，不是 UTF-8。
/// 直接按 UTF-8 解会整体失败（`from_utf8` 一遇非法字节就报错），现场表现是
/// 「导入成功 0 行」或房型乱码。所以先试 UTF-8，失败再按 GBK 解。
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: i64,
    pub revived: i64,
    pub skipped: i64,
    pub failed: Vec<FailedRow>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedRow {
    pub line: usize,
    pub reason: String,
}

struct Row {
    line: usize,
    room_no: String,
    index: Option<u16>,
    room_type: Option<String>,
    floor: Option<i64>,
}

/// 按文件名后缀猜格式（`.ini` 走 ini，其余按 CSV）。
pub fn import_bytes(conn: &Connection, file_name: &str, bytes: &[u8]) -> Result<ImportReport, String> {
    let text = decode(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let rows = if file_name.to_ascii_lowercase().ends_with(".ini") {
        parse_ini(text)
    } else {
        parse_csv(text)
    };
    Ok(apply(conn, rows))
}

/// 手写 CSV：支持 `"..."` 与 `""` 转义、CRLF/LF、跳过空行与注释行。
fn parse_csv(text: &str) -> Vec<Row> {
    let mut out = Vec::new();
    let mut header_seen = false;
    for (i, raw_line) in text.lines().enumerate() {
        let line_no = i + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let cells = split_csv_line(line);
        let first = cells.first().map(|s| s.trim().to_string()).unwrap_or_default();
        if first.is_empty() {
            continue;
        }
        // 首个非空行像表头就跳过（"房号" / "room_no"）
        if !header_seen {
            header_seen = true;
            let lower = first.to_ascii_lowercase();
            if first == "房号" || lower == "room_no" || lower == "roomno" || lower == "room" {
                continue;
            }
        }
        let index = cells
            .get(1)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<u16>().ok());
        let room_type = cells
            .get(2)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let floor = cells.get(3).and_then(|s| parse_floor(s.trim()));
        out.push(Row {
            line: line_no,
            room_no: first,
            index,
            room_type,
            floor,
        });
    }
    out
}

/// ini：`房号=序号`，`;`/`#` 注释，`[section]` 忽略。
fn parse_ini(text: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((left, right)) = line.split_once('=') else {
            continue;
        };
        let room_no = left.trim();
        if room_no.is_empty() {
            continue;
        }
        out.push(Row {
            line: i + 1,
            room_no: room_no.to_string(),
            index: right.trim().parse::<u16>().ok(),
            room_type: None,
            floor: None,
        });
    }
    out
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                cells.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    cells.push(cur);
    cells
}

/// 楼层：先取前导数字（"12楼" → 12），再认「一…十」这种中文数字。
fn parse_floor(text: &str) -> Option<i64> {
    if text.is_empty() {
        return None;
    }
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        return digits.parse().ok();
    }
    match text.chars().next() {
        Some('一') => Some(1),
        Some('二') => Some(2),
        Some('三') => Some(3),
        Some('四') => Some(4),
        Some('五') => Some(5),
        Some('六') => Some(6),
        Some('七') => Some(7),
        Some('八') => Some(8),
        Some('九') => Some(9),
        Some('十') => Some(10),
        _ => None,
    }
}

/// 房型名 → id，没有就建（与 demo_import 的规则一致，避免出现两套房型）。
fn ensure_room_type(conn: &Connection, name: &str) -> Result<i64, String> {
    let existing = ocw_store::list_room_types(conn).map_err(|e| e.to_string())?;
    if let Some(t) = existing.iter().find(|t| t.name == name) {
        return Ok(t.id);
    }
    let next = existing.len() as i64;
    Ok(ocw_store::insert_room_type(conn, name, next)
        .map_err(|e| e.to_string())?
        .id)
}

fn apply(conn: &Connection, rows: Vec<Row>) -> ImportReport {
    let mut report = ImportReport::default();
    for row in rows {
        // 序号超出 12 位可分配范围（0–4094）：记为坏行，不让整批导入失败。
        // 4095 与总控卡页1 字节相同，分给房间会让该房客卡被读成总控卡。
        if let Some(index) = row.index {
            if index > ocw_store::MAX_ROOM_INDEX {
                report.failed.push(FailedRow {
                    line: row.line,
                    reason: format!(
                        "序号 {index} 超出范围（0–{}）；4095 与总控卡字节相同，不能分配给房间",
                        ocw_store::MAX_ROOM_INDEX
                    ),
                });
                continue;
            }
        }
        let room_type_id = match row.room_type.as_deref() {
            Some(name) => match ensure_room_type(conn, name) {
                Ok(id) => Some(id),
                Err(e) => {
                    report.failed.push(FailedRow {
                        line: row.line,
                        reason: format!("建房型「{name}」失败：{e}"),
                    });
                    continue;
                }
            },
            None => None,
        };

        match ocw_store::insert_room_reporting(
            conn,
            &row.room_no,
            row.index,
            None,
            row.floor,
            room_type_id,
        ) {
            Ok(upsert) => {
                if upsert.revived_from.is_some() {
                    report.revived += 1;
                } else {
                    report.added += 1;
                }
            }
            Err(ocw_store::StoreError::Invalid(msg)) if msg.contains("房号或序号已存在") || msg.contains("已被") => {
                report.skipped += 1;
            }
            Err(e) => report.failed.push(FailedRow {
                line: row.line,
                reason: e.to_string(),
            }),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (Connection, std::path::PathBuf) {
        // 只按纳秒命名在并行跑时会撞名（Windows 计时器精度 100ns），
        // 两个用例共用同一个库 → 「库里多出别人插的房间」。加进程内自增序号。
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ocw-import-{}-{seq}.db",
            std::process::id()
        ));
        let conn = ocw_store::open(&path).unwrap();
        (conn, path)
    }

    #[test]
    fn csv_with_header_and_quotes() {
        let (conn, path) = temp_db();
        let csv = "房号,序号,房型,楼层\r\n201,0,\"豪华,双人房\",二楼\n202,,标间,3\r\n";
        let r = import_bytes(&conn, "rooms.csv", csv.as_bytes()).unwrap();
        assert_eq!(r.added, 2);
        assert!(r.failed.is_empty());
        assert_eq!(ocw_store::list_rooms(&conn).unwrap().len(), 2);
        let rooms = ocw_store::list_rooms(&conn).unwrap();
        let r201 = rooms.iter().find(|x| x.room_no == "201").unwrap();
        assert_eq!(r201.room_index, 0);
        assert_eq!(r201.floor, Some(2), "「二楼」应解析成 2");
        assert_eq!(r201.room_type_name.as_deref(), Some("豪华,双人房"));
        let _ = std::fs::remove_file(path);
        drop(conn);
    }

    #[test]
    fn duplicate_import_is_idempotent() {
        let (conn, path) = temp_db();
        let csv = "房号,序号\n301,1\n302,2\n";
        assert_eq!(import_bytes(&conn, "r.csv", csv.as_bytes()).unwrap().added, 2);
        let again = import_bytes(&conn, "r.csv", csv.as_bytes()).unwrap();
        assert_eq!(again.added, 0);
        assert_eq!(again.skipped, 2, "重复导入应跳过而不是报错");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bad_rows_are_collected_not_fatal() {
        let (conn, path) = temp_db();
        // 第二行序号与第一行冲突（同一序号不能两间房）
        let csv = "房号,序号\n401,5\n402,5\n403,6\n";
        let r = import_bytes(&conn, "r.csv", csv.as_bytes()).unwrap();
        assert_eq!(r.added, 2);
        assert_eq!(r.skipped + r.failed.len() as i64, 1);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ini_room_index_map() {
        let (conn, path) = temp_db();
        let ini = "; 房号=序号\n[Rooms]\n201=0\n253=1\n";
        let r = import_bytes(&conn, "RoomIndex.ini", ini.as_bytes()).unwrap();
        assert_eq!(r.added, 2);
        let rooms = ocw_store::list_rooms(&conn).unwrap();
        assert_eq!(
            rooms.iter().find(|x| x.room_no == "253").unwrap().room_index,
            1
        );
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// 序号 4094 可以导入；4095（与总控卡字节相同）与更大的值必须记为坏行，
    /// 而不是被截断后悄悄落库。
    #[test]
    fn out_of_range_index_is_a_failed_row() {
        let (conn, path) = temp_db();
        let csv = "房号,序号\n801,4094\n802,4095\n803,4096\n";
        let r = import_bytes(&conn, "r.csv", csv.as_bytes()).unwrap();

        assert_eq!(r.added, 1, "只有 4094 能进来：{r:?}");
        assert_eq!(r.failed.len(), 2, "4095 / 4096 都算坏行：{r:?}");
        assert!(
            r.failed.iter().any(|f| f.reason.contains("4095")),
            "错误文案要点明 4095：{r:?}"
        );
        assert_eq!(
            ocw_store::find_room(&conn, "801").unwrap().unwrap().room_index,
            4094
        );
        assert!(ocw_store::find_room(&conn, "802").unwrap().is_none());
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn revives_deleted_index() {
        let (conn, path) = temp_db();
        ocw_store::insert_room(&conn, "501", Some(50), None, None, None).unwrap();
        ocw_store::deactivate_room(&conn, "501").unwrap();

        let r = import_bytes(&conn, "r.csv", "房号,序号\n502,50\n".as_bytes()).unwrap();
        assert_eq!(r.revived, 1, "同序号已删房应复活而不是新建");
        assert!(ocw_store::find_room(&conn, "502").unwrap().is_some());
        drop(conn);
        let _ = std::fs::remove_file(path);
    }
}
