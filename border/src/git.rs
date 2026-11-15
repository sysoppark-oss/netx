//! git을 부른다. 셸을 쓰지 않고 인자 배열로만 부른다.
//! git이 내는 글(경로, 내용)은 모두 밖에서 온 자료로 다룬다.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Repo {
    dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Added,
    Modified,
    Deleted,
    Other,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub path: String,
    pub kind: Kind,
    pub added: u64,
    pub removed: u64,
    pub binary: bool,
}

#[derive(Debug, Default)]
pub struct Lines {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl Repo {
    pub fn open(dir: &Path) -> Repo {
        Repo { dir: dir.to_path_buf() }
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.quotepath=false", "-c", "core.fsmonitor=false"])
            .args(args)
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .output()
            .map_err(|e| format!("git을 실행하지 못했습니다: {e}"))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect())
        }
    }

    /// 이름을 커밋 번호로 바꾼다. 옵션처럼 생긴 이름은 받지 않는다.
    pub fn commit(&self, name: &str) -> Result<String, String> {
        if name.is_empty() || name.starts_with('-') || name.chars().any(|c| c.is_control()) {
            return Err("커밋 이름이 올바르지 않습니다".into());
        }
        let spec = format!("{name}^{{commit}}");
        let out = self.run(&["rev-parse", "--verify", "--quiet", "--end-of-options", &spec])?;
        let id = String::from_utf8_lossy(&out).trim().to_string();
        if id.len() >= 40 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(id)
        } else {
            Err("커밋을 찾지 못했습니다".into())
        }
    }

    pub fn merge_base(&self, a: &str, b: &str) -> Result<String, String> {
        let out = self.run(&["merge-base", a, b])?;
        let id = String::from_utf8_lossy(&out).trim().to_string();
        if id.is_empty() {
            Err("두 가지의 갈라진 자리를 찾지 못했습니다".into())
        } else {
            Ok(id)
        }
    }

    pub fn count_commits(&self, from: &str, to: &str) -> u64 {
        let range = format!("{from}..{to}");
        self.run(&["rev-list", "--count", &range])
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o).trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn changes(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        let names = self.run(&["diff", "--name-status", "-z", "--no-renames", from, to])?;
        let stats = self.run(&["diff", "--numstat", "-z", "--no-renames", from, to])?;
        let mut list = parse_name_status(&names);
        let counts = parse_numstat(&stats);
        for c in &mut list {
            if let Some((a, r, bin)) = counts.iter().find(|(p, ..)| *p == c.path).map(|(_, a, r, b)| (*a, *r, *b)) {
                c.added = a;
                c.removed = r;
                c.binary = bin;
            }
        }
        Ok(list)
    }

    /// 한 파일에서 더해진 줄과 빠진 줄.
    pub fn lines(&self, from: &str, to: &str, path: &str) -> Lines {
        let out = self.run(&[
            "diff", "-U0", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", from, to, "--", path,
        ]);
        match out {
            Ok(bytes) => parse_patch(&bytes),
            Err(_) => Lines::default(),
        }
    }

    pub fn file(&self, rev: &str, path: &str) -> Option<String> {
        let spec = format!("{rev}:{path}");
        let out = self.run(&["show", "--no-textconv", "--end-of-options", &spec]).ok()?;
        if out.len() > 32 * 1024 * 1024 {
            return None;
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    /// 기준점의 어느 파일에든 이 글이 있는가.
    pub fn has_text(&self, rev: &str, text: &str) -> bool {
        if text.is_empty() || text.starts_with('-') {
            return true;
        }
        self.run(&["grep", "-q", "-I", "-F", "-e", text, rev, "--"]).is_ok()
    }
}

pub fn parse_name_status(bytes: &[u8]) -> Vec<Change> {
    let mut out = Vec::new();
    let mut parts = bytes.split(|b| *b == 0).filter(|p| !p.is_empty());
    while let (Some(st), Some(path)) = (parts.next(), parts.next()) {
        let kind = match st.first() {
            Some(b'A') => Kind::Added,
            Some(b'M') => Kind::Modified,
            Some(b'D') => Kind::Deleted,
            _ => Kind::Other,
        };
        out.push(Change {
            path: String::from_utf8_lossy(path).into_owned(),
            kind,
            added: 0,
            removed: 0,
            binary: false,
        });
    }
    out
}

pub fn parse_numstat(bytes: &[u8]) -> Vec<(String, u64, u64, bool)> {
    let mut out = Vec::new();
    for rec in bytes.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let text = String::from_utf8_lossy(rec);
        let mut f = text.splitn(3, '\t');
        let (Some(a), Some(r), Some(p)) = (f.next(), f.next(), f.next()) else { continue };
        let bin = a == "-" || r == "-";
        out.push((p.to_string(), a.parse().unwrap_or(0), r.parse().unwrap_or(0), bin));
    }
    out
}

pub fn parse_patch(bytes: &[u8]) -> Lines {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = Lines::default();
    let mut in_hunk = false;
    for line in text.split('\n') {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if lines.added.len() + lines.removed.len() >= 200_000 {
            break;
        }
        if let Some(rest) = line.strip_prefix('+') {
            lines.added.push(rest.trim_end_matches('\r').to_string());
        } else if let Some(rest) = line.strip_prefix('-') {
            lines.removed.push(rest.trim_end_matches('\r').to_string());
        } else if line.starts_with("diff --git") {
            in_hunk = false;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_status() {
        let c = parse_name_status(b"A\0a b.rs\0M\0x/y\0D\0z\0T\0q\0");
        assert_eq!(c.len(), 4);
        assert_eq!(c[0].path, "a b.rs");
        assert_eq!(c[0].kind, Kind::Added);
        assert_eq!(c[2].kind, Kind::Deleted);
        assert_eq!(c[3].kind, Kind::Other);
    }

    #[test]
    fn numstat() {
        let n = parse_numstat(b"3\t1\ta.rs\0-\t-\timg.png\0");
        assert_eq!(n[0], ("a.rs".to_string(), 3, 1, false));
        assert!(n[1].3);
    }

    #[test]
    fn patch() {
        let p = b"diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n-old\n+new\n+--- not a header\n";
        let l = parse_patch(p);
        assert_eq!(l.removed, vec!["old"]);
        assert_eq!(l.added, vec!["new", "--- not a header"]);
    }

    #[test]
    fn broken_input_does_not_panic() {
        let mut seed = 0x9e3779b97f4a7c15u64;
        for _ in 0..300 {
            let mut buf = Vec::new();
            for _ in 0..200 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                buf.push(match seed % 7 {
                    0 => 0,
                    1 => b'\t',
                    2 => b'\n',
                    3 => b'+',
                    4 => b'@',
                    _ => (seed >> 8) as u8,
                });
            }
            let _ = parse_name_status(&buf);
            let _ = parse_numstat(&buf);
            let _ = parse_patch(&buf);
        }
    }
}
