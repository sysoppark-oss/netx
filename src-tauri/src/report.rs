//! 보고서 만들기: 고른 구역을 브라우저로 열리는 보고서(HTML) 한 파일로 만든다.
//! 근거: docs/REPORT_FEATURE_REQUEST_v1.md 1·2·4장.
//!
//! - 보고서에 들어가는 글자는 장비·서버가 보낸 것이 섞여 있다. 모두 바꿔 쓰고(`esc`), 스크립트·링크·바깥 자원을 넣지 않는다.
//! - 가리기는 글자를 바꿔 쓰기 **전에** 한다(꼬리표나 자료의 이름표를 건드리지 않게).
//! - 암호·커뮤니티 문자열·키는 애초에 싣지 않는다. 말씀 표시도 넣지 않는다(내보내는 파일).
//! - 여기 함수들은 파일을 쓰지 않는다. 저장은 `commands.rs`가 사용자가 고른 자리에 한다.

use std::cell::RefCell;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::Value;

use crate::history::{esc, local_time};

/// 구역마다 싣는 표의 상한(2.7)
pub const MAX_ROWS: usize = 2000;
/// 읽는 감사 로그의 상한. 넘으면 뒤쪽(최근)만 읽는다.
pub const MAX_AUDIT_BYTES: u64 = 16 * 1024 * 1024;

/// 만들 수 있는 구역 (id, 제목). 순서는 메뉴와 같다. 터미널과 원자료 JSON은 3단계.
pub const SECTIONS: [(&str, &str); 6] = [("mynet", "내 네트워크"), ("history", "최근 진단"), ("topology", "구성도"), ("capture", "캡처 분석"), ("manual", "직접 명령"), ("audit", "감사 로그")];
/// 한 보고서에 분석해 싣는 캡처 파일 수
pub const MAX_CAPTURES: usize = 10;

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub sections: Vec<String>,
    /// 기간 (1970년부터의 밀리초). 한 건짜리 보고서에서는 쓰지 않는다.
    pub from_ms: u64,
    pub to_ms: u64,
    pub mask: bool,
    /// 이 PC의 시간대(UTC와의 차이, 분). 시각 표기에만 쓴다.
    pub offset_minutes: i32,
    /// 주면 그 진단 기록 한 건만 담는다.
    #[serde(default)]
    pub history_id: Option<String>,
}

/// 내 네트워크 구역에 싣는 값
#[derive(Debug, Clone, Default)]
pub struct Net {
    pub iface: Option<String>,
    pub ip: Option<String>,
    pub prefix: Option<u8>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub link: Option<String>,
    pub public_ip: Option<String>,
    pub mac: Option<String>,
}

/// 보고서에 담을 자료. 모으는 일은 `commands.rs`가 한다.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// 판 글자 (예: "0.1.0 (3a23d53)")
    pub version: String,
    pub host: String,
    pub user: String,
    pub net: Option<Net>,
    /// (저장 시각, 기록 JSON), 최근 것부터
    pub history: Vec<(u64, String)>,
    /// 감사 로그 파일의 내용
    pub audit: String,
    /// 기간 안의 캡처 파일(최근 것부터, `MAX_CAPTURES`개까지)과 분석 결과. 열지 못했으면 그 까닭
    pub captures: Vec<(crate::capview::CaptureFile, Result<crate::capview::Analysis, String>)>,
    /// 기간 안의 캡처 파일 전체 수
    pub captures_total: usize,
    /// 직접 명령 기록 파일(`manual.jsonl`)의 내용
    pub manual: String,
}

pub struct Built {
    pub html: String,
    /// (구역 id, 기간 안 전체 건수, 실은 건수)
    pub counts: Vec<(String, usize, usize)>,
}

// ---- 가리기 (2.4) ----

/// 남에게 보여 줄 때 가리는 것: 내부 주소, MAC 뒷부분, PC 이름, 사용자 이름, 이 PC의 공인 주소.
/// 같은 내부 주소는 보고서 안에서 같은 번호가 된다. 바깥 목적지 주소와 도메인은 가리지 않는다.
pub struct Masker {
    on: bool,
    seen: RefCell<Vec<String>>,
    host: String,
    user: String,
    public: Option<String>,
    /// 이 PC 어댑터의 주소. 사설 대역이 아니어도(공인 주소를 직접 받은 PC) 가린다.
    own: Option<String>,
}

/// 내부에서만 쓰는 IPv4 대역: 사설, 링크 로컬, 통신사·사설망 공유 대역(100.64.0.0/10 — 테일스케일 주소도 여기다).
fn inner_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || ip.is_link_local() || (o[0] == 100 && o[1] & 0xc0 == 64)
}

/// 영문 낱말의 일부가 될 수 있는 글자. 한글 조사는 이름에 바로 붙으므로("MYPC에서") 낱말의 일부로 보지 않는다.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `name`이 낱말로 따로 서 있는 자리만 `to`로 바꾼다(영문 대소문자는 가리지 않는다).
fn replace_word(text: &str, name: &str, to: &str, only_after_path: bool) -> String {
    if name.is_empty() {
        return text.to_string();
    }
    let (low, needle) = (text.to_ascii_lowercase(), name.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = low.get(at..).and_then(|rest| rest.find(&needle)) {
        let (start, end) = (at + found, at + found + needle.len());
        let before = text.get(..start).unwrap_or("");
        let after = text.get(end..).unwrap_or("");
        let edge = |c: Option<char>, near: Option<char>| c.is_some_and(is_word) && near.is_some_and(is_word);
        let alone = !edge(name.chars().next(), before.chars().next_back()) && !edge(name.chars().next_back(), after.chars().next());
        let in_path = {
            let b = before.to_ascii_lowercase();
            ["users\\", "users\\\\", "users/", "home/"].iter().any(|p| b.ends_with(p))
        };
        out.push_str(text.get(at..start).unwrap_or(""));
        if alone && (in_path || !only_after_path) {
            out.push_str(to);
        } else {
            out.push_str(text.get(start..end).unwrap_or(""));
        }
        at = end;
    }
    out.push_str(text.get(at..).unwrap_or(""));
    out
}

fn mac_parts(run: &str, sep: char) -> Option<Vec<&str>> {
    let parts: Vec<&str> = run.split(sep).collect();
    (parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))).then_some(parts)
}

impl Masker {
    pub fn new(on: bool, host: &str, user: &str, public: Option<&str>) -> Masker {
        let tidy = |s: &str| s.trim().to_string();
        Masker { on, seen: RefCell::new(Vec::new()), host: tidy(host), user: tidy(user), public: public.map(tidy).filter(|p| !p.is_empty()), own: None }
    }

    /// 이 PC 어댑터의 주소를 알려 준다.
    pub fn with_own(mut self, ip: Option<&str>) -> Masker {
        self.own = ip.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        self
    }

    fn inner(&self, addr: &str) -> String {
        let mut seen = self.seen.borrow_mut();
        let key = addr.to_ascii_lowercase();
        let n = match seen.iter().position(|s| *s == key) {
            Some(i) => i + 1,
            None => {
                seen.push(key);
                seen.len()
            }
        };
        format!("내부-{n}")
    }

    /// `keep`에 드는 글자가 이어진 토막마다 `f`를 불러, 바꿀 글이 있으면 바꾼다.
    fn runs(text: &str, keep: impl Fn(char) -> bool, f: impl Fn(&str) -> Option<String>) -> String {
        let mut out = String::with_capacity(text.len());
        let mut run = String::new();
        let flush = |run: &mut String, out: &mut String| {
            if !run.is_empty() {
                match f(run) {
                    Some(new) => out.push_str(&new),
                    None => out.push_str(run),
                }
                run.clear();
            }
        };
        for c in text.chars() {
            if keep(c) {
                run.push(c);
            } else {
                flush(&mut run, &mut out);
                out.push(c);
            }
        }
        flush(&mut run, &mut out);
        out
    }

