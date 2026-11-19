//! NetX(Tauri)와 관제 PC 모듈이 만나는 단 하나의 자리.
//! 여기서만 NetX의 것(자료 폴더, 감사 로그, 공인 주소, [최근 진단] 기록의 모양)을 쓴다. 관제로 뗄 때 이 파일만 바꾼다.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use super::store::{Alert, Conn, Filter, Gap, Hour, Pc, Row, Sent, Transfer};
use super::wire::{Finished, Step, text};
use super::{Config, Hub, Join, Status, WindowView, lock, net, store};

/// 모듈을 열지 못했으면(기기 키를 저장하지 못함 등) 그 까닭
pub struct Shared(pub Result<Arc<Hub>, String>);

fn hub(shared: &State<'_, Shared>) -> Result<Arc<Hub>, String> {
    shared.0.clone()
}

fn names() -> (String, String) {
    let var = |keys: &[&str]| keys.iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.trim().is_empty()).unwrap_or_default();
    let mut host = var(&["COMPUTERNAME", "HOSTNAME"]);
    if host.is_empty() {
        host = std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_default();
    }
    (host, var(&["USERNAME", "USER", "LOGNAME"]))
}

/// 앱을 켤 때 한 번: 모듈을 열고, 받는 쪽과 보내는 쪽 일을 띄운다(둘 다 설정이 꺼져 있으면 아무것도 하지 않고 기다린다).
pub fn start(app: &AppHandle) {
    let opened = app.path().app_local_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했습니다: {e}")).and_then(|dir| {
        let (host, user) = names();
        Hub::open(&dir.join("collect"), &app.package_info().version.to_string(), &host, &user).map(Arc::new)
    });
    if let Ok(hub) = &opened {
        tauri::async_runtime::spawn(net::run_listener(hub.clone()));
        tauri::async_runtime::spawn(net::run_sender(hub.clone()));
    }
    app.manage(Shared(opened));
}

/// 감사 로그에 한 줄: 누가 무엇을 바꿨는지(내용은 남기지 않는다).
fn audit(app: &AppHandle, what: &str, detail: &[String]) {
    if let Ok(dir) = app.path().app_local_data_dir() {
        crate::audit::log(&dir, &crate::audit::AuditEntry { time: crate::audit::now(), run_id: 0, level: crate::model::Level::General, source: "collect", tool: what, argv: detail, risk: crate::model::Risk::Safe, root: false, confirmed: true });
    }
}

fn verdict(light: Option<&Value>) -> &'static str {
    match light.and_then(Value::as_str) {
        Some("green") => "ok",
        Some("yellow") => "caution",
        Some("red") => "danger",
        _ => "unknown",
    }
}

fn worse(a: &'static str, b: &'static str) -> &'static str {
    let rank = |v: &str| match v {
        "danger" => 3,
        "caution" => 2,
        "ok" => 1,
        _ => 0,
    };
    if rank(b) > rank(a) { b } else { a }
}

