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
        let typed = inputs.get(&input.name).map(|v| v.trim()).unwrap_or("");
        let mut value = if typed.is_empty() { input.default.clone().unwrap_or_default() } else { typed.to_string() };
        if value.is_empty() {
            if input.optional {
                continue;
            }
            return Err(format!("{}을(를) 입력하세요", input.label));
        }
        // 주소만 적어도 되게 한다: example.com → https://example.com
        if input.kind == ValueType::Url && !value.contains("://") {
            value = format!("https://{value}");
        }
        values::check(&input.rule(), &value).map_err(|e| format!("{}: {e}", input.label))?;

        if input.kind == ValueType::Url {
            let (host, port) = values::url_parts(&value).ok_or_else(|| format!("{}: URL을 해석할 수 없습니다", input.label))?;
            vars.insert(format!("{}.host", input.name), host);
            vars.insert(format!("{}.port", input.name), port.to_string());
        }
        vars.insert(input.name.clone(), value);
    }

    let mut steps = Vec::new();
    for (i, s) in recipe.steps.iter().enumerate() {
        let args = match built_args(&recipe.id, i, &vars) {
            Some(args) => args?,
            None => s.args.iter().map(|a| substitute(a, &vars)).collect(),
        };
        steps.push(PlanStep { order: Some(i as u32 + 1), tool: s.tool.clone(), args, reason: s.reason.clone(), expect: s.expect.clone() });
    }

    Ok(ExecutionPlan {
        intent: recipe.id.clone(),
        summary: recipe.title.clone(),
        target: recipe.inputs.first().and_then(|i| vars.get(&i.name).cloned()),
        steps,
    })
}

