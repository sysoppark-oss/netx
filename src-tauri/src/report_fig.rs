//! 내보내는 보고서에 넣는 그림. 기록에 남은 그림 자료(JSON)에서 스크립트 없는 HTML·SVG를 만든다.
//! 화면의 그림과 모양은 다르지만 같은 자료를 보여 준다. 자료 속 글자는 모두 바꿔 쓴다.

use serde_json::Value;

fn esc(text: &str) -> String {
    crate::history::esc(text)
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").chars().take(200).collect()
}

fn f(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(0.0)
}

fn list<'a>(v: &'a Value, key: &str, max: usize) -> Vec<&'a Value> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().take(max).collect()).unwrap_or_default()
}

fn color(light: &str) -> &'static str {
    match light {
        "green" => "#1a8f4c",
        "yellow" => "#b7791f",
        "red" => "#c53030",
        _ => "#0b6b6b",
    }
}

fn num(x: f64) -> String {
    if x.abs() >= 100.0 || x.fract() == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x:.1}")
    }
}

fn bytes_text(n: f64) -> String {
    if n >= 1_048_576.0 {
        format!("{:.1} MB", n / 1_048_576.0)
    } else if n >= 1024.0 {
        format!("{:.1} KB", n / 1024.0)
    } else {
        format!("{n:.0} B")
    }
}

fn bar_row(label: &str, share: f64, fill: &str, value: &str) -> String {
    format!(
        "<tr><td class=\"bl\">{}</td><td class=\"bb\"><div style=\"width:{:.1}%;background:{fill}\"></div></td><td class=\"bv\">{}</td></tr>",
        esc(label),
        (share * 100.0).clamp(1.0, 100.0),
        esc(value)
    )
}

fn bars(c: &Value, unit: &str) -> String {
    let items = list(c, "bars", 60);
    let max = items.iter().map(|b| f(b, "value")).fold(0.001, f64::max);
    let rows: String = items
        .iter()
        .map(|b| {
            let note = s(b, "note");
            let value = format!("{} {unit}{}", num(f(b, "value")), if note.is_empty() { String::new() } else { format!(" · {note}") });
            bar_row(&s(b, "label"), f(b, "value") / max, color(&s(b, "light")), &value)
        })
        .collect();
    format!("<table class=\"bars\">{rows}</table>")
}

fn line(c: &Value, unit: &str) -> String {
    let points: Vec<f64> = list(c, "points", 600).iter().filter_map(|p| p.as_f64()).filter(|x| x.is_finite()).collect();
    if points.is_empty() {
        return String::new();
    }
    let (min, max) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let avg = points.iter().sum::<f64>() / points.len() as f64;
    let span = (max - min).max(0.001);
    let step = if points.len() > 1 { 580.0 / (points.len() - 1) as f64 } else { 0.0 };
    let path: Vec<String> = points.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", 10.0 + step * i as f64, 100.0 - (p - min) / span * 85.0)).collect();
    format!(
        "<svg viewBox=\"0 0 600 110\" class=\"fig\"><rect width=\"600\" height=\"110\" fill=\"#f3f5f8\"/><polyline points=\"{}\" fill=\"none\" stroke=\"#0b6b6b\" stroke-width=\"2\"/></svg>\
<p class=\"m\">최저 {} · 평균 {} · 최고 {} {unit} ({}개)</p>",
        path.join(" "),
        num(min),
        num(avg),
        num(max),
        points.len()
    )
}

fn gauge(c: &Value, unit: &str) -> String {
    let (value, max) = (f(c, "value"), f(c, "max").max(0.001));
    format!("<table class=\"bars\">{}</table>", bar_row("측정값", value / max, "#0b6b6b", &format!("{} {unit} (눈금 끝 {})", num(value), num(max))))
}