/// NetX의 진단 기록([최근 진단]에 남기는 것과 같은 모양)에서 보낼 한 건을 꺼낸다. 원본 출력은 꺼내지 않는다.
/// 기록 안의 글자는 밖에서 온 것이므로 `get`으로만 읽는다. 다른 PC에서 가져온 기록은 보내지 않는다.
pub fn finished_from_record(record: &Value, now: u64) -> Option<Finished> {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_string();
    let meta = record.get("meta")?;
    if meta.get("imported").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let run = record.get("run")?;
    let planned = record.get("plan")?.get("steps")?.as_array()?;
    let mut steps = Vec::new();
    let (mut summary, mut calm) = (Vec::new(), Vec::new());
    let mut overall = "unknown";
    let mut started: Option<u64> = None;
    for step in planned.iter().take(super::wire::MAX_STEPS) {
        let Some(order) = step.get("order").and_then(Value::as_u64) else { continue };
        let key = order.to_string();
        let state = run.get("steps").and_then(|st| st.get(&key));
        let result = run.get("results").and_then(|r| r.get(&key));
        let status = match state.and_then(|st| st.get("status")).and_then(Value::as_str) {
            Some("done") => "ok",
            Some("timeout") => "timeout",
            Some("cancelled") => "cancelled",
            Some("capped") => "capped",
            Some("error") => "error",
            _ => "skipped",
        };
        if let Some(at) = state.and_then(|st| st.get("started_at")).and_then(Value::as_u64).filter(|at| *at > 0) {
            // 화면은 밀리초로 적는다. 초로 적힌 옛 기록도 받는다.
            let at = if at < 100_000_000_000 { at * 1000 } else { at };
            started = Some(started.map_or(at, |old| old.min(at)));
        }
        let v = verdict(result.and_then(|r| r.get("light")));
        overall = worse(overall, v);
        let mut notes = Vec::new();
        let note = text(&s(result.and_then(|r| r.get("text"))), 600);
        if !note.is_empty() {
            if matches!(v, "caution" | "danger") { &mut summary } else { &mut calm }.push(note.clone());
            notes.push(note);
        }
        for fact in result.and_then(|r| r.get("facts")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(19) {
            let (label, value) = (text(&s(fact.get("label")), 80), text(&s(fact.get("value")), 300));
            if !label.is_empty() || !value.is_empty() {
                notes.push(format!("{label}: {value}"));
            }
        }
        steps.push(Step {
            order: order.min(999) as u32,
            title: s(step.get("title")),
            tool: s(step.get("tool")),
            command: s(step.get("display")),
            state: status.into(),
            exit: state.and_then(|st| st.get("exit_code")).and_then(Value::as_i64),
            secs: state.and_then(|st| st.get("elapsed_ms")).and_then(Value::as_f64).map(|ms| (ms / 100.0).round() / 10.0),
            verdict: v.into(),
            notes,
        });
    }
    if steps.is_empty() {
        return None;
    }
    if summary.is_empty() {
        summary = calm;
    }
    let from_meta = verdict(meta.get("light"));
    Some(Finished {
        kind: s(record.get("recipe_id")),
        title: s(meta.get("title")),
        target: s(meta.get("target")),
        verdict: if from_meta == "unknown" { overall } else { from_meta }.into(),
        summary,
        started: started.unwrap_or(now).min(now),
        ended: now,
        steps,
    })
}

#[derive(Serialize)]
pub struct Overview {
    pub config: Config,
    /// 이 PC의 id(공개 키 지문)
    pub id: String,
    pub status: Status,
    pub window: WindowView,
    pub join: Join,
    /// 보낼 함에서 기다리는 건수
    pub waiting: u64,
    /// 이 PC의 테일스케일 주소(있으면)
    pub tailnet: Option<String>,
    pub limits: Limits,
}

/// 화면에 적어 보이는 한도
#[derive(Serialize)]
pub struct Limits {
    pub out_queue: u64,
    pub out_keep_days: u64,
    pub hour_items: u64,
    pub pc_items: u64,
    pub keep_days_max: u32,
}

#[tauri::command(async)]
pub fn collect_overview(shared: State<'_, Shared>) -> Result<Overview, String> {
    let hub = hub(&shared)?;
    Ok(Overview {
        config: hub.config(),
        id: hub.me.id(),
        status: lock(&hub.status).clone(),
        window: hub.window_view(),
        join: lock(&hub.join).clone(),
        waiting: hub.outbox.waiting(),
        tailnet: net::tailnet_addr().map(|a| a.to_string()),
        limits: Limits { out_queue: store::OUT_MAX_QUEUE, out_keep_days: store::OUT_KEEP / 86_400_000, hour_items: store::HOUR_MAX_ITEMS, pc_items: store::PC_MAX_ITEMS, keep_days_max: store::KEEP_DAYS_MAX },
    })
}

#[tauri::command(async)]
pub fn collect_set(app: AppHandle, shared: State<'_, Shared>, send: bool, mask: bool, receive: bool, name: String, keep_days: u32) -> Result<Config, String> {
    let hub = hub(&shared)?;
    let before = hub.config();
    let after = hub.set(send, mask, receive, &name, keep_days)?;
    let onoff = |b: bool| if b { "켬" } else { "끔" };
    if (before.send, before.mask, before.receive) != (after.send, after.mask, after.receive) {
        audit(&app, "settings", &[format!("보내기 {}", onoff(after.send)), format!("가리기 {}", onoff(after.mask)), format!("관제 PC로 쓰기 {}", onoff(after.receive))]);
    }
    Ok(after)
}

/// 화면이 끝난 진단의 기록을 넘긴다. 보내기도 받기도 꺼져 있으면 아무것도 하지 않는다.
#[tauri::command]
pub async fn collect_submit(shared: State<'_, Shared>, record: Value) -> Result<Option<u64>, String> {
    let hub = hub(&shared)?;
    let c = hub.config();
    if !c.receive && !(c.send && c.collector.is_some()) {
        return Ok(None);
    }
    let Some(f) = finished_from_record(&record, super::now_ms()) else { return Ok(None) };
    let public = if c.mask { crate::netinfo::current().await.public_ip } else { None };
    hub.submit(f, public.as_deref())
}

// ---- 보내는 쪽 ----

#[tauri::command]
pub async fn collect_join(app: AppHandle, shared: State<'_, Shared>, addr: String, port: Option<u16>, code: String) -> Result<Join, String> {
    let hub = hub(&shared)?;
    let port = port.filter(|p| *p >= 1024).unwrap_or(super::PORT_DEFAULT);
    let joined = net::join(hub, &addr, port, &code).await;
    audit(&app, "pair.join", &[text(&addr, 45), if joined.is_ok() { "코드 맞음" } else { "실패" }.into()]);
    joined
}

/// 승인을 기다리다 그만둔다(확인 숫자가 다를 때).
#[tauri::command(async)]
pub fn collect_join_cancel(shared: State<'_, Shared>) -> Result<(), String> {
    *lock(&hub(&shared)?.join) = Join::Idle;
    Ok(())
}

/// 관제 PC와의 짝을 끊는다. 보내기도 꺼진다.
#[tauri::command(async)]
pub fn collect_forget(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    hub(&shared)?.forget_collector()?;
    audit(&app, "pair.forget", &[]);
    Ok(())
}

/// 나간 건의 목록
#[tauri::command(async)]
pub fn collect_sent(shared: State<'_, Shared>) -> Result<Vec<Sent>, String> {
    Ok(hub(&shared)?.outbox.log())
}

// ---- 관제 PC 쪽 ----

#[tauri::command(async)]
pub fn collect_pair_open(app: AppHandle, shared: State<'_, Shared>) -> Result<WindowView, String> {
    let view = hub(&shared)?.open_window()?;
    audit(&app, "pair.open", &[]);
    Ok(view)
}

#[tauri::command(async)]
pub fn collect_pair_close(shared: State<'_, Shared>) -> Result<(), String> {
    hub(&shared)?.close_window();
    Ok(())
}

#[tauri::command(async)]
pub fn collect_pair_decide(app: AppHandle, shared: State<'_, Shared>, approve: bool) -> Result<(), String> {
    let hub = hub(&shared)?;
    let who = hub.window_view().pending.map(|p| format!("{} ({})", p.name, p.addr)).unwrap_or_default();
    hub.decide(approve)?;
    audit(&app, if approve { "pair.approve" } else { "pair.refuse" }, &[who]);
    Ok(())
}

#[tauri::command(async)]
pub fn collect_pc_set(app: AppHandle, shared: State<'_, Shared>, id: String, state: String) -> Result<(), String> {
    hub(&shared)?.set_pc(&id, &state)?;
    audit(&app, "pc.state", &[id.chars().take(16).collect(), state]);
    Ok(())
}

#[derive(Serialize)]
pub struct Board {
    pub pcs: Vec<Pc>,
    pub kinds: Vec<String>,
    pub gaps: Vec<Gap>,
    pub alerts: Vec<Alert>,
    /// 받지 않던 구간(시작, 끝)
    pub downtime: Vec<(u64, u64)>,
    /// 짝이 아닌 곳에서 온 접속(최근 것부터)
    pub strangers: Vec<Conn>,
}

/// 거점 하나의 상세: 접속 내역, 전송 내역, 7일 시간표, 이 거점의 빠진 구간과 경고
#[derive(Serialize)]
pub struct Site {
    pub pc: Pc,
    pub conns: Vec<Conn>,
    pub transfers: Vec<Transfer>,
    pub timeline: Vec<Hour>,
    pub gaps: Vec<Gap>,
    pub alerts: Vec<Alert>,
}

/// 거점 상세. `id`는 공개 키 지문(16진수)이어야 한다.
#[tauri::command(async)]
pub fn collect_site(shared: State<'_, Shared>, id: String) -> Result<Site, String> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("거점을 알 수 없습니다.".into());
    }
    let hub = hub(&shared)?;
    let now = super::now_ms();
    let pc = hub.inbox.pcs(now).into_iter().find(|p| p.id == id).ok_or("거점을 알 수 없습니다.")?;
    Ok(Site {
        conns: hub.inbox.conns(Some(&id), 300),
        transfers: hub.inbox.transfers(&id, 300),
        timeline: hub.inbox.timeline(&id, now),
        gaps: hub.inbox.gaps().into_iter().filter(|g| g.pc == id).collect(),
        alerts: hub.inbox.alerts().into_iter().filter(|a| a.pc == id).collect(),
        pc,
    })
}

