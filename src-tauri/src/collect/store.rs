//! 관제 PC DB(받은 쪽)와 보낼 함 DB(보내는 쪽). 둘 다 SQLite WAL, 값은 매개변수로만 넣는다.
//!
//! 조회 함수는 읽기만 한다. 쓰는 것은 받기(`take`), 짝 관리, 주기 작업(`purge`, `mark_listening`)뿐이다.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::wire::{self, MAX_ITEM_BYTES};

const HOUR: u64 = 3_600_000;
pub const DAY: u64 = 24 * HOUR;

/// 관제 PC의 보관 기한 기본값(받은 시각 기준)
pub const KEEP_DAYS_DEFAULT: u32 = 90;
pub const KEEP_DAYS_MAX: u32 = 730;
/// 한 PC가 한 시간에 보낼 수 있는 건수와 바이트
pub const HOUR_MAX_ITEMS: u64 = 600;
pub const HOUR_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// 한 PC가 관제 PC에 차지할 수 있는 건수와 바이트
pub const PC_MAX_ITEMS: u64 = 20_000;
pub const PC_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// 하루 안에 이만큼 상한을 넘으면 그 PC의 짝을 멈춘다. 사람이 다시 켜야 받는다.
pub const STRIKES_TO_STOP: u64 = 3;
/// 진단 시각이 받은 시각보다 이만큼 넘게 앞서면 받은 시각으로 바꾸고 표시한다.
pub const FUTURE_SLACK: u64 = 5 * 60_000;
/// 이만큼 보고(살아 있음 알림 포함)가 없으면 "미확인"
pub const QUIET_AFTER: u64 = 15 * 60_000;
/// 듣고 있다는 표시가 이만큼 끊겼으면 "받지 않던 구간"으로 적는다.
const DOWN_AFTER: u64 = 3 * 60_000;

/// 보낼 함의 한도
pub const OUT_MAX_QUEUE: u64 = 500;
pub const OUT_KEEP: u64 = 14 * DAY;
const OUT_LOG_KEEP: u64 = 500;

fn open(path: &Path, schema: &str) -> Result<Connection, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("폴더를 만들지 못했습니다: {e}"))?;
    }
    if !path.exists() {
        // 그 사용자만 열 수 있는 빈 파일을 먼저 만든다.
        super::key::write_private(path, b"")?;
    }
    let db = Connection::open(path).map_err(|e| format!("DB를 열지 못했습니다: {e}"))?;
    db.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    db.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(3)).map_err(|e| e.to_string())?;
    db.execute_batch(schema).map_err(|e| format!("DB를 준비하지 못했습니다: {e}"))?;
    Ok(db)
}

fn e(err: rusqlite::Error) -> String {
    format!("DB 오류: {err}")
}

