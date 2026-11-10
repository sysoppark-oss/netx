//! 값 검사. 옵션 값과 위치 인자를 종류별 규칙으로 확인한다.
//!
//! 공통 규칙: 비어 있지 않을 것, 제어 문자 없음, `-`로 시작하지 않을 것(옵션 인젝션 차단).

use std::net::{IpAddr, Ipv4Addr};

use crate::model::{OptionSpec, PositionalSpec, RecipeInput, Risk, ValueType};

const MAX_ARG_LEN: usize = 1024;

pub struct Rule<'a> {
    pub kind: ValueType,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub values: &'a [String],
    pub allow_values: &'a [String],
    pub deny_prefix: &'a [String],
}

impl OptionSpec {
    pub fn rule(&self) -> Rule<'_> {
        Rule {
            kind: self.kind,
            min: self.min,
            max: self.max,
            values: &self.values,
            allow_values: &self.allow_values,
            deny_prefix: &self.deny_prefix,
        }
    }
}

impl PositionalSpec {
    pub fn rule(&self) -> Rule<'_> {
        Rule { kind: self.kind, min: self.min, max: self.max, values: &self.values, allow_values: &[], deny_prefix: &[] }
    }
}

impl RecipeInput {
    pub fn rule(&self) -> Rule<'_> {
        Rule { kind: self.kind, min: self.min, max: self.max, values: &[], allow_values: &[], deny_prefix: &[] }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Checked {
    /// 값 때문에 위험도가 올라가는 경우(넓은 포트 범위, 공인 대역 등)
    pub bump: Option<(Risk, String)>,
    /// 허용 목록에 있는 값(옵션 위험도를 적용하지 않음)
    pub allowlisted: bool,
}

/// 모든 인자에 적용하는 기본 검사.
pub fn check_common(v: &str) -> Result<(), String> {
    if v.is_empty() {
        return Err("빈 값입니다".into());
    }
    if v.len() > MAX_ARG_LEN {
        return Err("값이 너무 깁니다".into());
    }
    if v.chars().any(|c| c.is_control()) {
        return Err("제어 문자가 들어 있습니다".into());
    }
    Ok(())
}

pub fn check(rule: &Rule, v: &str) -> Result<Checked, String> {
    check_common(v)?;
    if rule.allow_values.iter().any(|a| a == v) {
        return Ok(Checked { bump: None, allowlisted: true });
    }
    if v.starts_with('-') {
        return Err("값이 '-'로 시작할 수 없습니다".into());
    }
    if let Some(p) = rule.deny_prefix.iter().find(|p| v.starts_with(p.as_str())) {
        return Err(format!("'{p}'로 시작하는 값은 쓸 수 없습니다(파일 읽기 방지)"));
    }

    let ok = Ok(Checked::default());
    match rule.kind {
        ValueType::Boolean => Err("값을 받지 않는 옵션입니다".into()),
        ValueType::Number => {
            let n: i64 = v.parse().map_err(|_| "정수가 아닙니다".to_string())?;
            in_range(n as f64, rule)?;
            ok
        }
        ValueType::Float => {
            let n: f64 = v.parse().map_err(|_| "숫자가 아닙니다".to_string())?;
            if !n.is_finite() {
                return Err("숫자가 아닙니다".into());
            }
            in_range(n, rule)?;
            ok
        }
        ValueType::String => {
            if v.len() > 256 {
                return Err("값이 너무 깁니다".into());
            }
            ok
        }
        ValueType::Enum => {
            if rule.values.iter().any(|x| x == v) {
                ok
            } else {
                Err(format!("허용된 값이 아닙니다 ({})", rule.values.join(", ")))
            }
        }
        ValueType::Host => check_host(v).map(|_| Checked::default()),
        ValueType::Url => check_url(v).map(|_| Checked::default()),
        ValueType::Port => parse_port(v).map(|_| Checked::default()),
        ValueType::PortOrRange => check_port_range(v),
        ValueType::Cidr => check_cidr(v, rule),
        ValueType::Target => {
            if v.contains('/') {
                check_cidr(v, rule)
            } else {
                check_host(v).map(|_| Checked::default())
            }
        }
        ValueType::Path => check_filename(v).map(|_| Checked::default()),
        ValueType::Iface => chars_ok(v, 32, |c| c.is_ascii_alphanumeric() || "._-@:".contains(c), "인터페이스 이름")
            .map(|_| Checked::default()),
        ValueType::Expr => chars_ok(
            v,
            512,
            |c| c.is_ascii_alphanumeric() || " .:/-()!=<>&|[]_,*".contains(c),
            "필터식",
        )
        .map(|_| Checked::default()),
        ValueType::Format => {
            if v.len() > 512 {
                return Err("형식 문자열이 너무 깁니다".into());
            }
            ok
        }
        ValueType::Rate => {
            if !v.starts_with(|c: char| c.is_ascii_digit()) {
                return Err("숫자로 시작해야 합니다 (예: 100M)".into());
            }
            chars_ok(v, 16, |c| c.is_ascii_digit() || "./kKmMgGtT".contains(c), "크기·속도").map(|_| Checked::default())
        }
        ValueType::DigArg => check_host(v.strip_prefix('@').unwrap_or(v)).map(|_| Checked::default()),
        ValueType::LsofInet => {
            chars_ok(v, 128, |c| c.is_ascii_alphanumeric() || "@:.,[]-".contains(c), "주소 지정").map(|_| Checked::default())
        }
    }
}

fn in_range(n: f64, rule: &Rule) -> Result<(), String> {
    if let Some(min) = rule.min {
        if n < min {
            return Err(format!("{min} 이상이어야 합니다"));
        }
    }
    if let Some(max) = rule.max {
        if n > max {
            return Err(format!("{max} 이하여야 합니다"));
        }
    }
    Ok(())
}

fn chars_ok(v: &str, max_len: usize, allowed: impl Fn(char) -> bool, what: &str) -> Result<(), String> {
    if v.len() > max_len {
        return Err(format!("{what}이(가) 너무 깁니다"));
    }
    match v.chars().find(|c| !allowed(*c)) {
        Some(c) => Err(format!("{what}에 쓸 수 없는 문자입니다: '{c}'")),
        None => Ok(()),
    }
}

fn check_host(v: &str) -> Result<(), String> {
    if v.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if v.len() > 253 {
        return Err("호스트 이름이 너무 깁니다".into());
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if v.trim_end_matches('.').split('.').all(label_ok) {
        Ok(())
    } else {
        Err("호스트 이름 또는 IP 주소 형식이 아닙니다".into())
    }
}

fn check_url(v: &str) -> Result<(), String> {
    if v.chars().any(char::is_whitespace) {
        return Err("URL에 공백이 있습니다".into());
    }
    let url = url::Url::parse(v).map_err(|_| "URL 형식이 아닙니다 (예: https://example.com)".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("http 또는 https 주소만 쓸 수 있습니다".into());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("URL에 호스트가 없습니다".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL에 계정 정보를 넣을 수 없습니다".into());
    }
    Ok(())
}

fn parse_port(v: &str) -> Result<u16, String> {
    match v.parse::<u16>() {
        Ok(p) if p >= 1 => Ok(p),
        _ => Err("포트는 1~65535 사이여야 합니다".into()),
    }
}

fn check_port_range(v: &str) -> Result<Checked, String> {
    let Some((a, b)) = v.split_once('-') else {
        return parse_port(v).map(|_| Checked::default());
    };
    let (a, b) = (parse_port(a)?, parse_port(b)?);
    if a > b {
        return Err("포트 범위의 시작이 끝보다 큽니다".into());
    }
    let count = u32::from(b - a) + 1;
    let bump = (count > 100).then(|| (Risk::Caution, format!("포트 {count}개를 한꺼번에 확인합니다(포트 스캔)")));
    Ok(Checked { bump, allowlisted: false })
}

fn check_cidr(v: &str, rule: &Rule) -> Result<Checked, String> {
    let (addr, prefix) = v.split_once('/').ok_or("대역 형식이 아닙니다 (예: 192.168.0.0/24)")?;
    let addr: Ipv4Addr = addr.parse().map_err(|_| "대역의 주소가 IPv4 형식이 아닙니다".to_string())?;
    let prefix: u8 = prefix.parse().map_err(|_| "대역의 프리픽스가 숫자가 아닙니다".to_string())?;
    if prefix > 32 {
        return Err("프리픽스는 32 이하여야 합니다".into());
    }
    let min_prefix = rule.min.unwrap_or(16.0) as u8;
    if prefix < min_prefix {
        return Err(format!("대역이 너무 넓습니다. /{min_prefix} 이상으로 좁혀 주세요"));
    }
    let bump = (!is_internal(addr)).then(|| {
        (Risk::Danger, "사설 대역이 아닙니다. 직접 관리하거나 허가받은 네트워크인지 확인하세요".to_string())
    });
    Ok(Checked { bump, allowlisted: false })
}

fn is_internal(a: Ipv4Addr) -> bool {
    a.is_private() || a.is_loopback() || a.is_link_local()
}

/// 작업 폴더 안에 둘 파일 이름. 경로 구분자와 숨김 파일을 허용하지 않는다.
fn check_filename(v: &str) -> Result<(), String> {
    if v.starts_with('.') || v.contains("..") {
        return Err("파일 이름이 올바르지 않습니다".into());
    }
    chars_ok(v, 100, |c| c.is_ascii_alphanumeric() || "._-".contains(c), "파일 이름")
        .map_err(|_| "파일 이름에는 영문, 숫자, '.', '_', '-'만 쓸 수 있습니다(경로 불가)".to_string())
}

/// URL에서 호스트와 포트를 꺼낸다(레시피 변수용). `check_url`을 통과한 값에만 쓴다.
pub fn url_parts(v: &str) -> Option<(String, u16)> {
    let url = url::Url::parse(v).ok()?;
    let host = url.host_str()?.trim_matches(|c| c == '[' || c == ']').to_string();
    Some((host, url.port_or_known_default()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: ValueType) -> Rule<'static> {
        Rule { kind, min: None, max: None, values: &[], allow_values: &[], deny_prefix: &[] }
    }
    fn ok(kind: ValueType, v: &str) -> bool {
        check(&rule(kind), v).is_ok()
    }

    #[test]
    fn rejects_leading_dash_everywhere() {
        for kind in [ValueType::Host, ValueType::String, ValueType::Url, ValueType::Path, ValueType::Expr, ValueType::Format] {
            assert!(!ok(kind, "-oProxyCommand=evil"), "{kind:?}");
        }
    }

    #[test]
    fn rejects_control_chars_and_empty() {
        assert!(!ok(ValueType::String, "a\nb"));
        assert!(!ok(ValueType::String, "a\0b"));
        assert!(!ok(ValueType::Host, ""));
    }

    #[test]
    fn host_rules() {
        for v in ["example.com", "192.168.0.50", "::1", "2001:db8::1", "my_host-1.local", "example.com."] {
            assert!(ok(ValueType::Host, v), "{v}");
        }
        for v in ["exa mple.com", "a..b", "host;rm", "$(id)", "a/b", "host`id`"] {
            assert!(!ok(ValueType::Host, v), "{v}");
        }
    }

    #[test]
    fn url_rules() {
        assert!(ok(ValueType::Url, "https://example.com/path?q=1"));
        assert!(ok(ValueType::Url, "http://192.168.0.50:8080"));
        for v in ["file:///etc/passwd", "ftp://example.com", "example.com", "https://user:pw@example.com", "https://exa mple.com", "gopher://x"] {
            assert!(!ok(ValueType::Url, v), "{v}");
        }
    }

    #[test]
    fn port_rules() {
        assert!(ok(ValueType::Port, "443"));
        assert!(!ok(ValueType::Port, "0"));
        assert!(!ok(ValueType::Port, "65536"));
        assert_eq!(check(&rule(ValueType::PortOrRange), "80-90").unwrap().bump, None);
        let wide = check(&rule(ValueType::PortOrRange), "1-1024").unwrap();
        assert_eq!(wide.bump.unwrap().0, Risk::Caution);
        assert!(!ok(ValueType::PortOrRange, "90-80"));
    }

    #[test]
    fn cidr_rules() {
        assert_eq!(check(&rule(ValueType::Cidr), "192.168.0.0/24").unwrap().bump, None);
        assert_eq!(check(&rule(ValueType::Cidr), "8.8.8.0/24").unwrap().bump.unwrap().0, Risk::Danger);
        assert!(!ok(ValueType::Cidr, "10.0.0.0/8"), "기본 최소 프리픽스 /16보다 넓음");
        assert!(!ok(ValueType::Cidr, "192.168.0.0/33"));
        assert!(!ok(ValueType::Cidr, "192.168.0.0"));
    }

    #[test]
    fn filename_rules() {
        assert!(ok(ValueType::Path, "capture.pcap"));
        for v in ["/etc/passwd", "../x", "a/b", ".bashrc", "C:\\x", "a b"] {
            assert!(!ok(ValueType::Path, v), "{v}");
        }
    }

    #[test]
    fn allow_values_and_deny_prefix() {
        let allow = ["/dev/null".to_string()];
        let deny = ["@".to_string()];
        let r = Rule { kind: ValueType::Path, min: None, max: None, values: &[], allow_values: &allow, deny_prefix: &[] };
        assert!(check(&r, "/dev/null").unwrap().allowlisted);
        let r = Rule { kind: ValueType::Format, min: None, max: None, values: &[], allow_values: &[], deny_prefix: &deny };
        assert!(check(&r, "@/etc/passwd").is_err());
        assert!(check(&r, "ttfb:%{time_starttransfer}").is_ok());
    }

    #[test]
    fn url_parts_default_ports() {
        assert_eq!(url_parts("https://example.com/a"), Some(("example.com".into(), 443)));
        assert_eq!(url_parts("http://10.0.0.1:8080"), Some(("10.0.0.1".into(), 8080)));
    }
}
