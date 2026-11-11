//! 시리얼(콘솔 케이블) 접속.

use std::io::{ErrorKind, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, StopBits};

use super::{Input, Out};

/// 이 PC의 시리얼 포트 이름 목록
pub fn ports() -> Vec<String> {
    let mut names: Vec<String> = serialport::available_ports().unwrap_or_default().into_iter().map(|p| p.port_name).collect();
    // 리눅스 시험용: 가짜 포트(socat) 이름을 빈칸으로 나눠 적는다.
    #[cfg(not(target_os = "windows"))]
    names.extend(std::env::var("NETX_TEST_SERIAL").unwrap_or_default().split_whitespace().map(String::from));
    names.retain(|n| super::check_serial_port(n).is_ok());
    names.sort_by_key(|n| (n.len(), n.clone()));
    names.dedup();
    names
}

#[allow(clippy::too_many_arguments)]
pub fn run(port: &str, baud: u32, data_bits: u8, parity: &str, stop_bits: u8, flow: &str, tx: Sender<Input>, rx: Receiver<Input>, out: Out) -> String {
    let builder = serialport::new(port, baud)
        .data_bits(match data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        })
        .parity(match parity {
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            _ => Parity::None,
        })
        .stop_bits(if stop_bits == 2 { StopBits::Two } else { StopBits::One })
        .flow_control(match flow {
            "rtscts" => FlowControl::Hardware,
            "xonxoff" => FlowControl::Software,
            _ => FlowControl::None,
        })
        .timeout(Duration::from_millis(100));
    let mut writer = match builder.open() {
        Ok(p) => p,
        Err(e) => return format!("{port}를 열지 못했습니다: {e} (다른 프로그램이 쓰고 있는지 확인하세요)"),
    };
    let mut reader = match writer.try_clone() {
        Ok(p) => p,
        Err(e) => return format!("{port}를 준비하지 못했습니다: {e}"),
    };
    out.note(&format!("{port}를 열었습니다 ({baud} bps, {data_bits}-{}-{stop_bits}). 아무것도 안 보이면 Enter를 눌러 보세요.", parity.chars().next().unwrap_or('n').to_ascii_uppercase()));

    let stop = Arc::new(AtomicBool::new(false));
    {
        let (stop, out) = (stop.clone(), out.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while !stop.load(Ordering::Relaxed) {
                match reader.read(&mut buf) {
                    Ok(0) => {}
                    Ok(n) => out.bytes(&buf[..n]),
                    Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    Err(e) => {
                        let _ = tx.send(Input::Eof(format!("포트가 끊겼습니다: {e}")));
                        return;
                    }
                }
            }
        });
    }

    let message = loop {
        match rx.recv() {
            Ok(Input::Data(bytes)) => {
                if let Err(e) = writer.write_all(&bytes) {
                    break format!("보내지 못했습니다: {e}");
                }
            }
            Ok(Input::Resize(..)) => {}
            Ok(Input::Eof(message)) => break message,
            Ok(Input::Close) | Err(_) => break "포트를 닫았습니다".into(),
        }
    };
    stop.store(true, Ordering::Relaxed);
    message
}