fn hash(line: &str) -> String {
    Sha256::digest(line.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

// ================= 관제 PC =================

const INBOX_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS pcs(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, addr TEXT NOT NULL, state TEXT NOT NULL, own INTEGER NOT NULL DEFAULT 0,
  paired INTEGER NOT NULL, last_seq INTEGER NOT NULL DEFAULT 0, seen INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS items(
  pc TEXT NOT NULL, seq INTEGER NOT NULL, at INTEGER NOT NULL, sent_at INTEGER NOT NULL, got INTEGER NOT NULL,
  kind TEXT NOT NULL, title TEXT NOT NULL, target TEXT NOT NULL, verdict TEXT NOT NULL,
  skew INTEGER NOT NULL, stale INTEGER NOT NULL, masked INTEGER NOT NULL, size INTEGER NOT NULL, hash TEXT NOT NULL, body TEXT NOT NULL,
  PRIMARY KEY(pc, seq));
CREATE INDEX IF NOT EXISTS items_at ON items(at);
CREATE INDEX IF NOT EXISTS items_got ON items(got);
CREATE INDEX IF NOT EXISTS items_target ON items(target);
CREATE TABLE IF NOT EXISTS gaps(pc TEXT NOT NULL, first INTEGER NOT NULL, last INTEGER NOT NULL, got INTEGER NOT NULL, dropped INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS alerts(id INTEGER PRIMARY KEY AUTOINCREMENT, pc TEXT NOT NULL, at INTEGER NOT NULL, kind TEXT NOT NULL, strike INTEGER NOT NULL, note TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS downtime(first INTEGER NOT NULL, last INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sum_target(
  target TEXT NOT NULL, pc TEXT NOT NULL, n INTEGER NOT NULL, ok INTEGER NOT NULL, caution INTEGER NOT NULL, danger INTEGER NOT NULL,
  first_at INTEGER NOT NULL, last_at INTEGER NOT NULL, last_verdict TEXT NOT NULL, PRIMARY KEY(target, pc));
CREATE TABLE IF NOT EXISTS conns(
  id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, pc TEXT NOT NULL, addr TEXT NOT NULL, outcome TEXT NOT NULL, ms INTEGER NOT NULL,
  stored INTEGER NOT NULL, again INTEGER NOT NULL, refused INTEGER NOT NULL, bytes INTEGER NOT NULL, first_seq INTEGER NOT NULL, last_seq INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS conns_at ON conns(at);
CREATE INDEX IF NOT EXISTS conns_pc ON conns(pc, id);
CREATE TABLE IF NOT EXISTS plans(pc TEXT NOT NULL, what TEXT NOT NULL, every INTEGER NOT NULL, PRIMARY KEY(pc, what));
CREATE TABLE IF NOT EXISTS plan_now(pc TEXT NOT NULL, what TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(pc, what));
CREATE TABLE IF NOT EXISTS plan_state(pc TEXT PRIMARY KEY, rev INTEGER NOT NULL DEFAULT 0, asked INTEGER NOT NULL DEFAULT 0, have INTEGER NOT NULL DEFAULT 0, approved TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS sum_hour(hour INTEGER PRIMARY KEY, ok INTEGER NOT NULL, caution INTEGER NOT NULL, danger INTEGER NOT NULL, unknown INTEGER NOT NULL);
";

/// 받기를 거절한 까닭
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// 짝이 아니다
    Unknown,
    /// 짝이 멈춰 있다
    Stopped,
    /// 이미 지나간 순번(지금은 갖고 있지 않다)
    Old,
    /// 같은 순번에 다른 내용
    Conflict,
    TooBig,
    TooFast,
    Full,
    /// 모양이 아니다
    Shape,
}

impl Why {
    pub fn code(self) -> &'static str {
        match self {
            Why::Unknown => "unknown",
            Why::Stopped => "stopped",
            Why::Old => "old",
            Why::Conflict => "conflict",
            Why::TooBig => "too_big",
            Why::TooFast => "too_fast",
            Why::Full => "full",
            Why::Shape => "shape",
        }
    }

    /// 상한을 넘은 것(되풀이되면 짝을 멈춘다)인지
    fn strike(self) -> bool {
        matches!(self, Why::TooBig | Why::TooFast | Why::Full | Why::Shape | Why::Conflict)
    }

    fn note(self) -> &'static str {
        match self {
            Why::Unknown => "짝이 아닌 PC",
            Why::Stopped => "멈춘 짝",
            Why::Old => "지나간 순번",
            Why::Conflict => "같은 순번에 다른 내용이 왔습니다(저장하지 않음)",
            Why::TooBig => "한 건의 크기 상한을 넘었습니다",
            Why::TooFast => "한 시간에 보낼 수 있는 양을 넘었습니다",
            Why::Full => "이 PC가 쓸 수 있는 저장 공간을 넘었습니다",
            Why::Shape => "약속한 모양이 아닌 자료가 왔습니다",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    Stored,
    /// 이미 같은 내용으로 갖고 있다(받았다는 답을 못 받고 다시 보낸 것)
    Again,
    Refused(Why),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Pc {
    pub id: String,
    pub name: String,
    pub addr: String,
    /// active | stopped | removed
    pub state: String,
    /// 이 PC 자신
    pub own: bool,
    pub paired: u64,
    pub last_seq: u64,
    pub seen: u64,
    /// 일정 시간 보고가 없다("미확인" — 이상으로 단정하지 않는다)
    pub quiet: bool,
    pub items: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Filter {
    #[serde(default)]
    pub pc: String,
    #[serde(default)]
    pub from: u64,
    #[serde(default)]
    pub to: u64,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub offset: u32,
}

/// 목록 한 번에 돌려주는 줄 수
pub const PAGE: u32 = 200;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Row {
    pub pc: String,
    pub pc_name: String,
    pub seq: u64,
    pub at: u64,
    pub got: u64,
    pub kind: String,
    pub title: String,
    pub target: String,
    pub verdict: String,
    pub skew: bool,
    pub stale: bool,
    pub masked: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Gap {
    pub pc: String,
    pub first: u64,
    pub last: u64,
    pub got: u64,
    /// 보낸 쪽이 "보낼 함이 넘쳐 버렸다"고 알린 구간인지
    pub dropped: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Alert {
    pub pc: String,
    pub at: u64,
    pub kind: String,
    pub note: String,
}

/// 종합 분석: 한 대상을 한 PC가 본 것
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Seen {
    pub pc: String,
    pub pc_name: String,
    pub n: u64,
    pub ok: u64,
    pub caution: u64,
    pub danger: u64,
    pub first_at: u64,
    pub last_at: u64,
    pub last_verdict: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TargetView {
    pub target: String,
    pub total: u64,
    pub seen: Vec<Seen>,
    /// 한 PC에서만 나온 대상(다른 지점이 확인해 준 적이 없다)
    pub single: bool,
    /// PC마다 마지막 판정이 다르다
    pub differs: bool,
    /// 보낸 PC가 가린 내부 주소(가명). 그 PC 안에서만 같은 대상이다.
    pub alias: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Analysis {
    /// 요약 표를 만든 시각(0이면 아직 만들지 않았다)
    pub made: u64,
    /// 여러 PC가 본 대상을 앞에, 그다음 건수가 많은 순서로
    pub targets: Vec<TargetView>,
    /// (1970년부터의 시간 번호, 정상, 주의, 위험, 판정 없음) — 최근 30일
    pub hours: Vec<(u64, u64, u64, u64, u64)>,
}

/// 종합 분석에 보이는 대상 수
/// 접속 내역에 남기는 줄 수의 상한
pub const CONN_KEEP: i64 = 5_000;
/// 짝이 아닌 접속은 1분에 이만큼만 적는다(두드리기로 내역이 밀려나지 않게).
pub const CONN_STRANGER_PER_MIN: i64 = 60;
/// 거점 상세의 시간표 칸 수(7일, 1시간 칸)
pub const TIMELINE_HOURS: usize = 168;

/// 접속 한 번의 기록. 글자는 정해진 낱말과 숫자뿐이다(상대가 보낸 글자를 싣지 않는다).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Conn {
    pub at: u64,
    /// 상대 공개 키의 지문. 인증 전에 끊긴 접속은 빈 글자.
    pub pc: String,
    pub addr: String,
    /// ok, closed, pair_ok, pair_no, unknown, stopped, refused, strange, version, tls, busy, outside
    pub outcome: String,
    pub ms: u64,
    pub stored: u64,
    pub again: u64,
    pub refused: u64,
    pub bytes: u64,
    pub first_seq: u64,
    pub last_seq: u64,
}

/// 전송 한 건(본문 없이)
#[derive(Debug, Clone, Serialize)]
pub struct Transfer {
    pub seq: u64,
    pub at: u64,
    pub got: u64,
    pub kind: String,
    pub title: String,
    pub verdict: String,
    pub size: u64,
    pub skew: i64,
    pub stale: bool,
}

/// 한 시간 칸: 받은 건수, 그중 위험, 접속 수, 실패한 접속 수
#[derive(Debug, Clone, Default, Serialize)]
pub struct Hour {
    pub at: u64,
    pub items: u64,
    pub danger: u64,
    pub conns: u64,
    pub failed: u64,
}

pub const ANALYSIS_TARGETS: usize = 100;
/// 요약 표를 다시 만드는 간격
pub const SUMMARY_EVERY: u64 = 5 * 60_000;

pub struct Inbox {
    db: Mutex<Connection>,
}

impl Inbox {
    pub fn open(path: &Path) -> Result<Inbox, String> {
        Ok(Inbox { db: Mutex::new(open(path, INBOX_SCHEMA)?) })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 짝을 맺는다(또는 끊었던 짝을 되살린다). 순번은 이어 간다.
    pub fn pair(&self, id: &str, name: &str, addr: &str, own: bool, now: u64) -> Result<(), String> {
        let name = wire::text(name, 40);
        self.db()
            .execute(
                "INSERT INTO pcs(id, name, addr, state, own, paired, seen) VALUES(?1, ?2, ?3, 'active', ?4, ?5, ?5)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, addr = ?3, state = 'active', paired = ?5",
                params![id, name, wire::text(addr, 45), own, now as i64],
            )
            .map(|_| ())
            .map_err(e)
    }

    /// active | stopped | removed. 모르는 PC면 거짓.
    pub fn set_state(&self, id: &str, state: &str) -> Result<bool, String> {
        if !matches!(state, "active" | "stopped" | "removed") {
            return Err("모르는 상태입니다".into());
        }
        self.db().execute("UPDATE pcs SET state = ?2 WHERE id = ?1 AND own = 0", params![id, state]).map(|n| n > 0).map_err(e)
    }

    /// 받아도 되는 짝인지: Ok(()) 또는 거절 까닭
    pub fn admitted(&self, id: &str) -> Result<(), Why> {
        let state: Option<String> = self.db().query_row("SELECT state FROM pcs WHERE id = ?1", params![id], |r| r.get(0)).optional().unwrap_or(None);
        match state.as_deref() {
            Some("active") => Ok(()),
            Some("stopped") => Err(Why::Stopped),
            _ => Err(Why::Unknown),
        }
    }

    /// 살아 있음 알림을 받았다.
    pub fn seen(&self, id: &str, addr: &str, now: u64) {
        let _ = self.db().execute("UPDATE pcs SET seen = ?2, addr = ?3 WHERE id = ?1", params![id, now as i64, wire::text(addr, 45)]);
    }

    /// 보낸 쪽이 "보낼 함이 넘쳐 first~last를 버렸다"고 알렸다.
    pub fn dropped(&self, id: &str, first: u64, last: u64, now: u64) {
        if first == 0 || last < first {
            return;
        }
        let db = self.db();
        let known: i64 = db.query_row("SELECT COUNT(*) FROM gaps WHERE pc = ?1 AND first = ?2 AND last = ?3", params![id, first as i64, last as i64], |r| r.get(0)).unwrap_or(0);
        if known == 0 {
            let _ = db.execute("INSERT INTO gaps(pc, first, last, got, dropped) VALUES(?1, ?2, ?3, ?4, 1)", params![id, first as i64, last as i64, now as i64]);
        }
    }

    fn refuse(tx: &rusqlite::Transaction<'_>, pc: &str, why: Why, now: u64) -> Result<Taken, rusqlite::Error> {
        tx.execute("INSERT INTO alerts(pc, at, kind, strike, note) VALUES(?1, ?2, ?3, ?4, ?5)", params![pc, now as i64, why.code(), why.strike(), why.note()])?;
        if why.strike() {
            let strikes: i64 = tx.query_row("SELECT COUNT(*) FROM alerts WHERE pc = ?1 AND strike = 1 AND at > ?2", params![pc, now.saturating_sub(DAY) as i64], |r| r.get(0))?;
            if strikes as u64 >= STRIKES_TO_STOP {
                let stopped = tx.execute("UPDATE pcs SET state = 'stopped' WHERE id = ?1 AND own = 0 AND state = 'active'", params![pc])?;
                if stopped > 0 {
                    tx.execute("INSERT INTO alerts(pc, at, kind, strike, note) VALUES(?1, ?2, 'stopped', 0, '상한을 되풀이해 넘어 이 PC의 짝을 멈췄습니다. 다시 켜야 받습니다')", params![pc, now as i64])?;
                }
            }
        }
        Ok(Taken::Refused(why))
    }

    /// 한 건을 받는다. `line`은 보낸 쪽이 보낸 OCSF 한 줄 그대로다.
    pub fn take(&self, pc: &str, seq: u64, line: &str, now: u64) -> Result<Taken, String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(e)?;
        let taken = Self::take_in(&tx, pc, seq, line, now).map_err(e)?;
        tx.commit().map_err(e)?;
        Ok(taken)
    }

    fn take_in(tx: &rusqlite::Transaction<'_>, pc: &str, seq: u64, line: &str, now: u64) -> Result<Taken, rusqlite::Error> {
        let row: Option<(String, bool, i64)> = tx.query_row("SELECT state, own, last_seq FROM pcs WHERE id = ?1", params![pc], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let Some((state, own, last_seq)) = row else { return Ok(Taken::Refused(Why::Unknown)) };
        if state != "active" {
            return Ok(Taken::Refused(if state == "stopped" { Why::Stopped } else { Why::Unknown }));
        }
        if line.len() > MAX_ITEM_BYTES {
            return Self::refuse(tx, pc, Why::TooBig, now);
        }
        let body: Option<Value> = serde_json::from_str(line).ok();
        let parsed = body.as_ref().and_then(wire::from_ocsf).filter(|_| {
            let b = body.as_ref();
            seq > 0 && seq <= i64::MAX as u64 && b.and_then(|b| b.get("device")).and_then(|d| d.get("uid")).and_then(Value::as_str) == Some(pc) && b.and_then(|b| b.get("metadata")).and_then(|m| m.get("sequence")).and_then(Value::as_u64) == Some(seq)
        });
        let Some(parsed) = parsed else { return Self::refuse(tx, pc, Why::Shape, now) };
        let digest = hash(line);

        if seq <= last_seq as u64 {
            let had: Option<String> = tx.query_row("SELECT hash FROM items WHERE pc = ?1 AND seq = ?2", params![pc, seq as i64], |r| r.get(0)).optional()?;
            return match had {
                Some(h) if h == digest => Ok(Taken::Again),
                Some(_) => Self::refuse(tx, pc, Why::Conflict, now),
                None => Ok(Taken::Refused(Why::Old)),
            };
        }
        if !own {
            let (n, bytes): (i64, i64) = tx.query_row("SELECT COUNT(*), COALESCE(SUM(size), 0) FROM items WHERE pc = ?1 AND got > ?2", params![pc, now.saturating_sub(HOUR) as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if n as u64 >= HOUR_MAX_ITEMS || bytes as u64 + line.len() as u64 > HOUR_MAX_BYTES {
                return Self::refuse(tx, pc, Why::TooFast, now);
            }
        }
        let (n, bytes): (i64, i64) = tx.query_row("SELECT COUNT(*), COALESCE(SUM(size), 0) FROM items WHERE pc = ?1", params![pc], |r| Ok((r.get(0)?, r.get(1)?)))?;
        if n as u64 >= PC_MAX_ITEMS || bytes as u64 + line.len() as u64 > PC_MAX_BYTES {
            return Self::refuse(tx, pc, Why::Full, now);
        }

        if seq > last_seq as u64 + 1 {
            let (first, last) = (last_seq + 1, seq as i64 - 1);
            // 보낸 쪽이 버렸다고 알린 구간과 겹치면 따로 적지 않는다.
            let told: i64 = tx.query_row("SELECT COUNT(*) FROM gaps WHERE pc = ?1 AND dropped = 1 AND first <= ?2 AND last >= ?3", params![pc, first, last], |r| r.get(0))?;
            if told == 0 {
                tx.execute("INSERT INTO gaps(pc, first, last, got, dropped) VALUES(?1, ?2, ?3, ?4, 0)", params![pc, first, last, now as i64])?;
            }
        }
        let skew = parsed.at > now + FUTURE_SLACK;
        let at = if skew { now } else { parsed.at };
        let stale = parsed.at + OUT_KEEP < now;
        let f = &parsed.item;
        tx.execute(
            "INSERT INTO items(pc, seq, at, sent_at, got, kind, title, target, verdict, skew, stale, masked, size, hash, body) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![pc, seq as i64, at as i64, parsed.at.min(i64::MAX as u64) as i64, now as i64, f.kind, f.title, f.target, f.verdict, skew, stale, parsed.masked, line.len() as i64, digest, line],
        )?;
        tx.execute("UPDATE pcs SET last_seq = ?2, seen = ?3 WHERE id = ?1", params![pc, seq as i64, now as i64])?;
        Ok(Taken::Stored)
    }

    /// 듣고 있다는 표시를 남긴다. 표시가 끊겼었다면 그 사이를 "받지 않던 구간"으로 적는다.
    pub fn mark_listening(&self, now: u64) {
        let db = self.db();
        let last: Option<i64> = db.query_row("SELECT v FROM meta WHERE k = 'alive'", [], |r| r.get(0)).optional().unwrap_or(None);
        if let Some(last) = last.filter(|l| now > *l as u64 + DOWN_AFTER) {
            let _ = db.execute("INSERT INTO downtime(first, last) VALUES(?1, ?2)", params![last, now as i64]);
        }
        let _ = db.execute("INSERT INTO meta(k, v) VALUES('alive', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1", params![now as i64]);
    }

    /// 보관 기한(받은 시각 기준)이 지난 것을 지운다. 지운 건수.
    pub fn purge(&self, now: u64, keep_days: u32) -> usize {
        let cut = now.saturating_sub(u64::from(keep_days.clamp(1, KEEP_DAYS_MAX)) * DAY) as i64;
        let db = self.db();
        let n = db.execute("DELETE FROM items WHERE got < ?1", params![cut]).unwrap_or(0);
        let _ = db.execute("DELETE FROM gaps WHERE got < ?1", params![cut]);
        let _ = db.execute("DELETE FROM alerts WHERE at < ?1", params![cut]);
        let _ = db.execute("DELETE FROM downtime WHERE last < ?1", params![cut]);
        let _ = db.execute("DELETE FROM conns WHERE at < ?1", params![cut]);
        n
    }

    /// 주기 작업: 종합 분석이 읽는 요약 표를 다시 만든다. 화면 조회는 이 표만 읽고 DB에 쓰지 않는다.
    pub fn summarize(&self, now: u64) -> Result<(), String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(e)?;
        tx.execute_batch(
            "DELETE FROM sum_target; DELETE FROM sum_hour;
             INSERT INTO sum_target(target, pc, n, ok, caution, danger, first_at, last_at, last_verdict)
               SELECT target, pc, COUNT(*), SUM(verdict = 'ok'), SUM(verdict = 'caution'), SUM(verdict = 'danger'), MIN(at), MAX(at), verdict
               FROM items WHERE target != '' GROUP BY target, pc;
             INSERT INTO sum_hour(hour, ok, caution, danger, unknown)
               SELECT at / 3600000, SUM(verdict = 'ok'), SUM(verdict = 'caution'), SUM(verdict = 'danger'), SUM(verdict NOT IN ('ok', 'caution', 'danger'))
               FROM items WHERE kind NOT LIKE 'inventory.%' GROUP BY at / 3600000;",
        )
        .map_err(e)?;
        tx.execute("INSERT INTO meta(k, v) VALUES('summed', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1", params![now as i64]).map_err(e)?;
        tx.commit().map_err(e)
    }

    /// 요약 표를 마지막으로 만든 시각
    pub fn summarized(&self) -> u64 {
        self.db().query_row("SELECT v FROM meta WHERE k = 'summed'", [], |r| r.get::<_, i64>(0)).optional().ok().flatten().unwrap_or(0) as u64
    }

    // ---- 조회(읽기만) ----

    /// 종합 분석: 요약 표에서 읽는다.
    pub fn analysis(&self, now: u64) -> Analysis {
        let made = self.summarized();
        let db = self.db();
        let mut by_target: Vec<TargetView> = Vec::new();
        if let Ok(mut st) = db.prepare(
            "SELECT s.target, s.pc, COALESCE(p.name, ''), s.n, s.ok, s.caution, s.danger, s.first_at, s.last_at, s.last_verdict
             FROM sum_target s LEFT JOIN pcs p ON p.id = s.pc ORDER BY s.target, s.last_at DESC",
        ) {
            let rows = st.query_map([], |r| {
                let n = |i: usize| r.get::<_, i64>(i).map(|v| v as u64);
                Ok((r.get::<_, String>(0)?, Seen { pc: r.get(1)?, pc_name: r.get(2)?, n: n(3)?, ok: n(4)?, caution: n(5)?, danger: n(6)?, first_at: n(7)?, last_at: n(8)?, last_verdict: r.get(9)? }))
            });
            for (target, seen) in rows.map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default() {
                // 가명(내부-…)은 보낸 PC 안에서만 같은 대상이므로 PC마다 따로 둔다.
                let alias = target.starts_with("내부-");
                match by_target.last_mut().filter(|t| t.target == target && !alias) {
                    Some(t) => {
                        t.total += seen.n;
                        t.seen.push(seen);
                    }
                    None => by_target.push(TargetView { target, total: seen.n, seen: vec![seen], single: true, differs: false, alias }),
                }
            }
        }
        for t in &mut by_target {
            t.single = t.seen.len() == 1;
            t.differs = t.seen.iter().any(|s| s.last_verdict != t.seen[0].last_verdict);
        }
        by_target.sort_by(|a, b| b.seen.len().cmp(&a.seen.len()).then(b.total.cmp(&a.total)).then(a.target.cmp(&b.target)));
        by_target.truncate(ANALYSIS_TARGETS);

        let since = (now.saturating_sub(30 * DAY) / HOUR) as i64;
        let hours = db
            .prepare("SELECT hour, ok, caution, danger, unknown FROM sum_hour WHERE hour >= ?1 ORDER BY hour")
            .and_then(|mut st| {
                let n = |r: &rusqlite::Row<'_>, i: usize| r.get::<_, i64>(i).map(|v| v as u64);
                st.query_map(params![since], |r| Ok((n(r, 0)?, n(r, 1)?, n(r, 2)?, n(r, 3)?, n(r, 4)?))).map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        Analysis { made, targets: by_target, hours }
    }

    /// 보고서용: 기간(진단 시각) 안의 건을 최근 것부터 `max`건, 그리고 전체 건수. 요약 문장도 함께.
    pub fn report(&self, from: u64, to: u64, max: usize) -> (Vec<(Row, Vec<String>)>, u64) {
        let (from, to) = (from.min(i64::MAX as u64) as i64, to.min(i64::MAX as u64) as i64);
        let db = self.db();
        let total: i64 = db.query_row("SELECT COUNT(*) FROM items WHERE at >= ?1 AND at <= ?2", params![from, to], |r| r.get(0)).unwrap_or(0);
        let Ok(mut st) = db.prepare(
            "SELECT i.pc, COALESCE(p.name, ''), i.seq, i.at, i.got, i.kind, i.title, i.target, i.verdict, i.skew, i.stale, i.masked, i.body
             FROM items i LEFT JOIN pcs p ON p.id = i.pc WHERE i.at >= ?1 AND i.at <= ?2 ORDER BY i.at DESC, i.got DESC LIMIT ?3",
        ) else {
            return (Vec::new(), 0);
        };
        let rows = st.query_map(params![from, to, max as i64], |r| {
            let row = Row {
                pc: r.get(0)?,
                pc_name: r.get(1)?,
                seq: r.get::<_, i64>(2)? as u64,
                at: r.get::<_, i64>(3)? as u64,
                got: r.get::<_, i64>(4)? as u64,
                kind: r.get(5)?,
                title: r.get(6)?,
                target: r.get(7)?,
                verdict: r.get(8)?,
                skew: r.get(9)?,
                stale: r.get(10)?,
                masked: r.get(11)?,
            };
            let body: String = r.get(12)?;
            let summary = serde_json::from_str(&body).ok().and_then(|v| wire::from_ocsf(&v)).map(|p| p.item.summary).unwrap_or_default();
            Ok((row, summary))
        });
        (rows.map(|r| r.flatten().collect()).unwrap_or_default(), total as u64)
    }

    pub fn pcs(&self, now: u64) -> Vec<Pc> {
        let db = self.db();
        let Ok(mut st) = db.prepare(
            "SELECT p.id, p.name, p.addr, p.state, p.own, p.paired, p.last_seq, p.seen, COUNT(i.seq), COALESCE(SUM(i.size), 0)
             FROM pcs p LEFT JOIN items i ON i.pc = p.id GROUP BY p.id ORDER BY p.own DESC, p.name",
        ) else {
            return Vec::new();
        };
        let rows = st.query_map([], |r| {
            let (state, own, seen): (String, bool, i64) = (r.get(3)?, r.get(4)?, r.get(7)?);
            Ok(Pc {
                id: r.get(0)?,
                name: r.get(1)?,
                addr: r.get(2)?,
                quiet: !own && state == "active" && now > seen as u64 + QUIET_AFTER,
                state,
                own,
                paired: r.get::<_, i64>(5)? as u64,
                last_seq: r.get::<_, i64>(6)? as u64,
                seen: seen as u64,
                items: r.get::<_, i64>(8)? as u64,
                bytes: r.get::<_, i64>(9)? as u64,
            })
        });
        rows.map(|r| r.flatten().collect()).unwrap_or_default()
    }

    /// 거른 목록(진단 시각의 최근 것부터)과 전체 건수
    pub fn list(&self, f: &Filter) -> (Vec<Row>, u64) {
        let like = format!("%{}%", f.target.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let to = if f.to == 0 { i64::MAX } else { f.to.min(i64::MAX as u64) as i64 };
        let cond = "(?1 = '' OR i.pc = ?1) AND i.at >= ?2 AND i.at <= ?3 AND (?4 = '%%' OR i.target LIKE ?4 ESCAPE '\\') AND (?5 = '' OR i.kind = ?5) AND (?6 = '' OR i.verdict = ?6)";
        let args = params![f.pc, f.from.min(i64::MAX as u64) as i64, to, like, f.kind, f.verdict];
        let db = self.db();
        let total: i64 = db.query_row(&format!("SELECT COUNT(*) FROM items i WHERE {cond}"), args, |r| r.get(0)).unwrap_or(0);
        let sql = format!(
            "SELECT i.pc, COALESCE(p.name, ''), i.seq, i.at, i.got, i.kind, i.title, i.target, i.verdict, i.skew, i.stale, i.masked
             FROM items i LEFT JOIN pcs p ON p.id = i.pc WHERE {cond} ORDER BY i.at DESC, i.got DESC LIMIT {PAGE} OFFSET {}",
            f.offset
        );
        let Ok(mut st) = db.prepare(&sql) else { return (Vec::new(), 0) };
        let rows = st.query_map(args, |r| {
            Ok(Row {
                pc: r.get(0)?,
                pc_name: r.get(1)?,
                seq: r.get::<_, i64>(2)? as u64,
                at: r.get::<_, i64>(3)? as u64,
                got: r.get::<_, i64>(4)? as u64,
                kind: r.get(5)?,
                title: r.get(6)?,
                target: r.get(7)?,
                verdict: r.get(8)?,
                skew: r.get(9)?,
                stale: r.get(10)?,
                masked: r.get(11)?,
            })
        });
        (rows.map(|r| r.flatten().collect()).unwrap_or_default(), total as u64)
    }

    /// 한 건의 내용(다듬은 뒤)
    pub fn item(&self, pc: &str, seq: u64) -> Option<wire::Finished> {
        let body: String = self.db().query_row("SELECT body FROM items WHERE pc = ?1 AND seq = ?2", params![pc, seq as i64], |r| r.get(0)).optional().ok()??;
        wire::from_ocsf(&serde_json::from_str(&body).ok()?).map(|p| p.item)
    }

    // ---- 수집 예약표(관제 PC가 거점별로 정한 항목과 주기) ----

    /// 거점의 예약표에서 한 항목의 주기를 정하거나(분) 뺀다. 바뀌면 판 번호가 오른다.
    pub fn plan_set(&self, pc: &str, what: &str, every: Option<u32>) -> Result<(), String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(e)?;
        match every {
            Some(m) => tx.execute("INSERT INTO plans(pc, what, every) VALUES(?1, ?2, ?3) ON CONFLICT(pc, what) DO UPDATE SET every = ?3", params![pc, what, m]),
            None => tx.execute("DELETE FROM plans WHERE pc = ?1 AND what = ?2", params![pc, what]),
        }
        .map_err(e)?;
        tx.execute("INSERT INTO plan_state(pc, rev) VALUES(?1, 1) ON CONFLICT(pc) DO UPDATE SET rev = rev + 1", params![pc]).map_err(e)?;
        tx.commit().map_err(e)
    }

    /// [바로 수집]: 거점이 다음에 예약표를 물을 때 "이 항목을 한 번 바로 수집해 달라"는 표시를 함께 받아 가게 한다.
    pub fn plan_now(&self, pc: &str, what: &str, now: u64) -> Result<(), String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(e)?;
        tx.execute("INSERT INTO plan_now(pc, what, at) VALUES(?1, ?2, ?3) ON CONFLICT(pc, what) DO UPDATE SET at = ?3", params![pc, what, now as i64]).map_err(e)?;
        tx.execute("INSERT INTO plan_state(pc, rev) VALUES(?1, 1) ON CONFLICT(pc) DO UPDATE SET rev = rev + 1", params![pc]).map_err(e)?;
        tx.commit().map_err(e)
    }

    /// 거점별 바로 수집 표시: [(항목, 누른 때)]
    pub fn plan_nows(&self, pc: &str) -> Vec<(String, u64)> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT what, at FROM plan_now WHERE pc = ?1 ORDER BY what") else { return Vec::new() };
        st.query_map(params![pc], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64))).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    /// 거점의 예약표: (판 번호, [(항목, 주기)])
    pub fn plan(&self, pc: &str) -> (u64, Vec<(String, u32)>) {
        let db = self.db();
        let rev: i64 = db.query_row("SELECT rev FROM plan_state WHERE pc = ?1", params![pc], |r| r.get(0)).optional().unwrap_or(None).unwrap_or(0);
        let Ok(mut st) = db.prepare("SELECT what, every FROM plans WHERE pc = ?1 ORDER BY what") else { return (rev as u64, Vec::new()) };
        let items = st.query_map(params![pc], |r| Ok((r.get(0)?, r.get(1)?))).map(|r| r.flatten().collect()).unwrap_or_default();
        (rev as u64, items)
    }

    /// 거점이 예약표를 물어 온 것을 적는다: 언제, 거점이 가진 판, 거점에서 사람이 승인한 항목(쉼표로 이은 정해진 이름).
    pub fn plan_asked(&self, pc: &str, now: u64, have: u64, approved: &str) {
        let _ = self.db().execute(
            "INSERT INTO plan_state(pc, asked, have, approved) VALUES(?1, ?2, ?3, ?4) ON CONFLICT(pc) DO UPDATE SET asked = ?2, have = ?3, approved = ?4",
            params![pc, now as i64, have.min(i64::MAX as u64) as i64, approved],
        );
    }

    /// (마지막으로 물어 온 때, 거점이 가진 판, 승인한 항목들)
    pub fn plan_seen(&self, pc: &str) -> (u64, u64, Vec<String>) {
        self.db()
            .query_row("SELECT asked, have, approved FROM plan_state WHERE pc = ?1", params![pc], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get::<_, String>(2)?)))
            .optional()
            .unwrap_or(None)
            .map(|(a, h, s)| (a, h, s.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect()))
            .unwrap_or_default()
    }

    /// 한 PC의 한 종류에서 가장 최근 것부터 `n`건(진단 시각, 내용)
    pub fn latest(&self, pc: &str, kind: &str, n: u32) -> Vec<(u64, wire::Finished)> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT at, body FROM items WHERE pc = ?1 AND kind = ?2 ORDER BY seq DESC LIMIT ?3") else { return Vec::new() };
        st.query_map(params![pc, kind, n.min(30)], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)))
            .map(|rows| rows.flatten().filter_map(|(at, body)| Some((at, wire::from_ocsf(&serde_json::from_str(&body).ok()?)?.item))).collect())
            .unwrap_or_default()
    }

    pub fn kinds(&self) -> Vec<String> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT DISTINCT kind FROM items ORDER BY kind LIMIT 200") else { return Vec::new() };
        st.query_map([], |r| r.get(0)).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    pub fn gaps(&self) -> Vec<Gap> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT pc, first, last, got, dropped FROM gaps ORDER BY got DESC LIMIT 200") else { return Vec::new() };
        st.query_map([], |r| Ok(Gap { pc: r.get(0)?, first: r.get::<_, i64>(1)? as u64, last: r.get::<_, i64>(2)? as u64, got: r.get::<_, i64>(3)? as u64, dropped: r.get(4)? })).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    pub fn alerts(&self) -> Vec<Alert> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT pc, at, kind, note FROM alerts ORDER BY id DESC LIMIT 100") else { return Vec::new() };
        st.query_map([], |r| Ok(Alert { pc: r.get(0)?, at: r.get::<_, i64>(1)? as u64, kind: r.get(2)?, note: r.get(3)? })).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    /// 접속 한 번을 적는다. 짝이 아닌 접속은 분당 상한이 있고, 전체는 최근 것만 남긴다.
    pub fn log_conn(&self, c: &Conn) {
        let db = self.db();
        let known: bool = !c.pc.is_empty() && db.query_row("SELECT 1 FROM pcs WHERE id = ?1", params![c.pc], |_| Ok(())).optional().unwrap_or(None).is_some();
        if !known {
            let recent: i64 = db
                .query_row("SELECT COUNT(*) FROM conns c WHERE c.at > ?1 AND NOT EXISTS(SELECT 1 FROM pcs p WHERE p.id = c.pc)", params![c.at.saturating_sub(60_000) as i64], |r| r.get(0))
                .unwrap_or(0);
            if recent >= CONN_STRANGER_PER_MIN {
                return;
            }
        }
        let n = |v: u64| v.min(i64::MAX as u64) as i64;
        let _ = db.execute(
            "INSERT INTO conns(at, pc, addr, outcome, ms, stored, again, refused, bytes, first_seq, last_seq) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![n(c.at), c.pc, c.addr, c.outcome, n(c.ms), n(c.stored), n(c.again), n(c.refused), n(c.bytes), n(c.first_seq), n(c.last_seq)],
        );
        let _ = db.execute("DELETE FROM conns WHERE id <= (SELECT MAX(id) FROM conns) - ?1", params![CONN_KEEP]);
    }

    /// 거점별 최근 구간의 흐름: 지문 → (받은 건수, 바이트, 위험 판정 수)
    pub fn flow(&self, since: u64) -> std::collections::HashMap<String, (u64, u64, u64)> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT pc, COUNT(*), COALESCE(SUM(size), 0), COALESCE(SUM(verdict = 'danger'), 0) FROM items WHERE got >= ?1 GROUP BY pc") else { return Default::default() };
        st.query_map(params![since as i64], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64, r.get::<_, i64>(3)? as u64))))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// 접속 내역(최근 것부터). `pc`가 없으면 짝이 아닌 접속만.
    pub fn conns(&self, pc: Option<&str>, max: u32) -> Vec<Conn> {
        let db = self.db();
        let sql = match pc {
            Some(_) => "SELECT at, pc, addr, outcome, ms, stored, again, refused, bytes, first_seq, last_seq FROM conns WHERE pc = ?1 ORDER BY id DESC LIMIT ?2",
            None => "SELECT at, pc, addr, outcome, ms, stored, again, refused, bytes, first_seq, last_seq FROM conns c WHERE ?1 = '' AND NOT EXISTS(SELECT 1 FROM pcs p WHERE p.id = c.pc) ORDER BY id DESC LIMIT ?2",
        };
        let Ok(mut st) = db.prepare(sql) else { return Vec::new() };
        let u = |r: &rusqlite::Row<'_>, i: usize| r.get::<_, i64>(i).map(|v| v.max(0) as u64);
        st.query_map(params![pc.unwrap_or(""), max.min(PAGE * 5)], |r| {
            Ok(Conn { at: u(r, 0)?, pc: r.get(1)?, addr: r.get(2)?, outcome: r.get(3)?, ms: u(r, 4)?, stored: u(r, 5)?, again: u(r, 6)?, refused: u(r, 7)?, bytes: u(r, 8)?, first_seq: u(r, 9)?, last_seq: u(r, 10)? })
        })
        .map(|r| r.flatten().collect())
        .unwrap_or_default()
    }

    /// 전송 내역(받은 순서의 최근 것부터, 본문 없이)
    pub fn transfers(&self, pc: &str, max: u32) -> Vec<Transfer> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT seq, at, got, kind, title, verdict, size, skew, stale FROM items WHERE pc = ?1 ORDER BY seq DESC LIMIT ?2") else { return Vec::new() };
        st.query_map(params![pc, max.min(PAGE * 5)], |r| {
            Ok(Transfer {
                seq: r.get::<_, i64>(0)? as u64,
                at: r.get::<_, i64>(1)? as u64,
                got: r.get::<_, i64>(2)? as u64,
                kind: r.get(3)?,
                title: r.get(4)?,
                verdict: r.get(5)?,
                size: r.get::<_, i64>(6)? as u64,
                skew: r.get(7)?,
                stale: r.get(8)?,
            })
        })
        .map(|r| r.flatten().collect())
        .unwrap_or_default()
    }

    /// 최근 7일을 한 시간 칸으로 나눈 시간표(오래된 칸부터)
    pub fn timeline(&self, pc: &str, now: u64) -> Vec<Hour> {
        let end = now / HOUR + 1;
        let start = end.saturating_sub(TIMELINE_HOURS as u64);
        let mut hours: Vec<Hour> = (start..end).map(|h| Hour { at: h * HOUR, ..Hour::default() }).collect();
        let db = self.db();
        let from = (start * HOUR) as i64;
        if let Ok(mut st) = db.prepare("SELECT got / 3600000, COUNT(*), SUM(verdict = 'danger') FROM items WHERE pc = ?1 AND got >= ?2 GROUP BY 1") {
            let rows = st.query_map(params![pc, from], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)));
            for (h, n, d) in rows.map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default() {
                if let Some(slot) = h.checked_sub(start).and_then(|i| hours.get_mut(i as usize)) {
                    slot.items = n;
                    slot.danger = d;
                }
            }
        }
        if let Ok(mut st) = db.prepare("SELECT at / 3600000, COUNT(*), SUM(outcome NOT IN ('ok', 'closed', 'pair_ok')) FROM conns WHERE pc = ?1 AND at >= ?2 GROUP BY 1") {
            let rows = st.query_map(params![pc, from], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)));
            for (h, n, f) in rows.map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default() {
                if let Some(slot) = h.checked_sub(start).and_then(|i| hours.get_mut(i as usize)) {
                    slot.conns = n;
                    slot.failed = f;
                }
            }
        }
        hours
    }

    /// 받지 않던 구간(시작, 끝)
    pub fn downtime(&self) -> Vec<(u64, u64)> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT first, last FROM downtime ORDER BY last DESC LIMIT 100") else { return Vec::new() };
        st.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))).map(|r| r.flatten().collect()).unwrap_or_default()
    }
}

