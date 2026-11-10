//! Windows 본체를 조회하는 내장 도구.
//!
//! WSL 안의 ss·lsof는 WSL 내부 프로세스만 본다. Windows 프로그램이 쓰는 포트는
//! PowerShell의 Get-NetTCPConnection으로 조회한다(DESIGN.md 10.2).
//!
//! 스크립트는 코드에 고정되어 있다. 사용자 입력은 숫자로 바꾼 뒤에만 스크립트에 들어간다.

/// 도구 이름과 검증된 인자를 받아 powershell.exe에 넘길 인자 배열을 만든다.
pub fn powershell_args(tool: &str, argv: &[String]) -> Result<Vec<String>, String> {
    let script = match tool {
        "winports" => {
            let mut values = argv.iter().filter(|a| *a != "--");
            let range = values.next().ok_or("포트 번호가 없습니다")?;
            let (lo, hi) = range.split_once('-').unwrap_or((range, range));
            let parse = |v: &str| v.parse::<u16>().ok().filter(|p| *p >= 1).ok_or("포트 번호가 올바르지 않습니다");
            let (lo, hi) = (parse(lo)?, parse(hi)?);
            if lo > hi {
                return Err("포트 범위의 시작이 끝보다 큽니다".into());
            }
            let proto = match values.next().map(String::as_str) {
                None | Some("all") => "all",
                Some("tcp") => "tcp",
                Some("udp") => "udp",
                Some(_) => return Err("프로토콜은 all, tcp, udp 중 하나여야 합니다".into()),
            };
            if values.next().is_some() {
                return Err("인자가 너무 많습니다".into());
            }
            winports_script(lo, hi, proto)
        }
        _ => return Err(format!("Windows에서 실행할 수 없는 도구입니다: {tool}")),
    };
    Ok(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"].iter().map(|s| s.to_string()).chain([script]).collect())
}

/// `proto`는 "all", "tcp", "udp" 중 하나(호출하는 쪽에서 고정 문자열로만 넘긴다).
fn winports_script(lo: u16, hi: u16, proto: &'static str) -> String {
    let label = if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") };
    format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$lo = {lo}
$hi = {hi}
$proto = '{proto}'
$rows = @()
if ($proto -ne 'udp') {{
  Get-NetTCPConnection | Where-Object {{ $_.LocalPort -ge $lo -and $_.LocalPort -le $hi }} | ForEach-Object {{
    # 상태 값의 이름은 Windows 판마다 다르게 나올 수 있어, 상대 주소가 있는지로 구분한다.
    $st = if ($_.RemotePort -eq 0) {{ '수신 대기' }} else {{ "연결됨 $($_.RemoteAddress):$($_.RemotePort)" }}
    $rows += [pscustomobject]@{{ Proto = 'TCP'; Port = [int]$_.LocalPort; Local = "$($_.LocalAddress):$($_.LocalPort)"; State = $st; ProcId = $_.OwningProcess }}
  }}
}}
if ($proto -ne 'tcp') {{
  Get-NetUDPEndpoint | Where-Object {{ $_.LocalPort -ge $lo -and $_.LocalPort -le $hi }} | ForEach-Object {{
    $rows += [pscustomobject]@{{ Proto = 'UDP'; Port = [int]$_.LocalPort; Local = "$($_.LocalAddress):$($_.LocalPort)"; State = '-'; ProcId = $_.OwningProcess }}
  }}
}}
if ($rows.Count -eq 0) {{
  "Windows에서 포트 {label} 을(를) 쓰는 프로그램이 없습니다."
  exit 0
}}
'{{0,-5}} {{1,-28}} {{2,-8}} {{3,-22}} {{4}}' -f 'PROTO', 'LOCAL', 'PID', 'PROCESS', 'STATE'
$rows | Sort-Object Proto, Port, Local, ProcId, State -Unique | Select-Object -First 300 | ForEach-Object {{
  $p = Get-Process -Id $_.ProcId
  $name = if ($p) {{ $p.ProcessName }} else {{ '(알 수 없음)' }}
  '{{0,-5}} {{1,-28}} {{2,-8}} {{3,-22}} {{4}}' -f $_.Proto, $_.Local, $_.ProcId, $name, $_.State
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn winports_script_contains_only_the_number() {
        let a = powershell_args("winports", &args(&["8080"])).unwrap();
        assert_eq!(a[..4], ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]);
        assert!(a[4].contains("$lo = 8080\n$hi = 8080\n$proto = 'all'\n"));
        let a = powershell_args("winports", &args(&["21-30", "tcp"])).unwrap();
        assert!(a[4].contains("$lo = 21\n$hi = 30\n$proto = 'tcp'\n"));
    }

    #[test]
    fn rejects_anything_that_is_not_a_port() {
        for bad in ["8080; calc", "0", "99999", "$(calc)", "", "80 81", "30-21", "1-2-3", "1-99999"] {
            assert!(powershell_args("winports", &args(&[bad])).is_err(), "{bad}");
        }
        assert!(powershell_args("winports", &args(&["80", "tcp'; calc; '"])).is_err());
        assert!(powershell_args("winports", &args(&["80", "tcp", "extra"])).is_err());
        assert!(powershell_args("winports", &[]).is_err());
        assert!(powershell_args("ping", &args(&["8080"])).is_err());
    }
}
