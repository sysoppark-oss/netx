//! 온라인 AI로 보내기 전에 내부(사설) IP 주소를 가리고, 돌아온 글에서는 원래 주소로 되돌린다.

use std::net::Ipv4Addr;

pub struct Mask {
    /// (원래 주소, 가린 이름)
    pairs: Vec<(String, String)>,
}

impl Mask {
    /// 글에서 사설·링크 로컬 IPv4 주소를 찾아 가릴 이름을 정한다.
    pub fn build(text: &str) -> Mask {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for token in text.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
            let token = token.trim_matches('.');
            if let Ok(ip) = token.parse::<Ipv4Addr>()
                && (ip.is_private() || ip.is_link_local())
                && !pairs.iter().any(|(orig, _)| orig == token)
            {
                pairs.push((token.to_string(), format!("[내부IP-{}]", pairs.len() + 1)));
            }
        }
        // 긴 주소부터 바꿔야 10.0.0.1이 10.0.0.11의 앞부분을 망가뜨리지 않는다.
        pairs.sort_by_key(|(orig, _)| std::cmp::Reverse(orig.len()));
        Mask { pairs }
    }

    pub fn apply(&self, text: &str) -> String {
        self.pairs.iter().fold(text.to_string(), |acc, (orig, name)| acc.replace(orig, name))
    }

    pub fn restore(&self, text: &str) -> String {
        self.pairs.iter().fold(text.to_string(), |acc, (orig, name)| acc.replace(name, orig))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_private_addresses_and_restores_them() {
        let text = "1.|-- 192.168.0.1 0.0%\n2.|-- 10.0.0.11 0.0%\n3.|-- 10.0.0.1 5.0%\n4.|-- 8.8.8.8 0.0% (192.168.0.1)";
        let mask = Mask::build(text);
        let masked = mask.apply(text);
        assert!(!masked.contains("192.168.0.1"));
        assert!(!masked.contains("10.0.0.1"));
        assert!(masked.contains("8.8.8.8"), "공인 주소는 그대로 둔다");
        assert_eq!(masked.matches("[내부IP-1]").count(), 2, "같은 주소는 같은 이름");
        assert_eq!(mask.restore(&masked), text);
        assert_eq!(mask.restore("[내부IP-1]에서 손실"), "192.168.0.1에서 손실");
    }

    #[test]
    fn text_without_private_addresses_is_unchanged() {
        let text = "PING 8.8.8.8: 64 bytes, version 1.2.3";
        let mask = Mask::build(text);
        assert_eq!(mask.apply(text), text);
    }
}
