//! 임시 저장소를 만들어 출국 심사를 처음부터 끝까지 돌려 본다.

use border::check::{run, Request};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git");
    assert!(st.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&st.stderr));
}

fn write(dir: &Path, path: &str, text: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

const VISA: &str = r#"
version = 1
allow = ["src/**", "app/**", "Cargo.toml", "Cargo.lock", "docs/**"]
deny = ["word/**"]
always = ["src/security/**"]
[[usual]]
path = "app/build.rs"
line = '^"[a-z_]+",$'
[[guard]]
name = "보안 정책"
line = 'csp'
"#;

fn repo(name: &str) -> Tmp {
    let dir = std::env::temp_dir().join(format!("border-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, ".border/visa.toml", VISA);
    write(&dir, "src/a.rs", "fn a() {}\n");
    write(&dir, "src/conf.json", "{ \"csp\": \"self\" }\n");
    write(&dir, "app/build.rs", "fn main() {\n    cmds(&[\n        \"one\",\n    ]);\n}\n");
    write(&dir, "word/ko.json", "{}\n");
    write(&dir, "Cargo.toml", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n");
    write(&dir, "Cargo.lock", "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    git(&dir, &["checkout", "-q", "-b", "work"]);
    Tmp(dir)
}

fn commit(dir: &Path) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "work"]);
}

fn check(dir: &Path) -> border::check::Outcome {
    run(&Request { repo: dir, base: "main", head: "HEAD", visa_file: None }).unwrap()
}

fn declare(dir: &Path, lines: &[(&str, &str)]) {
    let mut s = String::new();
    for (w, p) in lines {
        s.push_str(&format!("[[declare]]\nwhat = \"{w}\"\nwhere = \"{p}\"\n"));
    }
    write(dir, ".border/declare.toml", &s);
}

#[test]
fn honest_plain_change_passes() {
    let t = repo("pass");
    write(&t.0, "src/a.rs", "fn a() { let _ = 1; }\n");
    write(&t.0, "app/build.rs", "fn main() {\n    cmds(&[\n        \"one\",\n        \"two\",\n    ]);\n}\n");
    declare(&t.0, &[("고침", "src/a.rs"), ("고침", "app/build.rs")]);
    commit(&t.0);
    let o = check(&t.0);
    assert_eq!(o.code(), 0, "{}", border::report::render(&o));
    assert_eq!(o.kept_count, 2);
}

#[test]
fn undeclared_change_is_missing() {
    let t = repo("missing");
    write(&t.0, "src/a.rs", "fn a() { let _ = 1; }\n");
    write(&t.0, "src/b.rs", "fn b() {}\n");
    declare(&t.0, &[("고침", "src/a.rs")]);
    commit(&t.0);
    let o = check(&t.0);
    assert_eq!(o.code(), 1);
    assert_eq!(o.missing, vec!["src/b.rs"]);
}

#[test]
fn declared_edit_but_deleted_is_mismatch() {
    let t = repo("mismatch");
    std::fs::remove_file(t.0.join("src/a.rs")).unwrap();
    declare(&t.0, &[("고침", "src/a.rs")]);
    commit(&t.0);
    let o = check(&t.0);
    assert_eq!(o.code(), 1);
    assert_eq!(o.mismatch.len(), 1);
}

#[test]
fn outside_visa_even_if_declared() {
    let t = repo("visa");
    write(&t.0, "word/ko.json", "{\"a\":1}\n");
    write(&t.0, "elsewhere.txt", "x\n");
    declare(&t.0, &[("고침", "word/ko.json"), ("만듦", "elsewhere.txt")]);
    commit(&t.0);
    let o = check(&t.0);
    assert_eq!(o.code(), 2);
    assert_eq!(o.visa_out.len(), 2);
}

#[test]
fn build_script_logic_change_is_shown_even_if_declared() {
    let t = repo("build");
    write(&t.0, "app/build.rs", "fn main() {\n    std::process::Command::new(\"x\");\n    cmds(&[\n        \"one\",\n    ]);\n}\n");
    declare(&t.0, &[("고침", "app/build.rs")]);
    commit(&t.0);
    let o = check(&t.0);
    assert_eq!(o.code(), 1);
    assert!(o.findings.iter().any(|f| f.class == "가" && f.declared));
}

#[test]
fn dependency_tricks_are_shown() {
    let t = repo("deps");
    write(
        &t.0,
        "Cargo.toml",
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nbuild = \"b.rs\"\n[dependencies]\nserde = { git = \"https://example.invalid/serde\" }\n",
    );
    write(&t.0, "Cargo.lock", "[[package]]\nname = \"serde\"\nversion = \"1.0.1\"\n\n[[package]]\nname = \"evil\"\nversion = \"0.0.1\"\n");
    declare(&t.0, &[("고침", "Cargo.toml"), ("고침", "Cargo.lock")]);
    commit(&t.0);
    let o = check(&t.0);
    let text = border::report::render(&o);
    assert_eq!(o.code(), 1, "{text}");
    assert!(o.findings.iter().any(|f| f.class == "가" && f.what.contains("package.build")), "{text}");
    assert!(o.findings.iter().any(|f| f.class == "다" && f.what.contains("git")), "{text}");
    assert!(o.findings.iter().any(|f| f.class == "다" && f.what.contains("evil")), "{text}");
    assert!(o.notices.iter().any(|n| n.contains("판이 바뀐 패키지 1개")), "{text}");
    assert_eq!(o.hosts, vec!["example.invalid"], "{text}");
}

#[test]
fn ai_files_guard_lines_hidden_chars_and_always() {
    let t = repo("misc");
    write(&t.0, "CLAUDE.md", "do things\n");
    write(&t.0, "src/conf.json", "{ \"csp\": \"*\" }\n");
    write(&t.0, "src/a.rs", "fn a() {} // \u{202E}x\n");
    write(&t.0, "src/security/v.rs", "fn v() {}\n");
    write(&t.0, ".border/visa.toml", &format!("{VISA}\n# widened\n"));
    declare(&t.0, &[("고침", "src/"), ("만듦", "src/"), ("만듦", "CLAUDE.md")]);
    commit(&t.0);
    let o = check(&t.0);
    let text = border::report::render(&o);
    for class in ["나", "라", "바", "늘", "비자"] {
        assert!(o.findings.iter().any(|f| f.class == class), "{class} 없음\n{text}");
    }
    assert_eq!(o.wide.len(), 2);
    assert!(!text.contains('\u{202E}'));
    // CLAUDE.md는 비자의 allow에 없다.
    assert_eq!(o.code(), 2);
}

#[test]
fn checker_uses_base_visa_not_branch_visa() {
    let t = repo("basevisa");
    write(&t.0, ".border/visa.toml", "version = 1\nallow = [\"**\"]\n");
    write(&t.0, "word/ko.json", "{\"a\":1}\n");
    declare(&t.0, &[("고침", "word/ko.json")]);
    commit(&t.0);
    let o = check(&t.0);
    assert_eq!(o.code(), 2);
    assert!(o.findings.iter().any(|f| f.class == "비자"));
}

#[test]
fn cannot_check_is_not_a_pass() {
    let t = repo("err");
    let r = run(&Request { repo: &t.0, base: "no-such-branch", head: "HEAD", visa_file: None });
    assert!(r.is_err());
    let r = run(&Request { repo: &t.0, base: "--output=x", head: "HEAD", visa_file: None });
    assert!(r.is_err());
}

#[test]
fn odd_file_names_do_not_break_the_report() {
    let t = repo("names");
    write(&t.0, "src/sp ace `tick`.rs", "x\n");
    write(&t.0, "src/한글.rs", "x\n");
    commit(&t.0);
    let o = check(&t.0);
    let text = border::report::render(&o);
    assert_eq!(o.missing.len(), 2);
    assert!(text.contains("한글.rs"));
    assert!(!text.contains("`tick`"));
}
