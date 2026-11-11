//! 실행 엔진. 기준: docs/DEV_PLAN_v0.1.md 5장, docs/DESIGN.md 11.3
//!
//! 검증기를 통과한 단계만 여기로 온다. 엔진은 인자를 해석하거나 고치지 않고,
//! 받은 배열 그대로 프로세스에 넘긴다(셸을 거치지 않는다).
//!
//! - Windows: `wsl.exe [--cd 작업폴더] [-u root] --exec timeout ... 도구 인자...`
//! - 그 밖(개발·테스트): 같은 명령을 직접 실행
//!
//! 모든 실행은 리눅스 쪽 `timeout`으로 감싼다. Windows에서 wsl.exe만 끝내면
//! WSL 안의 프로세스가 남을 수 있으므로, 중지는 WSL 안의 `timeout` 프로세스에
//! 신호를 보내는 방식으로 한다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::{Notify, watch};

use crate::env_check::{decode_wsl_output, hide_console_window};

pub mod windows;
pub mod builtin;

/// 한 단계의 출력 상한. 넘으면 실행을 멈춘다.
const MAX_LINES: usize = 5000;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 4000;
/// 중지 신호를 보낸 뒤 강제 종료까지 기다리는 시간
const GRACE: Duration = Duration::from_secs(3);

/// 실행할 프로세스 하나.
#[derive(Debug, Clone)]
pub struct Job {
    pub order: u32,
    pub program: String,
    pub argv: Vec<String>,
    pub root: bool,
    pub timeout_secs: u32,
    pub engine: JobEngine,
}

/// 어디에서 실행할지
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobEngine {
    /// WSL 안의 리눅스 도구 (Windows가 아니면 직접 실행)
    Wsl,
    /// Windows 본체 조회 (PowerShell, 고정된 스크립트)
    Windows,
    /// NetX 내장 기능 (외부 프로그램 없음)
    Builtin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Running,
    /// 프로세스가 스스로 끝남. 종료 코드는 `exit_code`에 있다.
    Done,
    Timeout,
    Cancelled,
    /// 출력 상한을 넘어 중단
    Capped,
    /// 프로세스를 시작하지 못함
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputEvent {
    pub run_id: u64,
    pub order: u32,
    pub stream: &'static str,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepEvent {
    pub run_id: u64,
    pub order: u32,
    pub program: String,
    pub status: StepStatus,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u64,
}

/// 실행 결과를 받는 쪽. 앱에서는 화면으로 이벤트를 보내고, 테스트에서는 모아 둔다.
pub trait Sink: Send + Sync + 'static {
    fn output(&self, ev: OutputEvent);
    fn step(&self, ev: StepEvent);
    fn done(&self, run_id: u64);
}

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);
static NEXT_TAG: AtomicU64 = AtomicU64::new(1);
static RUNS: Mutex<Option<HashMap<u64, watch::Sender<bool>>>> = Mutex::new(None);

