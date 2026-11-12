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
    Meta { title: tidy(&meta.title, 80), recipe: tidy(&meta.recipe, 40), target: tidy(&meta.target, 120), light, steps: meta.steps.min(99) }
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
        assert_eq!(items[0].meta, Meta { title: "둘째".into(), recipe: "route_latency".into(), target: "8.8.8.8".into(), light: "green".into(), steps: 2 });
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
}
