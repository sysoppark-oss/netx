//! 최근 진단 기록: 끝난 진단의 계획과 결과를 이 PC의 자료 폴더에만 남긴다(밖으로 보내지 않는다).
//!
//! 내용은 화면이 만든 JSON 그대로 두고, 여기서는 크기와 개수만 지킨다. 파일 이름은 여기서 정한 숫자뿐이라
//! 화면이 경로를 고를 수 없다. 터미널 접속 내용과 암호는 기록 대상이 아니다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 한 건의 상한
pub const MAX_ENTRY_BYTES: usize = 1536 * 1024;
/// 남기는 건수와 전체 크기의 상한. 넘으면 오래된 것부터 지운다.
const MAX_ENTRIES: usize = 30;
const MAX_TOTAL_BYTES: u64 = 30 * 1024 * 1024;

/// 목록에 보이는 요약. 화면이 만들어 보낸 값이므로 글자 길이를 줄이고 제어 문자를 지워서 돌려준다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Meta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub recipe: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub light: String,
    #[serde(default)]
    pub steps: u32,
    /// 다른 PC에서 내보낸 것을 가져온 기록인지
    #[serde(default)]
    pub imported: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    pub id: String,
    /// 저장한 시각 (1970년부터의 밀리초)
    pub time: u64,
    pub size: u64,
    pub meta: Meta,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    meta: Meta,
}

fn tidy(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

fn clean(meta: Meta) -> Meta {
    let light = if matches!(meta.light.as_str(), "green" | "yellow" | "red" | "info") { meta.light } else { "info".into() };
    Meta { title: tidy(&meta.title, 80), recipe: tidy(&meta.recipe, 40), target: tidy(&meta.target, 120), light, steps: meta.steps.min(99), imported: meta.imported }
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("history")
}

fn valid_id(id: &str) -> bool {
    (10..=16).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// (번호, 크기)를 최근 것부터
fn files(data_dir: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = std::fs::read_dir(dir(data_dir)) else { return Vec::new() };
    let mut list: Vec<(String, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let id = name.strip_suffix(".json")?;
            let meta = e.metadata().ok()?;
            (valid_id(id) && meta.is_file()).then(|| (id.to_string(), meta.len()))
        })
        .collect();
    list.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| b.0.cmp(&a.0)));
    list
}

fn prune(data_dir: &Path) {
    let mut total = 0u64;
    for (i, (id, size)) in files(data_dir).into_iter().enumerate() {
        total += size;
        if i >= MAX_ENTRIES || total > MAX_TOTAL_BYTES {
            let _ = std::fs::remove_file(dir(data_dir).join(format!("{id}.json")));
        }
    }
}

/// 한 건을 저장하고 번호를 돌려준다. `now_ms`는 저장 시각이자 번호다.
pub fn save(data_dir: &Path, json: &str, now_ms: u64) -> Result<String, String> {
    if json.len() > MAX_ENTRY_BYTES {
        return Err("기록이 너무 커서 저장하지 않았습니다.".into());
    }
    serde_json::from_str::<Envelope>(json).map_err(|_| "기록 형식이 올바르지 않습니다.".to_string())?;
    let folder = dir(data_dir);
    std::fs::create_dir_all(&folder).map_err(|e| format!("기록 폴더를 만들지 못했습니다: {e}"))?;
    // 같은 밀리초에 두 번 저장해도 덮어쓰지 않는다.
    let mut id = now_ms.max(1_000_000_000);
    while folder.join(format!("{id}.json")).exists() {
        id += 1;
    }
    std::fs::write(folder.join(format!("{id}.json")), json).map_err(|e| format!("기록을 저장하지 못했습니다: {e}"))?;
    prune(data_dir);
    Ok(id.to_string())
}

pub fn list(data_dir: &Path) -> Vec<Item> {
    files(data_dir)
        .into_iter()
        .take(MAX_ENTRIES)
        .filter_map(|(id, size)| {
            let text = std::fs::read(dir(data_dir).join(format!("{id}.json"))).ok()?;
            let envelope: Envelope = serde_json::from_slice(&text).ok()?;
            Some(Item { time: id.parse().ok()?, id, size, meta: clean(envelope.meta) })
        })
        .collect()
}