fn route(c: &Value) -> String {
    let hops = list(c, "hops", 64);
    let from = c.get("from").and_then(Value::as_u64).unwrap_or(0);
    let per_row = 8usize;
    let rows = hops.len().div_ceil(per_row).max(1);
    let mut svg = format!("<svg viewBox=\"0 0 660 {}\" class=\"fig\"><rect width=\"660\" height=\"{}\" fill=\"#f3f5f8\"/>", rows * 70 + 10, rows * 70 + 10);
    let mut table = String::from("<table><tr><th>홉</th><th>장비</th><th>손실</th><th>평균</th><th>가장 느림</th></tr>");
    let mut last: Option<(f64, f64)> = None;
    for (i, h) in hops.iter().enumerate() {
        let n = h.get("n").and_then(Value::as_u64).unwrap_or(i as u64 + 1);
        let loss = f(h, "loss");
        let tone = if from > 0 && n >= from { "#c53030" } else if loss > 0.0 { "#b7791f" } else { "#1a8f4c" };
        let (x, y) = (45.0 + (i % per_row) as f64 * 80.0, 30.0 + (i / per_row) as f64 * 70.0);
        if let Some((px, py)) = last {
            svg.push_str(&format!("<line x1=\"{px:.0}\" y1=\"{py:.0}\" x2=\"{x:.0}\" y2=\"{y:.0}\" stroke=\"#9aa3b2\" stroke-width=\"2\"/>"));
        }
        last = Some((x, y));
        svg.push_str(&format!(
            "<circle cx=\"{x:.0}\" cy=\"{y:.0}\" r=\"11\" fill=\"{tone}\"/><text x=\"{x:.0}\" y=\"{:.0}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#fff\">{n}</text><text x=\"{x:.0}\" y=\"{:.0}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1d2330\">{}ms</text>",
            y + 4.0,
            y + 27.0,
            num(f(h, "avg"))
        ));
        table.push_str(&format!("<tr><td>{n}</td><td>{}</td><td>{}%</td><td>{}ms</td><td>{}ms</td></tr>", esc(&s(h, "host")), num(loss), num(f(h, "avg")), num(f(h, "worst"))));
    }
    svg.push_str("</svg>");
    table.push_str("</table>");
    format!("{svg}<p class=\"m\">초록은 정상, 노랑은 그 홉만의 손실, 빨강은 손실이 끝까지 이어지는 구간입니다.</p>{table}")
}

fn radar(c: &Value) -> String {
    let hosts = list(c, "hosts", 120);
    let max = hosts.iter().filter_map(|h| h.get("ms").and_then(Value::as_f64)).fold(1.0, f64::max);
    let mut svg = String::from("<svg viewBox=\"0 0 320 320\" class=\"fig sq\"><rect width=\"320\" height=\"320\" fill=\"#f3f5f8\"/>");
    for r in [50, 100, 150] {
        svg.push_str(&format!("<circle cx=\"160\" cy=\"160\" r=\"{r}\" fill=\"none\" stroke=\"#c9ced8\"/>"));
    }
    svg.push_str("<circle cx=\"160\" cy=\"160\" r=\"5\" fill=\"#1d2330\"/>");
    let mut table = String::from("<table><tr><th>장비</th><th>응답 시간</th></tr>");
    for (i, h) in hosts.iter().enumerate() {
        let ms = h.get("ms").and_then(Value::as_f64).filter(|x| x.is_finite());
        let angle = i as f64 / hosts.len().max(1) as f64 * std::f64::consts::TAU;
        let radius = 25.0 + ms.unwrap_or(max) / max * 120.0;
        svg.push_str(&format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"5\" fill=\"{}\"/>", 160.0 + radius * angle.cos(), 160.0 + radius * angle.sin(), if ms.is_some() { "#0b6b6b" } else { "#9aa3b2" }));
        table.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", esc(&s(h, "host")), ms.map(|m| format!("{}ms", num(m))).unwrap_or_else(|| "응답 없음".into())));
    }
    svg.push_str("</svg>");
    table.push_str("</table>");
    format!("<div class=\"two\">{svg}{table}</div><p class=\"m\">가운데가 내 PC이고, 멀수록 응답이 느린 장비입니다.</p>")
}