    /// 주소와 MAC만 가린다(이름은 건드리지 않는다).
    fn addresses(&self, text: &str) -> String {
        // IPv4: 숫자와 점이 이어진 토막에서, 앞뒤의 점을 뗀 것이 주소일 때
        let text = Self::runs(text, |c| c.is_ascii_digit() || c == '.', |run| {
            let core = run.trim_matches('.');
            let ip: Ipv4Addr = core.parse().ok()?;
            let new = if self.public.as_deref() == Some(core) {
                "내 공인 IP".to_string()
            } else if inner_v4(ip) {
                self.inner(core)
            } else if self.own.as_deref() == Some(core) {
                // 어댑터가 공인 주소를 직접 받은 PC
                "내 공인 IP".to_string()
            } else {
                return None;
            };
            Some(run.replacen(core, &new, 1))
        });
        // IPv6의 내부 주소(fc00::/7, fe80::/10)와 MAC(aa:bb:cc:dd:ee:ff)
        let text = Self::runs(&text, |c| c.is_ascii_hexdigit() || c == ':', |run| {
            let core = run.trim_matches(':');
            if let Some(p) = mac_parts(core, ':') {
                return Some(run.replacen(core, &format!("{}:{}:{}:**:**:**", p.first()?, p.get(1)?, p.get(2)?), 1));
            }
            if run.matches(':').count() < 2 {
                return None;
            }
            let ip: Ipv6Addr = run.parse().ok()?;
            let first = ip.segments()[0];
            if self.public.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(run)) {
                Some("내 공인 IP".to_string())
            } else if first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80 {
                Some(self.inner(run))
            } else {
                None
            }
        });
        // Windows식 MAC (AA-BB-CC-DD-EE-FF)
        Self::runs(&text, |c| c.is_ascii_hexdigit() || c == '-', |run| {
            let p = mac_parts(run, '-')?;
            Some(format!("{}-{}-{}-**-**-**", p.first()?, p.get(1)?, p.get(2)?))
        })
    }

    fn names(&self, text: &str) -> String {
        // 짧은 이름은 낱말로 바꾸면 엉뚱한 곳이 바뀐다. 경로(C:\Users\이름) 안에서만 바꾼다.
        let short = |s: &str| s.chars().count() < 3;
        let text = if self.host.is_empty() || short(&self.host) { text.to_string() } else { replace_word(text, &self.host, "PC-1", false) };
        replace_word(&text, &self.user, "사용자", short(&self.user))
    }

    /// 보고서에 실을 글 한 토막을 가린다. 꺼져 있으면 그대로 돌려준다.
    pub fn text(&self, text: &str) -> String {
        if !self.on {
            return text.to_string();
        }
        self.names(&self.addresses(text))
    }

    /// 그림(SVG) 글: 꼬리표 안에서는 주소만, 꼬리표 사이의 글자에서는 이름까지 가린다.
    pub fn markup(&self, svg: &str) -> String {
        if !self.on {
            return svg.to_string();
        }
        let mut out = String::with_capacity(svg.len());
        for (i, piece) in svg.split('<').enumerate() {
            if i == 0 {
                out.push_str(&self.text(piece));
                continue;
            }
            out.push('<');
            match piece.split_once('>') {
                Some((tag, body)) => {
                    out.push_str(&self.addresses(tag));
                    out.push('>');
                    out.push_str(&self.text(body));
                }
                None => out.push_str(&self.addresses(piece)),
            }
        }
        out
    }

    /// 기록 자료(JSON)의 글자 값을 가린다. 이름표(키)와, 프로그램이 뜻으로 읽는 값은 건드리지 않는다.
    pub fn value(&self, v: &mut Value) {
        const FIXED: [&str; 14] = ["light", "status", "recipe", "recipe_id", "stream", "kind", "unit", "risk", "confirm", "tool", "severity", "intent", "type", "engine"];
        if !self.on {
            return;
        }
        match v {
            Value::String(s) => *s = self.text(s),
            Value::Array(items) => items.iter_mut().for_each(|x| self.value(x)),
            Value::Object(map) => {
                for (key, x) in map.iter_mut() {
                    if key == "topo_svg" {
                        if let Value::Object(pics) = x {
                            for pic in pics.values_mut() {
                                if let Value::String(s) = pic {
                                    *s = self.markup(s);
                                }
                            }
                        }
                    } else if !(FIXED.contains(&key.as_str()) && x.is_string()) {
                        self.value(x);
                    }
                }
            }
            _ => {}
        }
    }
}

/// 보고서에 그대로 넣어도 되는 그림(SVG)인지 본다: 그림에 쓰는 꼬리표와 속성만 있고, 짝이 맞고, 바깥을 가리키는 것이 없다.
/// 구성도 그림은 다른 PC가 내보낸 보고서에서 가져온 기록에도 들어 있으므로, 화면이 만든 것이라고 믿지 않는다.
pub fn safe_svg(svg: &str) -> bool {
    const TAGS: [&str; 12] = ["svg", "g", "rect", "line", "polyline", "polygon", "circle", "ellipse", "path", "text", "tspan", "title"];
    const ATTRS: [&str; 30] = [
        "xmlns", "viewbox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "points", "d", "fill", "stroke", "stroke-width", "stroke-dasharray", "stroke-linecap", "font-family", "font-size",
        "font-weight", "text-anchor", "opacity", "transform", "fill-opacity", "stroke-opacity",
    ];
    if crate::commands::check_topology_svg(svg).is_err() {
        return false;
    }
    let low = svg.to_ascii_lowercase();
    if low.contains("url(") || low.contains("javascript:") || svg.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return false;
    }
    let mut depth = 0usize;
    for (i, piece) in low.split('<').enumerate() {
        if i == 0 {
            if !piece.is_empty() {
                return false;
            }
            continue;
        }
        let Some((tag, _)) = piece.split_once('>') else { return false };
        if let Some(name) = tag.strip_prefix('/') {
            if !TAGS.contains(&name.trim()) || depth == 0 {
                return false;
            }
            depth -= 1;
            continue;
        }
        let (tag, closed) = match tag.strip_suffix('/') {
            Some(t) => (t, true),
            None => (tag, false),
        };
        // 따옴표 안을 지우고 이름만 본다.
        let mut bare = String::new();
        let mut quote = false;
        for c in tag.chars() {
            match (c, quote) {
                ('"', _) => quote = !quote,
                (_, false) => bare.push(c),
                _ => {}
            }
        }
        let mut words = bare.split_whitespace();
        if quote || !words.next().is_some_and(|name| TAGS.contains(&name)) {
            return false;
        }
        if !words.all(|w| w.strip_suffix('=').is_some_and(|a| ATTRS.contains(&a))) {
            return false;
        }
        if !closed {
            depth += 1;
        }
    }
    depth == 0
}

// ---- 구역 ----

struct Zone {
    id: &'static str,
    title: &'static str,
    count: usize,
    shown: usize,
    body: String,
    /// 기간이 없는 구역(만든 시각 기준)
    timeless: bool,
}

fn row(label: &str, value: &str) -> String {
    format!("<tr><th>{}</th><td>{}</td></tr>", esc(label), value)
}

fn zone_mynet(net: Option<&Net>, m: &Masker) -> Zone {
    let show = |v: Option<&str>| match v {
        Some(v) if !v.trim().is_empty() => esc(&m.text(v)),
        _ => "<span class=\"m\">확인되지 않음</span>".to_string(),
    };
    let body = match net {
        None => "<p class=\"m\">내 네트워크 정보를 읽지 못했습니다.</p>".to_string(),
        Some(n) => {
            let addr = n.ip.as_ref().map(|ip| match n.prefix {
                Some(p) => format!("{ip}/{p}"),
                None => ip.clone(),
            });
            let dns = (!n.dns.is_empty()).then(|| n.dns.join(", "));
            format!(
                "<table>{}{}{}{}{}{}{}</table>",
                row("어댑터", &show(n.iface.as_deref())),
                row("MAC 주소", &show(n.mac.as_deref())),
                row("내부 주소", &show(addr.as_deref())),
                row("게이트웨이", &show(n.gateway.as_deref())),
                row("DNS", &show(dns.as_deref())),
                row("링크 속도", &show(n.link.as_deref())),
                row("공인 주소", &show(n.public_ip.as_deref())),
            )
        }
    };
    Zone { id: "mynet", title: "내 네트워크", count: 1, shown: 1, body, timeless: true }
}

fn zone_history(entries: &[(u64, String)], offset: i32, m: &Masker) -> (Zone, Vec<Value>) {
    let mut body = String::new();
    let mut data = Vec::new();
    for (time, json) in entries {
        let Ok(mut entry) = serde_json::from_str::<Value>(json) else { continue };
        m.value(&mut entry);
        let Some(html) = crate::history::section(*time, &entry, offset) else { continue };
        body.push_str(&html);
        data.push(serde_json::json!({"time": time, "entry": entry}));
    }
    if data.is_empty() {
        body.push_str("<p class=\"m\">이 기간에 남은 진단 기록이 없습니다.</p>");
    }
    (Zone { id: "history", title: "최근 진단", count: data.len(), shown: data.len(), body, timeless: false }, data)
}