pub fn load(data_dir: &Path, id: &str) -> Result<String, String> {
    if !valid_id(id) {
        return Err("기록 번호가 올바르지 않습니다.".into());
    }
    let path = dir(data_dir).join(format!("{id}.json"));
    let size = std::fs::metadata(&path).map_err(|_| "그 기록을 찾지 못했습니다.".to_string())?.len();
    if size > MAX_ENTRY_BYTES as u64 {
        return Err("기록 파일이 너무 큽니다.".into());
    }
    std::fs::read_to_string(&path).map_err(|_| "기록을 읽지 못했습니다.".to_string())
}

/// 한 건을 지운다. `id`가 None이면 모두 지운다. 지운 건수를 돌려준다.
pub fn delete(data_dir: &Path, id: Option<&str>) -> Result<usize, String> {
    let targets: Vec<String> = match id {
        Some(id) if valid_id(id) => vec![id.to_string()],
        Some(_) => return Err("기록 번호가 올바르지 않습니다.".into()),
        None => files(data_dir).into_iter().map(|(id, _)| id).collect(),
    };
    Ok(targets.iter().filter(|id| std::fs::remove_file(dir(data_dir).join(format!("{id}.json"))).is_ok()).count())
}

// ---- 보고서에 싣기 ----
// 기록 한 건을 보고서의 한 토막으로 만든다. 보고서 전체는 `report.rs`가 만든다(이 기록은 그 "최근 진단" 구역이다).
// 스크립트를 넣지 않고, 기록에서 온 글자는 모두 바꿔 쓴다(장비가 보낸 글자가 섞여 있다).
// 내보내는 파일에는 말씀 표시를 넣지 않는다.

pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// 1970년부터의 밀리초를 현지 시각 글자로. `offset_minutes`는 UTC와의 차이(한국은 540).
pub fn local_time(ms: u64, offset_minutes: i32) -> (String, String) {
    let secs = (ms / 1000) as i64 + i64::from(offset_minutes.clamp(-900, 900)) * 60;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // 날짜 계산 (그레고리력)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rest / 3600, rest % 3600 / 60, rest % 60);
    (format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}"), format!("{year:04}{month:02}{day:02}-{h:02}{m:02}{s:02}"))
}

fn light_text(light: &str) -> (&'static str, &'static str) {
    match light {
        "green" => ("정상", "g"),
        "yellow" => ("주의", "y"),
        "red" => ("문제", "r"),
        _ => ("참고", "i"),
    }
}

fn status_text(status: &str) -> &'static str {
    match status {
        "done" => "완료",
        "timeout" => "시간 초과",
        "cancelled" => "중지됨",
        "capped" => "출력 상한에서 멈춤",
        "error" => "실행 오류",
        "running" => "실행 중",
        _ => "실행 안 함",
    }
}

