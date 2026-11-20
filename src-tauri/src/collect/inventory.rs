//! 예약 수집: 이 PC의 시스템 상세와 설치된 프로그램 목록을 읽는다.
//! 명령을 실행하지 않는다. 운영체제가 주는 값과 Windows의 "프로그램 제거" 목록(읽기 전용)만 읽는다.
//! 싣지 않는 것: 제품 키, 사용자 계정, 실행 중인 프로세스, 파일 목록, 일련번호, MAC 주소.

use super::wire::{Data, Disk, Finished, Program, Software, Speed, System};

/// 수집 항목: (이름, 화면 이름, 가장 잦은 주기(분), 기본 주기(분))
pub const ITEMS: [(&str, &str, u32, u32); 3] = [("speed", "네트워크 속도", 60, 360), ("system", "시스템 상세", 15, 360), ("software", "설치된 프로그램", 360, 1440)];

/// 재는 데 시간이 걸려 따로 도는 항목(네트워크를 쓴다)
pub fn slow(what: &str) -> bool {
    what == "speed"
}
/// 가장 뜸한 주기(7일)
pub const EVERY_MAX: u32 = 7 * 24 * 60;

pub fn known(what: &str) -> Option<(&'static str, &'static str, u32, u32)> {
    ITEMS.iter().copied().find(|i| i.0 == what)
}

fn gb(bytes: u64) -> String {
    format!("{:.0}GB", bytes as f64 / 1_073_741_824.0)
}

pub fn system(version: &str, host: &str) -> System {
    use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind};
    let sys = sysinfo::System::new_with_specifics(RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()).with_cpu(CpuRefreshKind::nothing()));
    let disks = Disks::new_with_refreshed_list();
    System {
        os: sysinfo::System::long_os_version().or_else(sysinfo::System::name).unwrap_or_default(),
        build: sysinfo::System::kernel_version().unwrap_or_default(),
        host: host.to_string(),
        cpu: sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default(),
        cores: sys.cpus().len() as u32,
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        disks: disks
            .list()
            .iter()
            .filter(|d| !d.is_removable() && d.total_space() > 0)
            .map(|d| Disk { name: d.mount_point().to_string_lossy().trim_end_matches('\\').to_string(), total: d.total_space(), free: d.available_space() })
            .collect(),
        uptime_secs: sysinfo::System::uptime(),
        netx: version.to_string(),
        tailnet: super::net::tailnet_addr().map(|a| a.to_string()).unwrap_or_default(),
    }
}

/// 같은 이름·판은 하나로, 이름순으로.
pub fn arrange(mut found: Vec<Program>) -> Vec<Program> {
    found.retain(|p| !p.name.trim().is_empty());
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.version.cmp(&b.version)));
    found.dedup_by(|a, b| a.name == b.name && a.version == b.version);
    found
}

#[cfg(windows)]
fn installed() -> Vec<Program> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    const PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut found = Vec::new();
    for (root, flags) in [(HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY), (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY), (HKEY_CURRENT_USER, KEY_READ)] {
        let Ok(list) = RegKey::predef(root).open_subkey_with_flags(PATH, flags) else { continue };
        for id in list.enum_keys().flatten().take(4000) {
            let Ok(key) = list.open_subkey_with_flags(&id, KEY_READ) else { continue };
            let text = |name: &str| key.get_value::<String, _>(name).unwrap_or_default();
            let name = text("DisplayName");
            // Windows가 "프로그램 제거" 화면에서 숨기는 것(부품, 업데이트 조각)은 뺀다.
            if name.trim().is_empty() || key.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 || !text("ParentKeyName").is_empty() {
                continue;
            }
            found.push(Program { name, version: text("DisplayVersion"), publisher: text("Publisher"), installed: text("InstallDate") });
        }
    }
    found
}

#[cfg(not(windows))]
fn installed() -> Vec<Program> {
    // 개발용(리눅스): dpkg 목록 파일을 읽는다. 없으면 빈 목록.
    let Ok(text) = std::fs::read_to_string("/var/lib/dpkg/status") else { return Vec::new() };
    text.split("\n\n")
        .filter(|b| b.contains("Status: install ok installed"))
        .map(|b| {
            let field = |k: &str| b.lines().find_map(|l| l.strip_prefix(k)).unwrap_or("").trim().to_string();
            Program { name: field("Package:"), version: field("Version:"), publisher: String::new(), installed: String::new() }
        })
        .collect()
}

pub fn software() -> Software {
    Software { programs: arrange(installed()), cut: false }
}

