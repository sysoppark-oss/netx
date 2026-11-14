//! 직접 명령으로 실행한 것의 기록: 명령, 종료 상태, 출력 앞부분.
//! 보고서의 "직접 명령" 구역(docs/REPORT_FEATURE_REQUEST_v1.md 4.1)이 읽는다.
//!
//! - 자료 폴더의 `manual.jsonl`에 단계마다 한 줄(JSON)을 덧붙인다.
//! - 출력은 앞 40줄·4,000바이트까지만 남긴다. 제어 문자는 지운다.
//! - 파일이 2MB를 넘으면 오래된 앞 절반을 버린다.
//! - 화면이 넘긴 기록은 자료다. `get`으로만 읽는다.

use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};

pub const FILE: &str = "manual.jsonl";
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const HEAD_LINES: usize = 40;
const HEAD_BYTES: usize = 4000;
const MAX_STEPS: usize = 40;

fn short(value: Option<&Value>, max: usize) -> String {
    value.and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control()).take(max).collect()
}

/// 끝난 실행(계획과 결과)을 단계마다 한 줄로 바꾼다. 실행되지 않은 단계는 남기지 않는다.
pub fn lines(record: &Value, now_ms: u64) -> Vec<String> {
    let null = Value::Null;
    let run = record.get("run").unwrap_or(&null);
    let planned = record.get("plan").and_then(|p| p.get("steps")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let output = run.get("lines").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();
    for step in planned.iter().take(MAX_STEPS) {
        let Some(order) = step.get("order").and_then(Value::as_u64) else { continue };
        let Some(state) = run.get("steps").and_then(|s| s.get(order.to_string())) else { continue };
        let status = match state.get("status").and_then(Value::as_str) {
            Some("done") => "ok",
            Some("timeout") => "timeout",
            Some("cancelled") => "cancelled",
            Some("capped") => "capped",
            Some("error") => "error",
            _ => continue,
        };
        let command = short(step.get("display"), 1000);
        if command.is_empty() {
            continue;
        }
        let mut head = String::new();
        let mut cut = false;
        let mut taken = 0;
        for line in output.iter().filter(|l| l.get("order").and_then(Value::as_u64) == Some(order)) {
            let text: String = line.get("line").and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control() || *c == '\t').collect();
            if taken >= HEAD_LINES || head.len() + text.len() + 1 > HEAD_BYTES {
                cut = true;
                break;
            }
            head.push_str(&text);
            head.push('\n');
            taken += 1;
        }
        let started = state.get("started_at").and_then(Value::as_u64).filter(|at| *at > 0).unwrap_or(now_ms);
        out.push(
            json!({
                "time": started,
                "command": command,
                "tool": short(step.get("tool"), 40),
                "risk": short(step.get("risk"), 20),
                "state": status,
                "exit": state.get("exit_code").and_then(Value::as_i64),
                "secs": state.get("elapsed_ms").and_then(Value::as_f64).map(|ms| (ms / 100.0).round() / 10.0),
                "output": head,
                "cut": cut,
            })
            .to_string(),
        );
    }
    out
}