#[tauri::command(async)]
pub fn collect_board(shared: State<'_, Shared>) -> Result<Board, String> {
    let hub = hub(&shared)?;
    Ok(Board { pcs: hub.inbox.pcs(super::now_ms()), kinds: hub.inbox.kinds(), gaps: hub.inbox.gaps(), alerts: hub.inbox.alerts(), downtime: hub.inbox.downtime(), strangers: hub.inbox.conns(None, 50) })
}

#[derive(Serialize)]
pub struct Page {
    pub rows: Vec<Row>,
    pub total: u64,
    pub page: u32,
}

#[tauri::command(async)]
pub fn collect_list(shared: State<'_, Shared>, filter: Filter) -> Result<Page, String> {
    let (rows, total) = hub(&shared)?.inbox.list(&filter);
    Ok(Page { rows, total, page: store::PAGE })
}

#[tauri::command(async)]
pub fn collect_item(shared: State<'_, Shared>, pc: String, seq: u64) -> Result<Option<Finished>, String> {
    Ok(hub(&shared)?.inbox.item(&pc, seq))
}

#[tauri::command(async)]
pub fn collect_analysis(shared: State<'_, Shared>) -> Result<store::Analysis, String> {
    Ok(hub(&shared)?.inbox.analysis(super::now_ms()))
}

