//! PC 밖으로 나가는 한 건의 모양과, 나가기 전에 가리는 규칙.
//!
//! 한 건 = 끝난 진단 하나. OCSF의 Scan Activity(class_uid 6007, 끝남 = activity_id 2)에 맞추고,
//! NetX만의 값(대상, 단계별 판정)은 `unmapped.netx`에 싣는다. 원본 출력은 싣지 않는다.
//! 받은 쪽은 이 글자들을 자료로만 다룬다: 제어 문자와 글자 방향을 바꾸는 문자를 지우고 길이를 자른다.

use std::net::{Ipv4Addr, Ipv6Addr};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;

/// 이 규격의 판
pub const VERSION: u32 = 1;
/// 한 건의 상한(줄 하나). 넘으면 보내는 쪽은 단계를 줄이고, 받는 쪽은 끊는다.
pub const MAX_ITEM_BYTES: usize = 128 * 1024;
pub const MAX_STEPS: usize = 40;
const MAX_NOTES: usize = 20;

/// 끝난 진단 한 건. NetX가 넘겨주는 것은 이것뿐이다.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Finished {
    /// 진단 종류(레시피 id)
    pub kind: String,
    pub title: String,
    /// 대상(주소·이름). 없을 수 있다.
    pub target: String,
    /// ok | caution | danger | unknown
    pub verdict: String,
    pub summary: Vec<String>,
    /// 1970년부터의 밀리초
    pub started: u64,
    pub ended: u64,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub order: u32,
    pub title: String,
    pub tool: String,
    pub command: String,
    /// ok | timeout | cancelled | capped | error | skipped
    pub state: String,
    pub exit: Option<i64>,
    pub secs: Option<f64>,
    pub verdict: String,
    pub notes: Vec<String>,
}

/// 화면과 보고서에 글자로만 보일 문자열로 다듬는다: 제어 문자, 글자 방향을 바꾸는 문자, 보이지 않는 문자를 지우고 자른다.
pub fn text(s: &str, max_chars: usize) -> String {
    s.chars().filter(|c| !bad_char(*c)).take(max_chars).collect::<String>().trim().to_string()
}

fn bad_char(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{061C}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E007F}')
}

pub fn verdict(s: &str) -> &'static str {
    match s {
        "ok" => "ok",
        "caution" => "caution",
        "danger" => "danger",
        _ => "unknown",
    }
}

fn state(s: &str) -> &'static str {
    match s {
        "ok" => "ok",
        "timeout" => "timeout",
        "cancelled" => "cancelled",
        "capped" => "capped",
        "error" => "error",
        _ => "skipped",
    }
}

/// 받은 것이든 보낼 것이든 같은 규칙으로 다듬는다.
pub fn tidy(f: Finished) -> Finished {
    Finished {
        kind: text(&f.kind, 60),
        title: text(&f.title, 120),
        target: text(&f.target, 255),
        verdict: verdict(&f.verdict).into(),
        summary: f.summary.iter().take(5).map(|s| text(s, 600)).filter(|s| !s.is_empty()).collect(),
        started: f.started,
        ended: f.ended,
        steps: f
            .steps
            .into_iter()
            .take(MAX_STEPS)
            .map(|s| Step {
                order: s.order.min(999),
                title: text(&s.title, 120),
                tool: text(&s.tool, 40),
                command: text(&s.command, 1000),
                state: state(&s.state).into(),
                exit: s.exit,
                secs: s.secs.filter(|v| v.is_finite() && *v >= 0.0),
                verdict: verdict(&s.verdict).into(),
                notes: s.notes.iter().take(MAX_NOTES).map(|n| text(n, 600)).filter(|n| !n.is_empty()).collect(),
            })
            .collect(),
    }
}

// ---- 가리기 ----

/// 보내기 전에 가리는 것: 내부 주소, MAC 뒷부분, PC 이름, 사용자 이름, 이 PC의 공인 주소(상세 보고서와 같은 범위).
/// 같은 내부 주소는 이 PC에서 언제나 같은 가명(`내부-ab12cd`)이 된다. 가명은 PC마다 다른 비밀 값으로 만들어
/// 받은 쪽이 원래 주소를 되짚을 수 없고, 다른 PC의 같은 주소와도 겹치지 않는다.
pub struct Mask {
    pub on: bool,
    secret: Vec<u8>,
    host: String,
    user: String,
    public: Option<String>,
}