fn runs<R>(f: impl FnOnce(&mut HashMap<u64, watch::Sender<bool>>) -> R) -> R {
    let mut guard = RUNS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// 실행을 시작하고 실행 번호를 돌려준다. 결과는 `sink`로 흘러간다.
pub fn start(jobs: Vec<Job>, parallel: bool, work_dir: PathBuf, sink: Arc<dyn Sink>) -> u64 {
    let run_id = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = watch::channel(false);
    runs(|m| m.insert(run_id, tx));

    tauri::async_runtime::spawn(async move {
        if parallel {
            let handles: Vec<_> = jobs
                .into_iter()
                .map(|job| {
                    let (work_dir, sink, rx) = (work_dir.clone(), sink.clone(), rx.clone());
                    tauri::async_runtime::spawn(async move { run_job(run_id, &job, &work_dir, &sink, rx).await })
                })
                .collect();
            for h in handles {
                let _ = h.await;
            }
        } else {
            for job in &jobs {
                run_job(run_id, job, &work_dir, &sink, rx.clone()).await;
            }
        }
        runs(|m| m.remove(&run_id));
        sink.done(run_id);
    });
    run_id
}

/// 지금 실행 중인 작업이 있는지 (자동 업데이트가 설치를 미룰지 정할 때 쓴다)
pub fn busy() -> bool {
    runs(|m| !m.is_empty())
}

/// 실행을 중지한다. 해당 실행이 없으면 false.
pub fn cancel(run_id: u64) -> bool {
    runs(|m| m.get(&run_id).map(|tx| tx.send(true).is_ok()).unwrap_or(false))
}

#[derive(PartialEq, Clone, Copy)]
enum Outcome {
    Normal,
    Cancelled,
    Capped,
    Timeout,
}

async fn run_job(run_id: u64, job: &Job, work_dir: &Path, sink: &Arc<dyn Sink>, mut cancel: watch::Receiver<bool>) {
    let started = Instant::now();
    let step = |status, exit_code| {
        sink.step(StepEvent { run_id, order: job.order, program: job.program.clone(), status, exit_code, elapsed_ms: started.elapsed().as_millis() as u64 })
    };

    if *cancel.borrow() {
        step(StepStatus::Cancelled, None);
        return;
    }
    step(StepStatus::Running, None);

    if job.engine == JobEngine::Builtin {
        let emit = |line: String| sink.output(OutputEvent { run_id, order: job.order, stream: "stdout", line });
        let work = async {
            match job.program.as_str() {
                "portscan" => builtin::portscan(&job.argv, &emit).await,
                "netmap" => builtin::netmap(&job.argv, &emit).await,
                other => Err(format!("내장 도구가 아닙니다: {other}")),
            }
        };
        tokio::select! {
            r = work => match r {
                Ok(code) => step(StepStatus::Done, Some(code)),
                Err(e) => {
                    sink.output(OutputEvent { run_id, order: job.order, stream: "stderr", line: e });
                    step(StepStatus::Error, None);
                }
            },
            _ = async { while cancel.changed().await.is_ok() { if *cancel.borrow() { return; } } std::future::pending::<()>().await } => step(StepStatus::Cancelled, None),
            _ = tokio::time::sleep(Duration::from_secs(u64::from(job.timeout_secs))) => step(StepStatus::Timeout, None),
        }
        return;
    }

    // `timeout`의 시간 인자에 고유한 소수부를 붙여, 중지할 때 이 실행만 골라 낼 수 있게 한다.
    let tag = format!("{}.{:06}", job.timeout_secs, NEXT_TAG.fetch_add(1, Ordering::Relaxed) % 1_000_000);
    let command = match job.engine {
        JobEngine::Wsl => Ok(build_command(job, &tag, work_dir)),
        JobEngine::Windows => build_windows_command(job),
        JobEngine::Builtin => Err("내장 도구는 여기서 실행하지 않습니다".into()),
    };
    let spawned = command.and_then(|mut c| c.spawn().map_err(|e| e.to_string()));
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            sink.output(OutputEvent { run_id, order: job.order, stream: "stderr", line: format!("실행하지 못했습니다: {e}") });
            step(StepStatus::Error, None);
            return;
        }
    };

    let limits = Arc::new(Limits::default());
    let mut pumps = Vec::new();
    if let Some(r) = child.stdout.take() {
        pumps.push(tauri::async_runtime::spawn(pump(r, "stdout", run_id, job.order, sink.clone(), limits.clone())));
    }
    if let Some(r) = child.stderr.take() {
        pumps.push(tauri::async_runtime::spawn(pump(r, "stderr", run_id, job.order, sink.clone(), limits.clone())));
    }

    // `timeout`이 제 역할을 못 했을 때를 대비한 바깥쪽 제한
    let slack = if job.engine == JobEngine::Wsl { 15 } else { 0 };
    let hard = tokio::time::sleep(Duration::from_secs(u64::from(job.timeout_secs) + slack));
    tokio::pin!(hard);
    // Windows 조회는 리눅스 쪽 프로세스가 없으므로 신호 없이 바로 끝낸다.
    let remote = job.engine == JobEngine::Wsl;
    let grace = if remote { GRACE } else { Duration::ZERO };
    let mut outcome = Outcome::Normal;
    let mut force_at: Option<tokio::time::Instant> = None;
    let mut cancel_closed = false;

    let status = loop {
        let force = async {
            match force_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            s = child.wait() => break s,
            r = cancel.changed(), if outcome == Outcome::Normal && !cancel_closed => {
                if r.is_err() {
                    cancel_closed = true;
                } else if *cancel.borrow() {
                    outcome = Outcome::Cancelled;
                    if remote {
                        signal_remote(&tag).await;
                    }
                    force_at = Some(tokio::time::Instant::now() + grace);
                }
            }
            _ = limits.hit.notified(), if outcome == Outcome::Normal => {
                outcome = Outcome::Capped;
                if remote {
                    signal_remote(&tag).await;
                }
                force_at = Some(tokio::time::Instant::now() + grace);
            }
            _ = &mut hard, if outcome == Outcome::Normal => {
                outcome = Outcome::Timeout;
                if remote {
                    signal_remote(&tag).await;
                }
                force_at = Some(tokio::time::Instant::now() + grace);
            }
            _ = force => {
                let _ = child.start_kill();
                force_at = None;
            }
        }
    };

    // 남은 출력을 마저 읽는다. 손자 프로세스가 파이프를 잡고 있을 수 있어 오래 기다리지 않는다.
    for p in pumps {
        let _ = tokio::time::timeout(Duration::from_secs(2), p).await;
    }

    let code = status.ok().and_then(|s| s.code());
    match outcome {
        Outcome::Cancelled => step(StepStatus::Cancelled, code),
        Outcome::Capped => step(StepStatus::Capped, code),
        Outcome::Timeout => step(StepStatus::Timeout, code),
        // GNU timeout: 124는 시간 초과, 137은 강제 종료
        Outcome::Normal if matches!(code, Some(124) | Some(137)) => step(StepStatus::Timeout, code),
        Outcome::Normal => step(StepStatus::Done, code),
    }
}

