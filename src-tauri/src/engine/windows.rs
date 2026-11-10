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
            let port: u16 = argv
                .iter()
                .find(|a| *a != "--")
                .and_then(|a| a.parse().ok())
                .filter(|p| *p >= 1)
                .ok_or("포트 번호가 올바르지 않습니다")?;
            winports_script(port)
        }
        _ => return Err(format!("Windows에서 실행할 수 없는 도구입니다: {tool}")),
    };
    Ok(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"].iter().map(|s| s.to_string()).chain([script]).collect())
}

fn winports_script(port: u16) -> String {
    format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$port = {port}
$rows = @()
$names = @{{ '1' = 'Closed'; '2' = 'Listen'; '3' = 'SynSent'; '4' = 'SynReceived'; '5' = 'Established'; '6' = 'FinWait1'; '7' = 'FinWait2'; '8' = 'CloseWait'; '9' = 'Closing'; '10' = 'LastAck'; '11' = 'TimeWait'; '12' = 'DeleteTCB' }}
Get-NetTCPConnection -LocalPort $port | ForEach-Object {{
  $st = "$($_.State)"
  if (-not $st) {{ $st = "$($_.CimInstanceProperties['State'].Value)" }}
  if ($names.ContainsKey($st)) {{ $st = $names[$st] }}
  $rows += [pscustomobject]@{{ Proto = 'TCP'; Local = "$($_.LocalAddress):$($_.LocalPort)"; State = $st; ProcId = $_.OwningProcess }}
}}
Get-NetUDPEndpoint -LocalPort $port | ForEach-Object {{
  $rows += [pscustomobject]@{{ Proto = 'UDP'; Local = "$($_.LocalAddress):$($_.LocalPort)"; State = '-'; ProcId = $_.OwningProcess }}
}}
if ($rows.Count -eq 0) {{
  "Windows에서 포트 $port 을(를) 쓰는 프로그램이 없습니다."
  exit 0
}}
'{{0,-5}} {{1,-28}} {{2,-12}} {{3,-8}} {{4}}' -f 'PROTO', 'LOCAL', 'STATE', 'PID', 'PROCESS'
$rows | Sort-Object Proto, Local, ProcId -Unique | ForEach-Object {{
  $p = Get-Process -Id $_.ProcId
  $name = if ($p) {{ $p.ProcessName }} else {{ '(알 수 없음)' }}
  '{{0,-5}} {{1,-28}} {{2,-12}} {{3,-8}} {{4}}' -f $_.Proto, $_.Local, $_.State, $_.ProcId, $name
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
        assert!(a[4].contains("$port = 8080\n"));
        assert!(a[4].contains("Get-NetTCPConnection -LocalPort $port"));
    }

    #[test]
    fn rejects_anything_that_is_not_a_port() {
        for bad in ["8080; calc", "0", "99999", "$(calc)", "", "80 81"] {
            assert!(powershell_args("winports", &args(&[bad])).is_err(), "{bad}");
        }
        assert!(powershell_args("winports", &[]).is_err());
        assert!(powershell_args("ping", &args(&["8080"])).is_err());
    }
}
