//! 의존성과 빌드 설정을 읽는다. 줄이 아니라 뜻(파싱한 값)으로 견준다.

use std::collections::{BTreeMap, BTreeSet};

/// 잠금 파일에서 패키지 이름과 판.
pub fn lock_packages(path: &str, text: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if path.ends_with("Cargo.lock") {
        let mut name: Option<String> = None;
        for line in text.lines() {
            if let Some(v) = quoted(line, "name = ") {
                name = Some(v);
            } else if let (Some(v), Some(n)) = (quoted(line, "version = "), name.take()) {
                out.entry(n).or_default().insert(v);
            }
        }
    } else if let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(serde_json::Value::Object(pk)) = root.get("packages") {
            for (key, val) in pk {
                let Some(name) = key.rsplit("node_modules/").next().filter(|n| !n.is_empty() && key.contains("node_modules/")) else { continue };
                let ver = val.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
                out.entry(name.to_string()).or_default().insert(ver);
            }
        }
    }
    out
}

fn quoted(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim();
    let inner = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

pub fn is_lock(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "Cargo.lock" || name == "package-lock.json"
}

#[derive(Debug, Default, PartialEq)]
pub struct Manifest {
    /// 빌드 때 실행되는 것에 닿는 값(빌드 스크립트, 빌드 의존성, proc-macro, npm scripts)을 글로 편 것.
    pub run: BTreeMap<String, String>,
    /// 등록소가 아닌 곳에서 받는 의존성: "이름 ← git 주소" 따위.
    pub sources: BTreeSet<String>,
    /// [patch], [replace], overrides 따위를 글로 편 것.
    pub patch: String,
    /// 의존성마다 켠 기능.
    pub features: BTreeMap<String, String>,
}

pub fn is_manifest(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "Cargo.toml" || name == "package.json"
}

/// 읽지 못하면 None. 부르는 쪽은 "읽지 못함"을 사람 확인으로 올린다.
pub fn manifest(path: &str, text: &str) -> Option<Manifest> {
    if path.ends_with("Cargo.toml") {
        cargo(text)
    } else {
        npm(text)
    }
}

fn cargo(text: &str) -> Option<Manifest> {
    let root: toml::Table = toml::from_str(text).ok()?;
    let mut m = Manifest::default();
    if let Some(b) = root.get("package").and_then(|p| p.get("build")) {
        m.run.insert("package.build".into(), b.to_string());
    }
    if let Some(l) = root.get("package").and_then(|p| p.get("links")) {
        m.run.insert("package.links".into(), l.to_string());
    }
    if let Some(v) = root.get("lib").and_then(|l| l.get("proc-macro").or_else(|| l.get("proc_macro"))) {
        if v.as_bool() == Some(true) {
            m.run.insert("lib.proc-macro".into(), "true".into());
        }
    }
    let mut tables: Vec<(String, &toml::Value)> = Vec::new();
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(t) = root.get(key) {
            tables.push((key.to_string(), t));
        }
        if let Some(t) = root.get("workspace").and_then(|w| w.get(key)) {
            tables.push((format!("workspace.{key}"), t));
        }
    }
    if let Some(toml::Value::Table(targets)) = root.get("target") {
        for (tname, tval) in targets {
            for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(t) = tval.get(key) {
                    tables.push((format!("target.{tname}.{key}"), t));
                }
            }
        }
    }
    for (label, table) in tables {
        let Some(table) = table.as_table() else { continue };
        if label.ends_with("build-dependencies") {
            m.run.insert(label.clone(), toml::Value::Table(table.clone()).to_string());
        }
        for (name, spec) in table {
            if let Some(t) = spec.as_table() {
                for src in ["git", "path", "registry", "registry-index"] {
                    if let Some(v) = t.get(src) {
                        m.sources.insert(format!("{name} ← {src} {}", v.as_str().unwrap_or("?")));
                    }
                }
                if let Some(f) = t.get("features") {
                    m.features.insert(format!("{label}.{name}"), f.to_string());
                }
            }
        }
    }
    let mut patch = String::new();
    for key in ["patch", "replace"] {
        if let Some(v) = root.get(key) {
            patch.push_str(&format!("[{key}] {v}\n"));
        }
    }
    m.patch = patch;
    Some(m)
}

fn npm(text: &str) -> Option<Manifest> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = root.as_object()?;
    let mut m = Manifest::default();
    if let Some(s) = obj.get("scripts") {
        m.run.insert("scripts".into(), s.to_string());
    }
    for key in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        if let Some(serde_json::Value::Object(deps)) = obj.get(key) {
            for (name, spec) in deps {
                let s = spec.as_str().unwrap_or("");
                let plain = s.chars().all(|c| c.is_ascii_alphanumeric() || " .^~<>=|*-+xX".contains(c));
                if !plain {
                    m.sources.insert(format!("{name} ← {s}"));
                }
            }
        }
    }
    let mut patch = String::new();
    for key in ["overrides", "resolutions", "bundleDependencies", "bundledDependencies"] {
        if let Some(v) = obj.get(key) {
            patch.push_str(&format!("{key} {v}\n"));
        }
    }
    m.patch = patch;
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_lock() {
        let t = "[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.2.0\"\n";
        let p = lock_packages("x/Cargo.lock", t);
        assert_eq!(p.len(), 2);
        assert!(p["a"].contains("1.0.0"));
    }

    #[test]
    fn npm_lock() {
        let t = r#"{"packages":{"":{"name":"x"},"node_modules/react":{"version":"19.0.0"},"node_modules/a/node_modules/@s/b":{"version":"1.2.3"}}}"#;
        let p = lock_packages("package-lock.json", t);
        assert!(p.contains_key("react"));
        assert!(p.contains_key("@s/b"));
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn cargo_manifest() {
        let base = manifest("Cargo.toml", "[package]\nname=\"x\"\n[dependencies]\nserde = \"1\"\nhmac = { version = \"0.12\", features = [\"std\"] }\n").unwrap();
        assert!(base.run.is_empty());
        assert!(base.sources.is_empty());
        let head = manifest(
            "Cargo.toml",
            "[package]\nname=\"x\"\nbuild = \"evil.rs\"\n[lib]\nproc-macro = true\n[dependencies]\nserde = { git = \"https://example.invalid/serde\" }\n[build-dependencies]\ncc = \"1\"\n[patch.crates-io]\nhmac = { path = \"../h\" }\n",
        )
        .unwrap();
        assert!(head.run.contains_key("package.build"));
        assert!(head.run.contains_key("lib.proc-macro"));
        assert!(head.run.contains_key("build-dependencies"));
        assert_eq!(head.sources.len(), 1);
        assert!(!head.patch.is_empty());
        assert!(manifest("Cargo.toml", "[[[").is_none());
    }

    #[test]
    fn npm_manifest() {
        let m = manifest("package.json", r#"{"scripts":{"postinstall":"node x.js"},"dependencies":{"a":"^1.2.3","b":"github:u/r","c":"file:../c"}}"#).unwrap();
        assert!(m.run.contains_key("scripts"));
        assert_eq!(m.sources.len(), 2);
        assert!(manifest("package.json", "[1,2]").is_none());
    }
}