fn source_text(source: &str) -> &'static str {
    match source {
        "recipe" => "진단 실행",
        "ai" => "AI 제안 실행",
        "manual" => "직접 명령",
        "mixed" => "조립한 진단",
        "terminal" => "터미널 접속",
        "bridge" => "연동",
        "export" | "report" => "보고서 만들기",
        "import" => "보고서 가져오기",
        "update" => "업데이트",
        "kill" => "프로세스 종료",
        "install" => "도구 설치",
        _ => "기타",
    }
}

fn risk_text(risk: &str) -> &'static str {
    match risk {
        "safe" => "안전",
        "caution" => "주의",
        "danger" => "위험",
        _ => "",
    }
}

/// 감사 로그 한 줄을 (시각 초, 표의 한 줄)로. 모양이 다른 줄은 버린다.
fn audit_row(line: &str, offset: i32, m: &Masker) -> Option<(u64, String)> {
    let v: Value = serde_json::from_str(line).ok()?;
    let time = v.get("time")?.as_u64()?;
    let text = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("");
    let cut = |s: String| -> String { s.chars().filter(|c| !c.is_control()).take(600).collect() };
    let source = text("source");
    let what = if source == "terminal" {
        let user = text("user");
        format!("{} {}{}", text("kind"), text("target"), if user.is_empty() { String::new() } else { format!(" (사용자 {user})") })
    } else {
        let argv: Vec<&str> = v.get("argv").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        match (source, text("tool")) {
            ("bridge", "rack.handoff") => format!("진단 대상을 넘겨받음: {} (보낸 곳 {})", argv.first().unwrap_or(&""), argv.get(1).unwrap_or(&"")),
            ("bridge", "rack.result") => format!("진단 결과를 돌려줌: {}", argv.join(" ")),
            (_, tool) => format!("{tool} {}", argv.join(" ")),
        }
    };
    let confirmed = v.get("confirmed").and_then(Value::as_bool).unwrap_or(false);
    let root = v.get("root").and_then(Value::as_bool).unwrap_or(false);
    Some((
        time,
        format!(
            "<tr><td class=\"n\">{}</td><td>{}</td><td class=\"k\">{}</td><td>{}</td><td>{}</td></tr>",
            local_time(time.saturating_mul(1000), offset).0,
            esc(source_text(source)),
            esc(&m.text(&cut(what))),
            risk_text(text("risk")),
            match (confirmed, root) {
                (true, true) => "확인함 · 관리자 권한",
                (true, false) => "확인함",
                (false, true) => "관리자 권한",
                (false, false) => "",
            },
        ),
    ))
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

fn kind_text(kind: &str) -> &'static str {
    match kind {
        "router" => "공유기·라우터",
        "pc" => "PC",
        "server" => "서버",
        "printer" => "프린터",
        "camera" => "카메라",
        "phone" => "휴대 기기",
        "network" => "네트워크 장비",
        _ => "알 수 없음",
    }
}

/// 구성도: 기간 안에서 가장 최근에 조사한 것의 그림과 장비 목록, 스위치 포트 연결.
/// 자료는 진단 기록(구성도 조사)에 들어 있다. 기록 속 글자는 장비가 보낸 것이므로 가리고 바꿔 쓴다.
fn zone_topology(entries: &[(u64, String)], offset: i32, m: &Masker) -> Zone {
    let text = |v: &Value, key: &str, max: usize| -> String { v.get(key).and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control()).take(max).collect() };
    let mut count = 0;
    let mut body = String::new();
    for (time, json) in entries {
        let Ok(mut entry) = serde_json::from_str::<Value>(json) else { continue };
        let Some((order, _)) = entry.get("run").and_then(|r| r.get("results")).and_then(Value::as_object).and_then(|results| results.iter().find(|(_, r)| r.get("topology").is_some_and(Value::is_object))) else { continue };
        count += 1;
        if count > 1 {
            continue; // 가장 최근 것만 싣고 나머지는 센다
        }
        let order = order.clone();
        m.value(&mut entry);
        let Some(topo) = entry.get("run").and_then(|r| r.get("results")).and_then(|r| r.get(&order)).and_then(|r| r.get("topology")) else { continue };
        body.push_str(&format!(
            "<p class=\"m\">조사한 시각: {} · 대역: <span class=\"k\">{}</span> · 게이트웨이: <span class=\"k\">{}</span></p>",
            local_time(*time, offset).0,
            esc(&text(topo, "cidr", 60)),
            esc(&text(topo, "gateway", 60)),
        ));
        match entry.get("topo_svg").and_then(|t| t.get(&order)).and_then(Value::as_str) {
            Some(svg) if safe_svg(svg) => body.push_str(&format!("<figure class=\"topo\">{}</figure>", m.markup(svg))),
            _ => body.push_str("<p class=\"m\">구성도 그림은 이 기록에 없습니다.</p>"),
        }
        let nodes = topo.get("nodes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        let mut rows = String::new();
        for node in nodes.iter().take(MAX_ROWS) {
            let ports: Vec<String> = node.get("ports").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().filter_map(Value::as_u64).take(60).map(|p| p.to_string()).collect();
            let role = match text(node, "role", 20).as_str() {
                "me" => " (이 PC)",
                "gateway" => " (게이트웨이)",
                _ => "",
            };
            let name = Some(text(node, "name", 80)).filter(|n| !n.is_empty()).unwrap_or_else(|| text(node, "label", 80));
            rows.push_str(&format!(
                "<tr><td>{}{role}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}</td><td class=\"k\">{}</td><td>{}</td></tr>",
                esc(&name),
                kind_text(&text(node, "kind", 20)),
                esc(&text(node, "ip", 60)),
                esc(&text(node, "mac", 30)),
                esc(&text(node, "vendor", 80)),
                esc(&ports.join(", ")),
                esc(&text(node, "descr", 160)),
            ));
        }
        body.push_str(&format!(
            "<h3>장비 {}대{}</h3><table class=\"w\"><tr><th>이름</th><th>종류(추정)</th><th>주소</th><th>MAC</th><th>제조사</th><th>열린 포트</th><th>장비 설명</th></tr>{rows}</table>",
            nodes.len(),
            if nodes.len() > MAX_ROWS { format!(" (앞 {MAX_ROWS}대만 실음)") } else { String::new() },
        ));
        let links = topo.get("links").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        if links.is_empty() {
            body.push_str("<p class=\"m\">스위치 포트 연결 관계는 알아내지 못했습니다(SNMP에 답하는 스위치·AP가 있을 때만 나옵니다).</p>");
        } else {
            let mut rows = String::new();
            for link in links.iter().take(MAX_ROWS) {
                let via = match text(link, "via", 10).as_str() {
                    "lldp" => "장비가 알림(LLDP)",
                    _ => "MAC 표로 짐작",
                };
                let shared = link.get("shared").and_then(Value::as_u64).unwrap_or(0);
                rows.push_str(&format!(
                    "<tr><td class=\"n\">{}</td><td>{}</td><td class=\"n\">{}</td><td>{via}</td><td>{}</td></tr>",
                    esc(&text(link, "switch", 60)),
                    esc(&text(link, "port", 60)),
                    esc(&text(link, "device", 60)),
                    if shared >= 2 { format!("같은 포트에 {shared}대 (사이에 다른 스위치·AP)") } else { String::new() },
                ));
            }
            body.push_str(&format!("<h3>포트 연결 관계 {}건</h3><table class=\"w\"><tr><th>스위치·AP</th><th>포트</th><th>물린 장비</th><th>근거</th><th>비고</th></tr>{rows}</table>", links.len()));
        }
    }
    if count == 0 {
        body.push_str("<p class=\"m\">이 기간에 조사한 구성도가 없습니다. 왼쪽 메뉴의 구성도 진단을 실행하면 여기에 실립니다.</p>");
    } else if count > 1 {
        body.push_str(&format!("<p class=\"m\">이 기간에 구성도를 {count}번 조사했습니다. 가장 최근 것만 실었습니다. 나머지는 \"최근 진단\" 구역에 있습니다.</p>"));
    }
    Zone { id: "topology", title: "구성도", count, shown: count.min(1), body, timeless: false }
}

