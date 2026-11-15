//! 출국 심사: 비자 · 신고 · 실제 변경을 맞춰 본다. 규칙만 쓴다.

use crate::deps;
use crate::git::{Change, Kind, Repo};
use crate::glob;
use crate::visa::{self, Declare, What};
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

pub const VISA_PATH: &str = ".border/visa.toml";
pub const DECLARE_PATH: &str = ".border/declare.toml";

#[derive(Deserialize)]
struct Rules {
    run: Vec<String>,
    ai: Vec<String>,
    registry: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Finding {
    /// 가, 나, 다, 라, 바, 비자, 늘
    pub class: &'static str,
    pub path: String,
    pub what: String,
    pub added: u64,
    pub removed: u64,
    pub declared: bool,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub base: String,
    pub head: String,
    pub commits: u64,
    pub files: usize,
    pub visa_draft: bool,
    pub visa_local: bool,
    pub findings: Vec<Finding>,
    pub visa_out: Vec<String>,
    /// 바뀌었는데 신고에 없는 것
    pub missing: Vec<String>,
    /// 신고한 것과 한 일이 다른 것 (예: 고친다고 하고 지움)
    pub mismatch: Vec<String>,
    pub declared_count: usize,
    pub kept_count: usize,
    pub unused: Vec<String>,
    pub wide: Vec<(String, usize)>,
    pub hosts: Vec<String>,
    pub notices: Vec<String>,
    pub biggest: Vec<(String, u64, u64)>,
}

impl Outcome {
    pub fn code(&self) -> i32 {
        if !self.visa_out.is_empty() {
            2
        } else if !self.findings.is_empty() || !self.missing.is_empty() || !self.mismatch.is_empty() {
            1
        } else {
            0
        }
    }
}

pub struct Request<'a> {
    pub repo: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    /// 기준점에 비자가 없을 때 시험 삼아 쓰는 로컬 파일. 권위가 없다.
    pub visa_file: Option<&'a Path>,
}

fn invisible(c: char) -> bool {
    matches!(c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200F}'
        | '\u{2060}' | '\u{FEFF}' | '\u{061C}' | '\u{00AD}')
        || (c.is_control() && c != '\t')
}

