//! 경로 무늬 맞추기. `**`는 폴더 여러 층, `*`는 한 층 안의 글자들, `?`는 한 글자.

pub fn matches(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    if pat.len() > 256 || segs.len() > 4096 {
        return false;
    }
    // 표를 채우는 방식. 무늬 조각 수 × 경로 조각 수 안에 끝난다.
    // ok[i][j] = 무늬의 i번째부터와 경로의 j번째부터가 맞는가
    let (n, m) = (pat.len(), segs.len());
    let mut ok = vec![vec![false; m + 1]; n + 1];
    ok[n][m] = true;
    for i in (0..n).rev() {
        for j in (0..=m).rev() {
            ok[i][j] = if pat[i] == "**" {
                ok[i + 1][j] || (j < m && ok[i][j + 1])
            } else {
                j < m && segment(pat[i], segs[j]) && ok[i + 1][j + 1]
            };
        }
    }
    ok[0][0]
}

pub fn any(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|p| matches(p, path))
}

fn segment(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // 별표 되돌아가기 방식. 입력 길이에 대해 곱셈 시간 안에 끝난다.
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn basic() {
        assert!(matches("src/**", "src/a/b.rs"));
        assert!(matches("src/**", "src/a.rs"));
        assert!(!matches("src/**", "srcx/a.rs"));
        assert!(matches("**/build.rs", "build.rs"));
        assert!(matches("**/build.rs", "src-tauri/build.rs"));
        assert!(!matches("**/build.rs", "src-tauri/xbuild.rs"));
        assert!(matches("**/*.nsh", "a/b/hooks.nsh"));
        assert!(matches("src/*.ts", "src/a.ts"));
        assert!(!matches("src/*.ts", "src/x/a.ts"));
        assert!(matches("a/?.md", "a/b.md"));
        assert!(!matches("README.md", "docs/README.md"));
        assert!(matches("a/**/z", "a/z"));
        assert!(matches("a/**/z", "a/b/c/z"));
    }

    #[test]
    fn odd_input_does_not_hang() {
        let p = "*a*a*a*a*a*a*a*a*a*b";
        let t = "a".repeat(2000);
        assert!(!matches(p, &t));
        let deep = "**/".repeat(40) + "x";
        assert!(!matches(&deep, &"a/".repeat(60)));
    }
}
