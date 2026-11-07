//! **仅调试用**：从仓库里的 `data/NacreHis.json` 导入客房房号表 `Kf_fh`。
//!
//! 正式使用时不走这条路（房间应从安装卡/现场导入）。这里只是让调试环境一启动就有
//! 真实的房号、房型、楼层可点，省得手搓 30 个房间。
//!
//! 只在 dev 构建里编译（`#[cfg(debug_assertions)]`），发布版不含这段代码。

use ocw_store::DbConnection;
use serde_json::Value;
use std::path::Path;

/// "二楼"/"12楼" → 2/12；认不出就返回 None（楼层是可选字段）。
fn parse_floor(text: &str) -> Option<i64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        return digits.parse().ok();
    }
    // 中文数字只处理到 十（现场楼层不会有几十层）
    let mut value = 0i64;
    for ch in t.chars() {
        let d = match ch {
            '一' => 1,
            '二' | '两' => 2,
            '三' => 3,
            '四' => 4,
            '五' => 5,
            '六' => 6,
            '七' => 7,
            '八' => 8,
            '九' => 9,
            '十' => 10,
            _ => break,
        };
        value = if ch == '十' && value == 0 { 10 } else { value + d };
    }
    (value > 0).then_some(value)
}

fn ensure_room_type(conn: &DbConnection, name: &str) -> Option<i64> {
    if let Ok(id) = conn.query_row("SELECT id FROM room_type WHERE name = ?1", [name], |r| {
        r.get::<_, i64>(0)
    }) {
        return Some(id);
    }
    match ocw_store::insert_room_type(conn, name, 0) {
        Ok(t) => Some(t.id),
        Err(_) => conn
            .query_row("SELECT id FROM room_type WHERE name = ?1", [name], |r| {
                r.get::<_, i64>(0)
            })
            .ok(),
    }
}

fn map_status(conn: &DbConnection, label: &str) -> String {
    conn.query_row("SELECT key FROM room_status WHERE label = ?1", [label], |r| {
        r.get::<_, String>(0)
    })
    .unwrap_or_else(|_| "vacant_clean".to_string())
}

/// 导入 `tables.Kf_fh` 里的房间，返回新增条数。已存在的房号会跳过（可重复执行）。
pub fn import_nacre_rooms(conn: &DbConnection, path: &Path) -> Result<usize, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("读不到 {}：{e}", path.display()))?;
    let root: Value = serde_json::from_str(&text).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let rows = root
        .get("tables")
        .and_then(|t| t.get("Kf_fh"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| "JSON 里没有 tables.Kf_fh".to_string())?;

    let mut added = 0usize;
    for row in rows {
        let room_no = row
            .get("FFhmc")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if room_no.is_empty() {
            continue;
        }
        // 幂等：已有同房号就跳过
        if matches!(ocw_store::find_room(conn, &room_no), Ok(Some(_))) {
            continue;
        }

        // FFhbh 是"房号编号"（000、001…），正好对应门锁里的序号
        let index = row
            .get("FFhbh")
            .and_then(|v| v.as_str())
            .and_then(|s| s.trim().parse::<u8>().ok())
            // 序号已被占用就让 insert_room 自动分配下一个
            .filter(|i| matches!(ocw_store::find_room_by_index(conn, *i), Ok(None)));

        let building = row.get("FDhmc").and_then(|v| v.as_str());
        let floor = row.get("FLhmc").and_then(|v| v.as_str()).and_then(parse_floor);
        let type_name = row
            .get("FType")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let type_id = type_name.and_then(|n| ensure_room_type(conn, n));
        let status = row
            .get("FFtmc")
            .or_else(|| row.get("FFkzt"))
            .and_then(|v| v.as_str())
            .map(|label| map_status(conn, label))
            .unwrap_or_else(|| "vacant_clean".to_string());

        match ocw_store::insert_room(conn, &room_no, index, building, floor, type_id) {
            Ok(room) => {
                let _ = ocw_store::set_room_status(conn, &room.room_no, &status);
                added += 1;
            }
            Err(e) => eprintln!("[dev] 跳过房间 {room_no}：{e}"),
        }
    }
    Ok(added)
}
