//! 의뢰받은 진단의 결과를 보낸 쪽에 돌려준다.
//!
//! 근거: 통합프로젝트 「변경 제안 — 진단 의뢰의 결과를 보낸 쪽에 자동으로 돌려주기 (1판 초안)」.
//! 초안 기준으로 먼저 만든 것이다. 형식이 확정되면 맞춰 고친다.
//!
//! - 의뢰를 받으면 `open`으로 기억한다(요청 번호, 보낸 모듈, 대상).
//! - 화면이 끝난 진단의 기록을 `publish`로 넘기면, 보내는 모양으로 다듬어(`shape`) 기억하고
//!   그 의뢰를 기다리는 접속에 알림 한 줄(`rack.result`)을 민다.
//! - 접속이 끊긴 뒤에는 `get`으로 물으면 준다(`rack.result.get`).
//!
//! 기록 속 글자(명령 출력, 장비가 보낸 글)는 자료다. 제어 문자를 지우고 길이를 자른 뒤 글자로만 싣는다.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use tokio::sync::broadcast;

/// 한 단계의 원본 출력 상한(바이트)
pub const STEP_OUTPUT_MAX: usize = 64 * 1024;
/// 알림 한 줄의 상한(바이트). 규격의 "한 메시지 1MB"보다 작게 잡는다.
pub const TOTAL_MAX: usize = 900 * 1024;
/// 기억하는 의뢰 수
const KEEP: usize = 20;
/// 한 진단에서 싣는 단계 수
const MAX_STEPS: usize = 40;

struct Job {
    request_id: String,
    from: String,
    target: String,
    seq: u64,
    /// 이 의뢰의 결과를 기다리며 붙어 있는 접속 수
    listeners: usize,
    last: Option<Value>,
}

static JOBS: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
static FEED: OnceLock<broadcast::Sender<(String, String)>> = OnceLock::new();

fn feed() -> &'static broadcast::Sender<(String, String)> {
    FEED.get_or_init(|| broadcast::channel(8).0)
}

fn jobs() -> std::sync::MutexGuard<'static, VecDeque<Job>> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner())
}

/// 접속마다 하나씩 받아 두는 알림 통로: (요청 번호, 보낼 한 줄)
pub fn subscribe() -> broadcast::Receiver<(String, String)> {
    feed().subscribe()
}

/// 의뢰를 기억한다. 같은 번호가 다시 오면 그대로 둔다.
pub fn open(request_id: &str, from: &str, target: &str) {
    let mut jobs = jobs();
    if jobs.iter().any(|j| j.request_id == request_id) {
        return;
    }
    jobs.push_back(Job { request_id: request_id.to_string(), from: from.to_string(), target: target.to_string(), seq: 0, listeners: 0, last: None });
    if jobs.len() > KEEP {
        jobs.pop_front();
    }
}

/// 이 접속이 그 의뢰의 결과를 기다린다. 모르는 의뢰면 거짓.
pub fn join(request_id: &str) -> bool {
    match jobs().iter_mut().find(|j| j.request_id == request_id) {
        Some(job) => {
            job.listeners += 1;
            true
        }
        None => false,
    }
}

/// 접속이 끊겼다.
pub fn leave(request_ids: &[String]) {
    let mut jobs = jobs();
    for id in request_ids {
        if let Some(job) = jobs.iter_mut().find(|j| &j.request_id == id) {
            job.listeners = job.listeners.saturating_sub(1);
        }
    }
}

/// 그 의뢰의 마지막 결과. 아직 없으면 `state: "pending"`, 모르는 의뢰면 없음.
pub fn get(request_id: &str) -> Option<Value> {
    let jobs = jobs();
    let job = jobs.iter().find(|j| j.request_id == request_id)?;
    Some(job.last.clone().unwrap_or_else(|| json!({"request_id": job.request_id, "module": super::MODULE_ID, "intent": "diagnose.target", "target": job.target, "state": "pending"})))
}

/// 보낸 결과가 어떻게 됐는지
#[derive(Debug, serde::Serialize, PartialEq)]
pub struct Sent {
    /// 보낸 모듈
    pub to: String,
    pub target: String,
    pub bytes: usize,
    /// 기다리던 접속이 있어 바로 보냈다. 거짓이면 들고 있다가 물으면 준다.
    pub live: bool,
}