/// 기록 한 건을 보고서의 한 토막으로 만든다. 가릴 것은 부르는 쪽이 `entry`에서 미리 가린다.
pub fn section(time: u64, entry: &serde_json::Value, offset_minutes: i32) -> Option<String> {
    use serde_json::Value;
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let meta = entry.get("meta")?;
    let (label, class) = light_text(&text(meta, "light"));
    let mut out = format!(
        "<section><h2><span class=\"l {class}\">{label}</span> {} <span class=\"t\">{}</span></h2><p class=\"m\">{} · 진단 종류: {}</p>",
        esc(&tidy(&text(meta, "title"), 80)),
        esc(&tidy(&text(meta, "target"), 120)),
        local_time(time, offset_minutes).0,
        esc(&tidy(&text(meta, "recipe"), 40)),
    );
    let run = entry.get("run")?;
    let lines = run.get("lines").and_then(Value::as_array);
    for step in entry.get("plan")?.get("steps")?.as_array()?.iter().take(40) {
        let order = step.get("order").and_then(Value::as_u64)?;
        let key = order.to_string();
        let state = run.get("steps").and_then(|s| s.get(&key));
        let status = state.map(|s| text(s, "status")).unwrap_or_default();
        let elapsed = state.and_then(|s| s.get("elapsed_ms")).and_then(Value::as_u64).unwrap_or(0);
        out.push_str(&format!(
            "<div class=\"s\"><h3>{}. {}</h3><pre class=\"c\">$ {}</pre><p class=\"m\">{}{}</p>",
            order,
            esc(&text(step, "title")),
            esc(&text(step, "display")),
            status_text(&status),
            if elapsed > 0 { format!(" · {:.1}초", elapsed as f64 / 1000.0) } else { String::new() },
        ));
        if let Some(result) = run.get("results").and_then(|r| r.get(&key)) {
            let (label, class) = light_text(&text(result, "light"));
            out.push_str(&format!("<p><span class=\"l {class}\">{label}</span> {}</p>", esc(&text(result, "text"))));
            let facts: Vec<String> = result
                .get("facts")
                .and_then(Value::as_array)
                .map(|f| f.iter().take(40).map(|x| format!("<tr><th>{}</th><td>{}</td></tr>", esc(&text(x, "label")), esc(&text(x, "value")))).collect())
                .unwrap_or_default();
            if !facts.is_empty() {
                out.push_str(&format!("<table>{}</table>", facts.concat()));
            }
            out.push_str(&crate::report_fig::charts_html(result));
        }
        // 구성도 그림: 화면이 기록할 때 함께 넣어 둔 그림(SVG). 다른 PC에서 가져온 기록일 수도 있으므로,
        // 그림에 쓰는 꼬리표와 속성만으로 된 것만 넣는다.
        if let Some(svg) = entry.get("topo_svg").and_then(|t| t.get(&key)).and_then(Value::as_str) {
            if crate::report::safe_svg(svg) {
                out.push_str(&format!("<figure><figcaption>네트워크 구성도</figcaption><div class=\"topo\">{svg}</div></figure>"));
            }
        }
        let raw: Vec<String> = lines
            .map(|l| l.iter().filter(|x| x.get("order").and_then(Value::as_u64) == Some(order)).map(|x| esc(&text(x, "line"))).collect())
            .unwrap_or_default();
        if !raw.is_empty() {
            out.push_str(&format!("<details><summary>원본 출력 ({}줄)</summary><pre>{}</pre></details>", raw.len(), raw.join("\n")));
        }
        out.push_str("</div>");
    }
    out.push_str("</section>");
    Some(out)
}

pub const STYLE: &str = "body{font-family:'Malgun Gothic',sans-serif;max-width:60em;margin:2em auto;padding:0 1em;color:#1d2330;line-height:1.6}\
h1{font-size:1.4em}h2{font-size:1.15em;border-top:2px solid #1d2330;padding-top:.8em;margin-top:2em}h3{font-size:1em;margin:.2em 0}\
.t{font-family:Consolas,monospace;color:#0b6b6b}.m{color:#5b6475;font-size:.9em;margin:.2em 0}.s{border:1px solid #c9ced8;border-radius:6px;padding:.6em .9em;margin:.7em 0}\
pre{background:#f3f5f8;padding:.6em;overflow:auto;font-family:Consolas,monospace;font-size:.85em;white-space:pre-wrap;word-break:break-all}.c{background:#e8f4f3}\
table{border-collapse:collapse;font-size:.9em;margin:.4em 0}th,td{border:1px solid #c9ced8;padding:.15em .6em;text-align:left}th{background:#f3f5f8;font-weight:normal}\
.l{display:inline-block;padding:0 .5em;border-radius:1em;font-size:.85em;color:#fff}.g{background:#1a8f4c}.y{background:#b7791f}.r{background:#c53030}.i{background:#5b6475}\
summary{cursor:pointer;color:#5b6475;font-size:.9em}.topo svg{width:100%;height:auto}";

pub const DATA_OPEN: &str = "<!--netx-data:";
pub const DATA_CLOSE: &str = "-->";
/// 가져올 수 있는 보고서 파일의 상한
pub const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;

