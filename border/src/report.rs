//! 보고서. AI가 쓴 글은 싣지 않는다. 경로와 이름은 밖에서 온 글자이므로 씻어서 낸다.

use crate::check::Outcome;
use std::fmt::Write;

/// 제어 문자와 글자 방향을 바꾸는 문자를 지우고 길이를 자른다.
pub fn clean(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            let bad = c.is_control()
                || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}' | '\u{061C}');
            if bad { '?' } else { c }
        })
        .take(200)
        .collect();
    if s.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// 마크다운 코드 글자로 감싼다.
pub fn tick(s: &str) -> String {
    format!("`{}`", clean(s).replace('`', "'"))
}

const SHOW: usize = 40;

fn list<T>(out: &mut String, title: &str, items: &[T], line: impl Fn(&T) -> String) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## {title} ({})", items.len());
    for it in items.iter().take(SHOW) {
        let _ = writeln!(out, "- {}", line(it));
    }
    if items.len() > SHOW {
        let _ = writeln!(out, "- … 외 {}건", items.len() - SHOW);
    }
}

pub fn verdict(code: i32) -> &'static str {
    match code {
        0 => "통과",
        1 => "사람 확인 필요",
        2 => "비자 밖",
        _ => "검사하지 못함",
    }
}

pub fn render(o: &Outcome) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# 출국 심사 — {} (기준 {}, 커밋 {}개, 바뀐 파일 {}개)", clean(&o.head), clean(&o.base), o.commits, o.files);
    let _ = writeln!(s, "\n판정: **{}**", verdict(o.code()));
    if o.visa_local {
        let _ = writeln!(s, "\n> 비자를 기준점이 아니라 로컬 파일에서 읽었습니다. 시험용이며 권위가 없습니다.");
    }
    if o.visa_draft {
        let _ = writeln!(s, "\n> 이 비자는 초안입니다. 사람이 확정하지 않았습니다.");
    }
    list(&mut s, "▲ 사람이 볼 것", &o.findings, |f| {
        format!("[{}] {} — {} (+{} −{}) · 신고: {}", f.class, tick(&f.path), f.what, f.added, f.removed, if f.declared { "있음" } else { "없음" })
    });
    list(&mut s, "비자 밖", &o.visa_out, |p| tick(p));
    list(&mut s, "신고 누락", &o.missing, |p| tick(p));
    list(&mut s, "신고와 다르게 한 것", &o.mismatch, |p| clean(p));
    list(&mut s, "새로 나타난 접속 주소", &o.hosts, |h| tick(h));
    list(&mut s, "넓은 신고", &o.wide, |(w, n)| format!("{} — 이 안에서 바뀐 파일 {n}개", tick(w)));
    list(&mut s, "신고했지만 하지 않은 것", &o.unused, |p| tick(p));
    list(&mut s, "알림", &o.notices, |n| n.clone());
    list(&mut s, "변경이 큰 파일", &o.biggest, |(p, a, r)| format!("{} (+{a} −{r})", tick(p)));
    let _ = writeln!(
        s,
        "\n신고 {}건 · 그대로 한 것 {}건 · 신고 누락 {}건 · 신고와 다름 {}건 · 비자 밖 {}건",
        o.declared_count,
        o.kept_count,
        o.missing.len(),
        o.mismatch.len(),
        o.visa_out.len()
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans() {
        assert_eq!(clean("a\u{202E}b\nc"), "a?b?c");
        assert_eq!(tick("a`b"), "`a'b`");
        assert!(clean(&"x".repeat(500)).chars().count() <= 201);
    }
}
