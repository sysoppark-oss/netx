//! 결과 해설. 도구 출력을 읽어 한 줄 결론, 핵심 수치, 그래프 자료를 만든다.
//!
//! AI 없이 동작하는 규칙 기반 해석이다. 출력에 있는 값만 쓰고, 해석할 수 없으면 종료 상태만 알려 준다.

use serde::Serialize;

use crate::engine::StepStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Light {
    Green,
    Yellow,
    Red,
    /// 좋고 나쁨이 없는 조회 결과
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bar {
    pub label: String,
    pub value: f64,
    pub light: Light,
    /// 막대 옆에 덧붙일 글(예: 손실률)
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Chart {
    /// 항목별 막대 (mtr 홉별 지연, fping 장비별 응답 시간)
    Bars { title: String, unit: String, bars: Vec<Bar> },
    /// 시간 순서 선 (ping 응답 시간, iperf3 초당 전송 속도)
    Line { title: String, unit: String, points: Vec<f64> },
    /// 계기판 (iperf3 최종 속도)
    Gauge { title: String, unit: String, value: f64, max: f64 },
}

/// 결과에서 바로 접속해 볼 수 있는 대상(열린 포트)
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub service: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub light: Light,
    pub text: String,
    pub facts: Vec<Fact>,
    pub charts: Vec<Chart>,
    pub targets: Vec<Target>,
}

impl StepResult {
    fn new(light: Light, text: impl Into<String>) -> Self {
        StepResult { light, text: text.into(), facts: vec![], charts: vec![], targets: vec![] }
    }
    fn fact(mut self, label: &str, value: impl Into<String>) -> Self {
        self.facts.push(Fact { label: label.into(), value: value.into() });
        self
    }
    fn chart(mut self, chart: Chart) -> Self {
        self.charts.push(chart);
        self
    }
}

/// 끝난 단계의 출력을 해석한다.
pub fn interpret(program: &str, lines: &[String], status: StepStatus, exit_code: Option<i32>) -> StepResult {
    let parsed = match program {
        "ping" => ping(lines),
        "mtr" => mtr(lines),
        "nc" => nc(lines),
        "curl" => curl(lines).map(|r| match curl_timing(lines) {
            Some(c) => r.chart(c),
            None => r,
        }),
        "dig" => dig(lines, exit_code),
        "fping" => fping(lines),
        "iperf3" => iperf3(lines),
        "winports" => winports(lines),
        "portscan" => portscan(lines),
        "ss" => listing(lines, "WSL 안에서 이 조건에 맞는 소켓"),
        "lsof" => listing(lines, "WSL 안에서 이 포트를 연 프로세스"),
        "tcpdump" => tcpdump(lines),
        _ => None,
    };

    match (status, parsed) {
        (StepStatus::Error, _) => StepResult::new(Light::Red, "실행하지 못했습니다. 위 출력의 오류를 확인하세요."),
        (StepStatus::Timeout, Some(r)) => StepResult { text: format!("시간 제한에 걸려 중단했습니다. 그때까지의 결과: {}", r.text), light: worse(r.light, Light::Yellow), ..r },
        (StepStatus::Timeout, None) => StepResult::new(Light::Red, "시간 제한 안에 끝나지 않아 중단했습니다. 대상이 응답하지 않을 수 있습니다."),
        (StepStatus::Capped, _) => StepResult::new(Light::Yellow, "출력이 너무 많아 중단했습니다. 조건을 좁혀 다시 실행하세요."),
        (StepStatus::Cancelled, Some(r)) => StepResult { text: format!("중지했습니다. 그때까지의 결과: {}", r.text), ..r },
        (StepStatus::Cancelled, None) => StepResult::new(Light::Info, "중지했습니다."),
        (_, Some(r)) => r,
        (_, None) => match exit_code {
            Some(0) => StepResult::new(Light::Info, "끝났습니다. 출력을 확인하세요."),
            Some(c) => StepResult::new(Light::Yellow, format!("종료 코드 {c}로 끝났습니다. 출력의 오류 내용을 확인하세요.")),
            None => StepResult::new(Light::Yellow, "비정상적으로 끝났습니다."),
        },
    }
}

fn worse(a: Light, b: Light) -> Light {
    let rank = |l| match l {
        Light::Info => 0,
        Light::Green => 1,
        Light::Yellow => 2,
        Light::Red => 3,
    };
    if rank(a) >= rank(b) { a } else { b }
}

/// `key=값` 또는 `key 값` 꼴에서 숫자를 꺼낸다.
fn number_after<'a>(line: &'a str, key: &str) -> Option<f64> {
    let rest = &line[line.find(key)? + key.len()..];
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn ms(v: f64) -> String {
    if v >= 100.0 { format!("{v:.0}ms") } else if v >= 10.0 { format!("{v:.1}ms") } else { format!("{v:.2}ms") }
}

// ------------------------------------------------------------------ ping

fn ping(lines: &[String]) -> Option<StepResult> {
    let times: Vec<f64> = lines.iter().filter_map(|l| number_after(l, "time=")).collect();
    let summary = lines.iter().find(|l| l.contains("packets transmitted"))?;
    let sent = summary.split_whitespace().next()?.parse::<u32>().ok()?;
    let received = summary.split(',').nth(1)?.split_whitespace().next()?.parse::<u32>().ok()?;
    let loss = if sent == 0 { 100.0 } else { 100.0 * f64::from(sent - received.min(sent)) / f64::from(sent) };

    let rtt = lines.iter().find(|l| l.contains("min/avg/max")).and_then(|l| {
        let values: Vec<f64> = l.split('=').nth(1)?.trim().split('/').filter_map(|v| v.trim_end_matches(" ms").trim().parse().ok()).collect();
        (values.len() >= 3).then(|| (values[0], values[1], values[2]))
    });

    let (light, text) = match (received, loss) {
        (0, _) => (Light::Red, format!("{sent}개를 보냈지만 응답이 하나도 없습니다. 대상이 꺼져 있거나 핑을 막고 있습니다.")),
        (_, l) if l > 0.0 => (Light::Yellow, format!("{sent}개 중 {received}개만 응답했습니다(손실 {l:.0}%). 구간 어딘가에서 패킷이 버려집니다.")),
        _ => match rtt {
            Some((_, avg, max)) if max > avg * 3.0 && max > 50.0 => {
                (Light::Yellow, format!("모두 응답했지만 응답 시간이 고르지 않습니다(평균 {}, 최대 {}).", ms(avg), ms(max)))
            }
            Some((_, avg, _)) => (Light::Green, format!("{sent}개 모두 응답했습니다(손실 0%, 평균 {}).", ms(avg))),
            None => (Light::Green, format!("{sent}개 모두 응답했습니다(손실 0%).")),
        },
    };

    let mut r = StepResult::new(light, text).fact("보냄/받음", format!("{sent}/{received}")).fact("손실", format!("{loss:.0}%"));
    if let Some((min, avg, max)) = rtt {
        r = r.fact("최소", ms(min)).fact("평균", ms(avg)).fact("최대", ms(max));
    }
    if times.len() >= 2 {
        r = r.chart(Chart::Line { title: "응답 시간".into(), unit: "ms".into(), points: times });
    }
    Some(r)
}

// ------------------------------------------------------------------ mtr

struct Hop {
    n: u32,
    host: String,
    loss: f64,
    avg: f64,
    worst: f64,
}

fn mtr_hops(lines: &[String]) -> Vec<Hop> {
    lines
        .iter()
        .filter_map(|l| {
            let (n, rest) = l.trim_start().split_once(".|--")?;
            let n: u32 = n.trim().parse().ok()?;
            let cols: Vec<&str> = rest.split_whitespace().collect();
            // 뒤에서부터: StDev Wrst Best Avg Last Snt Loss% | 앞은 호스트(이름과 IP가 함께 올 수 있다)
            if cols.len() < 8 {
                return None;
            }
            let k = cols.len();
            let loss: f64 = cols[k - 7].trim_end_matches('%').parse().ok()?;
            let avg: f64 = cols[k - 4].parse().ok()?;
            let worst: f64 = cols[k - 2].parse().ok()?;
            Some(Hop { n, host: cols[..k - 7].join(" "), loss, avg, worst })
        })
        .collect()
}

fn mtr(lines: &[String]) -> Option<StepResult> {
    let hops = mtr_hops(lines);
    let last = hops.last()?;

    // 끝까지 이어지는 손실이 시작되는 홉을 찾는다. 중간 홉만의 손실은 라우터가 응답을 아끼는 것일 수 있다.
    let persistent_from = hops.iter().rposition(|h| h.loss <= 0.0).map(|i| i + 1).unwrap_or(0);
    let middle_only: Vec<u32> = hops[..hops.len() - 1].iter().filter(|h| h.loss > 0.0).map(|h| h.n).collect();
    // 지연이 크게 뛰는 구간
    let jump = hops.windows(2).filter(|w| w[1].avg - w[0].avg > 100.0 && w[1].avg > w[0].avg * 3.0).map(|w| &w[1]).next();

    let (light, text) = if last.loss >= 100.0 {
        (Light::Red, format!("목적지({}번 홉)가 응답하지 않습니다. {}번 홉 이후로 응답이 끊깁니다.", last.n, persistent_from.max(1)))
    } else if last.loss > 0.0 {
        let from = &hops[persistent_from.min(hops.len() - 1)];
        (Light::Red, format!("목적지까지 손실 {:.0}%입니다. {}번 홉({})부터 끝까지 손실이 이어집니다. 이 구간이 원인으로 보입니다.", last.loss, from.n, from.host))
    } else if let Some(h) = jump {
        (Light::Yellow, format!("손실은 없지만 {}번 홉({})에서 지연이 크게 늘어납니다(평균 {}).", h.n, h.host, ms(h.avg)))
    } else if !middle_only.is_empty() {
        (
            Light::Green,
            format!(
                "목적지까지 손실이 없습니다(평균 {}). {}번 홉의 손실은 목적지에 영향이 없어, 라우터가 응답을 제한하는 것으로 보입니다.",
                ms(last.avg),
                middle_only.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            ),
        )
    } else {
        (Light::Green, format!("{}개 홉을 거쳐 목적지까지 손실 없이 도달합니다(평균 {}).", hops.len(), ms(last.avg)))
    };

    let bars = hops
        .iter()
        .map(|h| Bar {
            label: format!("{}. {}", h.n, h.host),
            value: h.avg,
            light: if h.loss >= 100.0 || (h.loss > 0.0 && h.n as usize > persistent_from) { Light::Red } else if h.loss > 0.0 { Light::Yellow } else { Light::Green },
            note: if h.loss > 0.0 { format!("손실 {:.0}%", h.loss) } else { String::new() },
        })
        .collect();

    Some(
        StepResult::new(light, text)
            .fact("홉 수", hops.len().to_string())
            .fact("목적지 손실", format!("{:.0}%", last.loss))
            .fact("목적지 평균", ms(last.avg))
            .fact("가장 느린 응답", ms(hops.iter().map(|h| h.worst).fold(0.0, f64::max)))
            .chart(Chart::Bars { title: "홉별 평균 지연".into(), unit: "ms".into(), bars }),
    )
}

// ------------------------------------------------------------------ nc

fn nc(lines: &[String]) -> Option<StepResult> {
    let all = lines.join("\n").to_lowercase();
    let open = lines.iter().filter(|l| l.contains("succeeded")).count();
    if open > 1 {
        return Some(StepResult::new(Light::Green, format!("포트 {open}개가 열려 있습니다.")).fact("열린 포트 수", open.to_string()));
    }
    Some(if open == 1 {
        StepResult::new(Light::Green, "포트가 열려 있습니다. 연결(L4)에는 문제가 없습니다.")
    } else if all.contains("refused") {
        StepResult::new(Light::Red, "연결이 거부되었습니다. 대상은 살아 있지만 이 포트에서 듣는 서비스가 없습니다.")
    } else if all.contains("timed out") {
        StepResult::new(Light::Red, "응답이 없습니다. 방화벽이 막고 있거나 대상이 꺼져 있습니다.")
    } else if all.contains("name or service not known") || all.contains("could not resolve") || all.contains("getaddrinfo") {
        StepResult::new(Light::Red, "이름을 주소로 바꾸지 못했습니다. DNS 문제입니다.")
    } else if all.contains("no route") || all.contains("unreachable") {
        StepResult::new(Light::Red, "대상까지 가는 경로가 없습니다. 라우팅이나 게이트웨이를 확인하세요.")
    } else {
        return None;
    })
}

// ------------------------------------------------------------------ curl

fn curl(lines: &[String]) -> Option<StepResult> {
    // 프록시 응답(200 Connection Established) 뒤에 실제 응답이 오므로 마지막 상태 줄을 쓴다.
    let status_line = lines.iter().rev().find_map(|l| {
        let l = l.trim_start_matches("< ").trim();
        let rest = l.strip_prefix("HTTP/")?;
        let code: u16 = rest.split_whitespace().nth(1)?.parse().ok()?;
        Some((code, rest.split_whitespace().skip(2).collect::<Vec<_>>().join(" ")))
    });
    let expire = lines.iter().find_map(|l| l.split_once("expire date:").map(|(_, d)| d.trim().to_string()));
    let all = lines.join("\n").to_lowercase();

    let mut r = match status_line {
        Some((code, reason)) => {
            let (light, text) = match code {
                200..=299 => (Light::Green, format!("웹 서버가 정상 응답했습니다(HTTP {code}).")),
                300..=399 => (Light::Green, format!("웹 서버가 다른 주소로 안내합니다(HTTP {code}). 서버는 정상입니다.")),
                401 | 403 => (Light::Yellow, format!("서버는 살아 있지만 접근을 거부합니다(HTTP {code}). 권한이나 접근 제한 설정을 확인하세요.")),
                404 => (Light::Yellow, "서버는 살아 있지만 그 주소에 페이지가 없습니다(HTTP 404).".to_string()),
                400..=499 => (Light::Yellow, format!("서버가 요청을 받아들이지 않습니다(HTTP {code}).")),
                502 | 503 | 504 => (Light::Red, format!("웹 서버 앞단은 응답하지만 뒤쪽 애플리케이션이 응답하지 않습니다(HTTP {code}). 네트워크가 아니라 서버 쪽(L7) 문제입니다.")),
                _ => (Light::Red, format!("서버 내부 오류입니다(HTTP {code}). 서버 쪽(L7) 문제입니다.")),
            };
            StepResult::new(light, text).fact("HTTP 상태", format!("{code} {reason}").trim().to_string())
        }
        None if all.contains("could not resolve host") => StepResult::new(Light::Red, "이름을 주소로 바꾸지 못했습니다. DNS 문제입니다."),
        None if all.contains("connection refused") => StepResult::new(Light::Red, "연결이 거부되었습니다. 웹 서버가 꺼져 있거나 포트가 다릅니다."),
        None if all.contains("timed out") => StepResult::new(Light::Red, "응답이 없습니다. 방화벽이 막고 있거나 서버가 응답하지 않습니다."),
        None if all.contains("ssl") || all.contains("certificate") => StepResult::new(Light::Red, "TLS 연결에 실패했습니다. 인증서나 TLS 설정 문제입니다."),
        None => return None,
    };

    if let Some(date) = expire {
        r = r.fact("인증서 만료", date.clone());
        if let Some(days) = days_until(&date) {
            r = r.fact("남은 기간", format!("{days}일"));
            if days < 0 {
                r.light = Light::Red;
                r.text.push_str(" 인증서가 만료되었습니다.");
            } else if days < 14 {
                r.light = worse(r.light, Light::Yellow);
                r.text.push_str(&format!(" 인증서가 {days}일 뒤 만료됩니다."));
            }
        }
    }
    Some(r)
}

/// "Nov 10 11:15:20 2027 GMT" 꼴의 날짜까지 남은 날 수.
fn days_until(date: &str) -> Option<i64> {
    let p: Vec<&str> = date.split_whitespace().collect();
    if p.len() < 4 {
        return None;
    }
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| *m == p[0])? as i64 + 1;
    let day: i64 = p[1].parse().ok()?;
    let year: i64 = p[3].parse().ok()?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64 / 86_400;
    Some(days_from_civil(year, month, day) - now)
}