// ================= 보낼 함 =================

const OUTBOX_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS log(
  seq INTEGER PRIMARY KEY, at INTEGER NOT NULL, queued INTEGER NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, target TEXT NOT NULL, verdict TEXT NOT NULL,
  state TEXT NOT NULL, sent INTEGER NOT NULL DEFAULT 0, line TEXT);
CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v INTEGER NOT NULL);
";

/// 나간(또는 나갈) 건의 기록. 보내는 PC에서 "언제, 무엇을" 보냈는지 보는 목록이다.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sent {
    pub seq: u64,
    pub at: u64,
    pub kind: String,
    pub title: String,
    pub target: String,
    pub verdict: String,
    /// queued | sent | dropped | local
    pub state: String,
    pub sent: u64,
}

pub struct Outbox {
    db: Mutex<Connection>,
}

impl Outbox {
    pub fn open(path: &Path) -> Result<Outbox, String> {
        Ok(Outbox { db: Mutex::new(open(path, OUTBOX_SCHEMA)?) })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn meta(db: &Connection, k: &str) -> u64 {
        db.query_row("SELECT v FROM meta WHERE k = ?1", params![k], |r| r.get::<_, i64>(0)).optional().ok().flatten().unwrap_or(0) as u64
    }

    fn set_meta(db: &Connection, k: &str, v: u64) -> Result<(), rusqlite::Error> {
        db.execute("INSERT INTO meta(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2", params![k, v as i64]).map(|_| ())
    }

    /// 이 PC의 다음 순번을 받아, 그 순번으로 만든 한 줄을 넣는다. `queue`가 거짓이면 보내지 않고 기록만 남긴다(이 PC의 거점 관제에만 넣을 때).
    /// 돌려주는 것: (순번, 한 줄). 한 줄을 만들지 못하면(너무 큼) 순번을 쓰지 않는다.
    pub fn push(&self, f: &wire::Finished, queue: bool, now: u64, make: impl FnOnce(u64) -> Option<String>) -> Result<Option<(u64, String)>, String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(e)?;
        let seq = Self::meta(&tx, "seq") + 1;
        let Some(line) = make(seq) else { return Ok(None) };
        Self::set_meta(&tx, "seq", seq).map_err(e)?;
        tx.execute(
            "INSERT INTO log(seq, at, queued, kind, title, target, verdict, state, line) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![seq as i64, f.ended.min(i64::MAX as u64) as i64, now as i64, f.kind, f.title, f.target, f.verdict, if queue { "queued" } else { "local" }, queue.then_some(&line)],
        )
        .map_err(e)?;
        Self::trim(&tx, now).map_err(e)?;
        tx.commit().map_err(e)?;
        Ok(Some((seq, line)))
    }