fn fortress(c: &Value) -> String {
    let doors: String = list(c, "doors", 96)
        .iter()
        .map(|d| {
            let listening = d.get("listening").and_then(Value::as_bool).unwrap_or(false);
            let connections = d.get("connections").and_then(Value::as_u64).unwrap_or(0);
            format!(
                "<div class=\"door {}\"><b>{}/{}</b><br>{}<br><small>PID {} · {}</small></div>",
                if listening { "open" } else { "" },
                esc(&s(d, "proto")),
                d.get("port").and_then(Value::as_u64).unwrap_or(0),
                esc(&s(d, "process")),
                d.get("pid").and_then(Value::as_u64).unwrap_or(0),
                if listening { "열고 기다림".to_string() } else { format!("연결 {connections}개") }
            )
        })
        .collect();
    let more = c.get("more").and_then(Value::as_u64).unwrap_or(0);
    format!("<div class=\"doors\">{doors}</div>{}", if more > 0 { format!("<p class=\"m\">이 밖에 {more}개가 더 있습니다.</p>") } else { String::new() })
}

fn highway(c: &Value) -> String {
    let Some(road) = c.get("road") else { return String::new() };
    let flows = list(road, "flows", 40);
    let max = flows.iter().map(|x| f(x, "bytes")).fold(1.0, f64::max);
    let rows: String = flows
        .iter()
        .map(|x| {
            let (label, tone) = match s(x, "class").as_str() {
                "secure" => ("암호화", "#1a8f4c"),
                "dns" => ("DNS", "#0b6b6b"),
                "trouble" => ("문제", "#c53030"),
                _ => ("평문", "#b7791f"),
            };
            let who = format!("{} {}/{} ({label})", s(x, "peer"), s(x, "proto"), x.get("port").and_then(Value::as_u64).unwrap_or(0));
            let value = format!("{} · 보냄 {} · 받음 {}{}", bytes_text(f(x, "bytes")), num(f(x, "sent")), num(f(x, "received")), if f(x, "trouble") > 0.0 { format!(" · 문제 {}", num(f(x, "trouble"))) } else { String::new() });
            bar_row(&who, f(x, "bytes") / max, tone, &value)
        })
        .collect();
    format!(
        "<p class=\"m\">내 PC {} · 패킷 {}개 · {}초</p><table class=\"bars\">{rows}</table>",
        esc(&s(road, "me")),
        num(f(road, "packets")),
        num(f(road, "seconds"))
    )
}

/// 결과 한 건의 그림들. 모르는 종류는 건너뛴다.
pub fn charts_html(result: &Value) -> String {
    list(result, "charts", 8)
        .iter()
        .filter_map(|c| {
            let unit = esc(&s(c, "unit"));
            let body = match s(c, "kind").as_str() {
                "bars" => bars(c, &unit),
                "line" => line(c, &unit),
                "gauge" => gauge(c, &unit),
                "route" => route(c),
                "radar" => radar(c),
                "fortress" => fortress(c),
                "highway" => highway(c),
                _ => return None,
            };
            (!body.is_empty()).then(|| format!("<figure><figcaption>{} <small>({unit})</small></figcaption>{body}</figure>", esc(&s(c, "title"))))
        })
        .collect()
}