/// 리눅스 쪽에서 실행될 명령(프로그램과 인자).
fn inner_command(job: &Job, tag: &str) -> Vec<String> {
    let mut v: Vec<String> = ["timeout", "--signal=INT", "--kill-after=3", tag, "stdbuf", "-oL", "-eL"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    v.push(job.program.clone());
    v.extend(job.argv.iter().cloned());
    v
}

fn build_command(job: &Job, tag: &str, work_dir: &Path) -> Command {
    let inner = inner_command(job, tag);
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("wsl.exe");
        c.arg("--cd").arg(work_dir);
        if job.root {
            c.args(["-u", "root"]);
        }
        c.arg("--exec").args(&inner);
        c
    } else {
        let mut c = Command::new(&inner[0]);
        c.args(&inner[1..]).current_dir(work_dir);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    hide_console_window(&mut cmd);
    cmd
}

fn build_windows_command(job: &Job) -> Result<Command, String> {
    if !cfg!(target_os = "windows") {
        return Err("Windows에서만 동작하는 도구입니다".into());
    }
    let args = windows::powershell_args(&job.program, &job.argv)?;
    let mut cmd = Command::new("powershell.exe");
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    hide_console_window(&mut cmd);
    Ok(cmd)
}

/// 리눅스 쪽 `timeout` 프로세스에 INT 신호를 보낸다. `timeout`은 받은 신호를 도구에 전달하므로
/// 도구가 요약(통계)을 출력하고 정상 종료할 기회를 얻는다.
async fn signal_remote(tag: &str) {
    let pattern = format!("^timeout --signal=INT --kill-after=3 {} ", tag.replace('.', "\\."));
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("wsl.exe");
        c.args(["-u", "root", "--exec", "pkill", "--signal", "INT", "-f", &pattern]);
        c
    } else {
        let mut c = Command::new("pkill");
        c.args(["--signal", "INT", "-f", &pattern]);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    hide_console_window(&mut cmd);
    let _ = tokio::time::timeout(Duration::from_secs(5), cmd.status()).await;
}

#[derive(Default)]
struct Limits {
    lines: AtomicUsize,
    bytes: AtomicUsize,
    capped: AtomicBool,
    hit: Notify,
}

async fn pump<R: AsyncRead + Unpin>(reader: R, stream: &'static str, run_id: u64, order: u32, sink: Arc<dyn Sink>, limits: Arc<Limits>) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if limits.capped.load(Ordering::Relaxed) {
            continue; // 상한을 넘은 뒤에는 읽어서 버리기만 한다
        }
        let lines = limits.lines.fetch_add(1, Ordering::Relaxed) + 1;
        let bytes = limits.bytes.fetch_add(buf.len(), Ordering::Relaxed) + buf.len();
        if lines > MAX_LINES || bytes > MAX_BYTES {
            if !limits.capped.swap(true, Ordering::Relaxed) {
                sink.output(OutputEvent { run_id, order, stream: "stderr", line: "[출력이 너무 많아 실행을 중단했습니다]".into() });
                limits.hit.notify_one();
            }
            continue;
        }
        sink.output(OutputEvent { run_id, order, stream, line: decode_line(&buf) });
    }
}

