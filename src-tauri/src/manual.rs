//! 사용자가 직접 적은 명령을 계획 단계로 바꾼다.
//!
//! 적은 글은 셸에 넘기지 않는다. 낱말로 나눠 (도구, 인자 배열)로 만들고 검증기를 통과해야 실행된다.
//! 그래서 파이프, 리다이렉션, 명령 잇기는 쓸 수 없다.

use crate::model::PlanStep;

const MAX_LEN: usize = 1000;

/// 명령 한 줄을 낱말로 나눈다. 따옴표 안의 공백은 낱말을 나누지 않는다.
pub fn tokenize(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("명령을 입력하세요".into());
    }
    if text.len() > MAX_LEN {
        return Err("명령이 너무 깁니다".into());
    }
    if text.chars().any(|c| c.is_control()) {
        return Err("명령은 한 줄로 적으세요".into());
    }

    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    in_token = true;
                }
                c if c.is_whitespace() => {
                    if in_token {
                        tokens.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                }
                '|' | ';' | '&' | '<' | '>' | '`' | '$' => {
                    return Err(format!(
                        "'{c}'는 쓸 수 없습니다. NetX는 명령을 셸에 넘기지 않으므로 파이프, 리다이렉션, 명령 잇기를 지원하지 않습니다. 필터식에 필요하면 따옴표로 감싸세요."
                    ));
                }
                c => {
                    cur.push(c);
                    in_token = true;
                }
            },
        }
    }
    if quote.is_some() {
        return Err("따옴표가 닫히지 않았습니다".into());
    }
    if in_token {
        tokens.push(cur);
    }
    Ok(tokens)
}

/// 명령 한 줄을 계획 단계로 바꾼다. 앞의 sudo는 뗀다(root가 필요한 도구는 NetX가 알아서 root로 실행한다).
pub fn to_step(text: &str) -> Result<PlanStep, String> {
    let mut tokens = tokenize(text)?;
    if tokens.first().map(String::as_str) == Some("sudo") {
        tokens.remove(0);
    }
    if tokens.is_empty() {
        return Err("도구 이름이 없습니다".into());
    }
    let tool = tokens.remove(0);
    Ok(PlanStep {
        order: None,
        expect: generic_expect(&tool).map(str::to_string),
        reason: Some("직접 추가한 명령입니다.".into()),
        tool,
        args: tokens,
    })
}

/// 도구별로 결과에서 무엇을 보면 되는지.
fn generic_expect(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "ping" => "줄마다 응답 시간(time=)이 나오고 마지막에 손실률(packet loss)이 나온다. 손실 0%에 시간이 고르면 정상.",
        "fping" => "응답한 주소는 alive, 응답 없는 주소는 unreachable로 나온다.",
        "mtr" => "구간(홉)별 손실률과 지연이 나온다. 어느 구간부터 끝까지 손실이 이어지면 그 구간이 원인.",
        "dig" => "ANSWER에 주소가 나오면 이름 풀이 정상. 비어 있거나 SERVFAIL이면 DNS 문제.",
        "nc" => "succeeded 또는 open이면 포트 열림, refused면 닫힘, timed out이면 중간에서 차단.",
        "curl" => "HTTP 상태 코드와 응답 내용 또는 단계별 시간이 나온다.",
        "tcpdump" => "조건에 맞는 패킷이 한 줄씩 나온다. 아무것도 안 나오면 조건에 맞는 통신이 없다는 뜻.",
        "ss" => "소켓 목록이 나온다. users 항목에 프로세스 이름과 번호가 보인다.",
        "lsof" => "포트나 파일을 연 프로세스의 이름(COMMAND)과 번호(PID)가 나온다.",
        "iperf3" => "구간마다 Bitrate(전송 속도)가 나오고 마지막 SUM 줄이 평균이다.",
        "winports" => "PROCESS 열이 포트를 쓰는 Windows 프로그램, PID가 프로세스 번호.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Level, Risk, Source};
    use crate::registry::Registry;
    use crate::security::validator::validate_step;

    #[test]
    fn splits_words_and_quotes() {
        assert_eq!(tokenize("ping -c 4  8.8.8.8").unwrap(), ["ping", "-c", "4", "8.8.8.8"]);
        assert_eq!(tokenize("tcpdump -i eth0 'port 443 and host 10.0.0.1'").unwrap(), ["tcpdump", "-i", "eth0", "port 443 and host 10.0.0.1"]);
        assert_eq!(tokenize("curl -w \"%{http_code}\" example.com").unwrap()[2], "%{http_code}");
        assert_eq!(tokenize("a ''").unwrap(), ["a", ""]);
    }

    #[test]
    fn rejects_shell_syntax() {
        for bad in ["ping 8.8.8.8 | grep ttl", "ping 8.8.8.8; id", "ping 8.8.8.8 && id", "ping $(id)", "ping `id`", "ping 8.8.8.8 > out", "ping 'abc", "", "   ", "ping\n8.8.8.8"] {
            assert!(tokenize(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn step_goes_through_the_validator_as_manual() {
        let reg = Registry::global();
        let step = to_step("sudo ping -c 4 8.8.8.8").unwrap();
        assert_eq!((step.tool.as_str(), step.args.len()), ("ping", 3));
        let v = validate_step(reg, &step, 1, Level::Middle, Source::Manual);
        assert!(!v.blocked);
        assert_eq!(v.risk, Risk::Safe);
        assert!(v.explain.iter().any(|e| e.token == "-c 4"));

        // 등록되지 않은 도구, 금지 옵션, 옵션 주입은 직접 입력이어도 차단된다.
        for bad in ["rm -rf /", "bash -c id", "tcpdump -z sh -i eth0", "curl -K /etc/passwd example.com", "ping -c 4 8.8.8.8 -f"] {
            let v = validate_step(reg, &to_step(bad).unwrap(), 1, Level::Op, Source::Manual);
            assert!(v.blocked || v.risk >= Risk::Danger, "{bad}");
        }
        assert!(validate_step(reg, &to_step("rm -rf /").unwrap(), 1, Level::Op, Source::Manual).blocked);
        assert!(validate_step(reg, &to_step("tcpdump -z sh -i eth0").unwrap(), 1, Level::Op, Source::Manual).blocked);
    }
}