/// 기록을 덧붙인다. 남긴 줄 수를 돌려준다.
pub fn save(data_dir: &Path, record: &Value, now_ms: u64) -> Result<usize, String> {
    let rows = lines(record, now_ms);
    if rows.is_empty() {
        return Ok(0);
    }
    let path = data_dir.join(FILE);
    std::fs::create_dir_all(data_dir).map_err(|e| format!("자료 폴더를 만들지 못했습니다: {e}"))?;
    // 너무 커졌으면 오래된 앞 절반을 버린다(줄 경계에서).
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        if let Ok(old) = std::fs::read_to_string(&path) {
            let mut from = old.len() / 2;
            while !old.is_char_boundary(from) {
                from += 1;
            }
            let rest = old.get(from..).and_then(|tail| tail.split_once('\n')).map(|(_, rest)| rest).unwrap_or("");
            std::fs::write(&path, rest).map_err(|e| format!("직접 명령 기록을 줄이지 못했습니다: {e}"))?;
        }
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("직접 명령 기록을 열지 못했습니다: {e}"))?;
    for row in &rows {
        writeln!(file, "{row}").map_err(|e| format!("직접 명령 기록을 쓰지 못했습니다: {e}"))?;
    }
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(output: Vec<&str>) -> Value {
        let lines: Vec<Value> = output.iter().map(|l| json!({"order": 1, "stream": "stdout", "line": l})).collect();
        json!({
            "plan": {"steps": [
                {"order": 1, "tool": "ping", "display": "ping -c 2 192.168.123.1", "risk": "safe"},
                {"order": 2, "tool": "nc", "display": "nc -vz 192.168.123.1 80", "risk": "safe"},
                {"order": 3, "tool": "dig", "display": "dig example.com", "risk": "safe"}
            ]},
            "run": {
                "steps": {"1": {"status": "done", "exit_code": 0, "elapsed_ms": 1040, "started_at": 1_794_355_321_000u64},
                          "2": {"status": "error", "exit_code": 1, "elapsed_ms": 3000, "started_at": 1_794_355_323_000u64},
                          "3": {"status": "pending"}},
                "lines": lines
            }
        })
    }

    #[test]
    fn one_line_per_finished_step() {
        let rows: Vec<Value> = lines(&record(vec!["PING 192.168.123.1\u{1b}[0m", "2 packets transmitted"]), 5).iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows.len(), 2, "실행되지 않은 단계는 남기지 않는다");
        assert_eq!(rows[0]["command"], "ping -c 2 192.168.123.1");
        assert_eq!((rows[0]["state"].as_str(), rows[0]["exit"].as_i64(), rows[0]["secs"].as_f64()), (Some("ok"), Some(0), Some(1.0)));
        assert_eq!(rows[0]["time"], 1_794_355_321_000u64);
        assert_eq!(rows[0]["output"], "PING 192.168.123.1[0m\n2 packets transmitted\n");
        assert_eq!(rows[0]["cut"], false);
        assert_eq!((rows[1]["state"].as_str(), rows[1]["exit"].as_i64(), rows[1]["output"].as_str()), (Some("error"), Some(1), Some("")));
    }

    #[test]
    fn keeps_only_the_head_of_the_output() {
        let many: Vec<String> = (0..100).map(|n| format!("line {n}")).collect();
        let row: Value = serde_json::from_str(&lines(&record(many.iter().map(String::as_str).collect()), 5)[0]).unwrap();
        assert_eq!(row["output"].as_str().unwrap().lines().count(), HEAD_LINES);
        assert_eq!(row["cut"], true);
        let wide = "가".repeat(3000);
        let row: Value = serde_json::from_str(&lines(&record(vec![&wide, &wide]), 5)[0]).unwrap();
        assert!(row["output"].as_str().unwrap().len() <= HEAD_BYTES);
        assert_eq!(row["cut"], true);
    }

    #[test]
    fn survives_broken_records_and_trims_the_file() {
        for bad in [json!(null), json!([1]), json!({"plan": 1, "run": "x"}), json!({"plan": {"steps": [3, {"order": "a"}, {"order": 1}]}, "run": {"steps": {"1": 5}, "lines": 7}})] {
            assert!(lines(&bad, 1).is_empty());
        }
        let dir = std::env::temp_dir().join(format!("netx-manual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(save(&dir, &json!({}), 1), Ok(0));
        assert!(!dir.join(FILE).exists(), "남길 것이 없으면 파일을 만들지 않는다");
        assert_eq!(save(&dir, &record(vec!["a"]), 1), Ok(2));
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap().lines().count(), 2);
        // 2MB를 넘으면 앞 절반을 버리고, 남은 것은 온전한 줄이다.
        let filler = format!("{}\n", json!({"time": 1, "command": "x".repeat(1000)}));
        std::fs::write(dir.join(FILE), filler.repeat(2200)).unwrap();
        save(&dir, &record(vec!["a"]), 1).unwrap();
        let kept = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!((kept.len() as u64) < MAX_BYTES * 3 / 4);
        assert!(kept.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