/// 잰 속도를 수집 한 건으로 만든다.
pub fn speed_item(s: Speed, started: u64, now: u64) -> Finished {
    let line = format!("내려받기 {} Mbps · 올리기 {} Mbps · 지연 {} ms", s.down_mbps, s.up_mbps, s.latency_ms);
    Finished { kind: "inventory.speed".into(), title: "네트워크 속도".into(), target: String::new(), verdict: "unknown".into(), summary: vec![line], started, ended: now, steps: Vec::new(), data: Some(Data::Speed(s)) }
}

/// 켜기 전에 보여 주는 설명(속도는 미리 재지 않는다).
pub fn speed_preview() -> Finished {
    Finished {
        kind: "inventory.speed".into(),
        title: "네트워크 속도".into(),
        summary: vec![
            format!("잴 때마다 {}와 HTTPS로 빈 자료를 15MB쯤(빠른 회선은 50MB쯤) 주고받습니다.", super::speed::SERVER),
            "관제 PC로 나가는 것: 내려받기 속도, 올리기 속도, 지연, 잰 때, 주고받은 양.".into(),
            "측정 서버에는 이 PC의 공인 IP가 보입니다(어느 서버든 접속하면 그렇습니다).".into(),
            "진단이 돌고 있는 동안에는 재지 않고 미룹니다.".into(),
        ],
        ..Finished::default()
    }
}

/// 수집 한 건을 진단 결과와 같은 모양으로 만든다(같은 보낼 함, 같은 길로 간다). 속도는 여기서 재지 않는다(`speed::measure`).
pub fn collect(what: &str, version: &str, host: &str, now: u64) -> Option<Finished> {
    let (name, title, _, _) = known(what).filter(|k| !slow(k.0))?;
    let (data, line) = match name {
        "system" => {
            let s = system(version, host);
            let free: u64 = s.disks.iter().map(|d| d.free).sum();
            let line = format!("{} · 메모리 {} · 디스크 남은 양 {}", s.os, gb(s.mem_total), gb(free));
            (Data::System(s), line)
        }
        _ => {
            let s = software();
            let line = format!("프로그램 {}개", s.programs.len());
            (Data::Software(s), line)
        }
    };
    Some(Finished { kind: format!("inventory.{name}"), title: title.into(), target: String::new(), verdict: "unknown".into(), summary: vec![line], started: now, ended: now, steps: Vec::new(), data: Some(data) })
}

/// 두 프로그램 목록의 차이: (새로 생긴 것, 없어진 것, 판이 바뀐 것(이름, 옛 판, 새 판))
pub fn diff(old: &[Program], new: &[Program]) -> (Vec<Program>, Vec<Program>, Vec<(String, String, String)>) {
    use std::collections::BTreeMap;
    let index = |l: &[Program]| l.iter().map(|p| (p.name.clone(), p.clone())).collect::<BTreeMap<_, _>>();
    let (a, b) = (index(old), index(new));
    let added = b.values().filter(|p| !a.contains_key(&p.name)).cloned().collect();
    let removed = a.values().filter(|p| !b.contains_key(&p.name)).cloned().collect();
    let changed = b.values().filter_map(|p| a.get(&p.name).filter(|o| o.version != p.version).map(|o| (p.name.clone(), o.version.clone(), p.version.clone()))).collect();
    (added, removed, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, version: &str) -> Program {
        Program { name: name.into(), version: version.into(), ..Program::default() }
    }

    #[test]
    fn reads_this_machine_without_running_anything() {
        let s = system("0.1.0", "host");
        assert!(s.mem_total > 0 && s.cores > 0);
        let f = collect("system", "0.1.0", "host", 7).unwrap();
        assert_eq!((f.kind.as_str(), f.steps.len(), f.ended), ("inventory.system", 0, 7));
        assert!(collect("software", "0.1.0", "host", 7).is_some());
        assert!(collect("processes", "0.1.0", "host", 7).is_none(), "정해진 항목만 돈다");
    }

    #[test]
    fn program_lists_are_arranged_and_compared_by_name() {
        let list = arrange(vec![p("b", "1"), p("A", "2"), p("b", "1"), p(" ", "9")]);
        assert_eq!(list.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["A", "b"]);
        let (added, removed, changed) = diff(&[p("A", "1"), p("gone", "1")], &[p("A", "2"), p("new", "1")]);
        assert_eq!((added[0].name.as_str(), removed[0].name.as_str()), ("new", "gone"));
        assert_eq!(changed, vec![("A".to_string(), "1".to_string(), "2".to_string())]);
    }
}