pub const STYLE: &str = "figure{margin:.6em 0;padding:.5em .7em;border:1px solid #c9ced8;border-radius:6px}figcaption{font-size:.9em;color:#5b6475;margin-bottom:.3em}\
.fig{width:100%;max-width:660px;display:block}.sq{max-width:320px}.two{display:flex;gap:1em;flex-wrap:wrap;align-items:flex-start}\
.bars{width:100%}.bars td{border:0;padding:.1em .4em;font-size:.85em}.bl{width:34%;font-family:Consolas,monospace;word-break:break-all}.bb{width:36%}.bb div{height:.8em;border-radius:2px}.bv{color:#5b6475}\
.doors{display:flex;flex-wrap:wrap;gap:.4em}.door{border:1px dashed #9aa3b2;border-radius:4px;padding:.3em .5em;font-size:.8em;min-width:8em}.door.open{border:2px solid #b7791f;background:#fdf6e7}";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn draws_every_kind_without_markup_from_data() {
        let result = json!({"charts": [
            {"kind": "bars", "title": "홉별 지연 <b>", "unit": "ms", "bars": [{"label": "1. 192.168.0.1 <img>", "value": 2.5, "light": "green", "note": "손실 0%"}, {"label": "2. x", "value": 40, "light": "red", "note": ""}]},
            {"kind": "line", "title": "응답 시간", "unit": "ms", "points": [10, 12.5, 9, 30]},
            {"kind": "gauge", "title": "속도", "unit": "Mbps", "value": 94.2, "max": 100},
            {"kind": "route", "title": "노선도", "unit": "ms", "from": 3, "hops": [{"n": 1, "host": "gw</text><script>", "loss": 0, "avg": 1.2, "worst": 3}, {"n": 2, "host": "isp", "loss": 10, "avg": 8, "worst": 20}, {"n": 3, "host": "???", "loss": 100, "avg": 0, "worst": 0}]},
            {"kind": "radar", "title": "레이더", "unit": "ms", "hosts": [{"host": "192.168.0.1", "ms": 1.5}, {"host": "192.168.0.9", "ms": null}]},
            {"kind": "fortress", "title": "요새", "unit": "문", "more": 4, "doors": [{"proto": "tcp", "port": 443, "process": "nginx.exe\"><x", "pid": 10, "listening": true, "connections": 0}, {"proto": "tcp", "port": 50000, "process": "chrome.exe", "pid": 11, "listening": false, "connections": 3}]},
            {"kind": "highway", "title": "하이웨이", "unit": "패킷", "road": {"me": "192.168.0.10", "packets": 120, "seconds": 9.5, "more": 0, "flows": [{"peer": "8.8.8.8", "port": 53, "proto": "udp", "class": "dns", "sent": 4, "received": 4, "bytes": 800, "trouble": 0, "kbps": 1}]}},
            {"kind": "future", "title": "모르는 것"}
        ]});
        let html = charts_html(&result);
        assert_eq!(html.matches("<figure>").count(), 7, "모르는 종류는 건너뛴다");
        assert!(html.contains("2.5 ms · 손실 0%") && html.contains("최저 9 · 평균 15.4 · 최고 30 ms (4개)") && html.contains("94.2 Mbps"));
        assert!(html.contains("fill=\"#c53030\"") && html.contains("<td>10%</td>") && html.contains("응답 없음"));
        assert!(html.contains("door open") && html.contains("연결 3개") && html.contains("이 밖에 4개") && html.contains("8.8.8.8 udp/53 (DNS)"));
        // 자료 속 글자는 꼬리표가 되지 못한다.
        assert!(!html.contains("<img") && !html.contains("<script") && !html.contains("<x"));
        assert!(html.contains("&lt;img&gt;") && html.contains("gw&lt;/text&gt;&lt;script&gt;") && html.contains("홉별 지연 &lt;b&gt;"));
        assert_eq!(html.matches("<svg").count(), html.matches("</svg>").count());
    }

    #[test]
    fn odd_values_do_not_break_it() {
        for result in [json!({}), json!({"charts": "x"}), json!({"charts": [1, null, {"kind": "line", "points": []}, {"kind": "bars"}, {"kind": "route", "hops": [{}]}, {"kind": "highway"}, {"kind": "radar", "hosts": [{"ms": "a"}]}, {"kind": "gauge", "value": "x", "max": 0}]})] {
            let html = charts_html(&result);
            assert!(!html.contains("NaN") && !html.contains("inf"), "{html}");
        }
    }
}