fn inner_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || ip.is_link_local() || ip.is_loopback() || (o[0] == 100 && o[1] & 0xc0 == 64)
}

fn inner_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `keep`에 드는 글자가 이어진 토막마다 `f`를 불러, 바꿀 글이 있으면 바꾼다.
fn runs(text: &str, keep: impl Fn(char) -> bool, f: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once('\0')) {
        if c != '\0' && keep(c) {
            run.push(c);
            continue;
        }
        if !run.is_empty() {
            out.push_str(&f(&run).unwrap_or_else(|| run.clone()));
            run.clear();
        }
        if c != '\0' {
            out.push(c);
        }
    }
    out
}

/// 낱말로 따로 서 있는 `name`만 `to`로 바꾼다(영문 대소문자는 가리지 않는다).
fn replace_word(text: &str, name: &str, to: &str) -> String {
    if name.chars().count() < 3 {
        return text.to_string();
    }
    let (low, needle) = (text.to_ascii_lowercase(), name.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = low.get(at..).and_then(|rest| rest.find(&needle)) {
        let (start, end) = (at + found, at + found + needle.len());
        let before = text.get(..start).and_then(|b| b.chars().next_back());
        let after = text.get(end..).and_then(|a| a.chars().next());
        let glued = |edge: Option<char>, near: Option<char>| edge.is_some_and(is_word) && near.is_some_and(is_word);
        let alone = !glued(name.chars().next(), before) && !glued(name.chars().next_back(), after);
        out.push_str(text.get(at..start).unwrap_or(""));
        out.push_str(if alone { to } else { text.get(start..end).unwrap_or("") });
        at = end;
    }
    out.push_str(text.get(at..).unwrap_or(""));
    out
}

impl Mask {
    pub fn new(on: bool, secret: &[u8], host: &str, user: &str, public: Option<&str>) -> Mask {
        Mask { on, secret: secret.to_vec(), host: host.trim().into(), user: user.trim().into(), public: public.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) }
    }

    /// 내부 주소의 가명. 이 PC 안에서는 같은 주소가 언제나 같은 값이다.
    pub fn alias(&self, addr: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC은 어떤 길이의 키도 받는다");
        mac.update(b"netx-collect-alias/1\0");
        mac.update(addr.to_ascii_lowercase().as_bytes());
        let tag = mac.finalize().into_bytes();
        format!("내부-{:02x}{:02x}{:02x}", tag[0], tag[1], tag[2])
    }

    pub fn text(&self, text: &str) -> String {
        if !self.on {
            return text.to_string();
        }
        let out = runs(text, |c| c.is_ascii_digit() || c == '.', |run| {
            let core = run.trim_matches('.');
            let ip: Ipv4Addr = core.parse().ok()?;
            let new = if self.public.as_deref() == Some(core) {
                "내 공인 IP".to_string()
            } else if inner_v4(ip) {
                self.alias(core)
            } else {
                return None;
            };
            Some(run.replacen(core, &new, 1))
        });
        let out = runs(&out, |c| c.is_ascii_hexdigit() || c == ':' || c == '-', |run| {
            for sep in [':', '-'] {
                let parts: Vec<&str> = run.split(sep).collect();
                if parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit())) {
                    return Some(format!("{}{sep}{}{sep}{}{sep}··{sep}··{sep}··", parts[0], parts[1], parts[2]));
                }
            }
            let ip: Ipv6Addr = run.parse().ok()?;
            inner_v6(ip).then(|| self.alias(&ip.to_string()))
        });
        let out = replace_word(&out, &self.host, "이 PC");
        replace_word(&out, &self.user, "사용자")
    }

    pub fn apply(&self, f: Finished) -> Finished {
        if !self.on {
            return f;
        }
        let t = |s: &str| self.text(s);
        Finished {
            kind: f.kind,
            title: t(&f.title),
            target: t(&f.target),
            verdict: f.verdict,
            summary: f.summary.iter().map(|s| t(s)).collect(),
            started: f.started,
            ended: f.ended,
            steps: f.steps.into_iter().map(|s| Step { title: t(&s.title), command: t(&s.command), notes: s.notes.iter().map(|n| t(n)).collect(), ..s }).collect(),
        }
    }
}

// ---- OCSF ----