/// 예전 판(보고서 만들기 화면이 생기기 전)이 내보내던 보고서 파일. 지금은 만들지 않고,
/// 예전 파일도 가져오기에 통과하는지 시험하는 데만 쓴다.
#[cfg(test)]
pub fn report(entries: &[(u64, String)], offset_minutes: i32, mask: bool, now_ms: u64) -> String {
    use base64::Engine;
    let hide = |text: String| if mask { crate::planner::redact::Mask::build(&text).apply(&text) } else { text };
    let sections: Vec<String> = entries.iter().filter_map(|(time, json)| section(*time, &serde_json::from_str(json).ok()?, offset_minutes)).collect();
    let html = hide(format!(
        "<!DOCTYPE html>\n<html lang=\"ko\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>NetX 진단 기록</title><style>{STYLE}{}</style></head><body>\
<h1>NetX 진단 기록</h1><p class=\"m\">내보낸 시각 {} · {}건{} · NetX의 [최근 진단] → [가져오기]로 이 파일을 열면 기록으로 다시 볼 수 있습니다</p>{}",
        crate::report_fig::STYLE,
        local_time(now_ms, offset_minutes).0,
        sections.len(),
        if mask { " · 내부 주소는 [내부IP-번호]로 가렸습니다" } else { " · 내부 네트워크의 주소가 들어 있습니다. 밖으로 보낼 때 주의하세요" },
        sections.concat(),
    ));
    let items: Vec<serde_json::Value> = entries
        .iter()
        .filter_map(|(time, json)| Some(serde_json::json!({"time": time, "entry": serde_json::from_str::<serde_json::Value>(&hide(json.clone())).ok()?})))
        .collect();
    let data = base64::engine::general_purpose::STANDARD.encode(serde_json::Value::Array(items).to_string());
    format!("{html}{DATA_OPEN}{data}{DATA_CLOSE}</body></html>\n")
}

/// 내보낸 보고서 파일에서 기록을 꺼내 이 PC의 기록에 더한다. 더한 건수를 돌려준다.
/// 가져온 기록에는 표시를 남긴다. 이미 있는 기록을 덮어쓰지 않는다.
pub fn import(data_dir: &Path, html: &str, now_ms: u64) -> Result<usize, String> {
    use base64::Engine;
    const BAD: &str = "NetX가 내보낸 보고서 파일이 아니거나 기록 자료가 들어 있지 않습니다.";
    if html.len() > MAX_IMPORT_BYTES {
        return Err("파일이 너무 큽니다 (64MB까지).".into());
    }
    let start = html.rfind(DATA_OPEN).ok_or(BAD)? + DATA_OPEN.len();
    let rest = html.get(start..).ok_or(BAD)?;
    let data = rest.get(..rest.find(DATA_CLOSE).ok_or(BAD)?).ok_or(BAD)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| BAD.to_string())?;
    let items: Vec<serde_json::Value> = serde_json::from_slice(&bytes).map_err(|_| BAD.to_string())?;
    let mut added = 0;
    // 오래된 것부터 넣어야 목록에서 원래 순서가 유지된다.
    for (i, item) in items.iter().take(MAX_ENTRIES).rev().enumerate() {
        let Some(mut entry) = item.get("entry").filter(|e| e.is_object()).cloned() else { continue };
        let Some(meta) = entry.get_mut("meta").and_then(serde_json::Value::as_object_mut) else { continue };
        meta.insert("imported".into(), serde_json::Value::Bool(true));
        if save(data_dir, &entry.to_string(), now_ms + i as u64).is_ok() {
            added += 1;
        }
    }
    if added == 0 {
        return Err("가져올 수 있는 기록이 없습니다.".into());
    }
    Ok(added)
}

/// 기간 안의 기록을 (저장 시각, 기록 JSON)으로, 최근 것부터. `id`를 주면 그 한 건만.
pub fn collect(data_dir: &Path, id: Option<&str>) -> Result<Vec<(u64, String)>, String> {
    let ids: Vec<String> = match id {
        Some(id) if valid_id(id) => vec![id.to_string()],
        Some(_) => return Err("기록 번호가 올바르지 않습니다.".into()),
        None => files(data_dir).into_iter().take(MAX_ENTRIES).map(|(id, _)| id).collect(),
    };
    Ok(ids.iter().filter_map(|id| Some((id.parse().ok()?, load(data_dir, id).ok()?))).collect())
}

