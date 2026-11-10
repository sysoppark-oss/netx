//! 예상 소요 시간. 화면의 진행 표시줄에 쓴다. 인자에서 알 수 있을 때만 값을 준다.

fn value_of(argv: &[String], flags: &[&str]) -> Option<f64> {
    let i = argv.iter().position(|a| flags.contains(&a.as_str()))?;
    argv.get(i + 1)?.parse().ok()
}

pub fn estimate_secs(tool: &str, argv: &[String]) -> Option<u32> {
    let secs = match tool {
        "ping" => {
            let count = value_of(argv, &["-c"])?;
            let interval = value_of(argv, &["-i"]).unwrap_or(1.0);
            (count - 1.0).max(0.0) * interval + 1.0
        }
        // 측정마다 1초, 이름 조회와 마무리에 몇 초
        "mtr" => value_of(argv, &["-c", "--report-cycles"])? * value_of(argv, &["-i", "--interval"]).unwrap_or(1.0) + 6.0,
        "iperf3" => value_of(argv, &["-t", "--time"]).unwrap_or(10.0) + 2.0,
        "nc" => value_of(argv, &["-w"])?,
        "curl" => value_of(argv, &["-m", "--max-time"])?,
        "winports" => 4.0,
        "portscan" => {
            let v: Vec<&String> = argv.iter().filter(|a| *a != "--").collect();
            let hosts = v.first().and_then(|t| crate::engine::builtin::cidr_hosts(t)).map(|h| h.len()).unwrap_or(1) as f64;
            let ports = v.get(1).and_then(|p| crate::engine::builtin::parse_ports(p).ok()).map(|p| p.len()).unwrap_or(1) as f64;
            // 닫힌 포트는 제한 시간(0.8초)까지 기다리고, 256개씩 동시에 시도한다.
            (hosts * ports / 256.0).ceil() * 0.8 + 1.0
        }
        "dig" | "ss" | "lsof" => 2.0,
        _ => return None,
    };
    Some(secs.ceil().clamp(1.0, 3600.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn est(tool: &str, args: &[&str]) -> Option<u32> {
        estimate_secs(tool, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn estimates_from_arguments() {
        assert_eq!(est("ping", &["-c", "5", "--", "8.8.8.8"]), Some(5));
        assert_eq!(est("ping", &["8.8.8.8"]), None);
        assert_eq!(est("mtr", &["-r", "-c", "10", "--", "8.8.8.8"]), Some(16));
        assert_eq!(est("iperf3", &["-c", "10.0.0.1", "-t", "30"]), Some(32));
        assert_eq!(est("iperf3", &["-c", "10.0.0.1"]), Some(12));
        assert_eq!(est("tcpdump", &["-c", "100"]), None);
    }
}