fn severity(v: &str) -> u8 {
    match v {
        "ok" => 1,
        "caution" => 3,
        "danger" => 4,
        _ => 0,
    }
}

/// 보내는 모양(OCSF Scan Activity). `pc`는 보낸 PC의 id, `seq`는 그 PC의 순번.
pub fn to_ocsf(f: &Finished, pc: &str, seq: u64, masked: bool, product_version: &str) -> Value {
    let bad = f.steps.iter().filter(|s| matches!(s.verdict.as_str(), "caution" | "danger")).count();
    json!({
        "category_uid": 6, "class_uid": 6007, "activity_id": 2, "type_uid": 600702,
        "severity_id": severity(&f.verdict),
        "time": f.ended, "start_time": f.started, "end_time": f.ended,
        "duration": f.ended.saturating_sub(f.started),
        "message": f.summary.join(" / "),
        "num_detections": bad,
        "metadata": {
            "version": "1.3.0",
            "product": {"name": "NetX", "vendor_name": "Antioch", "version": product_version},
            "uid": format!("{pc}:{seq}"),
            "sequence": seq,
        },
        "device": {"uid": pc},
        "scan": {"name": f.title, "uid": f.kind, "type_id": 99, "type": "network diagnosis"},
        "unmapped": {"netx": {"v": VERSION, "kind": f.kind, "target": f.target, "verdict": f.verdict, "masked": masked, "summary": f.summary, "steps": f.steps}},
    })
}

/// 받은 한 건에서 꺼낸 것. 모든 글자는 다듬은 뒤다.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub at: u64,
    pub masked: bool,
    pub item: Finished,
}

/// 받은 OCSF 한 건을 읽는다. 밖에서 온 자료이므로 `get`으로만 읽고, 모양이 아니면 없음.
pub fn from_ocsf(body: &Value) -> Option<Parsed> {
    if body.get("class_uid").and_then(Value::as_u64) != Some(6007) {
        return None;
    }
    let netx = body.get("unmapped")?.get("netx")?;
    if netx.get("v").and_then(Value::as_u64) != Some(u64::from(VERSION)) {
        return None;
    }
    let s = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_string();
    let list = |v: Option<&Value>, max: usize| v.and_then(Value::as_array).map(|a| a.iter().take(max).filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let steps = netx
        .get("steps")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(MAX_STEPS)
                .map(|st| Step {
                    order: st.get("order").and_then(Value::as_u64).unwrap_or(0).min(999) as u32,
                    title: s(st.get("title")),
                    tool: s(st.get("tool")),
                    command: s(st.get("command")),
                    state: s(st.get("state")),
                    exit: st.get("exit").and_then(Value::as_i64),
                    secs: st.get("secs").and_then(Value::as_f64),
                    verdict: s(st.get("verdict")),
                    notes: list(st.get("notes"), MAX_NOTES),
                })
                .collect()
        })
        .unwrap_or_default();
    let ended = body.get("time").and_then(Value::as_u64)?;
    let item = tidy(Finished {
        kind: s(netx.get("kind")),
        title: s(body.get("scan").and_then(|sc| sc.get("name"))),
        target: s(netx.get("target")),
        verdict: s(netx.get("verdict")),
        summary: list(netx.get("summary"), 5),
        started: body.get("start_time").and_then(Value::as_u64).unwrap_or(ended),
        ended,
        steps,
    });
    Some(Parsed { at: ended, masked: netx.get("masked").and_then(Value::as_bool).unwrap_or(false), item })
}

