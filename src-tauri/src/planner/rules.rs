//! 규칙 엔진. AI가 없거나 응답하지 않을 때 키워드로 레시피를 찾고, 입력에서 대상을 뽑아낸다.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};

use crate::model::{Recipe, ValueType};
use crate::registry::Registry;
use crate::security::values::url_parts;

pub struct RecipeMatch<'a> {
    pub recipe: &'a Recipe,
    /// 입력에서 찾아낸 값. 못 찾은 항목은 빠져 있다.
    pub inputs: BTreeMap<String, String>,
}

/// 문장에서 찾아낸 대상들(종류마다 처음 나온 것 하나).
#[derive(Default, Debug)]
struct Found {
    url: Option<String>,
    cidr: Option<String>,
    host: Option<String>,
    port: Option<String>,
}

impl Found {
    /// 레시피 입력 종류에 맞는 값을 준다. URL이 필요한데 호스트만 있으면 https://를 붙이고,
    /// 호스트가 필요한데 URL만 있으면 URL의 호스트를 쓴다.
    fn value_for(&self, kind: ValueType) -> Option<String> {
        match kind {
            ValueType::Url => self.url.clone().or_else(|| self.host.as_ref().map(|h| format!("https://{h}"))),
            ValueType::Host => self.host.clone().or_else(|| self.url.as_deref().and_then(url_parts).map(|(h, _)| h)),
            ValueType::Cidr => self.cidr.clone(),
            ValueType::Port => self.port.clone(),
            _ => None,
        }
    }
}

/// 상황 설명에 가장 잘 맞는 레시피를 고른다. 맞는 키워드가 하나도 없으면 None.
pub fn match_recipe<'a>(reg: &'a Registry, text: &str) -> Option<RecipeMatch<'a>> {
    let lower = text.to_lowercase();
    let found = extract(text);

    let (recipe, hits, _) = reg
        .recipes()
        .iter()
        .map(|r| {
            let hits = r.keywords.iter().filter(|k| lower.contains(&k.to_lowercase())).count();
            // 입력에 들어 있는 값의 종류도 단서가 된다(동점일 때 가른다).
            let fits = r.inputs.iter().filter(|i| found.value_for(i.kind).is_some()).count();
            (r, hits, fits)
        })
        .max_by_key(|(_, hits, fits)| (*hits, *fits))?;
    if hits == 0 {
        return None;
    }

    let inputs = recipe.inputs.iter().filter_map(|i| found.value_for(i.kind).map(|v| (i.name.clone(), v))).collect();
    Some(RecipeMatch { recipe, inputs })
}

/// 문장에서 URL, 대역, 호스트, 포트를 찾아낸다.
fn extract(text: &str) -> Found {
    let mut found = Found::default();
    let mentions_port = text.contains("포트") || text.to_lowercase().contains("port");

    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')' | '"' | '\'' | '`')) {
        // 앞뒤 기호와 조사("example.com이", "8080번")를 떼어 낸다.
        let tok = raw.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        let ascii_end = tok.char_indices().find(|(_, c)| !c.is_ascii()).map(|(i, _)| i).unwrap_or(tok.len());
        let tok = tok[..ascii_end].trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/');
        if tok.is_empty() {
            continue;
        }

        if (tok.starts_with("http://") || tok.starts_with("https://")) && tok.len() > 8 {
            found.url.get_or_insert_with(|| tok.to_string());
        } else if let Some((addr, prefix)) = tok.split_once('/')
            && addr.parse::<Ipv4Addr>().is_ok()
            && prefix.parse::<u8>().is_ok_and(|p| p <= 32)
        {
            found.cidr.get_or_insert_with(|| tok.to_string());
        } else if tok.parse::<IpAddr>().is_ok() || looks_like_domain(tok) {
            found.host.get_or_insert_with(|| tok.to_string());
        } else if mentions_port && tok.parse::<u16>().is_ok_and(|p| p >= 1) {
            // 문장에 "포트"라는 말이 있을 때만 숫자를 포트로 본다.
            found.port.get_or_insert_with(|| tok.to_string());
        }
    }
    found
}

fn looks_like_domain(tok: &str) -> bool {
    let labels: Vec<&str> = tok.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && labels.last().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(text: &str) -> Option<(String, BTreeMap<String, String>)> {
        match_recipe(Registry::global(), text).map(|r| (r.recipe.id.clone(), r.inputs))
    }
    fn one(k: &str, v: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(k.to_string(), v.to_string())])
    }

    #[test]
    fn web_problems_go_to_web_recipe() {
        assert_eq!(m("회사 홈페이지 https://example.com 이 안 열려요"), Some(("web_down".into(), one("url", "https://example.com"))));
        assert_eq!(m("example.com 사이트 접속이 안 돼"), Some(("web_down".into(), one("url", "https://example.com"))));
        assert_eq!(m("192.168.0.50 웹 서버 죽었나 확인해 줘"), Some(("web_down".into(), one("url", "https://192.168.0.50"))));
    }

    #[test]
    fn latency_scan_port_bandwidth() {
        assert_eq!(m("8.8.8.8까지 핑이 느려요"), Some(("route_latency".into(), one("target", "8.8.8.8"))));
        assert_eq!(m("192.168.0.0/24 대역에서 살아있는 장비 스캔"), Some(("subnet_scan".into(), one("cidr", "192.168.0.0/24"))));
        assert_eq!(m("8080 포트를 누가 쓰고 있지?"), Some(("port_hunt".into(), one("port", "8080"))));
        assert_eq!(m("포트 3000번 충돌"), Some(("port_hunt".into(), one("port", "3000"))));
        assert_eq!(m("192.168.0.10 이랑 회선 속도 측정"), Some(("bandwidth".into(), one("server", "192.168.0.10"))));
    }

    #[test]
    fn missing_target_still_matches_recipe() {
        let (id, inputs) = m("인터넷이 자꾸 끊겨요").unwrap();
        assert_eq!(id, "route_latency");
        assert!(inputs.is_empty());
    }

    #[test]
    fn unrelated_text_matches_nothing() {
        assert!(m("오늘 점심 뭐 먹지").is_none());
        assert!(m("").is_none());
    }

    #[test]
    fn extraction_strips_korean_particles() {
        let f = extract("example.com이 느리고 10.0.0.1에서 포트 443번이 막힘");
        assert_eq!(f.host.as_deref(), Some("example.com"));
        assert_eq!(f.port.as_deref(), Some("443"));
        assert_eq!(extract("서버(192.168.0.50)가 이상함").host.as_deref(), Some("192.168.0.50"));
        assert_eq!(extract("숫자 8080만 있음").port, None);
    }
}
