//! 레시피를 실행 계획으로 바꾼다.
//!
//! 레시피의 인자에 있는 `{이름}` 자리에 사용자가 입력한 값을 넣는다.
//! 입력값은 넣기 전에 종류별 규칙으로 검사하고, 만들어진 계획은 검증기를 다시 통과한다.

use std::collections::BTreeMap;

use crate::model::{ExecutionPlan, PlanStep, Recipe, ValueType};
use crate::security::values;

pub fn instantiate(recipe: &Recipe, inputs: &BTreeMap<String, String>) -> Result<ExecutionPlan, String> {
    let mut vars: BTreeMap<String, String> = BTreeMap::new();

    for input in &recipe.inputs {
        let value = inputs.get(&input.name).map(|v| v.trim()).unwrap_or("");
        if value.is_empty() {
            return Err(format!("{}을(를) 입력하세요", input.label));
        }
        values::check(&input.rule(), value).map_err(|e| format!("{}: {e}", input.label))?;

        if input.kind == ValueType::Url {
            let (host, port) = values::url_parts(value).ok_or_else(|| format!("{}: URL을 해석할 수 없습니다", input.label))?;
            vars.insert(format!("{}.host", input.name), host);
            vars.insert(format!("{}.port", input.name), port.to_string());
        }
        vars.insert(input.name.clone(), value.to_string());
    }

    let steps = recipe
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| PlanStep {
            order: Some(i as u32 + 1),
            tool: s.tool.clone(),
            args: s.args.iter().map(|a| substitute(a, &vars)).collect(),
            reason: s.reason.clone(),
            expect: s.expect.clone(),
        })
        .collect();

    Ok(ExecutionPlan {
        intent: recipe.id.clone(),
        summary: recipe.title.clone(),
        target: recipe.inputs.first().and_then(|i| vars.get(&i.name).cloned()),
        steps,
    })
}

/// `{이름}`을 값으로 바꾼다. 한 번만 훑으므로 값 안의 중괄호는 다시 해석하지 않는다.
/// 등록되지 않은 이름(예: curl의 `%{time_connect}`)은 그대로 둔다.
fn substitute(template: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if vars.contains_key(&after[..close]) => {
                out.push_str(&vars[&after[..close]]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Level, Risk, Source};
    use crate::registry::Registry;
    use crate::security::validator::validate_plan;

    fn inputs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn substitute_leaves_unknown_braces() {
        let vars = inputs(&[("port", "8080"), ("url.host", "example.com")]);
        assert_eq!(substitute(":{port}", &vars), ":8080");
        assert_eq!(substitute("{url.host}", &vars), "example.com");
        assert_eq!(substitute("t:%{time_connect} {port}", &vars), "t:%{time_connect} 8080");
        assert_eq!(substitute("{unclosed", &vars), "{unclosed");
    }

    #[test]
    fn substituted_value_is_not_reinterpreted() {
        let vars = inputs(&[("a", "{b}"), ("b", "X")]);
        assert_eq!(substitute("{a}", &vars), "{b}");
    }

    /// 모든 레시피가 예시 입력으로, 자기 최소 등급에서 차단 없이 통과해야 한다.
    #[test]
    fn every_recipe_validates_with_its_example_inputs() {
        let reg = Registry::global();
        for recipe in reg.recipes() {
            let example: BTreeMap<String, String> = recipe
                .inputs
                .iter()
                .map(|i| (i.name.clone(), i.example.clone().expect("레시피 입력에는 example이 있어야 한다")))
                .collect();
            let plan = instantiate(recipe, &example).unwrap_or_else(|e| panic!("{}: {e}", recipe.id));
            let v = validate_plan(reg, &plan, recipe.min_level, Source::Recipe, recipe.parallel);
            for step in &v.steps {
                assert!(!step.blocked, "{} {}: {:?}", recipe.id, step.display, step.issues);
                assert!(step.risk <= Risk::Caution, "{} {}", recipe.id, step.display);
            }
            assert!(!v.blocked);
        }
    }

    #[test]
    fn web_down_expands_url_parts() {
        let reg = Registry::global();
        let plan = instantiate(reg.recipe("web_down").unwrap(), &inputs(&[("url", "http://192.168.0.50:8080/app")])).unwrap();
        assert_eq!(plan.steps[0].args, ["+short", "192.168.0.50"]);
        assert_eq!(plan.steps[1].args, ["-vz", "-w", "3", "192.168.0.50", "8080"]);
        assert_eq!(plan.steps[2].args.last().unwrap(), "http://192.168.0.50:8080/app");
    }

    #[test]
    fn bad_inputs_are_rejected_before_substitution() {
        let reg = Registry::global();
        let bad: &[(&str, &[(&str, &str)])] = &[
            ("web_down", &[("url", "file:///etc/passwd")]),
            ("web_down", &[("url", "-K/etc/passwd")]),
            ("web_down", &[("url", "")]),
            ("web_down", &[]),
            ("route_latency", &[("target", "8.8.8.8; rm -rf /")]),
            ("route_latency", &[("target", "-f")]),
            ("subnet_scan", &[("cidr", "10.0.0.0/8")]),
            ("port_hunt", &[("port", "8080 -K")]),
            ("port_hunt", &[("port", "99999")]),
            ("quick_capture", &[("iface", "eth0 -z sh"), ("port", "443")]),
        ];
        for (id, pairs) in bad {
            assert!(instantiate(reg.recipe(id).unwrap(), &inputs(pairs)).is_err(), "{id} {pairs:?}");
        }
    }

    #[test]
    fn recipe_level_gates() {
        let reg = Registry::global();
        let recipe = reg.recipe("quick_capture").unwrap();
        let plan = instantiate(recipe, &inputs(&[("iface", "eth0"), ("port", "443")])).unwrap();
        assert!(validate_plan(reg, &plan, Level::General, Source::Recipe, false).blocked);
        assert!(!validate_plan(reg, &plan, Level::Junior, Source::Recipe, false).blocked);

        // 공인 대역 스캔은 레시피라도 초급에서는 차단, 중급부터 위험 표시 후 허용
        let scan = instantiate(reg.recipe("subnet_scan").unwrap(), &inputs(&[("cidr", "8.8.8.0/24")])).unwrap();
        assert!(validate_plan(reg, &scan, Level::Junior, Source::Recipe, false).blocked);
        let v = validate_plan(reg, &scan, Level::Middle, Source::Recipe, false);
        assert!(!v.blocked);
        assert_eq!(v.risk, Risk::Danger);
    }
}