/// 한 줄을 화면에 보일 문자열로 바꾼다. wsl.exe 자체 메시지는 UTF-16LE로 온다.
fn decode_line(raw: &[u8]) -> String {
    // UTF-16 줄을 '\n' 바이트에서 자르면 다음 줄 맨 앞에 0 바이트가 남는다.
    let raw = raw.strip_prefix(&[0]).unwrap_or(raw);
    decode_wsl_output(raw)
        .chars()
        .filter(|c| *c == '\t' || !c.is_control())
        .take(MAX_LINE_CHARS)
        .collect()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Collect {
        lines: Mutex<Vec<(u32, &'static str, String)>>,
        steps: Mutex<Vec<StepEvent>>,
        done: Notify,
    }
    impl Sink for Collect {
        fn output(&self, ev: OutputEvent) {
            self.lines.lock().unwrap().push((ev.order, ev.stream, ev.line));
        }
        fn step(&self, ev: StepEvent) {
            self.steps.lock().unwrap().push(ev);
        }
        fn done(&self, _run_id: u64) {
            self.done.notify_one();
        }
    }
    impl Collect {
        fn final_status(&self, order: u32) -> (StepStatus, Option<i32>) {
            let steps = self.steps.lock().unwrap();
            let ev = steps.iter().rev().find(|e| e.order == order && e.status != StepStatus::Running).expect("끝난 단계");
            (ev.status, ev.exit_code)
        }
        fn text(&self) -> String {
            self.lines.lock().unwrap().iter().map(|(_, _, l)| l.as_str()).collect::<Vec<_>>().join("\n")
        }
    }

    fn job(order: u32, program: &str, args: &[&str], timeout_secs: u32) -> Job {
        Job { order, program: program.into(), argv: args.iter().map(|s| s.to_string()).collect(), root: false, timeout_secs, engine: JobEngine::Wsl }
    }

    async fn run(jobs: Vec<Job>, parallel: bool) -> (Arc<Collect>, u64) {
        let sink = Arc::new(Collect::default());
        let id = start(jobs, parallel, std::env::temp_dir(), sink.clone());
        (sink, id)
    }

    async fn finished(sink: &Collect) {
        tokio::time::timeout(Duration::from_secs(30), sink.done.notified()).await.expect("실행이 끝나야 한다");
    }

    #[tokio::test]
    async fn runs_and_streams_output() {
        let (sink, _) = run(vec![job(1, "ping", &["-c", "2", "-i", "0.2", "--", "127.0.0.1"], 20)], false).await;
        finished(&sink).await;
        assert_eq!(sink.final_status(1), (StepStatus::Done, Some(0)));
        assert!(sink.text().contains("bytes from 127.0.0.1"), "{}", sink.text());
    }

    #[tokio::test]
    async fn nonzero_exit_is_reported_as_done_with_code() {
        let (sink, _) = run(vec![job(1, "nc", &["-vz", "-w", "1", "--", "127.0.0.1", "9"], 20)], false).await;
        finished(&sink).await;
        assert_eq!(sink.final_status(1), (StepStatus::Done, Some(1)));
        assert!(sink.text().contains("refused"), "{}", sink.text());
    }

    #[tokio::test]
    async fn cancel_stops_the_tool_gracefully() {
        let (sink, id) = run(vec![job(1, "ping", &["-i", "0.2", "--", "127.0.0.1"], 60)], false).await;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(cancel(id));
        finished(&sink).await;
        assert_eq!(sink.final_status(1).0, StepStatus::Cancelled);
        // INT 신호가 도구까지 전달되어 통계가 출력되어야 한다
        assert!(sink.text().contains("ping statistics"), "{}", sink.text());
        assert!(!cancel(id), "끝난 실행은 중지할 수 없다");
    }

    #[tokio::test]
    async fn timeout_is_enforced() {
        let (sink, _) = run(vec![job(1, "ping", &["-i", "0.2", "--", "127.0.0.1"], 1)], false).await;
        finished(&sink).await;
        assert_eq!(sink.final_status(1).0, StepStatus::Timeout);
    }

    #[tokio::test]
    async fn output_cap_stops_the_run() {
        let (sink, _) = run(vec![job(1, "seq", &["1", "100000000"], 30)], false).await;
        finished(&sink).await;
        assert_eq!(sink.final_status(1).0, StepStatus::Capped);
        assert!(sink.lines.lock().unwrap().len() <= MAX_LINES + 1);
    }

    #[tokio::test]
    async fn missing_program_fails_without_hanging() {
        let (sink, _) = run(vec![job(1, "netx-no-such-tool", &[], 5)], false).await;
        finished(&sink).await;
        let (status, code) = sink.final_status(1);
        assert_eq!(status, StepStatus::Done);
        assert_ne!(code, Some(0));
    }

    #[tokio::test]
    async fn parallel_and_sequential_runs_finish_all_steps() {
        for parallel in [true, false] {
            let jobs = vec![
                job(1, "ping", &["-c", "1", "--", "127.0.0.1"], 20),
                job(2, "ping", &["-c", "1", "--", "127.0.0.1"], 20),
            ];
            let (sink, _) = run(jobs, parallel).await;
            finished(&sink).await;
            assert_eq!(sink.final_status(1).0, StepStatus::Done);
            assert_eq!(sink.final_status(2).0, StepStatus::Done);
        }
    }

    #[tokio::test]
    async fn cancel_skips_remaining_sequential_steps() {
        let jobs = vec![job(1, "ping", &["-i", "0.2", "--", "127.0.0.1"], 60), job(2, "ping", &["-c", "1", "--", "127.0.0.1"], 20)];
        let (sink, id) = run(jobs, false).await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        cancel(id);
        finished(&sink).await;
        assert_eq!(sink.final_status(1).0, StepStatus::Cancelled);
        assert_eq!(sink.final_status(2).0, StepStatus::Cancelled);
    }

    #[tokio::test]
    async fn windows_only_tool_reports_an_error_elsewhere() {
        let mut j = job(1, "winports", &["8080"], 5);
        j.engine = JobEngine::Windows;
        let (sink, _) = run(vec![j], false).await;
        finished(&sink).await;
        assert_eq!(sink.final_status(1).0, StepStatus::Error);
        assert!(sink.text().contains("Windows에서만"), "{}", sink.text());
    }

    #[test]
    fn shell_metacharacters_are_passed_as_plain_arguments() {
        let j = job(1, "ping", &["-c", "1", "127.0.0.1; touch /tmp/netx-pwned"], 5);
        let inner = inner_command(&j, "5.000001");
        assert_eq!(inner.last().unwrap(), "127.0.0.1; touch /tmp/netx-pwned");
        assert_eq!(inner[..8], ["timeout", "--signal=INT", "--kill-after=3", "5.000001", "stdbuf", "-oL", "-eL", "ping"]);
    }

    #[test]
    fn decode_line_handles_utf16_and_control_chars() {
        let utf16: Vec<u8> = "오류: 배포판 없음\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_line(&utf16), "오류: 배포판 없음");
        let mut next = vec![0u8];
        next.extend("ok\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(decode_line(&next), "ok");
        assert_eq!(decode_line(b"plain \x1b[2Jtext\n"), "plain [2Jtext");
    }
}
