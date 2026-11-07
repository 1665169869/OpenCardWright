-- OpenCardWright schema v1
--
-- 两条设计约束写进了 schema：
--   * room.room_index UNIQUE —— 序号一旦被某把锁绑定就永不复用，删房只能逻辑删除；
--   * issuance 只追加 —— 发卡流水是账，不允许 UPDATE/DELETE（应用层不做，这里用注释约束）。

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS room (
    room_no         TEXT PRIMARY KEY,
    room_index      INTEGER NOT NULL UNIQUE CHECK (room_index >= 0 AND room_index <= 255),
    building        TEXT,
    floor           INTEGER,
    room_type       TEXT,
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    install_card_at TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_room_index ON room (room_index);

CREATE TABLE IF NOT EXISTS operator (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    login        TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    role         TEXT NOT NULL CHECK (role IN ('admin', 'frontdesk')),
    pwd_hash     TEXT NOT NULL,
    active       INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS issuance (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid            TEXT NOT NULL UNIQUE,
    room_no         TEXT,
    kind            TEXT NOT NULL,
    days            INTEGER NOT NULL DEFAULT 0,
    hours           INTEGER NOT NULL DEFAULT 0,
    card_no         INTEGER CHECK (card_no IS NULL OR (card_no >= 0 AND card_no <= 65535)),
    raw_hex         TEXT,
    operator_id     INTEGER REFERENCES operator (id),
    reader_id       TEXT,
    result          TEXT NOT NULL,
    error_code      INTEGER,
    idempotency_key TEXT UNIQUE,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    duration_ms     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_issuance_card_no ON issuance (card_no);
CREATE INDEX IF NOT EXISTS idx_issuance_created ON issuance (created_at DESC);
-- 记录页的筛选常用列（v7 内追加即可：迁移每次启动都会执行本文件，索引对读方透明）
CREATE INDEX IF NOT EXISTS idx_issuance_room ON issuance (room_no);
CREATE INDEX IF NOT EXISTS idx_issuance_kind ON issuance (kind);

CREATE TABLE IF NOT EXISTS audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    operator_id INTEGER REFERENCES operator (id),
    action      TEXT NOT NULL,
    target      TEXT,
    detail_json TEXT,
    result      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_at ON audit_log (at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log (action);

CREATE TABLE IF NOT EXISTS device (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    reader_id     TEXT,
    port          INTEGER,
    driver_sha256 TEXT,
    first_seen    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_seen     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS setting (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- ---------------- v2 ----------------
-- 部门：仅作为用户归属标签，不做房间数据隔离。
CREATE TABLE IF NOT EXISTS department (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    note       TEXT,
    active     INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- 房型：房间分类，纯展示/筛选，不绑定价、不影响发卡时长。
CREATE TABLE IF NOT EXISTS room_type (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- 房态：7 种固定用途，颜色/图标可在设置里改（存 room_status 表）
CREATE TABLE IF NOT EXISTS room_status (
    key        TEXT PRIMARY KEY,
    label      TEXT NOT NULL,
    icon       TEXT NOT NULL,
    color      TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO room_status (key, label, icon, color, sort_order) VALUES
    ('vacant_clean',   '空净房', 'CheckCircle',       '#52c41a', 10),
    ('vacant_dirty',   '空脏房', 'ClockCircle',       '#faad14', 20),
    ('reserved',       '预定房', 'Calendar',          '#13c2c2', 30),
    ('occupied_partial','未满房', 'User',             '#1677ff', 40),
    ('occupied_full',  '已满房', 'Lock',              '#722ed1', 50),
    ('maintenance',    '维修房', 'Tool',              '#ff4d4f', 60),
    ('other',          '其他房', 'QuestionCircle',    '#8c8c8c', 70);

-- 列级迁移（operator.department_id / room.room_type_id / issuance.expires_at）
-- 由 lib.rs 的 apply_v2() 用 PRAGMA table_info 探测后执行，这里不写 ALTER。

-- ---------------- v8 ----------------
-- 预定：入住—退房日期区间（**退房日不含**，2026-11-06 → 2026-11-07 表示住 1 晚）。
-- 与当前房态解耦：未来预定不改房态；只有区间覆盖今天时才把空房标成「预定房」。
-- 同一房间的有效预定不允许日期重叠；取消保留记录（status='cancelled' + cancelled_at）。
CREATE TABLE IF NOT EXISTS reservation (
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

CREATE INDEX IF NOT EXISTS idx_reservation_room_dates ON reservation (room_no, check_in, check_out);
CREATE INDEX IF NOT EXISTS idx_reservation_dates ON reservation (check_in, check_out);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