pub fn run(req: &Request) -> Result<Outcome, String> {
    let rules: Rules = toml::from_str(include_str!("rules.toml")).map_err(|_| "기본 분류를 읽지 못했습니다".to_string())?;
    let repo = Repo::open(req.repo);
    let base_id = repo.commit(req.base).map_err(|e| format!("기준점({}): {e}", crate::report::clean(req.base)))?;
    let head_id = repo.commit(req.head).map_err(|e| format!("심사할 자리({}): {e}", crate::report::clean(req.head)))?;
    let fork = repo.merge_base(&base_id, &head_id)?;

    let mut out = Outcome { base: req.base.to_string(), head: req.head.to_string(), ..Default::default() };
    let visa_text = match req.visa_file {
        Some(p) => {
            out.visa_local = true;
            std::fs::read_to_string(p).map_err(|_| "비자 파일을 열지 못했습니다".to_string())?
        }
        None => repo.file(&base_id, VISA_PATH).ok_or("기준점에 비자(.border/visa.toml)가 없습니다")?,
    };
    let visa = visa::parse_visa(&visa_text)?;
    out.visa_draft = visa.draft;

    let declares = match repo.file(&head_id, DECLARE_PATH) {
        Some(t) => visa::parse_declare(&t)?,
        None => {
            out.notices.push("신고 파일(.border/declare.toml)이 없습니다. 바뀐 것은 모두 신고 누락으로 셉니다.".into());
            Vec::new()
        }
    };
    out.declared_count = declares.len();
    let mut used = vec![0usize; declares.len()];

    let changes = repo.changes(&fork, &head_id)?;
    out.commits = repo.count_commits(&fork, &head_id);
    out.files = changes.len();
    let host_re = Regex::new(r"https?://([A-Za-z0-9][A-Za-z0-9.-]{0,252})").map_err(|_| "내부 오류".to_string())?;
    let mut hosts_added: BTreeSet<String> = BTreeSet::new();
    let mut hosts_removed: BTreeSet<String> = BTreeSet::new();
    let mut bumps = 0usize;

    for ch in &changes {
        let path = ch.path.as_str();
        if path == DECLARE_PATH {
            continue;
        }
        // 1. 신고와 맞는가
        let declared = declared_as(&declares, &mut used, ch, &mut out);
        // 2. 비자 안인가
        if path == VISA_PATH {
            push(&mut out, "비자", ch, "비자가 바뀜", declared);
        } else if glob::any(&visa.deny, path) || !glob::any(&visa.allow, path) {
            out.visa_out.push(path.to_string());
        }
        // 3. 위험 분류
        let is_lock = deps::is_lock(path);
        let lines = if ch.binary || is_lock { Default::default() } else { repo.lines(&fork, &head_id, path) };
        let whole_file = ch.kind != Kind::Modified || ch.binary;
        let unusual = || lines.added.iter().chain(lines.removed.iter()).any(|l| !visa.is_usual(path, l));

        if glob::any(&rules.run, path) && (whole_file || unusual()) {
            push(&mut out, "가", ch, kind_word(ch), declared);
        }
        if glob::any(&rules.ai, path) {
            push(&mut out, "나", ch, kind_word(ch), declared);
        } else if glob::any(&visa.ai_docs, path) {
            out.notices.push(format!("다음 AI 세션이 읽는 문서가 바뀜: {} (+{} −{})", crate::report::tick(path), ch.added, ch.removed));
        }
        if glob::any(&rules.registry, path) {
            push(&mut out, "다", ch, "패키지를 받는 곳을 정하는 파일", declared);
        }
        if glob::any(&visa.always, path) {
            push(&mut out, "늘", ch, kind_word(ch), declared);
        }
        for g in &visa.guard {
            if let Some(p) = &g.path {
                if !glob::matches(p, path) {
                    continue;
                }
            }
            let hit = match &g.line {
                Some(re) => lines.added.iter().chain(lines.removed.iter()).any(|l| re.is_match(l)),
                None => whole_file || unusual(),
            };
            if hit {
                let what = format!("풀지 않는 설정: {}", crate::report::clean(&g.name));
                push(&mut out, "라", ch, &what, declared);
            }
        }
        if deps::is_manifest(path) {
            manifest_check(&repo, &fork, &head_id, ch, declared, &mut out);
        }
        if is_lock {
            bumps += lock_check(&repo, &fork, &head_id, ch, declared, &mut out);
        }
        let hidden = lines.added.iter().filter(|l| l.chars().any(invisible)).count();
        if hidden > 0 {
            push(&mut out, "바", ch, &format!("보이지 않는 글자가 든 줄 {hidden}개"), declared);
        }
        for (set, src) in [(&mut hosts_added, &lines.added), (&mut hosts_removed, &lines.removed)] {
            for l in src {
                for c in host_re.captures_iter(l) {
                    if set.len() < 500 {
                        set.insert(c[1].trim_end_matches('.').to_ascii_lowercase());
                    }
                }
            }
        }
    }

    for (i, d) in declares.iter().enumerate() {
        if used[i] == 0 {
            out.unused.push(d.written.clone());
        }
        if d.wide {
            out.wide.push((d.written.clone(), used[i]));
        }
    }
    out.hosts = hosts_added
        .into_iter()
        .filter(|h| !hosts_removed.contains(h))
        .take(100)
        .filter(|h| !repo.has_text(&fork, h))
        .collect();
    if bumps > 0 {
        out.notices.push(format!("판이 바뀐 패키지 {bumps}개"));
    }
    let mut big: Vec<&Change> = changes.iter().filter(|c| c.path != DECLARE_PATH && !deps::is_lock(&c.path)).collect();
    big.sort_by_key(|c| std::cmp::Reverse(c.added + c.removed));
    out.biggest = big.iter().take(5).map(|c| (c.path.clone(), c.added, c.removed)).collect();
    Ok(out)
}

fn kind_word(ch: &Change) -> &'static str {
    match ch.kind {
        Kind::Added => "새로 생김",
        Kind::Modified => "바뀜",
        Kind::Deleted => "지워짐",
        Kind::Other => "종류가 바뀜",
    }
}

