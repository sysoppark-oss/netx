//! 도구·레시피 레지스트리.
//!
//! `tools/*.json`과 `recipes/*.json`을 빌드할 때 실행 파일에 넣는다.
//! 실행 중에 외부 파일을 읽지 않으므로, 설치 후 스키마를 바꿔 검증을 우회할 수 없다.
//! 새 도구는 JSON 파일을 추가하고 아래 목록에 한 줄을 더하면 된다.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::model::{Recipe, ToolSchema, ValueType};

const TOOL_FILES: &[&str] = &[
    include_str!("../../tools/ping.json"),
    include_str!("../../tools/fping.json"),
    include_str!("../../tools/tcpdump.json"),
    include_str!("../../tools/mtr.json"),
    include_str!("../../tools/ss.json"),
    include_str!("../../tools/dig.json"),
    include_str!("../../tools/nc.json"),
    include_str!("../../tools/curl.json"),
    include_str!("../../tools/lsof.json"),
    include_str!("../../tools/iperf3.json"),
    include_str!("../../tools/winports.json"),
    include_str!("../../tools/portscan.json"),
];

const RECIPE_FILES: &[&str] = &[
    include_str!("../../recipes/web_down.json"),
    include_str!("../../recipes/route_latency.json"),
    include_str!("../../recipes/subnet_scan.json"),
    include_str!("../../recipes/port_hunt.json"),
    include_str!("../../recipes/bandwidth.json"),
    include_str!("../../recipes/quick_capture.json"),
];

pub struct Registry {
    tools: Vec<ToolSchema>,
    recipes: Vec<Recipe>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

impl Registry {
    pub fn global() -> &'static Registry {
        REGISTRY.get_or_init(|| Registry::load().expect("내장 스키마에 오류가 있습니다"))
    }

    pub fn load() -> Result<Registry, String> {
        let tools = TOOL_FILES
            .iter()
            .map(|raw| serde_json::from_str::<ToolSchema>(raw).map_err(|e| format!("도구 스키마 해석 실패: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let recipes = RECIPE_FILES
            .iter()
            .map(|raw| serde_json::from_str::<Recipe>(raw).map_err(|e| format!("레시피 해석 실패: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let reg = Registry { tools, recipes };
        reg.self_check()?;
        Ok(reg)
    }

    pub fn tool(&self, name: &str) -> Option<&ToolSchema> {
        self.tools.iter().find(|t| t.tool == name)
    }

    pub fn tools(&self) -> &[ToolSchema] {
        &self.tools
    }

    pub fn recipe(&self, id: &str) -> Option<&Recipe> {
        self.recipes.iter().find(|r| r.id == id)
    }

    pub fn recipes(&self) -> &[Recipe] {
        &self.recipes
    }

    /// 스키마 자체의 모순을 찾는다. 테스트와 시작 시점에 실행된다.
    fn self_check(&self) -> Result<(), String> {
        let mut names = HashSet::new();
        for t in &self.tools {
            let name = &t.tool;
            if !names.insert(name.as_str()) {
                return Err(format!("도구 이름 중복: {name}"));
            }
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                return Err(format!("도구 이름은 영문 소문자와 숫자만: {name}"));
            }
            if t.engines.is_empty() {
                return Err(format!("{name}: 실행 엔진이 없습니다"));
            }
            let mut flags = HashSet::new();
            for o in &t.options {
                for f in std::iter::once(&o.flag).chain(&o.aliases) {
                    if !(f.starts_with('-') || f.starts_with('+')) || f.len() < 2 || f == "--" {
                        return Err(format!("{name}: 옵션 이름이 올바르지 않습니다: {f}"));
                    }
                    if !flags.insert(f.as_str()) {
                        return Err(format!("{name}: 옵션 중복: {f}"));
                    }
                }
                if o.kind == ValueType::Enum && o.values.is_empty() {
                    return Err(format!("{name} {}: enum에 values가 없습니다", o.flag));
                }
                if o.label.is_empty() {
                    return Err(format!("{name} {}: 설명(label)이 없습니다", o.flag));
                }
            }
            for (idx, p) in t.positional.iter().enumerate() {
                if p.kind == ValueType::Boolean {
                    return Err(format!("{name}: 위치 인자는 boolean일 수 없습니다: {}", p.name));
                }
                if p.variadic && idx + 1 != t.positional.len() {
                    return Err(format!("{name}: variadic은 마지막 위치 인자만 가능합니다: {}", p.name));
                }
            }
        }

        let mut ids = HashSet::new();
        for r in &self.recipes {
            if !ids.insert(r.id.as_str()) {
                return Err(format!("레시피 id 중복: {}", r.id));
            }
            if r.steps.is_empty() {
                return Err(format!("레시피 {}: 단계가 없습니다", r.id));
            }
            for s in &r.steps {
                if self.tool(&s.tool).is_none() {
                    return Err(format!("레시피 {}: 등록되지 않은 도구 {}", r.id, s.tool));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Risk;

    #[test]
    fn embedded_schemas_load_and_pass_self_check() {
        let reg = Registry::load().expect("스키마 로드");
        assert_eq!(reg.tools().len(), 12);
        assert_eq!(reg.recipes().len(), 6);
    }

    #[test]
    fn known_dangerous_options_are_forbidden() {
        let reg = Registry::global();
        let risk = |tool: &str, flag: &str| {
            reg.tool(tool)
                .and_then(|t| t.options.iter().find(|o| o.flag == flag || o.aliases.iter().any(|a| a == flag)))
                .map(|o| o.risk)
        };
        for (tool, flag) in [
            ("tcpdump", "-z"),
            ("nc", "-e"),
            ("nc", "-c"),
            ("curl", "-K"),
            ("curl", "--config"),
            ("curl", "-T"),
            ("dig", "-f"),
            ("ss", "-K"),
        ] {
            assert_eq!(risk(tool, flag), Some(Risk::Forbidden), "{tool} {flag}");
        }
    }
}