/// 1970-01-01부터의 날 수 (그레고리력).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ------------------------------------------------------------------ dig

fn dig(lines: &[String], exit_code: Option<i32>) -> Option<StepResult> {
    let all = lines.join("\n");
    if all.contains("timed out") || all.contains("no servers could be reached") {
        return Some(StepResult::new(Light::Red, "DNS 서버가 응답하지 않습니다. DNS 서버 주소나 네트워크를 확인하세요."));
    }
    // +short 출력: 줄마다 값 하나
    let answers: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with(';')).collect();
    let long_form = lines.iter().any(|l| l.starts_with(";;"));
    if long_form {
        let status = lines.iter().find_map(|l| l.split_once("status: ").map(|(_, s)| s.split(',').next().unwrap_or("").to_string()))?;
        return Some(match status.as_str() {
            "NOERROR" => StepResult::new(Light::Green, "DNS 서버가 정상 응답했습니다.").fact("상태", status),
            "NXDOMAIN" => StepResult::new(Light::Red, "그런 이름이 없습니다(NXDOMAIN). 도메인 철자나 등록 상태를 확인하세요.").fact("상태", status),
            _ => StepResult::new(Light::Yellow, format!("DNS 서버가 오류를 돌려주었습니다({status}).")).fact("상태", status),
        });
    }
    if exit_code != Some(0) {
        return None;
    }
    Some(if answers.is_empty() {
        StepResult::new(Light::Red, "이름에 해당하는 주소가 없습니다. DNS 등록을 확인하세요.")
    } else {
        StepResult::new(Light::Green, format!("이름이 주소로 풀립니다: {}", answers.join(", "))).fact("응답 수", answers.len().to_string())
    })
}