fn push(out: &mut Outcome, class: &'static str, ch: &Change, what: &str, declared: bool) {
    if out.findings.len() < 5000 {
        out.findings.push(Finding { class, path: ch.path.clone(), what: what.to_string(), added: ch.added, removed: ch.removed, declared });
    }
}

fn declared_as(declares: &[Declare], used: &mut [usize], ch: &Change, out: &mut Outcome) -> bool {
    let want = match ch.kind {
        Kind::Added => What::Make,
        Kind::Deleted => What::Remove,
        _ => What::Edit,
    };
    let mut covered = false;
    let mut exact = false;
    for (i, d) in declares.iter().enumerate() {
        if glob::matches(&d.pattern, &ch.path) {
            covered = true;
            if d.what == want {
                used[i] += 1;
                exact = true;
            }
        }
    }
    if exact {
        out.kept_count += 1;
    } else if covered {
        out.mismatch.push(format!("{} — 실제로는 {}", ch.path, kind_word(ch)));
    } else {
        out.missing.push(ch.path.clone());
    }
    exact
}

fn manifest_check(repo: &Repo, fork: &str, head: &str, ch: &Change, declared: bool, out: &mut Outcome) {
    if ch.kind == Kind::Deleted {
        return;
    }
    let Some(now) = repo.file(head, &ch.path).and_then(|t| deps::manifest(&ch.path, &t)) else {
        push(out, "가", ch, "설정 파일을 읽지 못함", declared);
        return;
    };
    let before = repo.file(fork, &ch.path).and_then(|t| deps::manifest(&ch.path, &t)).unwrap_or_default();
    let keys: Vec<String> = now
        .run
        .iter()
        .filter(|(k, v)| before.run.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .chain(before.run.keys().filter(|k| !now.run.contains_key(*k)).cloned())
        .collect();
    if !keys.is_empty() {
        let what = format!("빌드 때 실행되는 설정이 바뀜: {}", crate::report::clean(&keys.join(", ")));
        push(out, "가", ch, &what, declared);
    }
    let fresh: Vec<&String> = now.sources.difference(&before.sources).collect();
    if !fresh.is_empty() {
        let list: Vec<String> = fresh.iter().take(5).map(|s| crate::report::clean(s)).collect();
        push(out, "다", ch, &format!("등록소가 아닌 곳에서 받는 의존성: {}", list.join("; ")), declared);
    }
    if now.patch != before.patch {
        push(out, "다", ch, "의존성 바꿔치기 설정(patch, overrides)이 바뀜", declared);
    }
    let feats = now.features.iter().filter(|(k, v)| before.features.get(*k) != Some(*v)).count();
    if feats > 0 {
        out.notices.push(format!("켠 기능이 바뀐 의존성 {feats}개: {}", crate::report::tick(&ch.path)));
    }
}

/// 새 패키지 이름은 사람 확인으로 올리고, 판만 바뀐 개수를 돌려준다.
fn lock_check(repo: &Repo, fork: &str, head: &str, ch: &Change, declared: bool, out: &mut Outcome) -> usize {
    if ch.kind == Kind::Deleted {
        return 0;
    }
    let now = repo.file(head, &ch.path).map(|t| deps::lock_packages(&ch.path, &t)).unwrap_or_default();
    let before = repo.file(fork, &ch.path).map(|t| deps::lock_packages(&ch.path, &t)).unwrap_or_default();
    let fresh: Vec<&String> = now.keys().filter(|k| !before.contains_key(*k)).collect();
    if !fresh.is_empty() {
        let shown: Vec<String> = fresh.iter().take(8).map(|s| crate::report::clean(s)).collect();
        let more = if fresh.len() > shown.len() { format!(" 외 {}개", fresh.len() - shown.len()) } else { String::new() };
        push(out, "다", ch, &format!("새 패키지 {}개: {}{more}", fresh.len(), shown.join(", ")), declared);
    }
    now.iter().filter(|(k, v)| before.get(*k).is_some_and(|b| b != *v)).count()
}