/// 캡처 분석: 파일마다 요약, 프로토콜, 찾은 문제, 상위 대화. 패킷 내용은 싣지 않는다.
fn zone_capture(captures: &[(crate::capview::CaptureFile, Result<crate::capview::Analysis, String>)], total: usize, offset: i32, m: &Masker) -> Zone {
    let mut body = String::new();
    if captures.is_empty() {
        body.push_str("<p class=\"m\">이 기간에 저장한 캡처 파일이 없습니다.</p>");
    } else {
        body.push_str(&format!(
            "<p class=\"m\">작업 폴더의 캡처 파일을 분석한 요약입니다. 패킷의 내용(페이로드)은 싣지 않습니다.{}</p>",
            if total > captures.len() { format!(" 전체 {total}개 중 최근 {}개입니다.", captures.len()) } else { String::new() }
        ));
    }
    for (file, analysis) in captures {
        body.push_str(&format!(
            "<section><h2><span class=\"t\">{}</span></h2><p class=\"m\">저장한 시각: {} · 크기: {}</p>",
            esc(&file.name),
            local_time(file.modified.saturating_mul(1000), offset).0,
            size_text(file.size),
        ));
        match analysis {
            Err(why) => body.push_str(&format!("<p class=\"m\">이 파일은 분석하지 못했습니다: {}</p>", esc(&m.text(why)))),
            Ok(a) => {
                body.push_str(&format!(
                    "<table>{}{}{}{}</table>",
                    row("패킷", &format!("{}개", a.packets)),
                    row("전체 크기", &size_text(a.bytes)),
                    row("캡처한 시간", &format!("{:.1}초", a.seconds.max(0.0))),
                    row("링크 형식", &esc(&a.link)),
                ));
                if a.findings.is_empty() {
                    body.push_str("<p class=\"m\">찾은 문제가 없습니다.</p>");
                } else {
                    let mut rows = String::new();
                    for f in a.findings.iter().take(50) {
                        let (label, class) = if f.light == "red" { ("문제", "r") } else { ("주의", "y") };
                        rows.push_str(&format!("<tr><td><span class=\"l {class}\">{label}</span></td><td>{}</td><td>{}</td><td>{}건</td></tr>", esc(&m.text(&f.title)), esc(&m.text(&f.text)), f.count));
                    }
                    body.push_str(&format!("<h3>찾은 문제</h3><table class=\"w\"><tr><th>정도</th><th>무엇</th><th>풀이</th><th>수</th></tr>{rows}</table>"));
                }
                let mut rows = String::new();
                for p in a.protocols.iter().take(15) {
                    rows.push_str(&format!("<tr><td>{}</td><td>{}개</td><td>{}</td></tr>", esc(&p.label), p.packets, size_text(p.bytes)));
                }
                if !rows.is_empty() {
                    body.push_str(&format!("<h3>프로토콜</h3><table><tr><th>종류</th><th>패킷</th><th>크기</th></tr>{rows}</table>"));
                }
                let mut rows = String::new();
                for t in a.talks.iter().take(10) {
                    rows.push_str(&format!("<tr><td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}개</td><td>{}</td></tr>", esc(&m.text(&t.a)), esc(&m.text(&t.b)), t.packets, size_text(t.bytes)));
                }
                if !rows.is_empty() {
                    body.push_str(&format!("<h3>상위 대화</h3><table><tr><th>한쪽</th><th>다른 쪽</th><th>패킷</th><th>크기</th></tr>{rows}</table>"));
                }
                for note in a.notes.iter().take(10) {
                    body.push_str(&format!("<p class=\"m\">{}</p>", esc(&m.text(note))));
                }
            }
        }
        body.push_str("</section>");
    }
    Zone { id: "capture", title: "캡처 분석", count: total, shown: captures.len(), body, timeless: false }
}

/// 직접 명령: 실행한 명령, 종료 상태, 출력 앞부분. 기록은 `manuallog`가 남긴다.
fn zone_manual(log: &str, from_ms: u64, to_ms: u64, offset: i32, m: &Masker) -> Zone {
    let mut count = 0;
    let mut rows = Vec::new();
    for line in log.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(time) = v.get("time").and_then(Value::as_u64) else { continue };
        if time < from_ms || time > to_ms {
            continue;
        }
        count += 1;
        if rows.len() >= MAX_ROWS {
            continue;
        }
        let text = |key: &str, max: usize| -> String { v.get(key).and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(max).collect() };
        let state = match text("state", 20).as_str() {
            "ok" => "끝남",
            "error" => "오류",
            "timeout" => "시간 초과",
            "cancelled" => "중단",
            "capped" => "출력 상한에서 멈춤",
            _ => "알 수 없음",
        };
        let exit = v.get("exit").and_then(Value::as_i64).map(|c| c.to_string()).unwrap_or_default();
        let secs = v.get("secs").and_then(Value::as_f64).map(|s| format!("{s:.1}초")).unwrap_or_default();
        let output = text("output", 4000);
        let cut = v.get("cut").and_then(Value::as_bool).unwrap_or(false);
        rows.push(format!(
            "<tr><td class=\"n\">{}</td><td class=\"k\">{}</td><td>{state}</td><td>{exit}</td><td>{secs}</td><td>{}</td></tr>",
            local_time(time, offset).0,
            esc(&m.text(&text("command", 1000).replace('\n', " "))),
            if output.trim().is_empty() { "<span class=\"m\">출력 없음</span>".to_string() } else { format!("<pre>{}{}</pre>", esc(&m.text(&output)), if cut { "\n… (뒤는 싣지 않음)" } else { "" }) },
        ));
    }
    let shown = rows.len();
    let body = if rows.is_empty() {
        "<p class=\"m\">이 기간에 직접 명령으로 실행한 기록이 없습니다. 직접 명령의 결과는 이 기능이 들어온 판부터 남습니다.</p>".to_string()
    } else {
        format!(
            "<p class=\"m\">직접 명령으로 실행한 것을 최근 것부터 적었습니다. 출력은 앞 40줄까지입니다.{}</p>\
<table class=\"w\"><tr><th>시각</th><th>명령</th><th>상태</th><th>종료 코드</th><th>걸린 시간</th><th>출력 앞부분</th></tr>{}</table>",
            if count > shown { format!(" 전체 {count}건 중 최근 {shown}건입니다.") } else { String::new() },
            rows.concat()
        )
    };
    Zone { id: "manual", title: "직접 명령", count, shown, body, timeless: false }
}

fn zone_audit(log: &str, from_ms: u64, to_ms: u64, offset: i32, m: &Masker) -> Zone {
    let (from, to) = (from_ms / 1000, to_ms.div_ceil(1000));
    // 최근 것부터, 상한까지만 만든다(나머지는 세기만 한다).
    let mut count = 0;
    let mut rows = Vec::new();
    for line in log.lines().rev() {
        let Some(time) = serde_json::from_str::<Value>(line).ok().and_then(|v| v.get("time")?.as_u64()) else { continue };
        if time < from || time > to {
            continue;
        }
        count += 1;
        if rows.len() < MAX_ROWS {
            if let Some((_, html)) = audit_row(line, offset, m) {
                rows.push(html);
            }
        }
    }
    let shown = rows.len();
    let body = if rows.is_empty() {
        "<p class=\"m\">이 기간에 남은 기록이 없습니다.</p>".to_string()
    } else {
        format!(
            "<p class=\"m\">실행한 명령, 확인한 것, 다른 프로그램에서 받은 의뢰를 최근 것부터 적었습니다. 암호와 터미널에 친 글자는 기록하지 않습니다.{}</p>\
<table class=\"w\"><tr><th>시각</th><th>종류</th><th>내용</th><th>위험도</th><th>확인</th></tr>{}</table>",
            if count > shown { format!(" 전체 {count}건 중 최근 {shown}건입니다.") } else { String::new() },
            rows.concat()
        )
    };
    Zone { id: "audit", title: "감사 로그", count, shown, body, timeless: false }
}

const STYLE: &str = ".head th{white-space:nowrap}.note{border-left:4px solid #1d2330;padding:.3em .8em;background:#f3f5f8}\
.z>h1{font-size:1.3em;border-bottom:3px solid #1d2330;padding-bottom:.2em;margin-top:2.5em}.z>h1+section>h2{border-top:0;margin-top:.6em}.w{width:100%}.n{white-space:nowrap;font-family:Consolas,monospace;font-size:.9em}\
.k{font-family:Consolas,monospace;font-size:.9em;word-break:break-all}footer{margin-top:3em;border-top:1px solid #c9ced8;padding-top:.6em;color:#5b6475;font-size:.85em}\
@media print{body{max-width:none;margin:0}.z{break-before:page}.s,figure,tr{break-inside:avoid}}";

const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">";