// ------------------------------------------------------------------ fping

fn fping(lines: &[String]) -> Option<StepResult> {
    // "192.168.0.1 (0.52 ms)" 또는 "192.168.0.1"
    let hosts: Vec<(String, Option<f64>)> = lines
        .iter()
        .filter_map(|l| {
            let l = l.trim();
            let host = l.split_whitespace().next()?;
            if host.parse::<std::net::IpAddr>().is_err() || l.contains("unreachable") || l.contains("error") {
                return None;
            }
            Some((host.to_string(), number_after(l, "(")))
        })
        .collect();
    if hosts.is_empty() {
        return Some(StepResult::new(Light::Yellow, "응답한 장비가 없습니다. 대역이 맞는지, 장비들이 핑을 막고 있지 않은지 확인하세요."));
    }
    let mut bars: Vec<Bar> =
        hosts.iter().map(|(h, t)| Bar { label: h.clone(), value: t.unwrap_or(0.0), light: Light::Green, note: String::new() }).collect();
    bars.truncate(64);
    Some(
        StepResult::new(Light::Green, format!("장비 {}대가 응답했습니다. 핑을 막아 둔 장비는 여기에 나오지 않습니다.", hosts.len()))
            .fact("응답한 장비", format!("{}대", hosts.len()))
            .chart(Chart::Bars { title: "장비별 응답 시간".into(), unit: "ms".into(), bars }),
    )
}