/// 예전 판의 내보내기(시험용).
#[cfg(test)]
pub fn export(data_dir: &Path, folder: &Path, id: Option<&str>, offset_minutes: i32, mask: bool, now_ms: u64) -> Result<String, String> {
    let ids: Vec<String> = match id {
        Some(id) if valid_id(id) => vec![id.to_string()],
        Some(_) => return Err("기록 번호가 올바르지 않습니다.".into()),
        None => files(data_dir).into_iter().take(MAX_ENTRIES).map(|(id, _)| id).collect(),
    };
    let entries: Vec<(u64, String)> = ids.iter().filter_map(|id| Some((id.parse().ok()?, load(data_dir, id).ok()?))).collect();
    if entries.is_empty() {
        return Err("내보낼 기록이 없습니다.".into());
    }
    let html = report(&entries, offset_minutes, mask, now_ms);
    std::fs::create_dir_all(folder).map_err(|e| format!("폴더를 만들지 못했습니다: {e}"))?;
    let stamp = local_time(now_ms, offset_minutes).1;
    // 같은 이름이 있으면 덮어쓰지 않는다.
    let path = (1..100)
        .map(|n| folder.join(if n == 1 { format!("netx-report-{stamp}.html") } else { format!("netx-report-{stamp}-{n}.html") }))
        .find(|p| !p.exists())
        .ok_or("보고서 파일 이름을 정하지 못했습니다.")?;
    std::fs::write(&path, html.as_bytes()).map_err(|e| format!("보고서를 저장하지 못했습니다: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("netx-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn entry(title: &str) -> String {
        serde_json::json!({"meta": {"title": title, "recipe": "route_latency", "target": "8.8.8.8", "light": "green", "steps": 2}, "plan": {"steps": []}, "run": {"lines": []}}).to_string()
    }

    #[test]
    fn saves_lists_loads_and_deletes() {
        let d = temp("basic");
        let a = save(&d, &entry("첫째"), 1_794_400_000_000).unwrap();
        let b = save(&d, &entry("둘째"), 1_794_400_000_000).unwrap();
        assert_ne!(a, b, "같은 시각에도 덮어쓰지 않는다");
        let items = list(&d);
        assert_eq!(items.iter().map(|i| i.meta.title.as_str()).collect::<Vec<_>>(), ["둘째", "첫째"], "최근 것부터");
        assert_eq!(items[0].meta, Meta { title: "둘째".into(), recipe: "route_latency".into(), target: "8.8.8.8".into(), light: "green".into(), steps: 2, imported: false });
        assert!(load(&d, &a).unwrap().contains("첫째"));
        assert_eq!(delete(&d, Some(&a)).unwrap(), 1);
        assert!(load(&d, &a).is_err());
        assert_eq!(delete(&d, None).unwrap(), 1);
        assert!(list(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_bad_input_and_paths() {
        let d = temp("bad");
        assert!(save(&d, "not json", 1_794_400_000_000).is_err());
        assert!(save(&d, &"x".repeat(MAX_ENTRY_BYTES + 1), 1_794_400_000_000).is_err());
        for id in ["../audit", "1794400000000/../x", "abc", "", "123", "17944000000000000000", "1794400000000.json"] {
            assert!(load(&d, id).is_err(), "{id}");
            assert!(delete(&d, Some(id)).is_err(), "{id}");
        }
        // 요약의 이상한 값은 고쳐서 돌려준다.
        let odd = serde_json::json!({"meta": {"title": "a\u{1b}[31mb".repeat(40), "light": "purple", "steps": 5000}}).to_string();
        save(&d, &odd, 1_794_400_000_000).unwrap();
        let item = &list(&d)[0];
        assert!(item.meta.title.chars().count() == 80 && !item.meta.title.contains('\u{1b}'));
        assert_eq!((item.meta.light.as_str(), item.meta.steps), ("info", 99));
        // 폴더에 섞여 있는 다른 파일은 건드리지 않는다.
        std::fs::write(dir(&d).join("notes.txt"), "x").unwrap();
        assert_eq!(list(&d).len(), 1);
        delete(&d, None).unwrap();
        assert!(dir(&d).join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keeps_only_the_most_recent() {
        let d = temp("prune");
        for i in 0..(MAX_ENTRIES as u64 + 5) {
            save(&d, &entry(&format!("기록 {i}")), 1_794_400_000_000 + i).unwrap();
        }
        let items = list(&d);
        assert_eq!(items.len(), MAX_ENTRIES);
        assert_eq!(items[0].meta.title, format!("기록 {}", MAX_ENTRIES + 4));
        assert_eq!(items.last().unwrap().meta.title, "기록 5");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn full_entry() -> String {
        serde_json::json!({
            "meta": {"title": "웹 장애 진단", "recipe": "web_down", "target": "192.168.0.10", "light": "red", "steps": 2},
            "plan": {"steps": [
                {"order": 1, "title": "포트 연결 시험 (nc)", "display": "nc -vz -w 3 -- 192.168.0.10 443"},
                {"order": 2, "title": "HTTP 시험 (curl)", "display": "curl -I https://192.168.0.10"}
            ]},
            "run": {
                "steps": {"1": {"status": "done", "exit_code": 1, "elapsed_ms": 3100}},
                "results": {"1": {"light": "red", "text": "포트가 닫혀 있습니다 <script>alert(1)</script>", "facts": [{"label": "대상", "value": "192.168.0.10:443"}]}},
                "lines": [
                    {"order": 1, "stream": "stderr", "line": "nc: connect to 192.168.0.10 port 443 failed </pre><img src=x onerror=alert(1)>"},
                    {"order": 1, "stream": "stderr", "line": "서버 8.8.8.8 과 10.0.0.1\u{1b}[0m"}
                ]
            }
        })
        .to_string()
    }

    #[test]
    fn report_escapes_everything_and_reads_well() {
        let html = report(&[(1_794_443_732_486, full_entry())], 540, false, 1_794_443_800_000);
        assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<title>NetX 진단 기록</title>"));
        assert!(html.contains("2026-11-12 09:35") && html.contains("내보낸 시각 2026-11-12 09:36"), "현지 시각");
        assert!(html.contains("<span class=\"l r\">문제</span> 웹 장애 진단") && html.contains("1. 포트 연결 시험 (nc)"));
        assert!(html.contains("$ nc -vz -w 3 -- 192.168.0.10 443") && html.contains("완료 · 3.1초"));
        assert!(html.contains("<th>대상</th><td>192.168.0.10:443</td>") && html.contains("원본 출력 (2줄)"));
        assert!(html.contains("2. HTTP 시험 (curl)") && html.contains("실행 안 함"));
        // 기록 속 글자는 꼬리표나 스크립트가 되지 못한다.
        assert!(!html.contains("<script") && !html.contains("<img") && !html.contains('\u{1b}'));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;") && html.contains("&lt;/pre&gt;&lt;img src=x onerror=alert(1)&gt;"));
        assert_eq!(html.matches("<pre").count(), html.matches("</pre>").count());
        // 내부 주소 가리기: 사설 주소만 가리고 공인 주소는 둔다.
        let masked = report(&[(1_794_443_732_486, full_entry())], 540, true, 1_794_443_800_000);
        assert!(!masked.contains("192.168.0.10") && !masked.contains("10.0.0.1") && masked.contains("[내부IP-") && masked.contains("8.8.8.8"));
        // 깨진 기록은 건너뛴다.
        let html = report(&[(1, "{}".into()), (2, "nope".into())], 540, false, 0);
        assert!(html.contains("0건"));
    }

    #[test]
    fn local_time_follows_the_calendar() {
        assert_eq!(local_time(0, 0).0, "1970-01-01 00:00");
        assert_eq!(local_time(951_782_400_000, 0).0, "2000-02-29 00:00");
        assert_eq!(local_time(1_794_443_732_486, 540), ("2026-11-12 09:35".to_string(), "20261112-093532".to_string()));
        assert_eq!(local_time(1_794_443_732_486, -300).0, "2026-11-11 19:35");
        assert_eq!(local_time(1_794_443_732_486, 99_999).0, local_time(1_794_443_732_486, 900).0, "터무니없는 값은 잘라 쓴다");
    }

    #[test]
    fn exports_one_or_all_without_overwriting() {
        let d = temp("export");
        let out = d.join("work");
        let a = save(&d, &full_entry(), 1_794_400_000_000).unwrap();
        save(&d, &entry("둘째"), 1_794_400_000_001).unwrap();
        let one = export(&d, &out, Some(&a), 540, false, 1_794_443_800_000).unwrap();
        assert!(one.ends_with("netx-report-20261112-093640.html"), "{one}");
        assert!(std::fs::read_to_string(&one).unwrap().contains("1건"));
        let all = export(&d, &out, None, 540, true, 1_794_443_800_000).unwrap();
        assert!(all.ends_with("netx-report-20261112-093640-2.html"), "{all}");
        let text = std::fs::read_to_string(&all).unwrap();
        assert!(text.contains("2건") && text.find("둘째").unwrap() < text.find("웹 장애 진단").unwrap(), "최근 것부터");
        assert!(export(&d, &out, Some("../x"), 540, false, 0).is_err());
        delete(&d, None).unwrap();
        assert!(export(&d, &out, None, 540, false, 0).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn report_carries_pictures_and_can_be_imported_elsewhere() {
        let with_chart = serde_json::json!({
            "meta": {"title": "라우팅 지연 분석", "recipe": "route_latency", "target": "192.168.0.1", "light": "green", "steps": 1},
            "recipe_id": "route_latency", "inputs": {"target": "192.168.0.1"},
            "plan": {"steps": [{"order": 1, "title": "연결 확인 (ping)", "display": "ping -c 5 -- 192.168.0.1"}]},
            "run": {"steps": {"1": {"status": "done", "elapsed_ms": 4000}}, "lines": [],
                "results": {"1": {"light": "green", "text": "모두 응답", "facts": [], "charts": [{"kind": "line", "title": "응답 시간", "unit": "ms", "points": [1, 2, 3]}]}}},
            "topo_svg": {"1": "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>gw</text></svg>", "2": "<svg onload=\"x()\"></svg>"}
        })
        .to_string();
        let html = report(&[(1_794_443_732_486, with_chart.clone()), (1_794_443_000_000, full_entry())], 540, false, 1_794_443_800_000);
        assert!(html.contains("<figcaption>응답 시간") && html.contains("<polyline") && html.contains("네트워크 구성도") && html.contains("<text>gw</text>"));
        assert!(!html.contains("onload"), "검사를 통과하지 못한 그림은 넣지 않는다");
        assert!(html.ends_with("--></body></html>\n"));

        // 다른 PC에서 가져오기
        let other = temp("import");
        assert_eq!(import(&other, &html, 1_794_500_000_000).unwrap(), 2);
        let items = list(&other);
        assert_eq!(items.iter().map(|i| i.meta.title.as_str()).collect::<Vec<_>>(), ["라우팅 지연 분석", "웹 장애 진단"], "원래 순서대로");
        assert!(items.iter().all(|i| i.meta.imported));
        assert!(load(&other, &items[0].id).unwrap().contains("\"points\":[1,2,3]"));
        // 한 번 더 가져와도 덮어쓰지 않고 더해진다.
        assert_eq!(import(&other, &html, 1_794_500_000_000).unwrap(), 2);
        assert_eq!(list(&other).len(), 4);

        // 가린 보고서는 가져온 기록에도 내부 주소가 없다.
        let masked = report(&[(1_794_443_732_486, with_chart)], 540, true, 1_794_443_800_000);
        let third = temp("import-masked");
        import(&third, &masked, 1_794_500_000_000).unwrap();
        let text = load(&third, &list(&third)[0].id).unwrap();
        assert!(!text.contains("192.168.0.1") && text.contains("[내부IP-1]"));

        // 엉뚱한 파일
        for bad in ["", "<html>그냥 문서</html>", "<!--netx-data:!!!-->", "<!--netx-data:bm9wZQ==-->", "<!--netx-data:W10=-->", "<!--netx-data:W3sidGltZSI6MX1d-->"] {
            assert!(import(&third, bad, 0).is_err(), "{bad}");
        }
        for d in [other, third] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}