/// 보고서 전체를 만든다. `stop`이 서면 그만두고 오류를 돌려준다(아무것도 남기지 않는다).
pub fn build(req: &Request, src: &Sources, now_ms: u64, stop: &AtomicBool) -> Result<Built, String> {
    use base64::Engine;
    const STOPPED: &str = "보고서 만들기를 취소했습니다.";
    let single = req.history_id.is_some();
    let wanted: Vec<&str> = if single { vec!["history"] } else { SECTIONS.iter().map(|(id, _)| *id).filter(|id| req.sections.iter().any(|s| s == id)).collect() };
    if wanted.is_empty() {
        return Err("보고서에 넣을 구역을 하나 이상 고르세요.".into());
    }
    if !single && req.from_ms > req.to_ms {
        return Err("기간의 시작이 끝보다 늦습니다.".into());
    }
    let m = Masker::new(req.mask, &src.host, &src.user, src.net.as_ref().and_then(|n| n.public_ip.as_deref())).with_own(src.net.as_ref().and_then(|n| n.ip.as_deref()));
    let offset = req.offset_minutes;
    let mut zones = Vec::new();
    let mut data = Vec::new();
    for id in wanted {
        if stop.load(Ordering::SeqCst) {
            return Err(STOPPED.into());
        }
        zones.push(match id {
            "mynet" => zone_mynet(src.net.as_ref(), &m),
            "history" => {
                let picked: Vec<(u64, String)> = src.history.iter().filter(|(t, _)| single || (req.from_ms..=req.to_ms).contains(t)).cloned().collect();
                let (zone, items) = zone_history(&picked, offset, &m);
                data = items;
                zone
            }
            "topology" => {
                let picked: Vec<(u64, String)> = src.history.iter().filter(|(t, _)| (req.from_ms..=req.to_ms).contains(t)).cloned().collect();
                zone_topology(&picked, offset, &m)
            }
            "capture" => zone_capture(&src.captures, src.captures_total, offset, &m),
            "manual" => zone_manual(&src.manual, req.from_ms, req.to_ms, offset, &m),
            _ => zone_audit(&src.audit, req.from_ms, req.to_ms, offset, &m),
        });
    }
    if stop.load(Ordering::SeqCst) {
        return Err(STOPPED.into());
    }

    let made = local_time(now_ms, offset).0;
    let period = if single {
        "고른 진단 기록 한 건".to_string()
    } else {
        format!("{} ~ {}", local_time(req.from_ms, offset).0, local_time(req.to_ms, offset).0)
    };
    let list: Vec<String> = zones
        .iter()
        .map(|z| {
            let n = if z.timeless {
                "만든 시각 기준".to_string()
            } else if z.count > z.shown {
                format!("전체 {}건 중 {}건", z.count, z.shown)
            } else {
                format!("{}건", z.count)
            };
            format!("{} ({n})", z.title)
        })
        .collect();
    let masked = if req.mask {
        "켬 — 내부 주소는 \"내부-번호\", MAC은 앞 세 자리만, PC 이름은 \"PC-1\", 사용자 이름은 \"사용자\", 이 PC의 공인 주소는 \"내 공인 IP\"로 적었습니다. 바깥 목적지 주소와 도메인은 가리지 않습니다."
    } else {
        "끔 — 내부 주소와 PC 이름이 그대로 들어 있습니다. 밖으로 보낼 때 주의하세요."
    };
    let mut html = format!(
        "<!DOCTYPE html>\n<html lang=\"ko\"><head><meta charset=\"utf-8\">{CSP}<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>NetX 보고서 {}</title><style>{}{}{STYLE}</style></head><body>\
<h1>NetX 보고서</h1><p class=\"note\">이 보고서는 만든 시각까지 이 PC가 기록한 사실을 모은 것이며, 판정이 아닙니다.</p>\
<table class=\"head\">{}{}{}{}{}{}</table>",
        esc(&made),
        crate::history::STYLE,
        crate::report_fig::STYLE,
        row("프로그램", "NetX"),
        row("판", &esc(&src.version)),
        row("만든 시각", &esc(&made)),
        row("기간", &esc(&period)),
        row("가리기", masked),
        row("담은 구역", &esc(&list.join(", "))),
    );
    for z in &zones {
        html.push_str(&format!("<section class=\"z\" id=\"{}\"><h1>{}</h1>{}</section>", z.id, z.title, z.body));
    }
    html.push_str(&format!(
        "<footer>NetX {} — 맑은물속교회가 무료로 나눕니다 · 소스: github.com/sysoppark-oss/netx{}</footer>",
        esc(&src.version),
        if data.is_empty() { "" } else { " · 진단 기록은 NetX의 [최근 진단] → [가져오기]로 이 파일을 열면 다시 볼 수 있습니다" },
    ));
    // 진단 기록은 다른 NetX에서 가져올 수 있게 자료를 주석으로 넣는다(화면에는 보이지 않는다). 가린 것은 가린 채로 들어간다.
    if !data.is_empty() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(Value::Array(data).to_string());
        html.push_str(&format!("{}{encoded}{}", crate::history::DATA_OPEN, crate::history::DATA_CLOSE));
    }
    html.push_str("</body></html>\n");
    Ok(Built { html, counts: zones.iter().map(|z| (z.id.to_string(), z.count, z.shown)).collect() })
}