/// 입력 조합에 따라 인자가 달라지는 레시피는 여기서 조립한다. 해당 없으면 None(틀 그대로 사용).
fn built_args(recipe_id: &str, step: usize, vars: &BTreeMap<String, String>) -> Option<Result<Vec<String>, String>> {
    let get = |k: &str| vars.get(k).map(String::as_str);
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();

    match (recipe_id, step) {
        ("quick_capture", 0) => {
            let proto = get("proto").unwrap_or("all");
            let port = get("port");
            if port.is_some() && matches!(proto, "icmp" | "arp") {
                return Some(Err("ICMP와 ARP에는 포트가 없습니다. 포트를 비우거나 프로토콜을 바꾸세요.".into()));
            }
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let mut args = own(&["-i", get("iface").unwrap_or("any"), "-n", "-c", get("count").unwrap_or("100"), "-w", &format!("capture-{stamp}.pcap")]);

            // 필터식: 프로토콜 and 포트 and 상대 주소
            let mut conditions: Vec<Vec<String>> = Vec::new();
            if proto != "all" {
                conditions.push(own(&[proto]));
            }
            if let Some(p) = port {
                conditions.push(own(&[if p.contains('-') { "portrange" } else { "port" }, p]));
            }
            if let Some(h) = get("host") {
                conditions.push(own(&["host", h]));
            }
            for (i, c) in conditions.into_iter().enumerate() {
                if i > 0 {
                    args.push("and".into());
                }
                args.extend(c);
            }
            Some(Ok(args))
        }
        ("port_hunt", _) => {
            let port = get("port")?;
            let proto = get("proto").unwrap_or("all");
            let (lo, hi) = port.split_once('-').unwrap_or((port, port));
            Some(Ok(match step {
                0 => own(&[port, proto]),
                1 => {
                    let flags = match proto {
                        "tcp" => "-tlpn",
                        "udp" => "-ulpn",
                        _ => "-tulpn",
                    };
                    if lo == hi {
                        own(&[flags, "sport", "=", &format!(":{lo}")])
                    } else {
                        own(&[flags, "sport", ">=", &format!(":{lo}"), "and", "sport", "<=", &format!(":{hi}")])
                    }
                }
                _ => {
                    let spec = match proto {
                        "tcp" => format!("TCP:{port}"),
                        "udp" => format!("UDP:{port}"),
                        _ => format!(":{port}"),
                    };
                    own(&["-nP", "-i", &spec])
                }
            }))
        }
        _ => None,
    }
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
            ("web_down", &[("url", "exa mple.com")]),
            ("quick_capture", &[("iface", "eth0"), ("proto", "icmp"), ("port", "80")]),
            ("quick_capture", &[("iface", "eth0"), ("proto", "tcp; id")]),
            ("quick_capture", &[("iface", "eth0"), ("count", "0")]),
            ("port_hunt", &[("port", "30-21")]),
            ("port_hunt", &[("port", "80"), ("proto", "sctp")]),
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

    fn args(recipe: &str, pairs: &[(&str, &str)]) -> Vec<Vec<String>> {
        let plan = instantiate(Registry::global().recipe(recipe).unwrap(), &inputs(pairs)).unwrap();
        let v = validate_plan(Registry::global(), &plan, Level::Middle, Source::Recipe, false);
        assert!(!v.blocked, "{:?}", v.steps.iter().map(|s| (&s.display, &s.issues)).collect::<Vec<_>>());
        plan.steps.into_iter().map(|s| s.args).collect()
    }

    #[test]
    fn url_scheme_is_optional() {
        let a = args("web_down", &[("url", "example.com")]);
        assert_eq!(a[0], ["+short", "example.com"]);
        assert_eq!(a[1][3..], ["example.com", "443"]);
        assert_eq!(a[2].last().unwrap(), "https://example.com");
        assert_eq!(args("web_down", &[("url", "http://10.0.0.5:8080")])[1][3..], ["10.0.0.5", "8080"]);
        assert_eq!(args("web_down", &[("url", "10.0.0.5:8443/admin")])[1][3..], ["10.0.0.5", "8443"]);
    }

    #[test]
    fn capture_filter_is_built_from_choices() {
        let tail = |pairs: &[(&str, &str)]| args("quick_capture", pairs)[0][7..].to_vec();
        let a = &args("quick_capture", &[("iface", "eth0")])[0];
        assert_eq!(a[..6], ["-i", "eth0", "-n", "-c", "100", "-w"]);
        assert!(a[6].starts_with("capture-") && a[6].ends_with(".pcap"));
        assert_eq!(a.len(), 7, "조건이 없으면 필터도 없다");

        assert_eq!(args("quick_capture", &[])[0][1], "any", "인터페이스 기본값");
        assert_eq!(tail(&[("proto", "tcp"), ("port", "21-30")]), ["tcp", "and", "portrange", "21-30"]);
        assert_eq!(tail(&[("proto", "udp"), ("port", "53")]), ["udp", "and", "port", "53"]);
        assert_eq!(tail(&[("proto", "icmp")]), ["icmp"]);
        assert_eq!(tail(&[("port", "443"), ("host", "10.0.0.1")]), ["port", "443", "and", "host", "10.0.0.1"]);
        assert_eq!(args("quick_capture", &[("count", "500")])[0][4], "500");
    }

    #[test]
    fn port_hunt_supports_ranges_and_protocols() {
        let a = args("port_hunt", &[("port", "8080")]);
        assert_eq!(a[0], ["8080", "all"]);
        assert_eq!(a[1], ["-tulpn", "sport", "=", ":8080"]);
        assert_eq!(a[2], ["-nP", "-i", ":8080"]);

        let a = args("port_hunt", &[("port", "21-30"), ("proto", "tcp")]);
        assert_eq!(a[0], ["21-30", "tcp"]);
        assert_eq!(a[1], ["-tlpn", "sport", ">=", ":21", "and", "sport", "<=", ":30"]);
        assert_eq!(a[2], ["-nP", "-i", "TCP:21-30"]);

        assert_eq!(args("port_hunt", &[("port", "1-65535"), ("proto", "udp")])[2], ["-nP", "-i", "UDP:1-65535"]);
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