// ------------------------------------------------------------------ iperf3

/// "941 Mbits/sec" 같은 표기를 Mbps로 바꾼다.
fn bitrate_mbps(line: &str) -> Option<f64> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    let i = cols.iter().position(|c| c.ends_with("bits/sec"))?;
    let value: f64 = cols.get(i.checked_sub(1)?)?.parse().ok()?;
    Some(match cols[i].chars().next()? {
        'G' => value * 1000.0,
        'M' => value,
        'K' => value / 1000.0,
        _ => value / 1_000_000.0,
    })
}

fn iperf3(lines: &[String]) -> Option<StepResult> {
    if let Some(err) = lines.iter().find(|l| l.contains("iperf3: error")) {
        let text = if err.contains("busy") || err.contains("control message") || err.contains("reset by peer") {
            "서버가 다른 사람의 측정을 처리하는 중입니다. 서버 포트를 5202~5210 중 하나로 바꾸거나 다른 서버를 골라 다시 실행하세요."
        } else if err.contains("unable to connect") || err.contains("refused") || err.contains("timed out") {
            "iperf3 서버에 연결하지 못했습니다. 공개 서버라면 포트를 바꾸거나 다른 서버를 고르고, 사내 서버라면 `iperf3 -s`가 실행 중인지와 포트가 열려 있는지 확인하세요."
        } else {
            "측정에 실패했습니다. 출력의 오류 내용을 확인하세요."
        };
        return Some(StepResult::new(Light::Red, text));
    }
    // 연결이 여러 개면 [SUM] 줄을, 하나면 일반 줄을 쓴다.
    let has_sum = lines.iter().any(|l| l.starts_with("[SUM]"));
    let rows: Vec<&String> = lines.iter().filter(|l| if has_sum { l.starts_with("[SUM]") } else { l.starts_with('[') && l.contains("sec") && !l.contains("ID]") }).collect();
    let is_final = |l: &str| l.contains("sender") || l.contains("receiver");
    let points: Vec<f64> = rows.iter().filter(|l| !is_final(l)).filter_map(|l| bitrate_mbps(l)).collect();
    let receiver = rows.iter().find(|l| l.contains("receiver")).and_then(|l| bitrate_mbps(l));
    let sender = rows.iter().find(|l| l.contains("sender")).and_then(|l| bitrate_mbps(l));
    let rate = receiver.or(sender).or_else(|| (!points.is_empty()).then(|| points.iter().sum::<f64>() / points.len() as f64))?;

    let pretty = |v: f64| if v >= 1000.0 { format!("{:.2} Gbps", v / 1000.0) } else { format!("{v:.1} Mbps") };
    let min = points.iter().copied().fold(f64::INFINITY, f64::min);
    let unstable = points.len() >= 3 && min < rate * 0.5;
    let (light, text) = if unstable {
        (Light::Yellow, format!("평균 {}이지만 속도가 크게 출렁입니다(최저 {}). 구간이 혼잡하거나 불안정합니다.", pretty(rate), pretty(min)))
    } else {
        (Light::Green, format!("실제 전송 속도는 {}입니다. 회선 속도와 비교해 보세요.", pretty(rate)))
    };

    let mut r = StepResult::new(light, text).fact("받은 쪽 속도", pretty(rate));
    if let Some(s) = sender {
        r = r.fact("보낸 쪽 속도", pretty(s));
    }
    let max = [100.0, 1000.0, 2500.0, 10_000.0, 100_000.0].into_iter().find(|m| rate <= *m).unwrap_or(rate);
    r = r.chart(Chart::Gauge { title: "전송 속도".into(), unit: "Mbps".into(), value: rate, max });
    if points.len() >= 2 {
        r = r.chart(Chart::Line { title: "초당 전송 속도".into(), unit: "Mbps".into(), points });
    }
    Some(r)
}