/// 보고서 파일의 기본 이름 (2.1)
pub fn file_name(now_ms: u64, offset_minutes: i32) -> String {
    format!("netx-report-{}.html", local_time(now_ms, offset_minutes).1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_794_443_800_000; // 2026-11-12 09:36 (KST)

    fn req(sections: &[&str], mask: bool) -> Request {
        Request { sections: sections.iter().map(|s| s.to_string()).collect(), from_ms: NOW - 7 * 86_400_000, to_ms: NOW, mask, offset_minutes: 540, history_id: None }
    }

    fn entry(target: &str, raw: &str) -> String {
        serde_json::json!({
            "meta": {"title": "웹 장애 진단", "recipe": "web_down", "target": target, "light": "red", "steps": 1},
            "recipe_id": "web_down", "inputs": {"url": target},
            "plan": {"steps": [{"order": 1, "title": "포트 연결 시험 (nc)", "display": format!("nc -vz -w 3 -- {target} 443"), "tool": "nc"}]},
            "run": {
                "steps": {"1": {"status": "done", "exit_code": 1, "elapsed_ms": 3100}},
                "results": {"1": {"light": "red", "text": raw, "facts": [{"label": "대상", "value": format!("{target}:443")}]}},
                "lines": [{"order": 1, "stream": "stderr", "line": raw}]
            },
            "topo_svg": {"1": format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><text x=\"10\">MYPC {target}</text></svg>")}
        })
        .to_string()
    }

    fn sources() -> Sources {
        let audit = [
            serde_json::json!({"time": NOW / 1000 - 60, "run_id": 3, "level": "general", "source": "recipe", "tool": "nc", "argv": ["-vz", "192.168.0.10", "443"], "risk": "safe", "root": false, "confirmed": false}),
            serde_json::json!({"time": NOW / 1000 - 30, "source": "terminal", "kind": "ssh", "target": "192.168.0.1:22", "user": "admin"}),
            serde_json::json!({"time": NOW / 1000 - 20, "run_id": 0, "level": "general", "source": "bridge", "tool": "rack.handoff", "argv": ["142.250.197.142:443", "storyfirewall"], "risk": "safe", "root": false, "confirmed": false}),
            serde_json::json!({"time": NOW / 1000 - 40 * 86_400, "run_id": 1, "level": "general", "source": "recipe", "tool": "old-one", "argv": [], "risk": "caution", "root": true, "confirmed": true}),
        ];
        Sources {
            version: "0.1.0 (abc1234)".into(),
            host: "MYPC".into(),
            user: "jgpark".into(),
            net: Some(Net {
                iface: Some("Wi-Fi".into()),
                ip: Some("192.168.0.7".into()),
                prefix: Some(24),
                gateway: Some("192.168.0.1".into()),
                dns: vec!["192.168.0.1".into(), "8.8.8.8".into()],
                link: Some("866 Mbps".into()),
                public_ip: Some("203.0.113.9".into()),
                mac: Some("a4:5e:60:11:22:33".into()),
            }),
            history: vec![(NOW - 1000, entry("192.168.0.10", "MYPC에서 192.168.0.10 연결 실패, 공인 203.0.113.9, 경로 C:\\Users\\jgpark\\work, 서버 8.8.8.8"))],
            audit: audit.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n") + "\n깨진 줄\n",
            ..Sources::default()
        }
    }

    /// 구성도를 조사한 진단 기록
    fn map_entry(name: &str, svg: &str) -> String {
        serde_json::json!({
            "meta": {"title": "구성도", "recipe": "network_map", "target": "192.168.0.0/24", "light": "green", "steps": 1},
            "recipe_id": "network_map", "inputs": {"cidr": "192.168.0.0/24"},
            "plan": {"steps": [{"order": 1, "title": "구성도 조사", "display": "netmap 192.168.0.0/24", "tool": "netmap"}]},
            "run": {
                "steps": {"1": {"status": "done", "exit_code": 0, "elapsed_ms": 9000}},
                "results": {"1": {"light": "green", "text": "장비 2대", "facts": [], "topology": {
                    "cidr": "192.168.0.0/24", "gateway": "192.168.0.1", "me": "192.168.0.7",
                    "nodes": [
                        {"ip": "192.168.0.1", "mac": "aa:bb:cc:dd:ee:ff", "role": "gateway", "kind": "router", "label": "공유기", "vendor": "ipTIME", "ports": [53, 80], "name": null, "descr": null, "ping_ms": 1.2, "notes": []},
                        {"ip": "192.168.0.21", "mac": "00:11:22:33:44:55", "role": "host", "kind": "printer", "label": "프린터", "vendor": "HP", "ports": [9100], "name": name, "descr": "HP LaserJet", "ping_ms": null, "notes": []}
                    ],
                    "links": [{"switch": "192.168.0.2", "port": "Gi0/3", "device": "192.168.0.21", "via": "fdb", "shared": 2}]
                }}},
                "lines": []
            },
            "topo_svg": {"1": svg}
        })
        .to_string()
    }

    fn capture(name: &str, who: &str, problem: &str) -> (crate::capview::CaptureFile, Result<crate::capview::Analysis, String>) {
        use crate::capview::{Analysis, CaptureFile, Finding, Share, Talk};
        let file = CaptureFile { name: name.into(), size: 5 * 1024 * 1024, modified: NOW / 1000 - 600 };
        let analysis = Analysis {
            name: name.into(),
            size: file.size,
            started: 0.0,
            seconds: 12.34,
            packets: 2000,
            bytes: 1_500_000,
            link: "이더넷".into(),
            protocols: vec![Share { label: "TCP".into(), packets: 1800, bytes: 1_400_000 }],
            talks: vec![Talk { a: who.into(), b: "8.8.8.8".into(), packets: 900, bytes: 700_000 }],
            findings: vec![Finding { light: "red", title: "TCP 재전송".into(), text: problem.into(), count: 14, mark: "retrans" }],
            notes: vec![format!("{who}가 가장 많이 보냈습니다")],
        };
        (file, Ok(analysis))
    }

    #[test]
    fn second_stage_sections_topology_capture_manual() {
        const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>192.168.0.1</text></svg>";
        let mut src = sources();
        src.history = vec![(NOW - 500, map_entry("PRINTER-1", SVG)), (NOW - 900, map_entry("OLD", SVG)), (NOW - 1000, entry("192.168.0.10", "x"))];
        src.captures = vec![capture("cap-1.pcap", "192.168.0.7", "192.168.0.7 → 8.8.8.8 에서 다시 보냄"), (crate::capview::CaptureFile { name: "big.pcap".into(), size: 80 << 20, modified: NOW / 1000 - 700 }, Err("파일이 너무 큽니다(80MB).".into()))];
        src.captures_total = 14;
        src.manual = [
            serde_json::json!({"time": NOW - 60_000, "command": "ping -c 2 192.168.0.1", "tool": "ping", "state": "ok", "exit": 0, "secs": 1.0, "output": "PING 192.168.0.1\n2 packets transmitted\n", "cut": false}).to_string(),
            serde_json::json!({"time": NOW - 30_000, "command": "nc -vz 8.8.8.8 53", "tool": "nc", "state": "error", "exit": 1, "secs": 3.0, "output": "", "cut": true}).to_string(),
            serde_json::json!({"time": NOW - 40 * 86_400_000, "command": "old-command", "state": "ok"}).to_string(),
            "깨진 줄".to_string(),
        ]
        .join("\n");

        let built = build(&req(&["manual", "capture", "topology"], false), &src, NOW, &no_stop()).unwrap();
        let html = &built.html;
        check(html).unwrap();
        assert_eq!(built.counts, [("topology".to_string(), 2, 1), ("capture".to_string(), 14, 2), ("manual".to_string(), 2, 2)]);
        assert!(html.contains("구성도 (전체 2건 중 1건), 캡처 분석 (전체 14건 중 2건), 직접 명령 (2건)"));
        let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("{s}"));
        assert!(at("id=\"topology\"") < at("id=\"capture\"") && at("id=\"capture\"") < at("id=\"manual\""));
        // 구성도: 가장 최근 것의 그림과 장비, 포트 연결
        assert!(html.contains("<figure class=\"topo\"><svg") && html.contains("<h3>장비 2대</h3>"));
        assert!(html.contains("<td>PRINTER-1</td><td>프린터</td><td class=\"n\">192.168.0.21</td><td class=\"n\">00:11:22:33:44:55</td><td>HP</td><td class=\"k\">9100</td><td>HP LaserJet</td>"));
        assert!(html.contains("<td>공유기 (게이트웨이)</td><td>공유기·라우터</td>") && !html.contains("OLD"));
        assert!(html.contains("<td>Gi0/3</td>") && html.contains("MAC 표로 짐작") && html.contains("같은 포트에 2대"));
        assert!(html.contains("구성도를 2번 조사했습니다"));
        // 캡처 분석: 요약, 문제, 대화. 못 연 파일은 까닭만
        assert!(html.contains("전체 14개 중 최근 2개입니다") && html.contains("<th>패킷</th><td>2000개</td>") && html.contains("<th>캡처한 시간</th><td>12.3초</td>"));
        assert!(html.contains("<span class=\"l r\">문제</span></td><td>TCP 재전송</td>") && html.contains("<td>14건</td>"));
        assert!(html.contains("<td class=\"n\">192.168.0.7</td><td class=\"n\">8.8.8.8</td><td>900개</td>"));
        assert!(html.contains("이 파일은 분석하지 못했습니다: 파일이 너무 큽니다(80MB)."));
        // 직접 명령: 최근 것부터, 기간 밖과 깨진 줄은 뺀다
        assert!(at("nc -vz 8.8.8.8 53") < at("ping -c 2 192.168.0.1") && !html.contains("old-command"));
        assert!(html.contains("<td>오류</td><td>1</td><td>3.0초</td><td><span class=\"m\">출력 없음</span></td>"));
        assert!(html.contains("<pre>PING 192.168.0.1\n2 packets transmitted\n</pre>"));
        assert!(!html.contains(crate::history::DATA_OPEN), "진단 기록 구역을 고르지 않으면 자료 주석을 넣지 않는다");

        if let Ok(path) = std::env::var("NETX_REPORT_SAMPLE") {
            std::fs::write(path, html).unwrap();
        }

        // 가리면 내부 주소와 MAC 뒷부분이 세 구역 모두에서 가려지고, 바깥 주소는 남는다
        let masked = build(&req(&["topology", "capture", "manual"], true), &src, NOW, &no_stop()).unwrap().html;
        check(&masked).unwrap();
        for hidden in ["192.168.0.21", "192.168.0.7", "192.168.0.1<", "00:11:22:33:44:55", "aa:bb:cc:dd:ee:ff"] {
            assert!(!masked.contains(hidden), "{hidden}");
        }
        assert!(masked.contains("00:11:22:**:**:**") && masked.contains("8.8.8.8") && masked.contains("ping -c 2 내부-"));

        // 자료가 없으면 그렇다고 적는다. 위험한 그림은 싣지 않는다
        let empty = build(&req(&["topology", "capture", "manual"], false), &sources(), NOW, &no_stop()).unwrap();
        assert_eq!(empty.counts, [("topology".to_string(), 0, 0), ("capture".to_string(), 0, 0), ("manual".to_string(), 0, 0)]);
        assert!(empty.html.contains("이 기간에 조사한 구성도가 없습니다") && empty.html.contains("이 기간에 저장한 캡처 파일이 없습니다") && empty.html.contains("직접 명령으로 실행한 기록이 없습니다"));
        src.history = vec![(NOW - 500, map_entry("P", "<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"x()\"></svg>"))];
        let unsafe_figure = build(&req(&["topology"], false), &src, NOW, &no_stop()).unwrap().html;
        check(&unsafe_figure).unwrap();
        assert!(unsafe_figure.contains("구성도 그림은 이 기록에 없습니다") && !unsafe_figure.contains("onload"));
    }

    fn no_stop() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// 보고서에서 실행될 수 있는 것을 찾는다(2.2의 5). 꼬리표와 속성은 여기 적힌 것만 있어야 한다.
    fn check(html: &str) -> Result<(), String> {
        const TAGS: [&str; 38] = [
            "html", "head", "meta", "title", "style", "body", "h1", "h2", "h3", "p", "span", "table", "tr", "th", "td", "section", "div", "pre", "details", "summary", "figure", "figcaption", "footer", "b", "br",
            "small", "svg", "polyline", "line", "circle", "rect", "text", "g", "path", "tspan", "ellipse", "polygon", "defs",
        ];
        const ATTRS: [&str; 34] = [
            "lang", "charset", "http-equiv", "content", "name", "class", "id", "xmlns", "viewbox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "points", "d", "fill", "stroke", "stroke-width",
            "stroke-dasharray", "font-size", "text-anchor", "style", "role", "transform", "opacity",
        ];
        let (body, _) = html.split_once(crate::history::DATA_OPEN).unwrap_or((html, ""));
        let low = body.to_ascii_lowercase();
        // 글자는 꼬리표가 되지 못하면 아무 일도 하지 않는다. 그래서 꼬리표 안과 꾸밈(style) 안만 본다.
        let outside = |text: &str| ["javascript:", "url(", "@import", "http://", "https://", "//"].iter().find(|bad| text.replace("xmlns=\"http://www.w3.org/2000/svg\"", "").contains(**bad)).map(|bad| format!("{bad} 가 들어 있다"));
        let styles: Vec<&str> = low.split("<style>").skip(1).filter_map(|s| s.split("</style>").next()).collect();
        if styles.len() != 1 {
            return Err(format!("꾸밈이 {}개", styles.len()));
        }
        if let Some(why) = styles.iter().find_map(|s| outside(s)) {
            return Err(why);
        }
        let low = low.replace(styles[0], "");
        for piece in low.split('<').skip(1) {
            let tag = piece.split('>').next().unwrap_or("");
            if tag == "!doctype html" {
                continue;
            }
            if let Some(why) = outside(tag) {
                return Err(format!("{why}: <{tag}>"));
            }
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| !c.is_whitespace() && *c != '/').collect();
            if !TAGS.contains(&name.as_str()) {
                return Err(format!("꼬리표 <{name}>"));
            }
            // 속성: 따옴표 안을 지운 뒤 이름만 본다.
            let mut bare = String::new();
            let mut quote = false;
            for c in tag.chars() {
                match (c, quote) {
                    ('"', _) => quote = !quote,
                    (_, false) => bare.push(c),
                    _ => {}
                }
            }
            if quote {
                return Err(format!("닫히지 않은 따옴표: {tag}"));
            }
            for word in bare.split_whitespace().skip(1) {
                let attr = word.trim_end_matches('=').trim_end_matches('/');
                if !attr.is_empty() && !ATTRS.contains(&attr) {
                    return Err(format!("속성 {attr} (<{name}>)"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn builds_the_chosen_sections_with_the_common_head() {
        let built = build(&req(&["audit", "mynet", "history", "nope"], false), &sources(), NOW, &no_stop()).unwrap();
        let html = &built.html;
        assert!(html.starts_with("<!DOCTYPE html>") && html.contains(CSP) && html.ends_with("--></body></html>\n"));
        assert!(html.contains("이 보고서는 만든 시각까지 이 PC가 기록한 사실을 모은 것이며, 판정이 아닙니다."));
        assert!(html.contains("<th>판</th><td>0.1.0 (abc1234)</td>") && html.contains("<th>만든 시각</th><td>2026-11-12 09:36</td>"));
        assert!(html.contains("<th>기간</th><td>2026-11-05 09:36 ~ 2026-11-12 09:36</td>") && html.contains("끔 — 내부 주소와 PC 이름이 그대로"));
        assert!(html.contains("내 네트워크 (만든 시각 기준), 최근 진단 (1건), 감사 로그 (3건)"), "메뉴 순서, 기간 밖은 세지 않는다");
        let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("{s}"));
        assert!(at("id=\"mynet\"") < at("id=\"history\"") && at("id=\"history\"") < at("id=\"audit\""));
        assert_eq!(built.counts, [("mynet".to_string(), 1, 1), ("history".to_string(), 1, 1), ("audit".to_string(), 3, 3)]);
        // 내 네트워크
        assert!(html.contains("<th>내부 주소</th><td>192.168.0.7/24</td>") && html.contains("<th>DNS</th><td>192.168.0.1, 8.8.8.8</td>") && html.contains("<td>a4:5e:60:11:22:33</td>"));
        // 감사 로그: 최근 것부터, 터미널과 연동 의뢰
        assert!(at("진단 대상을 넘겨받음: 142.250.197.142:443 (보낸 곳 storyfirewall)") < at("ssh 192.168.0.1:22 (사용자 admin)"));
        assert!(at("ssh 192.168.0.1:22") < at("nc -vz 192.168.0.10 443") && !html.contains("old-one"));
        // 말씀과 헌정 문구는 없고 바닥글 한 줄만
        assert!(html.contains("<footer>NetX 0.1.0 (abc1234) — 맑은물속교회가 무료로 나눕니다 · 소스: github.com/sysoppark-oss/netx"));
        check(html).unwrap();

        // 고른 것만
        let only = build(&req(&["mynet"], true), &sources(), NOW, &no_stop()).unwrap().html;
        assert!(only.contains("id=\"mynet\"") && !only.contains("id=\"audit\"") && !only.contains("id=\"history\"") && !only.contains(crate::history::DATA_OPEN));
        assert!(build(&req(&[], true), &sources(), NOW, &no_stop()).is_err());
        assert!(build(&req(&["terminal"], true), &sources(), NOW, &no_stop()).is_err(), "아직 없는 구역만 고르면 만들지 않는다");
        let mut backwards = req(&["audit"], true);
        backwards.from_ms = NOW + 1;
        assert!(build(&backwards, &sources(), NOW, &no_stop()).is_err());
    }

    #[test]
    fn masking_hides_inner_addresses_names_and_this_pcs_public_address() {
        let html = build(&req(&["mynet", "history", "audit"], true), &sources(), NOW, &no_stop()).unwrap().html;
        for gone in ["192.168.", "MYPC", "jgpark", "203.0.113.9", "11:22:33"] {
            assert!(!html.contains(gone), "{gone} 가 남았다");
        }
        // 같은 주소는 같은 번호, 바깥 주소는 그대로
        assert!(html.contains("<th>내부 주소</th><td>내부-1/24</td>") && html.contains("<th>게이트웨이</th><td>내부-2</td>") && html.contains("<th>DNS</th><td>내부-2, 8.8.8.8</td>"));
        assert!(html.contains("<th>공인 주소</th><td>내 공인 IP</td>") && html.contains("<td>a4:5e:60:**:**:**</td>"));
        assert!(html.contains("PC-1에서 내부-3 연결 실패, 공인 내 공인 IP, 경로 C:\\Users\\사용자\\work, 서버 8.8.8.8"));
        assert!(html.contains("ssh 내부-2:22") && html.contains("nc -vz 내부-3 443") && html.contains("142.250.197.142:443"));
        assert!(html.contains("<text x=\"10\">PC-1 내부-3</text>"), "그림 속 글자도 가린다");
        assert!(html.contains("켬 — 내부 주소는"));
        check(&html).unwrap();
    }

    #[test]
    fn masker_rules() {
        let m = Masker::new(true, "DESKTOP-AB12", "kim", Some("198.51.100.4"));
        assert_eq!(m.text("10.0.0.1 10.0.0.11 10.0.0.1. 172.16.5.5:80 169.254.1.2 8.8.8.8 1.2.3"), "내부-1 내부-2 내부-1. 내부-3:80 내부-4 8.8.8.8 1.2.3");
        assert_eq!(m.text("내 주소 198.51.100.4, 남 198.51.100.40"), "내 주소 내 공인 IP, 남 198.51.100.40");
        // 테일스케일·통신사 공유 대역도 내부 주소다. 어댑터가 공인 주소를 직접 받았으면 그것도 가린다.
        let direct = Masker::new(true, "", "", None).with_own(Some("203.0.113.77"));
        assert_eq!(direct.text("100.89.153.49 100.128.0.1 203.0.113.77 203.0.113.7"), "내부-1 100.128.0.1 내 공인 IP 203.0.113.7");
        assert_eq!(m.text("fe80::1c2:3ff:fe04:5 fd12:3456::1 2001:4860:4860::8888 09:35:32"), "내부-5 내부-6 2001:4860:4860::8888 09:35:32");
        assert_eq!(m.text("AA-BB-CC-DD-EE-FF aa:bb:cc:dd:ee:ff aabbccddeeff"), "AA-BB-CC-**-**-** aa:bb:cc:**:**:** aabbccddeeff");
        assert_eq!(m.text("desktop-ab12의 KIM은 kimchi, akim, C:\\Users\\kim\\a"), "PC-1의 사용자은 kimchi, akim, C:\\Users\\사용자\\a");
        assert_eq!(Masker::new(true, "", "박정근", None).text("박정근의 폴더"), "사용자의 폴더");
        // 짧은 이름은 경로 안에서만
        let short = Masker::new(true, "", "jo", None);
        assert_eq!(short.text("jo 는 C:\\Users\\jo\\x 와 /home/jo/y, C:\\\\Users\\\\jo\\\\z"), "jo 는 C:\\Users\\사용자\\x 와 /home/사용자/y, C:\\\\Users\\\\사용자\\\\z");
        // 꺼져 있으면 그대로
        assert_eq!(Masker::new(false, "A-PC", "kim", None).text("192.168.0.1 kim"), "192.168.0.1 kim");
        // 자료: 이름표와 뜻으로 읽는 값은 그대로
        let named = Masker::new(true, "green", "meta", None);
        let mut v = serde_json::json!({"meta": {"light": "green", "title": "green 의 meta"}, "run": {"lines": [{"stream": "stderr", "line": "192.168.1.1"}]}});
        named.value(&mut v);
        assert_eq!(v, serde_json::json!({"meta": {"light": "green", "title": "PC-1 의 사용자"}, "run": {"lines": [{"stream": "stderr", "line": "내부-1"}]}}));
    }

    #[test]
    fn one_record_report_and_import_round_trip() {
        let src = sources();
        let mut one = req(&["mynet", "audit"], true);
        one.history_id = Some("1794443799000".into());
        let html = build(&one, &src, NOW, &no_stop()).unwrap().html;
        assert!(html.contains("<th>기간</th><td>고른 진단 기록 한 건</td>") && html.contains("id=\"history\"") && !html.contains("id=\"mynet\"") && !html.contains("id=\"audit\""));

        // 새 보고서도 가져오기에 통과하고, 가린 것은 가린 채로 들어온다.
        let dir = std::env::temp_dir().join(format!("netx-report-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(crate::history::import(&dir, &html, NOW).unwrap(), 1);
        let text = crate::history::load(&dir, &crate::history::list(&dir)[0].id).unwrap();
        assert!(text.contains("내부-1") && !text.contains("192.168.0.10") && !text.contains("jgpark") && text.contains("\"recipe_id\":\"web_down\""));
        // 가져온 것으로 다시 만든 보고서도 규칙을 지킨다.
        let again = build(&req(&["history"], false), &Sources { history: vec![(NOW, text)], ..Sources::default() }, NOW, &no_stop()).unwrap().html;
        check(&again).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn caps_long_tables_and_stops_when_asked() {
        let line = |i: u64| serde_json::json!({"time": NOW / 1000 - i, "run_id": i, "level": "general", "source": "recipe", "tool": format!("t{i}"), "argv": [], "risk": "safe", "root": false, "confirmed": false}).to_string();
        // 파일에는 오래된 것부터 적힌다.
        let log: Vec<String> = (0..2500).rev().map(line).collect();
        let src = Sources { audit: log.join("\n"), ..Sources::default() };
        let built = build(&req(&["audit"], true), &src, NOW, &no_stop()).unwrap();
        assert_eq!(built.counts, [("audit".to_string(), 2500, MAX_ROWS)]);
        assert!(built.html.contains("전체 2500건 중 최근 2000건입니다.") && built.html.contains("감사 로그 (전체 2500건 중 2000건)"));
        assert!(built.html.contains("<td class=\"k\">t0 </td>") && built.html.contains("t1999 ") && !built.html.contains("t2000 "), "최근 것을 싣는다");
        assert_eq!(built.html.matches("<tr><td class=\"n\">").count(), MAX_ROWS);
        assert!(build(&req(&["audit"], true), &src, NOW, &AtomicBool::new(true)).is_err());
        assert_eq!(file_name(NOW, 540), "netx-report-20261112-093640.html");
    }

    #[test]
    fn only_plain_drawings_pass() {
        let good = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\" width=\"20\" height=\"20\"><rect x=\"0\" y=\"0\" width=\"10\" height=\"10\" rx=\"2\" fill=\"#0b1020\"/><text x=\"1\" y=\"2\" font-family=\"'Malgun Gothic',sans-serif\" font-size=\"11\" text-anchor=\"middle\">공유기 &lt;gw&gt; --&gt;</text><polyline points=\"1,2 3,4\" fill=\"none\" stroke=\"#fff\" stroke-width=\"1.5\"/></svg>";
        assert!(safe_svg(good));
        for bad in [
            "<svg></svg><iframe src=\"//x\"></iframe>",
            "<svg><style>*{background:red}</style></svg>",
            "<svg><image xlink:href=\"x\"/></svg>",
            "<svg><rect fill=\"url(//x)\"/></svg>",
            "<svg><rect style=\"x\"/></svg>",
            "<svg><rect x='1'/></svg>",
            "<svg><text>a</text>",
            "<svg></svg></div>",
            "<svg></svg><!-- x -->",
            "<svg><rect x=\"1/></svg>",
            "<svg><animate attributeName=\"x\"/></svg>",
            " <svg></svg>",
            "<svg><text>a</text></svg>뒤에 붙은 글<p>",
            "<svg><text>a\u{1b}[31m</text></svg>",
        ] {
            assert!(!safe_svg(bad), "{bad}");
        }
    }

    /// 무작위 입력: 어떤 글자가 들어와도 실행될 수 있는 꼬리표·속성이 생기지 않는다.
    #[test]
    fn hostile_text_never_becomes_markup() {
        let bits = [
            "<script>alert(1)</script>", "\"><img src=x onerror=alert(1)>", "'><svg onload=alert(1)>", "</pre>", "</td></tr></table>", "-->", "<!--", "javascript:alert(1)", "<a href=\"http://evil.example/\">", "&lt;", "&#x3c;",
            "\u{1b}[31m", "\u{0}", "\r\n", "<style>*{background:url(http://x/)}</style>", "<iframe src=//x>", " onclick=\"x()\" ", "192.168.0.1", "aa:bb:cc:dd:ee:ff", "`", "\\", "한글 <b>굵게</b>", "]]>", "<meta http-equiv=refresh content=0>",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..300 {
            let mut junk = |n: u64| -> String { (0..1 + next() % n).map(|_| bits[(next() % bits.len() as u64) as usize]).collect() };
            let (a, b, c, d) = (junk(4), junk(4), junk(3), junk(3));
            let src = Sources {
                version: c.clone(),
                host: "PC".into(),
                user: "u".into(),
                net: Some(Net { iface: Some(a.clone()), ip: Some(b.clone()), prefix: None, gateway: Some(c.clone()), dns: vec![d.clone(), a.clone()], link: Some(b.clone()), public_ip: Some(d.clone()), mac: Some(a.clone()) }),
                history: vec![(NOW - 5, entry(&a, &b))],
                audit: [
                    serde_json::json!({"time": NOW / 1000 - 1, "source": c, "tool": a, "argv": [b, d], "risk": c, "root": true, "confirmed": true}).to_string(),
                    serde_json::json!({"time": NOW / 1000 - 2, "source": "terminal", "kind": d, "target": a, "user": b}).to_string(),
                    serde_json::json!({"time": NOW / 1000 - 3, "source": "bridge", "tool": "rack.handoff", "argv": [a, c]}).to_string(),
                ]
                .join("\n"),
                captures: vec![capture("c.pcap", &a, &b), (crate::capview::CaptureFile { name: "d.pcap".into(), size: 1, modified: NOW / 1000 - 5 }, Err(c.clone()))],
                captures_total: 2,
                manual: [
                    serde_json::json!({"time": NOW - 5, "command": a, "tool": b, "state": c, "exit": 1, "secs": 1.5, "output": d, "cut": true}).to_string(),
                    serde_json::json!({"time": NOW - 6, "command": d, "state": "ok", "output": format!("{a}\n{b}")}).to_string(),
                ]
                .join("\n"),
            };
            let mut src = src;
            src.history.push((NOW - 6, map_entry(&d, &a)));
            for mask in [false, true] {
                let html = build(&req(&["mynet", "history", "topology", "capture", "manual", "audit"], mask), &src, NOW, &no_stop()).unwrap().html;
                if let Err(why) = check(&html) {
                    panic!("{round}회째(가리기 {mask}): {why}");
                }
                assert!(!html.contains('\u{1b}') && !html.contains('\u{0}'));
                assert_eq!(html.matches("<pre").count(), html.matches("</pre>").count());
                assert_eq!(html.matches("<table").count(), html.matches("</table>").count());
                assert_eq!(html.matches("<section").count(), html.matches("</section>").count());
            }
        }
    }
}