/// 끝난 진단의 기록을 그 의뢰의 결과로 내보낸다. 모르는 의뢰면 없음.
pub fn publish(request_id: &str, record: &Value, finished: u64) -> Option<Sent> {
    let mut jobs = jobs();
    let job = jobs.iter_mut().find(|j| j.request_id == request_id)?;
    job.seq += 1;
    let params = shape(&job.request_id, job.seq, &job.target, record, finished);
    let mut line = json!({"jsonrpc": "2.0", "method": "rack.result", "params": params}).to_string();
    line.push('\n');
    let sent = Sent { to: job.from.clone(), target: job.target.clone(), bytes: line.len(), live: job.listeners > 0 };
    job.last = Some(params);
    let _ = feed().send((job.request_id.clone(), line));
    Some(sent)
}

/// 제어 문자를 지운다(줄바꿈과 탭은 남길 수 있다). 바이트 상한에 맞춰 글자 경계에서 자른다.
fn clean(text: &str, max: usize, keep_lines: bool) -> (String, bool) {
    let mut out = String::with_capacity(text.len().min(max));
    for c in text.chars() {
        if c.is_control() && !(keep_lines && (c == '\n' || c == '\t')) {
            continue;
        }
        if out.len() + c.len_utf8() > max {
            return (out, true);
        }
        out.push(c);
    }
    (out, false)
}

fn short(value: Option<&Value>, max: usize) -> String {
    clean(value.and_then(Value::as_str).unwrap_or(""), max, false).0
}

fn verdict(light: Option<&Value>) -> &'static str {
    match light.and_then(Value::as_str) {
        Some("green") => "ok",
        Some("yellow") => "caution",
        Some("red") => "danger",
        _ => "unknown",
    }
}

