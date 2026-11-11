//! SSH. Windows에 들어 있는 OpenSSH 클라이언트를 가상 터미널(ConPTY) 안에서 돌린다.
//! 암호, 호스트 키 확인, 키 파일은 ssh가 직접 다루고 NetX는 글자만 주고받는다.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use super::{Input, Out};

/// ssh 프로그램. Windows에서는 시스템 폴더의 것을 먼저 쓴다(PATH에 끼어든 다른 ssh를 피한다).
fn ssh_program() -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let path = std::path::Path::new(&root).join("System32").join("OpenSSH").join("ssh.exe");
            if path.is_file() {
                return path;
            }
        }
        return "ssh.exe".into();
    }
    "ssh".into()
}

/// ssh에 넘길 인자. 값은 모두 `Spec::check`를 통과한 것이다.
pub fn ssh_args(host: &str, port: u16, user: &str) -> Vec<String> {
    vec!["-p".into(), port.to_string(), "-l".into(), user.into(), host.into()]
}

fn pty_size((cols, rows): (u16, u16)) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

pub fn run(host: &str, port: u16, user: &str, size: (u16, u16), tx: Sender<Input>, rx: Receiver<Input>, out: Out) -> String {
    out.note(&format!("{user}@{host}:{port}에 SSH로 접속합니다. 암호는 아래 프롬프트에 직접 입력하세요(NetX는 저장하지 않습니다)."));
    let mut command = CommandBuilder::new(ssh_program());
    command.args(ssh_args(host, port, user));
    run_command(command, size, tx, rx, out)
}

/// 프로그램을 가상 터미널 안에서 돌리고 글자를 주고받는다.
fn run_command(mut command: CommandBuilder, size: (u16, u16), tx: Sender<Input>, rx: Receiver<Input>, out: Out) -> String {
    let pair = match native_pty_system().openpty(pty_size(size)) {
        Ok(p) => p,
        Err(e) => return format!("가상 터미널을 만들지 못했습니다: {e}"),
    };
    command.env("TERM", "xterm-256color");
    let mut child = match pair.slave.spawn_command(command) {
        Ok(c) => c,
        Err(e) => return format!("ssh를 실행하지 못했습니다: {e}\nWindows 설정 > 시스템 > 선택적 기능에서 'OpenSSH 클라이언트'가 설치돼 있는지 확인하세요."),
    };
    drop(pair.slave);
    let mut killer = child.clone_killer();
    let (Ok(mut reader), Ok(mut writer)) = (pair.master.try_clone_reader(), pair.master.take_writer()) else {
        let _ = killer.kill();
        return "가상 터미널을 준비하지 못했습니다".into();
    };

    {
        let out = out.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.bytes(&buf[..n]);
            }
        });
    }
    // ssh가 끝나면 알린다(Windows에서는 읽는 쪽이 스스로 끝나지 않는다).
    std::thread::spawn(move || {
        let message = match child.wait() {
            Ok(status) if status.success() => "접속이 끝났습니다".to_string(),
            Ok(status) => format!("ssh가 끝났습니다 (종료 코드 {})", status.exit_code()),
            Err(e) => format!("ssh가 끝났습니다: {e}"),
        };
        let _ = tx.send(Input::Eof(message));
    });

    loop {
        match rx.recv() {
            Ok(Input::Data(bytes)) => {
                if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                    let _ = killer.kill();
                }
            }
            Ok(Input::Resize(cols, rows)) => {
                let _ = pair.master.resize(pty_size((cols, rows)));
            }
            Ok(Input::Eof(message)) => {
                // 마지막 출력이 화면에 닿을 틈을 준다.
                std::thread::sleep(Duration::from_millis(200));
                return message;
            }
            Ok(Input::Close) | Err(_) => {
                let _ = killer.kill();
                return "접속을 닫았습니다".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 가상 터미널 안의 프로그램과 글자를 주고받고, 끝나면 종료 코드를 알린다.
    #[cfg(unix)]
    #[test]
    fn program_in_a_virtual_terminal() {
        use crate::term::testing;
        let (out, seen, tx, rx) = testing::session();
        let mut command = CommandBuilder::new("sh");
        command.args(["-c", "printf 'name? '; read x; echo \"hello $x ($TERM, $(stty size))\"; exit 3"]);
        tx.send(Input::Resize(100, 30)).unwrap();
        tx.send(Input::Data(b"netx\r".to_vec())).unwrap();
        let message = run_command(command, (80, 24), tx.clone(), rx, out);
        assert_eq!(message, "ssh가 끝났습니다 (종료 코드 3)");
        let text = seen.text();
        assert!(text.contains("name? ") && text.contains("hello netx (xterm-256color, 30 100)"), "{text}");
        // 닫으면 프로그램을 끝낸다.
        let (out, _, tx, rx) = testing::session();
        tx.send(Input::Close).unwrap();
        assert_eq!(run_command(CommandBuilder::new("cat"), (80, 24), tx.clone(), rx, out), "접속을 닫았습니다");
    }

    #[test]
    fn args_are_separate_items() {
        assert_eq!(ssh_args("192.168.123.21", 2222, "admin"), ["-p", "2222", "-l", "admin", "192.168.123.21"]);
    }
}