    /// 한도(건수, 보관 기한)를 넘은 것을 오래된 것부터 버리고, 버린 구간을 적어 둔다.
    fn trim(tx: &rusqlite::Transaction<'_>, now: u64) -> Result<(), rusqlite::Error> {
        let queued: i64 = tx.query_row("SELECT COUNT(*) FROM log WHERE state = 'queued'", [], |r| r.get(0))?;
        let over = (queued as u64).saturating_sub(OUT_MAX_QUEUE) as i64;
        let cut = now.saturating_sub(OUT_KEEP) as i64;
        let range: (Option<i64>, Option<i64>) = tx.query_row(
            "SELECT MIN(seq), MAX(seq) FROM log WHERE state = 'queued' AND (queued < ?1 OR seq IN (SELECT seq FROM log WHERE state = 'queued' ORDER BY seq LIMIT ?2))",
            params![cut, over],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if let (Some(first), Some(last)) = range {
            tx.execute("UPDATE log SET state = 'dropped', line = NULL WHERE state = 'queued' AND seq >= ?1 AND seq <= ?2", params![first, last])?;
            let old_first = Self::meta(tx, "drop_first");
            Self::set_meta(tx, "drop_first", if old_first == 0 { first as u64 } else { old_first.min(first as u64) })?;
            Self::set_meta(tx, "drop_last", Self::meta(tx, "drop_last").max(last as u64))?;
        }
        tx.execute("DELETE FROM log WHERE state != 'queued' AND seq <= (SELECT MAX(seq) FROM log) - ?1", params![OUT_LOG_KEEP as i64])?;
        Ok(())
    }

    /// 다음에 보낼 것(가장 오래된 것)
    pub fn next(&self) -> Option<(u64, String)> {
        self.db().query_row("SELECT seq, line FROM log WHERE state = 'queued' AND line IS NOT NULL ORDER BY seq LIMIT 1", [], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?))).optional().ok()?
    }

    /// "받았다"는 답을 받았다. 이때만 보낼 함에서 지운다.
    pub fn done(&self, seq: u64, now: u64) {
        let _ = self.db().execute("UPDATE log SET state = 'sent', sent = ?2, line = NULL WHERE seq = ?1 AND state = 'queued'", params![seq as i64, now as i64]);
    }

    /// 관제 PC가 받지 않겠다고 한 건(지나간 순번 등). 다시 보내지 않는다.
    pub fn give_up(&self, seq: u64) {
        let _ = self.db().execute("UPDATE log SET state = 'dropped', line = NULL WHERE seq = ?1 AND state = 'queued'", params![seq as i64]);
    }

    /// 버렸지만 아직 관제 PC에 알리지 못한 구간
    pub fn dropped(&self) -> Option<(u64, u64)> {
        let db = self.db();
        let (first, last) = (Self::meta(&db, "drop_first"), Self::meta(&db, "drop_last"));
        (first > 0 && last >= first).then_some((first, last))
    }

    /// 버린 구간을 관제 PC에 알렸다.
    pub fn dropped_told(&self) {
        let db = self.db();
        let _ = Self::set_meta(&db, "drop_first", 0);
        let _ = Self::set_meta(&db, "drop_last", 0);
    }

    pub fn waiting(&self) -> u64 {
        self.db().query_row("SELECT COUNT(*) FROM log WHERE state = 'queued'", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as u64
    }

    /// 보내기를 끄거나 짝을 끊을 때: 기다리던 것을 비운다(다른 관제 PC로 새지 않게).
    pub fn clear_queue(&self) {
        let _ = self.db().execute("UPDATE log SET state = 'dropped', line = NULL WHERE state = 'queued'", []);
    }

    /// 나간 건의 목록(최근 것부터)
    pub fn log(&self) -> Vec<Sent> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT seq, at, kind, title, target, verdict, state, sent FROM log ORDER BY seq DESC LIMIT 200") else { return Vec::new() };
        st.query_map([], |r| {
            Ok(Sent { seq: r.get::<_, i64>(0)? as u64, at: r.get::<_, i64>(1)? as u64, kind: r.get(2)?, title: r.get(3)?, target: r.get(4)?, verdict: r.get(5)?, state: r.get(6)?, sent: r.get::<_, i64>(7)? as u64 })
        })
        .map(|r| r.flatten().collect())
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::wire::{Finished, encode};

    const NOW: u64 = 1_763_000_000_000;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("netx-collect-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("x.db")
    }

    fn item(target: &str, at: u64) -> Finished {
        Finished { kind: "route_latency".into(), title: "라우팅 지연 분석".into(), target: target.into(), verdict: "ok".into(), started: at, ended: at, ..Default::default() }
    }

    fn line(pc: &str, seq: u64, target: &str, at: u64) -> String {
        encode(&item(target, at), pc, seq, false, "t").unwrap()
    }

    #[test]
    fn connection_log_is_bounded_and_feeds_the_site_timeline() {
        let inbox = Inbox::open(&tmp("conns")).unwrap();
        inbox.pair("pcA", "용인", "100.64.0.2", false, NOW).unwrap();
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "a", NOW), NOW).unwrap(), Taken::Stored);
        let conn = |pc: &str, outcome: &str, at: u64| Conn { at, pc: pc.into(), addr: "100.64.0.2".into(), outcome: outcome.into(), stored: 1, first_seq: 1, last_seq: 1, ..Conn::default() };
        inbox.log_conn(&conn("pcA", "ok", NOW));
        inbox.log_conn(&conn("pcA", "refused", NOW + 1));
        // 짝이 아닌 접속은 1분에 정해진 수만 남고, 짝의 내역을 밀어내지 않는다
        for i in 0..200 {
            inbox.log_conn(&conn("", "tls", NOW + 2 + i));
        }
        assert_eq!(inbox.conns(None, 1000).len(), CONN_STRANGER_PER_MIN as usize);
        let mine = inbox.conns(Some("pcA"), 10);
        assert_eq!(mine.iter().map(|c| c.outcome.as_str()).collect::<Vec<_>>(), ["refused", "ok"]);
        inbox.log_conn(&conn("pcA", "ok", NOW + 500));
        assert_eq!(inbox.conns(Some("pcA"), 10).len(), 3, "짝의 접속은 상한에 걸리지 않는다");

        let hours = inbox.timeline("pcA", NOW);
        assert_eq!(hours.len(), TIMELINE_HOURS);
        let last = hours.last().unwrap();
        assert_eq!((last.items, last.danger, last.conns, last.failed), (1, 0, 3, 1));
        assert!(hours[..TIMELINE_HOURS - 1].iter().all(|h| h.items == 0 && h.conns == 0));

        let moved = inbox.transfers("pcA", 10);
        assert_eq!((moved.len(), moved[0].seq, moved[0].kind.as_str()), (1, 1, "route_latency"));
        // 보관 기한이 지나면 접속 내역도 지운다
        inbox.purge(NOW + 91 * DAY, 90);
        assert!(inbox.conns(Some("pcA"), 10).is_empty());
    }

    #[test]
    fn stores_in_order_and_refuses_replays_and_strangers() {
        let inbox = Inbox::open(&tmp("order")).unwrap();
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "a", NOW), NOW).unwrap(), Taken::Refused(Why::Unknown));
        inbox.pair("pcA", "용인", "100.64.0.2", false, NOW).unwrap();
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "a", NOW), NOW).unwrap(), Taken::Stored);
        // 같은 내용을 다시 보내면 받았다고만 답한다
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "a", NOW), NOW).unwrap(), Taken::Again);
        // 같은 순번에 다른 내용은 저장하지 않고 경보
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "b", NOW), NOW).unwrap(), Taken::Refused(Why::Conflict));
        assert_eq!(inbox.alerts()[0].kind, "conflict");
        assert_eq!(inbox.item("pcA", 1).unwrap().target, "a");
        // 다른 PC의 이름으로 된 자료, 순번이 본문과 다른 자료
        assert_eq!(inbox.take("pcA", 2, &line("pcB", 2, "a", NOW), NOW).unwrap(), Taken::Refused(Why::Shape));
        assert_eq!(inbox.take("pcA", 2, &line("pcA", 3, "a", NOW), NOW).unwrap(), Taken::Refused(Why::Shape));
        assert_eq!(inbox.list(&Filter::default()).1, 1);
    }

    #[test]
    fn repeated_violations_stop_the_pair_until_a_person_restarts_it() {
        let inbox = Inbox::open(&tmp("strike")).unwrap();
        inbox.pair("pcA", "용인", "100.64.0.2", false, NOW).unwrap();
        let big = "x".repeat(MAX_ITEM_BYTES + 1);
        for _ in 0..STRIKES_TO_STOP {
            assert_eq!(inbox.take("pcA", 1, &big, NOW).unwrap(), Taken::Refused(Why::TooBig));
        }
        assert_eq!(inbox.admitted("pcA"), Err(Why::Stopped));
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "a", NOW), NOW).unwrap(), Taken::Refused(Why::Stopped));
        assert_eq!(inbox.alerts()[0].kind, "stopped");
        assert!(inbox.set_state("pcA", "active").unwrap());
        // 하루가 지나면 지난 위반은 세지 않는다
        assert_eq!(inbox.take("pcA", 1, &line("pcA", 1, "a", NOW + DAY + 1), NOW + DAY + 1).unwrap(), Taken::Stored);
        assert!(!inbox.set_state("nobody", "active").unwrap());
        assert!(inbox.set_state("pcA", "weird").is_err());
    }

    #[test]
    fn hourly_limit_applies_per_pc_but_not_to_this_pc() {
        let inbox = Inbox::open(&tmp("rate")).unwrap();
        inbox.pair("pcA", "용인", "100.64.0.2", false, NOW).unwrap();
        inbox.pair("me", "평창", "", true, NOW).unwrap();
        for seq in 1..=HOUR_MAX_ITEMS {
            assert_eq!(inbox.take("pcA", seq, &line("pcA", seq, "a", NOW), NOW).unwrap(), Taken::Stored);
            assert_eq!(inbox.take("me", seq, &line("me", seq, "a", NOW), NOW).unwrap(), Taken::Stored);
        }
        let next = HOUR_MAX_ITEMS + 1;
        assert_eq!(inbox.take("pcA", next, &line("pcA", next, "a", NOW), NOW).unwrap(), Taken::Refused(Why::TooFast));
        assert_eq!(inbox.take("me", next, &line("me", next, "a", NOW), NOW).unwrap(), Taken::Stored);
        // 한 시간이 지나면 다시 받는다
        assert_eq!(inbox.take("pcA", next, &line("pcA", next, "a", NOW + HOUR + 1), NOW + HOUR + 1).unwrap(), Taken::Stored);
    }

    #[test]
    fn gaps_clock_skew_and_stale_times_are_marked() {
        let inbox = Inbox::open(&tmp("gap")).unwrap();
        inbox.pair("pcA", "용인", "100.64.0.2", false, NOW).unwrap();
        inbox.take("pcA", 1, &line("pcA", 1, "a", NOW), NOW).unwrap();
        // 4가 먼저 오면 2~3이 빠진 구간
        inbox.take("pcA", 4, &line("pcA", 4, "a", NOW + 10 * 60_000), NOW).unwrap();
        // 보낸 쪽이 5~6을 버렸다고 알린 뒤 7이 오면 구간을 한 번만 적는다
        inbox.dropped("pcA", 5, 6, NOW);
        inbox.dropped("pcA", 5, 6, NOW);
        inbox.take("pcA", 7, &line("pcA", 7, "a", NOW - OUT_KEEP - 1), NOW).unwrap();
        let mut gaps = inbox.gaps();
        gaps.sort_by_key(|g| g.first);
        assert_eq!(gaps.iter().map(|g| (g.first, g.last, g.dropped)).collect::<Vec<_>>(), vec![(2, 3, false), (5, 6, true)]);
        let (rows, _) = inbox.list(&Filter::default());
        let four = rows.iter().find(|r| r.seq == 4).unwrap();
        assert!(four.skew && four.at == NOW, "미래 시각은 받은 시각으로 바꾼다");
        assert!(rows.iter().find(|r| r.seq == 7).unwrap().stale);
        // 지나간 순번이지만 갖고 있지 않은 것
        assert_eq!(inbox.take("pcA", 2, &line("pcA", 2, "a", NOW), NOW).unwrap(), Taken::Refused(Why::Old));
    }

    #[test]
    fn filters_and_retention_use_the_right_clock() {
        let inbox = Inbox::open(&tmp("filter")).unwrap();
        inbox.pair("pcA", "용인", "100.64.0.2", false, NOW).unwrap();
        inbox.pair("pcB", "평창", "100.64.0.3", false, NOW).unwrap();
        inbox.take("pcA", 1, &line("pcA", 1, "example.com", NOW - 2 * DAY), NOW - 2 * DAY).unwrap();
        inbox.take("pcA", 2, &line("pcA", 2, "100%_x", NOW - DAY), NOW).unwrap();
        inbox.take("pcB", 1, &line("pcB", 1, "example.com:443", NOW), NOW).unwrap();
        let count = |f: Filter| inbox.list(&f).1;
        assert_eq!(count(Filter::default()), 3);
        assert_eq!(count(Filter { pc: "pcA".into(), ..Default::default() }), 2);
        assert_eq!(count(Filter { target: "example".into(), ..Default::default() }), 2);
        assert_eq!(count(Filter { target: "%".into(), ..Default::default() }), 1, "%는 글자로 찾는다");
        assert_eq!(count(Filter { target: "' OR 1=1 --".into(), ..Default::default() }), 0);
        assert_eq!(count(Filter { from: NOW - DAY, to: NOW, ..Default::default() }), 2);
        assert_eq!(count(Filter { kind: "route_latency".into(), verdict: "ok".into(), ..Default::default() }), 3);
        assert_eq!(count(Filter { verdict: "danger".into(), ..Default::default() }), 0);
        assert_eq!(inbox.kinds(), vec!["route_latency".to_string()]);
        assert_eq!(inbox.list(&Filter::default()).0[0].pc_name, "평창");
        // 보관 기한은 진단 시각이 아니라 받은 시각으로 센다: 2번은 어제 진단했지만 오늘 받았다
        assert_eq!(inbox.purge(NOW + 1, 1), 1);
        assert_eq!(count(Filter::default()), 2);
        let pcs = inbox.pcs(NOW + QUIET_AFTER + 1);
        assert!(pcs.iter().all(|p| p.quiet));
        assert_eq!(pcs.iter().find(|p| p.id == "pcA").unwrap().items, 1);
    }

    #[test]
    fn analysis_reads_the_summary_and_tells_single_site_and_differing_targets_apart() {
        let inbox = Inbox::open(&tmp("sum")).unwrap();
        for (id, name) in [("pcA", "용인"), ("pcB", "평창")] {
            inbox.pair(id, name, "100.64.0.2", false, NOW).unwrap();
        }
        let put = |pc: &str, seq: u64, target: &str, verdict: &str, at: u64| {
            let f = Finished { verdict: verdict.into(), ..item(target, at) };
            assert_eq!(inbox.take(pc, seq, &encode(&f, pc, seq, false, "t").unwrap(), NOW).unwrap(), Taken::Stored);
        };
        put("pcA", 1, "example.com", "ok", NOW - 3 * HOUR);
        put("pcA", 2, "example.com", "danger", NOW - HOUR);
        put("pcB", 1, "example.com", "ok", NOW - 2 * HOUR);
        put("pcA", 3, "only-a.example", "caution", NOW);
        put("pcA", 4, "내부-ab12cd", "ok", NOW);
        put("pcB", 2, "내부-ab12cd", "ok", NOW);
        // 주기 작업 전에는 비어 있다(조회가 표를 만들지 않는다)
        assert_eq!(inbox.analysis(NOW), Analysis::default());
        inbox.summarize(NOW).unwrap();
        let a = inbox.analysis(NOW);
        assert_eq!(a.made, NOW);
        let first = &a.targets[0];
        assert_eq!((first.target.as_str(), first.total, first.single, first.differs), ("example.com", 3, false, true));
        let from_a = first.seen.iter().find(|s| s.pc == "pcA").unwrap();
        assert_eq!((from_a.n, from_a.ok, from_a.danger, from_a.last_verdict.as_str(), from_a.pc_name.as_str()), (2, 1, 1, "danger", "용인"));
        let only = a.targets.iter().find(|t| t.target == "only-a.example").unwrap();
        assert!(only.single && !only.differs);
        // 가명은 같은 글자라도 PC마다 다른 대상이다
        let aliases: Vec<_> = a.targets.iter().filter(|t| t.alias).collect();
        assert_eq!(aliases.len(), 2);
        assert!(aliases.iter().all(|t| t.single));
        assert_eq!(a.hours.iter().map(|h| h.1 + h.2 + h.3 + h.4).sum::<u64>(), 6);
        assert_eq!(a.hours.last().unwrap().0, NOW / HOUR);
        // 보고서용 목록
        let (rows, total) = inbox.report(NOW - HOUR, NOW, 2);
        assert_eq!((rows.len(), total), (2, 4));
    }

    #[test]
    fn listening_marks_leave_downtime_between_them() {
        let inbox = Inbox::open(&tmp("down")).unwrap();
        inbox.mark_listening(NOW);
        inbox.mark_listening(NOW + 60_000);
        assert!(inbox.downtime().is_empty());
        inbox.mark_listening(NOW + HOUR);
        assert_eq!(inbox.downtime(), vec![(NOW + 60_000, NOW + HOUR)]);
    }

    #[test]
    fn outbox_numbers_items_and_forgets_only_after_an_answer() {
        let out = Outbox::open(&tmp("out")).unwrap();
        let push = |target: &str, queue: bool, now: u64| out.push(&item(target, now), queue, now, |seq| Some(line("me", seq, target, now))).unwrap().unwrap().0;
        assert_eq!(push("a", true, NOW), 1);
        assert_eq!(push("b", false, NOW), 2);
        assert_eq!(push("c", true, NOW), 3);
        assert_eq!(out.waiting(), 2);
        assert_eq!(out.next().unwrap().0, 1);
        // 답을 받기 전에는 그대로 있다
        assert_eq!(out.next().unwrap().0, 1);
        out.done(1, NOW + 5);
        assert_eq!(out.next().unwrap().0, 3);
        out.give_up(3);
        assert!(out.next().is_none());
        let log = out.log();
        assert_eq!(log.iter().map(|s| (s.seq, s.state.as_str())).collect::<Vec<_>>(), vec![(3, "dropped"), (2, "local"), (1, "sent")]);
        // 한 줄을 만들지 못하면 순번을 쓰지 않는다
        assert!(out.push(&item("z", NOW), true, NOW, |_| None).unwrap().is_none());
        assert_eq!(push("d", true, NOW), 4);
    }

    #[test]
    fn outbox_drops_the_oldest_when_full_or_expired_and_remembers_the_range() {
        let out = Outbox::open(&tmp("drop")).unwrap();
        let push = |now: u64| out.push(&item("a", now), true, now, |seq| Some(line("me", seq, "a", now))).unwrap().unwrap().0;
        for _ in 0..OUT_MAX_QUEUE + 3 {
            push(NOW);
        }
        assert_eq!(out.waiting(), OUT_MAX_QUEUE);
        assert_eq!(out.dropped(), Some((1, 3)));
        assert_eq!(out.next().unwrap().0, 4);
        out.dropped_told();
        assert_eq!(out.dropped(), None);
        // 보관 기한이 지난 것
        let late = NOW + OUT_KEEP + 1;
        let seq = push(late);
        assert_eq!(out.waiting(), 1);
        assert_eq!(out.dropped(), Some((4, seq - 1)));
        out.clear_queue();
        assert_eq!(out.waiting(), 0);
    }
}
