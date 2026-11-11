//! 기본 진단 도구의 인자 읽기. 아는 옵션만 받고, 모르는 옵션은 "전체 진단 필요"로 거절한다.

use std::collections::HashMap;

use super::unsupported;

#[derive(Debug)]
pub struct Args {
    values: HashMap<&'static str, String>,
    flags: Vec<&'static str>,
    pub rest: Vec<String>,
}

impl Args {
    /// `valued`는 값을 받는 옵션, `flags`는 값이 없는 옵션. `-vz`처럼 붙여 쓴 한 글자 옵션도 받는다.
    pub fn parse(argv: &[String], valued: &[&'static str], flags: &[&'static str]) -> Result<Args, String> {
        let mut out = Args { values: HashMap::new(), flags: Vec::new(), rest: Vec::new() };
        let mut it = argv.iter();
        let mut options = true;
        while let Some(arg) = it.next() {
            if !options || !arg.starts_with('-') || arg == "-" {
                out.rest.push(arg.clone());
            } else if arg == "--" {
                options = false;
            } else if let Some(name) = valued.iter().find(|v| *v == arg) {
                let value = it.next().ok_or_else(|| format!("{name} 옵션의 값이 없습니다"))?;
                out.values.insert(name, value.clone());
            } else if let Some(name) = flags.iter().find(|f| *f == arg) {
                out.flags.push(name);
            } else if !arg.starts_with("--") && arg.len() > 2 && arg[1..].chars().all(|c| flags.iter().any(|f| f.len() == 2 && f.ends_with(c))) {
                for c in arg[1..].chars() {
                    out.flags.extend(flags.iter().find(|f| f.len() == 2 && f.ends_with(c)));
                }
            } else {
                return Err(unsupported(&format!("`{arg}` 옵션")));
            }
        }
        Ok(out)
    }

    pub fn has(&self, flag: &str) -> bool {
        self.flags.contains(&flag)
    }

    /// 숫자 옵션. 없으면 기본값, 범위를 벗어나면 오류.
    pub fn number(&self, flag: &str, default: f64, min: f64, max: f64) -> Result<f64, String> {
        match self.values.get(flag) {
            None => Ok(default),
            Some(v) => v.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= min && *n <= max).ok_or_else(|| format!("{flag} 옵션의 값이 올바르지 않습니다 ({min}~{max})")),
        }
    }

    pub fn value(&self, flag: &str) -> Option<&str> {
        self.values.get(flag).map(String::as_str)
    }

    /// 위치 인자가 정확히 하나
    pub fn one(&self, what: &str) -> Result<&str, String> {
        match &self.rest[..] {
            [one] => Ok(one),
            [] => Err(format!("{what}이(가) 없습니다")),
            _ => Err(format!("{what}은(는) 하나만 적습니다")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn reads_known_options_only() {
        let a = Args::parse(&s(&["-vz", "-w", "3", "--", "host", "443"]), &["-w"], &["-v", "-z", "-4"]).unwrap();
        assert!(a.has("-v") && a.has("-z") && !a.has("-4"));
        assert_eq!(a.number("-w", 5.0, 1.0, 60.0).unwrap(), 3.0);
        assert_eq!(a.rest, ["host", "443"]);
        assert!(a.one("대상").is_err());
        assert!(Args::parse(&s(&["-u", "host"]), &["-w"], &["-v"]).unwrap_err().contains("`-u`"));
        assert!(Args::parse(&s(&["-vu", "host"]), &["-w"], &["-v"]).is_err());
        assert!(Args::parse(&s(&["-w"]), &["-w"], &[]).is_err());
        let a = Args::parse(&s(&["-w", "999", "h"]), &["-w"], &[]).unwrap();
        assert!(a.number("-w", 5.0, 1.0, 60.0).is_err());
        assert_eq!(a.one("대상").unwrap(), "h");
        // `--` 뒤는 옵션이 아니다
        assert_eq!(Args::parse(&s(&["--", "-x"]), &[], &[]).unwrap().rest, ["-x"]);
    }
}