/// 한 줄로 만든다. 상한을 넘으면 뒤 단계부터 줄이고, 그래도 넘으면 없음.
pub fn encode(f: &Finished, pc: &str, seq: u64, masked: bool, product_version: &str) -> Option<String> {
    let mut f = f.clone();
    loop {
        let line = to_ocsf(&f, pc, seq, masked, product_version).to_string();
        if line.len() <= MAX_ITEM_BYTES - 512 {
            return Some(line);
        }
        f.steps.pop()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Finished {
        Finished {
            kind: "route_latency".into(),
            title: "라우팅 지연 분석".into(),
            target: "192.168.123.1".into(),
            verdict: "caution".into(),
            summary: vec!["3번째 구간(10.0.0.1)에서 지연이 큽니다".into()],
            started: 1_763_000_000_000,
            ended: 1_763_000_004_000,
            steps: vec![Step { order: 1, title: "경로 추적".into(), tool: "tracert".into(), command: "tracert -d 192.168.123.1".into(), state: "ok".into(), exit: Some(0), secs: Some(3.2), verdict: "caution".into(), notes: vec!["MYPC에서 aa:bb:cc:dd:ee:ff 로".into()] }],
        }
    }

    #[test]
    fn round_trip_keeps_the_item() {
        let f = tidy(sample());
        let line = encode(&f, "pc1", 7, false, "0.1.0").unwrap();
        let back = from_ocsf(&serde_json::from_str(&line).unwrap()).unwrap();
        assert_eq!(back.item, f);
        assert_eq!(back.at, f.ended);
        assert!(!back.masked);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["class_uid"], 6007);
        assert_eq!(v["metadata"]["uid"], "pc1:7");
        assert_eq!(v["severity_id"], 3);
        assert_eq!(v["num_detections"], 1);
    }

    #[test]
    fn masking_hides_inner_addresses_with_stable_aliases() {
        let m = Mask::new(true, b"secret-a", "MYPC", "kim", Some("203.0.113.9"));
        let out = m.apply(sample());
        assert!(!out.target.contains("192.168"));
        assert!(out.target.starts_with("내부-"));
        // 같은 주소는 같은 가명
        assert!(out.steps[0].command.contains(&out.target));
        assert!(!out.summary[0].contains("10.0.0.1"));
        assert_eq!(out.steps[0].notes[0], "이 PC에서 aa:bb:cc:··:··:·· 로");
        assert_eq!(m.text("8.8.8.8 과 203.0.113.9"), "8.8.8.8 과 내 공인 IP");
        assert_eq!(m.text("fe80::1 그리고 2606:4700::1111"), format!("{} 그리고 2606:4700::1111", m.alias("fe80::1")));
        // 다른 PC는 같은 주소라도 가명이 다르다
        let other = Mask::new(true, b"secret-b", "", "", None);
        assert_ne!(m.alias("192.168.123.1"), other.alias("192.168.123.1"));
        // 꺼져 있으면 그대로
        assert_eq!(Mask::new(false, b"s", "MYPC", "kim", None).apply(sample()), sample());
        // 낱말 안의 일부는 건드리지 않는다
        assert_eq!(m.text("kimchi와 kim"), "kimchi와 사용자");
    }

    #[test]
    fn text_drops_control_and_direction_characters() {
        assert_eq!(text("a\u{202E}b\x1b[31mc\u{200B}\n", 10), "ab[31mc");
        assert_eq!(text("가나다라", 2), "가나");
    }

    #[test]
    fn malformed_items_are_refused_not_panicked() {
        for raw in ["null", "[]", "{}", r#"{"class_uid":6007}"#, r#"{"class_uid":6007,"unmapped":{"netx":{"v":99}},"time":1}"#, r#"{"class_uid":6007,"unmapped":{"netx":{"v":1}}}"#, r#"{"class_uid":"6007","unmapped":7}"#] {
            assert!(from_ocsf(&serde_json::from_str(raw).unwrap()).is_none(), "{raw}");
        }
        // 틀린 형의 값은 빈 값으로 읽는다
        let odd = json!({"class_uid": 6007, "time": 5, "scan": 3, "unmapped": {"netx": {"v": 1, "target": 9, "verdict": "x", "steps": [1, {"order": "a", "notes": [1, "n\u{0007}"]}], "summary": "s"}}});
        let p = from_ocsf(&odd).unwrap();
        assert_eq!(p.item.verdict, "unknown");
        assert_eq!(p.item.steps.len(), 2);
        assert_eq!(p.item.steps[1].notes, vec!["n".to_string()]);
    }

    #[test]
    fn oversized_items_lose_steps_first() {
        let mut f = sample();
        f.steps = (0..MAX_STEPS as u32).map(|i| Step { order: i, command: "x".repeat(1000), notes: vec!["가".repeat(600); MAX_NOTES], ..Default::default() }).collect();
        let f = tidy(f);
        let line = encode(&f, "pc", 1, true, "0").unwrap();
        assert!(line.len() <= MAX_ITEM_BYTES);
        let back = from_ocsf(&serde_json::from_str(&line).unwrap()).unwrap();
        assert!(back.item.steps.len() < MAX_STEPS && !back.item.steps.is_empty());
    }
}
