//! 비자(사람이 정한 테두리)와 신고(에이전트가 적은 것)의 형식.

use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisaFile {
    pub version: u32,
    /// 초안이면 보고서 맨 위에 그렇게 적는다.
    #[serde(default)]
    pub draft: bool,
    /// 에이전트가 건드려도 되는 경로.
    pub allow: Vec<String>,
    /// 건드리면 안 되는 경로. allow보다 앞선다.
    #[serde(default)]
    pub deny: Vec<String>,
    /// 바뀌면 언제나 사람이 보는 경로.
    #[serde(default)]
    pub always: Vec<String>,
    /// 다음 AI 세션이 읽는 문서. 바뀌면 알림만 낸다.
    #[serde(default)]
    pub ai_docs: Vec<String>,
    /// "평소 일": 이 파일에서 이런 모양의 줄이 바뀌는 것은 걸지 않는다.
    #[serde(default)]
    pub usual: Vec<UsualRule>,
    /// "풀지 않는 설정": 분류 라.
    #[serde(default)]
    pub guard: Vec<GuardRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsualRule {
    pub path: String,
    pub line: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardRule {
    pub name: String,
    /// 이 경로의 파일에서(없으면 모든 파일에서)
    #[serde(default)]
    pub path: Option<String>,
    /// 이 모양의 줄이 바뀌면(없으면 평소 일이 아닌 어떤 줄이든)
    #[serde(default)]
    pub line: Option<String>,
}

pub struct Visa {
    pub draft: bool,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub always: Vec<String>,
    pub ai_docs: Vec<String>,
    pub usual: Vec<(String, Regex)>,
    pub guard: Vec<Guard>,
}

pub struct Guard {
    pub name: String,
    pub path: Option<String>,
    pub line: Option<Regex>,
}

const MAX_RULES: usize = 500;

fn regex(text: &str) -> Result<Regex, String> {
    if text.len() > 300 {
        return Err("줄 모양이 너무 깁니다".into());
    }
    regex::RegexBuilder::new(text)
        .size_limit(1 << 20)
        .build()
        .map_err(|_| format!("줄 모양을 읽지 못했습니다: {}", crate::report::clean(text)))
}

fn check_patterns(list: &[String]) -> Result<(), String> {
    if list.len() > MAX_RULES {
        return Err("경로 목록이 너무 깁니다".into());
    }
    for p in list {
        if p.is_empty() || p.len() > 300 || p.starts_with('/') || p.split('/').any(|s| s == "..") {
            return Err(format!("경로 무늬가 올바르지 않습니다: {}", crate::report::clean(p)));
        }
    }
    Ok(())
}

pub fn parse_visa(text: &str) -> Result<Visa, String> {
    let f: VisaFile = toml::from_str(text).map_err(|e| format!("비자 파일을 읽지 못했습니다: {}", first_line(&e.to_string())))?;
    if f.version != 1 {
        return Err("비자 파일의 판 번호를 모릅니다".into());
    }
    for l in [&f.allow, &f.deny, &f.always, &f.ai_docs] {
        check_patterns(l)?;
    }
    if f.usual.len() > MAX_RULES || f.guard.len() > MAX_RULES {
        return Err("비자의 규칙이 너무 많습니다".into());
    }
    let mut usual = Vec::new();
    for u in &f.usual {
        check_patterns(std::slice::from_ref(&u.path))?;
        usual.push((u.path.clone(), regex(&u.line)?));
    }
    let mut guard = Vec::new();
    for g in &f.guard {
        if let Some(p) = &g.path {
            check_patterns(std::slice::from_ref(p))?;
        }
        if g.path.is_none() && g.line.is_none() {
            return Err("풀지 않는 설정에는 경로나 줄 모양이 있어야 합니다".into());
        }
        let line = match &g.line {
            Some(l) => Some(regex(l)?),
            None => None,
        };
        guard.push(Guard { name: g.name.clone(), path: g.path.clone(), line });
    }
    Ok(Visa { draft: f.draft, allow: f.allow, deny: f.deny, always: f.always, ai_docs: f.ai_docs, usual, guard })
}

impl Visa {
    pub fn is_usual(&self, path: &str, line: &str) -> bool {
        let t = line.trim();
        t.is_empty() || self.usual.iter().any(|(p, re)| crate::glob::matches(p, path) && re.is_match(t))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclareFile {
    #[serde(default)]
    declare: Vec<DeclareLine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclareLine {
    what: String,
    #[serde(rename = "where")]
    place: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum What {
    Make,
    Edit,
    Remove,
}

#[derive(Debug, Clone)]
pub struct Declare {
    pub what: What,
    /// 경로 무늬. 폴더로 적은 것은 `폴더/**`로 바꿔 둔다.
    pub pattern: String,
    /// 적힌 그대로.
    pub written: String,
    pub wide: bool,
}

pub fn parse_declare(text: &str) -> Result<Vec<Declare>, String> {
    let f: DeclareFile = toml::from_str(text).map_err(|e| format!("신고 파일을 읽지 못했습니다: {}", first_line(&e.to_string())))?;
    if f.declare.len() > 2000 {
        return Err("신고가 너무 많습니다".into());
    }
    let mut out = Vec::new();
    for d in f.declare {
        let what = match d.what.as_str() {
            "만듦" => What::Make,
            "고침" => What::Edit,
            "지움" => What::Remove,
            _ => return Err("신고의 '무엇을'은 만듦, 고침, 지움 가운데 하나여야 합니다".into()),
        };
        check_patterns(std::slice::from_ref(&d.place))?;
        let wide = d.place.ends_with('/') || d.place.contains('*') || d.place.contains('?');
        let pattern = if d.place.ends_with('/') { format!("{}**", d.place) } else { d.place.clone() };
        out.push(Declare { what, pattern, written: d.place, wide });
    }
    Ok(out)
}

fn first_line(s: &str) -> String {
    crate::report::clean(s.lines().next().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visa_ok_and_bad() {
        let v = parse_visa(
            r#"
version = 1
allow = ["src/**"]
deny = ["word/**"]
[[usual]]
path = "**/build.rs"
line = '^"[a-z0-9_]+",$'
[[guard]]
name = "창 권한"
path = "src-tauri/capabilities/**"
"#,
        )
        .unwrap();
        assert!(v.is_usual("src-tauri/build.rs", "    \"collect_set\","));
        assert!(!v.is_usual("src-tauri/build.rs", "std::process::Command::new(\"x\");"));
        assert!(!v.is_usual("other.rs", "\"collect_set\","));
        assert!(parse_visa("version = 2\nallow = []").is_err());
        assert!(parse_visa("version = 1\nallow = [\"../x\"]").is_err());
        assert!(parse_visa("version = 1\nallow = []\nextra = 1").is_err());
        assert!(parse_visa("version = 1\nallow = []\n[[guard]]\nname = \"x\"").is_err());
        assert!(parse_visa("version = 1\nallow = []\n[[usual]]\npath = \"a\"\nline = \"(\"").is_err());
    }

    #[test]
    fn declare_ok_and_bad() {
        let d = parse_declare("[[declare]]\nwhat = \"고침\"\nwhere = \"src/a.rs\"\n[[declare]]\nwhat = \"만듦\"\nwhere = \"docs/\"\n").unwrap();
        assert_eq!(d.len(), 2);
        assert!(!d[0].wide);
        assert!(d[1].wide);
        assert_eq!(d[1].pattern, "docs/**");
        assert!(parse_declare("[[declare]]\nwhat = \"읽음\"\nwhere = \"a\"").is_err());
        assert!(parse_declare("[[declare]]\nwhat = \"고침\"\nwhere = \"/etc/passwd\"").is_err());
        assert!(parse_declare("").unwrap().is_empty());
    }

    #[test]
    fn random_text_does_not_panic() {
        let mut seed = 88172645463325252u64;
        for _ in 0..300 {
            let mut s = String::new();
            for _ in 0..120 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                s.push(match seed % 9 {
                    0 => '[',
                    1 => ']',
                    2 => '=',
                    3 => '"',
                    4 => '\n',
                    5 => '*',
                    _ => char::from_u32((seed >> 16) as u32 % 0x3000).unwrap_or('a'),
                });
            }
            let _ = parse_visa(&s);
            let _ = parse_declare(&s);
        }
    }
}