// ------------------------------------------------------------------ 조회 도구

fn winports(lines: &[String]) -> Option<StepResult> {
    if lines.iter().any(|l| l.contains("쓰는 프로그램이 없습니다")) {
        return Some(StepResult::new(Light::Info, "Windows에서 이 포트를 쓰는 프로그램이 없습니다."));
    }
    let rows: Vec<Vec<&str>> = lines.iter().filter(|l| l.starts_with("TCP") || l.starts_with("UDP")).map(|l| l.split_whitespace().collect()).collect();
    if rows.is_empty() {
        return None;
    }
    let mut owners: Vec<String> = rows.iter().filter(|c| c.len() >= 4).map(|c| format!("{}(PID {})", c[3], c[2])).collect();
    owners.sort();
    owners.dedup();
    let shown = owners.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    let more = if owners.len() > 5 { format!(" 외 {}개", owners.len() - 5) } else { String::new() };
    // 프로그램별 소켓 수
    let mut counts: Vec<(String, usize)> = Vec::new();
    for c in rows.iter().filter(|c| c.len() >= 4) {
        match counts.iter_mut().find(|(n, _)| n == c[3]) {
            Some(e) => e.1 += 1,
            None => counts.push((c[3].to_string(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    let bars: Vec<Bar> = counts.iter().take(12).map(|(n, k)| Bar { label: n.clone(), value: *k as f64, light: Light::Info, note: String::new() }).collect();
    let mut r = StepResult::new(Light::Info, format!("Windows에서 {shown}{more}이(가) 쓰고 있습니다.")).fact("소켓 수", rows.len().to_string()).fact("프로그램 수", owners.len().to_string());
    if bars.len() > 1 {
        r = r.chart(Chart::Bars { title: "프로그램별 소켓 수".into(), unit: "개".into(), bars });
    }
    Some(r)
}

/// 내장 포트 스캔: "192.168.0.1  open  22(SSH) 80(HTTP)"
fn portscan(lines: &[String]) -> Option<StepResult> {
    let mut hosts: Vec<(String, Vec<u16>)> = Vec::new();
    for l in lines {
        let mut cols = l.split_whitespace();
        let (Some(host), Some("open")) = (cols.next(), cols.next()) else { continue };
        if host.parse::<std::net::IpAddr>().is_err() {
            continue;
        }
        let ports: Vec<u16> = cols.filter_map(|p| p.split('(').next()?.parse().ok()).collect();
        if !ports.is_empty() {
            hosts.push((host.to_string(), ports));
        }
    }
    if hosts.is_empty() {
        if !lines.iter().any(|l| l.starts_with("스캔 끝")) {
            return None;
        }
        return Some(StepResult::new(Light::Yellow, "고른 포트가 열려 있는 장비를 찾지 못했습니다. 포트가 닫혀 있거나 방화벽이 막고 있을 수 있습니다."));
    }
    hosts.sort_by_key(|(h, _)| h.parse::<std::net::Ipv4Addr>().map(u32::from).unwrap_or(u32::MAX));
    let total: usize = hosts.iter().map(|(_, p)| p.len()).sum();
    let bars: Vec<Bar> = hosts
        .iter()
        .take(64)
        .map(|(h, p)| Bar { label: h.clone(), value: p.len() as f64, light: Light::Info, note: p.iter().map(u16::to_string).collect::<Vec<_>>().join(", ") })
        .collect();
    let targets: Vec<Target> = hosts
        .iter()
        .flat_map(|(h, p)| p.iter().map(move |port| Target { host: h.clone(), port: *port, service: crate::engine::builtin::service_name(*port).to_string() }))
        .take(200)
        .collect();
    let mut r = StepResult::new(Light::Green, format!("장비 {}대에서 열린 포트 {}개를 찾았습니다. 아래 목록에서 바로 접속해 볼 수 있습니다.", hosts.len(), total))
        .fact("장비", format!("{}대", hosts.len()))
        .fact("열린 포트", format!("{total}개"))
        .chart(Chart::Bars { title: "장비별 열린 포트 수".into(), unit: "개".into(), bars });
    r.targets = targets;
    Some(r)
}

/// curl -w로 받은 단계별 누적 시간을 구간별 시간(ms) 막대로 바꾼다.
fn curl_timing(lines: &[String]) -> Option<Chart> {
    let line = lines.iter().find(|l| l.starts_with("netx_times "))?;
    let get = |key: &str| line.split_whitespace().find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')?.parse::<f64>().ok());
    let (dns, connect, tls, ttfb, total) = (get("dns")?, get("connect")?, get("tls")?, get("ttfb")?, get("total")?);
    if total <= 0.0 {
        return None;
    }
    // tls가 0이면 HTTP(암호화 없음)
    let tls_end = if tls > 0.0 { tls } else { connect };
    let stages = [
        ("이름 풀이(DNS)", dns),
        ("TCP 연결", connect - dns),
        ("TLS 협상", tls_end - connect),
        ("서버 응답 대기", ttfb - tls_end),
        ("내려받기", total - ttfb),
    ];
    let bars = stages
        .iter()
        .map(|(label, secs)| {
            let v = (secs.max(0.0) * 1000.0 * 10.0).round() / 10.0;
            let light = if v >= 1000.0 { Light::Red } else if v >= 300.0 { Light::Yellow } else { Light::Green };
            Bar { label: label.to_string(), value: v, light, note: String::new() }
        })
        .collect();
    Some(Chart::Bars { title: format!("단계별 걸린 시간 (전체 {:.0}ms)", total * 1000.0), unit: "ms".into(), bars })
}

fn listing(lines: &[String], what: &str) -> Option<StepResult> {
    let rows = lines.iter().filter(|l| !l.trim().is_empty()).count().saturating_sub(1);
    let header = lines.first().is_some_and(|l| l.starts_with("Netid") || l.starts_with("State") || l.starts_with("COMMAND"));
    if !header && !lines.is_empty() {
        return None;
    }
    Some(if rows == 0 {
        StepResult::new(Light::Info, format!("{what}이(가) 없습니다."))
    } else {
        StepResult::new(Light::Info, format!("{what} {rows}개를 찾았습니다. 출력의 프로세스 이름과 번호를 확인하세요.")).fact("찾은 수", rows.to_string())
    })
}

fn tcpdump(lines: &[String]) -> Option<StepResult> {
    let captured = lines.iter().find_map(|l| l.strip_suffix(" packets captured").or_else(|| l.strip_suffix(" packet captured")))?.trim().parse::<u64>().ok()?;
    let dropped = lines.iter().find_map(|l| l.strip_suffix(" packets dropped by kernel")).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    let mut r = if captured == 0 {
        StepResult::new(Light::Yellow, "조건에 맞는 패킷이 하나도 잡히지 않았습니다. 인터페이스와 필터를 확인하세요. WSL에서는 Windows 본체의 트래픽이 보이지 않을 수 있습니다.")
    } else {
        StepResult::new(Light::Info, format!("패킷 {captured}개를 캡처했습니다. 작업 폴더의 pcap 파일을 Wireshark로 열어 볼 수 있습니다."))
    };
    r = r.fact("캡처", format!("{captured}개"));
    if dropped > 0 {
        r = r.fact("놓침", format!("{dropped}개"));
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }
    fn done(program: &str, text: &str, code: i32) -> StepResult {
        interpret(program, &lines(text), StepStatus::Done, Some(code))
    }

    #[test]
    fn ping_ok_loss_and_dead() {
        let ok = done("ping", "PING 8.8.8.8 (8.8.8.8) 56(84) bytes of data.\n64 bytes from 8.8.8.8: icmp_seq=1 ttl=115 time=31.2 ms\n64 bytes from 8.8.8.8: icmp_seq=2 ttl=115 time=30.8 ms\n\n--- 8.8.8.8 ping statistics ---\n2 packets transmitted, 2 received, 0% packet loss, time 1001ms\nrtt min/avg/max/mdev = 30.8/31.0/31.2/0.2 ms", 0);
        assert_eq!(ok.light, Light::Green);
        assert!(ok.text.contains("평균 31.0ms"), "{}", ok.text);
        assert!(matches!(&ok.charts[0], Chart::Line { points, .. } if points == &[31.2, 30.8]));

        let loss = done("ping", "--- x ping statistics ---\n5 packets transmitted, 3 received, 40% packet loss, time 4005ms\nrtt min/avg/max/mdev = 1.0/2.0/3.0/0.5 ms", 1);
        assert_eq!(loss.light, Light::Yellow);
        assert!(loss.text.contains("손실 40%"));

        let dead = done("ping", "--- x ping statistics ---\n5 packets transmitted, 0 received, 100% packet loss, time 4100ms", 1);
        assert_eq!(dead.light, Light::Red);
    }

    const MTR_HEAD: &str = "Start: 2026-11-10T23:27:28+0900\nHOST: DESKTOP                         Loss%   Snt   Last   Avg  Best  Wrst StDev\n";

    #[test]
    fn mtr_clean_path() {
        let r = done("mtr", &format!("{MTR_HEAD}  1.|-- gw.mshome.net (172.26.224.1)  0.0%    10    0.5   0.8   0.4   2.4   0.6\n  2.|-- 192.168.123.1                 0.0%    10    5.3   3.9   2.8   6.5   1.2\n  3.|-- 8.8.8.8                       0.0%    10   35.0  36.1  34.0  40.2   1.9"), 0);
        assert_eq!(r.light, Light::Green, "{}", r.text);
        assert!(r.text.contains("3개 홉"));
        let Chart::Bars { bars, .. } = &r.charts[0] else { panic!() };
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0].label, "1. gw.mshome.net (172.26.224.1)");
        assert_eq!(bars[2].value, 36.1);
    }

    #[test]
    fn mtr_middle_loss_is_not_blamed() {
        let r = done("mtr", &format!("{MTR_HEAD}  1.|-- 192.168.0.1   0.0%  10  1.0  1.0  1.0  1.0  0.0\n  2.|-- 1.208.165.133 80.0%  10 18.9 15.8 12.6 18.9  4.5\n  3.|-- 8.8.8.8       0.0%  10 35.0 36.1 34.0 40.2  1.9"), 0);
        assert_eq!(r.light, Light::Green, "{}", r.text);
        assert!(r.text.contains("2번 홉의 손실"));
        let Chart::Bars { bars, .. } = &r.charts[0] else { panic!() };
        assert_eq!(bars[1].light, Light::Yellow);
    }

    #[test]
    fn mtr_persistent_loss_names_the_hop() {
        let r = done("mtr", &format!("{MTR_HEAD}  1.|-- 192.168.0.1   0.0%  10  1.0  1.0  1.0  1.0  0.0\n  2.|-- 10.1.1.1     30.0%  10 18.9 15.8 12.6 18.9  4.5\n  3.|-- 8.8.8.8      30.0%  10 35.0 36.1 34.0 40.2  1.9"), 0);
        assert_eq!(r.light, Light::Red);
        assert!(r.text.contains("2번 홉(10.1.1.1)부터"), "{}", r.text);

        let dead = done("mtr", &format!("{MTR_HEAD}  1.|-- 192.168.0.1   0.0%  10  1.0  1.0  1.0  1.0  0.0\n  2.|-- ???          100.0   10  0.0  0.0  0.0  0.0  0.0"), 0);
        assert_eq!(dead.light, Light::Red);
        assert!(dead.text.contains("응답하지 않습니다"));
    }

    #[test]
    fn nc_states() {
        assert_eq!(done("nc", "Connection to example.com (93.184.216.34) 443 port [tcp/https] succeeded!", 0).light, Light::Green);
        assert!(done("nc", "nc: connect to 127.0.0.1 port 9 (tcp) failed: Connection refused", 1).text.contains("거부"));
        assert!(done("nc", "nc: connect to 10.0.0.9 port 443 (tcp) timed out: Operation now in progress", 1).text.contains("방화벽"));
    }

    #[test]
    fn curl_status_and_certificate() {
        let ok = done("curl", "*  subject: CN=github.com\n*  expire date: Nov 10 11:15:20 2099 GMT\n< HTTP/1.1 200 Connection Established\n< HTTP/2 200 \nHTTP/2 200 ", 0);
        assert_eq!(ok.light, Light::Green);
        assert!(ok.facts.iter().any(|f| f.label == "인증서 만료"));

        let bad = done("curl", "< HTTP/1.1 502 Bad Gateway\nHTTP/1.1 502 Bad Gateway", 0);
        assert_eq!(bad.light, Light::Red);
        assert!(bad.text.contains("L7"));

        let expired = done("curl", "*  expire date: Jan  1 00:00:00 2020 GMT\nHTTP/1.1 200 OK", 0);
        assert_eq!(expired.light, Light::Red);
        assert!(expired.text.contains("만료되었습니다"));

        assert!(done("curl", "curl: (6) Could not resolve host: nope.invalid", 6).text.contains("DNS"));
        assert!(done("curl", "curl: (7) Failed to connect to 127.0.0.1 port 9: Connection refused", 7).text.contains("거부"));
    }

    #[test]
    fn days_from_civil_matches_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 11, 11), 20_768);
    }

    #[test]
    fn dig_short_answers() {
        assert_eq!(done("dig", "93.184.216.34\n", 0).light, Light::Green);
        assert_eq!(done("dig", "", 0).light, Light::Red);
        assert!(done("dig", ";; communications error to 10.0.0.1#53: timed out\n;; no servers could be reached", 9).text.contains("응답하지 않습니다"));
        assert!(done("dig", ";; ->>HEADER<<- opcode: QUERY, status: NXDOMAIN, id: 1", 0).text.contains("NXDOMAIN"));
    }

    #[test]
    fn fping_hosts() {
        let r = done("fping", "192.168.0.1 (0.52 ms)\n192.168.0.7 (3.10 ms)\n", 1);
        assert!(r.text.contains("2대"));
        let Chart::Bars { bars, .. } = &r.charts[0] else { panic!() };
        assert_eq!(bars[1].value, 3.10);
        assert_eq!(done("fping", "", 1).light, Light::Yellow);
    }

    #[test]
    fn portscan_lists_targets_and_curl_shows_stage_times() {
        let r = done("portscan", "장비 254대 × 포트 3개에 TCP 연결을 시도합니다.\n192.168.0.20  open  22(SSH) 80(HTTP)\n192.168.0.1  open  80(HTTP) 443(HTTPS) 8081\n스캔 끝: 열린 포트가 있는 장비 2대 / 전체 254대", 0);
        assert_eq!(r.light, Light::Green);
        assert_eq!(r.targets.len(), 5);
        assert_eq!((r.targets[0].host.as_str(), r.targets[0].port, r.targets[0].service.as_str()), ("192.168.0.1", 80, "HTTP"));
        assert_eq!(r.targets[4].port, 80);
        assert_eq!(done("portscan", "스캔 끝: 열린 포트가 있는 장비 0대 / 전체 254대", 1).light, Light::Yellow);

        let r = done("curl", "HTTP/2 200 \n\nnetx_times dns=0.010 connect=0.030 tls=0.090 ttfb=0.400 total=0.450", 0);
        let Some(Chart::Bars { bars, .. }) = r.charts.last() else { panic!("단계별 시간 그래프가 없다") };
        assert_eq!(bars.iter().map(|b| b.value).collect::<Vec<_>>(), [10.0, 20.0, 60.0, 310.0, 50.0]);
        assert_eq!(bars[3].light, Light::Yellow);
    }

    #[test]
    fn iperf3_sum_lines_and_errors() {
        let out = "[  5]   0.00-1.00   sec  2.33 GBytes  20.0 Gbits/sec    0    895 KBytes\n[SUM]   0.00-1.00   sec  112 MBytes   940 Mbits/sec    0\n[SUM]   1.00-2.00   sec  113 MBytes   950 Mbits/sec    0\n[SUM]   0.00-2.00   sec  225 MBytes   945 Mbits/sec    0             sender\n[SUM]   0.00-2.00   sec  224 MBytes   942 Mbits/sec                  receiver\n\niperf Done.";
        let r = done("iperf3", out, 0);
        assert_eq!(r.light, Light::Green);
        assert!(r.text.contains("942.0 Mbps"), "{}", r.text);
        assert!(matches!(&r.charts[0], Chart::Gauge { value, max, .. } if *value == 942.0 && *max == 1000.0));
        assert!(matches!(&r.charts[1], Chart::Line { points, .. } if points == &[940.0, 950.0]));

        let single = done("iperf3", "[  5]   0.00-1.00   sec  11.2 MBytes  94.1 Mbits/sec\n[  5]   1.00-2.00   sec  1.2 MBytes  10.0 Mbits/sec\n[  5]   2.00-3.00   sec  11.2 MBytes  94.0 Mbits/sec\n[  5]   0.00-3.00   sec  23.6 MBytes  66.0 Mbits/sec   receiver", 0);
        assert_eq!(single.light, Light::Yellow, "{}", single.text);

        let err = done("iperf3", "iperf3: error - unable to connect to server - server may have stopped running: Connection refused", 1);
        assert_eq!(err.light, Light::Red);
        assert!(err.text.contains("iperf3 -s"));
    }

    #[test]
    fn lookups_and_fallbacks() {
        assert!(done("winports", "PROTO LOCAL  PID  PROCESS  STATE\nTCP   :::445  4  System  수신 대기", 0).text.contains("System(PID 4)"));
        assert!(done("winports", "Windows에서 포트 9 을(를) 쓰는 프로그램이 없습니다.", 0).text.contains("없습니다"));
        assert!(done("ss", "Netid State Recv-Q Send-Q Local Address:Port Peer Address:PortProcess", 0).text.contains("없습니다"));
        assert!(done("lsof", "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nsshd 1 root 3u IPv4 1 0t0 TCP *:22 (LISTEN)", 0).text.contains("1개"));
        assert!(done("tcpdump", "tcpdump: listening on eth0\n0 packets captured\n0 packets received by filter\n0 packets dropped by kernel", 0).text.contains("잡히지 않았습니다"));

        assert_eq!(done("unknown", "whatever", 0).light, Light::Info);
        assert_eq!(done("unknown", "whatever", 2).light, Light::Yellow);
        assert_eq!(interpret("ping", &[], StepStatus::Timeout, Some(124)).light, Light::Red);
        assert_eq!(interpret("ping", &[], StepStatus::Error, None).light, Light::Red);
        assert!(interpret("tcpdump", &lines("3 packets captured"), StepStatus::Cancelled, Some(0)).text.starts_with("중지했습니다"));
    }
}