/// NetX의 진단 기록(화면이 [최근 진단]에 남기는 것과 같은 모양)을 보내는 모양으로 바꾼다.
/// 기록은 화면이 만든 것이지만 그 안의 글자는 밖에서 온 것이므로 `get`으로만 읽고 다듬어 싣는다.
pub fn shape(request_id: &str, seq: u64, target: &str, record: &Value, finished: u64) -> Value {
    let null = Value::Null;
    let run = record.get("run").unwrap_or(&null);
    let planned = record.get("plan").and_then(|p| p.get("steps")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let lines = run.get("lines").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);

    let mut steps = Vec::new();
    let mut summary = Vec::new();
    let mut calm = Vec::new();
    let mut cancelled = false;
    let mut started: Option<u64> = None;
    for step in planned.iter().take(MAX_STEPS) {
        let Some(order) = step.get("order").and_then(Value::as_u64) else { continue };
        let key = order.to_string();
        let state = run.get("steps").and_then(|s| s.get(&key));
        let result = run.get("results").and_then(|r| r.get(&key));
        let status = match state.and_then(|s| s.get("status")).and_then(Value::as_str) {
            Some("done") => "ok",
            Some("timeout") => "timeout",
            Some("cancelled") => "cancelled",
            Some("capped") => "capped",
            Some("error") => "error",
            Some("running") => "running",
            _ => "skipped",
        };
        cancelled |= status == "cancelled";
        if let Some(at) = state.and_then(|s| s.get("started_at")).and_then(Value::as_u64).filter(|at| *at > 0) {
            // 화면은 밀리초로 적는다.
            let at = if at > 100_000_000_000 { at / 1000 } else { at };
            started = Some(started.map_or(at, |s| s.min(at)));
        }

        let mut notes = Vec::new();
        let text = short(result.and_then(|r| r.get("text")), 600);
        if !text.is_empty() {
            let bad = matches!(verdict(result.and_then(|r| r.get("light"))), "caution" | "danger");
            if bad { &mut summary } else { &mut calm }.push(text.clone());
            notes.push(text);
        }
        for fact in result.and_then(|r| r.get("facts")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(30) {
            let label = short(fact.get("label"), 80);
            let value = short(fact.get("value"), 300);
            if !label.is_empty() || !value.is_empty() {
                notes.push(format!("{label}: {value}"));
            }
        }

        let mut raw = String::new();
        for line in lines.iter().filter(|l| l.get("order").and_then(Value::as_u64) == Some(order)) {
            if raw.len() > STEP_OUTPUT_MAX {
                break;
            }
            raw.push_str(line.get("line").and_then(Value::as_str).unwrap_or(""));
            raw.push('\n');
        }
        let (output, truncated) = clean(&raw, STEP_OUTPUT_MAX, true);

        steps.push(json!({
            "order": order,
            "title": short(step.get("title"), 120),
            "tool": short(step.get("tool"), 40),
            "command": short(step.get("display"), 1000),
            "risk": short(step.get("risk"), 20),
            "state": status,
            "exit": state.and_then(|s| s.get("exit_code")).and_then(Value::as_i64),
            "secs": state.and_then(|s| s.get("elapsed_ms")).and_then(Value::as_f64).map(|ms| (ms / 100.0).round() / 10.0),
            "verdict": verdict(result.and_then(|r| r.get("light"))),
            "notes": notes,
            "output": output,
            "output_truncated": truncated,
        }));
    }
    if summary.is_empty() {
        summary = calm;
    }
    summary.truncate(5);

    let meta = record.get("meta").unwrap_or(&null);
    let mut params = json!({
        "request_id": request_id,
        "seq": seq,
        "module": super::MODULE_ID,
        "intent": "diagnose.target",
        "target": target,
        "state": if cancelled { "cancelled" } else { "done" },
        "started": started,
        "finished": finished,
        "recipe": {
            "id": short(record.get("recipe_id"), 40),
            "title": short(meta.get("title"), 120),
            "depth": short(record.get("inputs").and_then(|i| i.get("depth")), 20),
        },
        "verdict": verdict(meta.get("light")),
        "summary": summary,
        "steps": steps,
    });

    // 전체 상한을 넘으면 가장 긴 원본 출력부터 반씩 줄인다.
    while params.to_string().len() > TOTAL_MAX {
        let Some(list) = params.get_mut("steps").and_then(Value::as_array_mut) else { break };
        let Some(longest) = list.iter_mut().max_by_key(|s| s.get("output").and_then(Value::as_str).map_or(0, str::len)) else { break };
        let now = longest.get("output").and_then(Value::as_str).unwrap_or("").to_string();
        if now.is_empty() {
            break;
        }
        let (half, _) = clean(&now, now.len() / 2, true);
        if let Some(step) = longest.as_object_mut() {
            step.insert("output".into(), Value::String(half));
            step.insert("output_truncated".into(), Value::Bool(true));
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(output: &str) -> Value {
        json!({
            "meta": {"title": "라우팅 지연 분석", "recipe": "route_latency", "target": "142.250.197.142", "light": "yellow", "steps": 2},
            "recipe_id": "route_latency",
            "inputs": {"target": "142.250.197.142", "depth": "normal"},
            "plan": {"steps": [
                {"order": 1, "tool": "mtr", "title": "경로 추적 (mtr)", "display": "mtr -r -w -b -c 10 -- 142.250.197.142", "risk": "safe"},
                {"order": 2, "tool": "ping", "title": "핑", "display": "ping -c 5 142.250.197.142", "risk": "safe"}
            ]},
            "run": {
                "steps": {"1": {"status": "done", "exit_code": 0, "elapsed_ms": 25310, "started_at": 1794355321000u64},
                          "2": {"status": "done", "exit_code": 0, "elapsed_ms": 4020, "started_at": 1794355322000u64}},
                "results": {"1": {"light": "yellow", "text": "8번째 구간부터 느립니다\u{1b}[31m", "facts": [{"label": "8홉 평균", "value": "41ms"}]},
                            "2": {"light": "green", "text": "손실이 없습니다", "facts": []}},
                "lines": [{"order": 1, "stream": "stdout", "line": output}, {"order": 2, "stream": "stdout", "line": "5 packets transmitted"}]
            }
        })
    }

    #[test]
    fn shapes_a_record_step_by_step() {
        let out = shape("id-1", 1, "142.250.197.142:443", &record("HOST: me\u{7} Loss%"), 1794355400);
        assert_eq!(out["target"], "142.250.197.142:443", "대상은 의뢰받은 값 그대로");
        assert_eq!(out["state"], "done");
        assert_eq!(out["verdict"], "caution");
        assert_eq!(out["started"], 1794355321u64);
        assert_eq!(out["recipe"], json!({"id": "route_latency", "title": "라우팅 지연 분석", "depth": "normal"}));
        assert_eq!(out["summary"], json!(["8번째 구간부터 느립니다[31m"]), "나쁜 판정의 풀이만 요약에 오른다, 제어 문자는 지운다");
        let first = &out["steps"][0];
        assert_eq!(first["command"], "mtr -r -w -b -c 10 -- 142.250.197.142");
        assert_eq!((first["state"].as_str(), first["exit"].as_i64(), first["secs"].as_f64()), (Some("ok"), Some(0), Some(25.3)));
        assert_eq!(first["verdict"], "caution");
        assert_eq!(first["notes"], json!(["8번째 구간부터 느립니다[31m", "8홉 평균: 41ms"]));
        assert_eq!(first["output"], "HOST: me Loss%\n");
        assert_eq!(first["output_truncated"], false);
        assert_eq!(out["steps"][1]["output"], "5 packets transmitted\n");
    }

    #[test]
    fn cuts_long_output_per_step_and_in_total() {
        let out = shape("id-2", 1, "10.0.0.5", &record(&"가".repeat(60_000)), 1);
        let text = out["steps"][0]["output"].as_str().unwrap();
        assert!(text.len() <= STEP_OUTPUT_MAX && text.len() > STEP_OUTPUT_MAX - 4);
        assert_eq!(out["steps"][0]["output_truncated"], true);

        // 단계가 많아 전체 상한을 넘으면 긴 출력부터 줄인다.
        let mut big = record("x");
        let steps: Vec<Value> = (1..=30).map(|n| json!({"order": n, "tool": "t", "title": "t", "display": "t", "risk": "safe"})).collect();
        let lines: Vec<Value> = (1..=30).map(|n| json!({"order": n, "stream": "stdout", "line": "a".repeat(STEP_OUTPUT_MAX)})).collect();
        big["plan"]["steps"] = Value::Array(steps);
        big["run"]["lines"] = Value::Array(lines);
        let out = shape("id-3", 1, "10.0.0.5", &big, 1);
        assert!(out.to_string().len() <= TOTAL_MAX);
        assert!(out["steps"].as_array().unwrap().iter().any(|s| s["output_truncated"] == true));
    }

    #[test]
    fn survives_broken_records() {
        for bad in [json!(null), json!("x"), json!([1, 2]), json!({"plan": 3, "run": []}), json!({"plan": {"steps": [{"order": "x"}, 5, {"order": 1}]}, "run": {"lines": [1, {"order": 1}], "steps": {"1": 7}, "results": {"1": "y"}}})] {
            let out = shape("id-4", 1, "10.0.0.5", &bad, 1);
            assert_eq!(out["request_id"], "id-4");
            assert_eq!(out["verdict"], "unknown");
        }
        let odd = shape("id-5", 1, "10.0.0.5", &json!({"plan": {"steps": [{"order": 1}]}, "run": {"steps": {"1": {"status": "cancelled"}}}}), 1);
        assert_eq!(odd["state"], "cancelled");
        assert_eq!(odd["steps"][0]["state"], "cancelled");
    }

    #[test]
    fn remembers_jobs_and_answers_later() {
        let id = "00000000-0000-4000-8000-00000000a001";
        assert_eq!(get(id), None, "모르는 의뢰");
        assert_eq!(publish(id, &record("x"), 1), None);
        open(id, "storyfirewall", "10.0.0.5:80");
        assert_eq!(get(id).unwrap()["state"], "pending");

        // 기다리는 접속이 없으면 들고만 있는다.
        let sent = publish(id, &record("x"), 1).unwrap();
        assert_eq!((sent.to.as_str(), sent.target.as_str(), sent.live), ("storyfirewall", "10.0.0.5:80", false));
        assert_eq!(get(id).unwrap()["seq"], 1);

        // 기다리는 접속이 있으면 그 접속으로 한 줄이 간다.
        let mut rx = subscribe();
        assert!(join(id));
        let sent = publish(id, &record("y"), 2).unwrap();
        assert!(sent.live);
        let (to, line) = loop {
            let got = rx.try_recv().unwrap();
            if got.0 == id {
                break got;
            }
        };
        assert_eq!(to, id);
        assert!(line.ends_with('\n') && line.len() == sent.bytes);
        let note: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(note["method"], "rack.result");
        assert!(note.get("id").is_none(), "알림에는 id가 없다");
        assert_eq!(note["params"]["seq"], 2);
        leave(&[id.to_string()]);
        assert!(!publish(id, &record("z"), 3).unwrap().live);
        assert!(!join("00000000-0000-4000-8000-00000000ffff"));
    }
}