/// 상세 보고서의 "관제 PC" 구역에 실을 것: 기간 안의 건(최근 것부터)과 전체 건수. 모듈을 열지 못했으면 빈 목록.
pub fn report_rows(app: &AppHandle, from: u64, to: u64, max: usize) -> (Vec<crate::report::Collected>, usize) {
    let Some(hub) = app.try_state::<Shared>().and_then(|s| s.0.clone().ok()) else { return (Vec::new(), 0) };
    let (rows, total) = hub.inbox.report(from, to, max);
    let rows = rows.into_iter().map(|(r, summary)| crate::report::Collected { at: r.at, got: r.got, pc: if r.pc_name.is_empty() { r.pc.chars().take(8).collect() } else { r.pc_name }, kind: r.kind, title: r.title, target: r.target, verdict: r.verdict, masked: r.masked, skew: r.skew, summary }).collect();
    (rows, total as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record() -> Value {
        json!({
            "meta": {"title": "라우팅 지연 분석", "recipe": "route_latency", "target": "example.com", "light": "yellow", "steps": 2},
            "recipe_id": "route_latency",
            "inputs": {"target": "example.com"},
            "plan": {"steps": [
                {"order": 1, "title": "경로 추적", "tool": "tracert", "display": "tracert -d example.com", "risk": "safe"},
                {"order": 2, "title": "핑", "tool": "ping", "display": "ping -n 4 example.com", "risk": "safe"},
                {"title": "순번 없음"}
            ]},
            "run": {
                "steps": {"1": {"status": "done", "exit_code": 0, "elapsed_ms": 3210, "started_at": 1_763_000_000_000u64}, "2": {"status": "timeout"}},
                "results": {"1": {"light": "yellow", "text": "3번째 구간에서 지연", "facts": [{"label": "홉", "value": "9"}]}, "2": {"light": "green", "text": "응답 있음"}},
                "lines": [{"order": 1, "line": "비밀 원본 출력"}]
            }
        })
    }

    #[test]
    fn record_becomes_an_item_without_raw_output() {
        let now = 1_763_000_010_000;
        let f = finished_from_record(&record(), now).unwrap();
        assert_eq!((f.kind.as_str(), f.title.as_str(), f.target.as_str(), f.verdict.as_str()), ("route_latency", "라우팅 지연 분석", "example.com", "caution"));
        assert_eq!((f.started, f.ended), (1_763_000_000_000, now));
        assert_eq!(f.summary, vec!["3번째 구간에서 지연".to_string()]);
        assert_eq!(f.steps.len(), 2);
        assert_eq!(f.steps[0].notes, vec!["3번째 구간에서 지연".to_string(), "홉: 9".to_string()]);
        assert_eq!((f.steps[0].state.as_str(), f.steps[0].exit, f.steps[0].secs), ("ok", Some(0), Some(3.2)));
        assert_eq!(f.steps[1].state, "timeout");
        assert!(!serde_json::to_string(&f).unwrap().contains("비밀 원본 출력"));
    }

    #[test]
    fn imported_or_shapeless_records_are_not_sent() {
        let mut imported = record();
        imported["meta"]["imported"] = json!(true);
        assert!(finished_from_record(&imported, 1).is_none());
        for raw in [json!(null), json!({}), json!({"meta": {}, "run": {}, "plan": {"steps": "x"}}), json!({"meta": {}, "run": {}, "plan": {"steps": []}}), json!({"meta": 1, "run": [], "plan": {"steps": [{"order": -1}]}})] {
            assert!(finished_from_record(&raw, 1).is_none(), "{raw}");
        }
        // 판정이 기록에 없으면 단계에서 가장 나쁜 것
        let mut bare = record();
        bare["meta"]["light"] = json!("info");
        assert_eq!(finished_from_record(&bare, 1).unwrap().verdict, "caution");
    }
}
