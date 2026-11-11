//! TFTP 클라이언트(내장, RFC 1350, 바이너리 전송). Windows에는 tftp 프로그램이 기본으로 없다.
//! 서버 기능은 없다(포트를 열고 기다리지 않는다).

use std::io::{Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::console::{Console, size_text, words};
use super::{Input, Out, files, plain};

const BLOCK: usize = 512;
/// 받는 파일 크기 상한
const MAX_GET: u64 = 1 << 30;

const RRQ: u16 = 1;
const WRQ: u16 = 2;
const DATA: u16 = 3;
const ACK: u16 = 4;
const ERROR: u16 = 5;

#[derive(Clone, Copy)]
pub struct Timing {
    pub timeout: Duration,
    pub retries: u32,
}

pub const TIMING: Timing = Timing { timeout: Duration::from_secs(3), retries: 5 };

#[derive(Debug, PartialEq)]
pub enum Packet<'a> {
    Data(u16, &'a [u8]),
    Ack(u16),
    Error(u16, String),
    Other,
}

pub fn request(op: u16, name: &str) -> Vec<u8> {
    let mut out = op.to_be_bytes().to_vec();
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out.extend_from_slice(b"octet\0");
    out
}

fn head(op: u16, block: u16) -> Vec<u8> {
    [op.to_be_bytes(), block.to_be_bytes()].concat()
}

pub fn parse(buf: &[u8]) -> Packet<'_> {
    let (Some(op), Some(arg)) = (buf.get(0..2), buf.get(2..4)) else { return Packet::Other };
    let (op, arg) = (u16::from_be_bytes([op[0], op[1]]), u16::from_be_bytes([arg[0], arg[1]]));
    let rest = buf.get(4..).unwrap_or(&[]);
    match op {
        DATA if rest.len() <= BLOCK => Packet::Data(arg, rest),
        ACK => Packet::Ack(arg),
        ERROR => {
            let text = rest.split(|b| *b == 0).next().unwrap_or(&[]);
            Packet::Error(arg, plain(&String::from_utf8_lossy(&text[..text.len().min(200)])))
        }
        _ => Packet::Other,
    }
}

/// 원격 파일 이름: 비어 있지 않고 제어 문자가 없어야 한다.
pub fn check_remote(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 255 || name.chars().any(|c| c.is_control()) {
        return Err("원격 파일 이름이 올바르지 않습니다".into());
    }
    Ok(())
}

/// 패킷을 보내고 기다리던 답이 올 때까지 받는다. 답이 없으면 다시 보낸다.
/// 서버는 첫 답을 새 포트에서 보내므로 그 포트를 기억하고, 그 뒤로는 그 주소에서 온 것만 받는다.
#[allow(clippy::too_many_arguments)]
fn exchange(sock: &UdpSocket, server: SocketAddr, peer: &mut Option<SocketAddr>, send: &[u8], want: &dyn Fn(&Packet) -> bool, buf: &mut [u8], timing: Timing, keep_going: &mut dyn FnMut() -> bool) -> Result<usize, String> {
    for _ in 0..=timing.retries {
        sock.send_to(send, peer.unwrap_or(server)).map_err(|e| format!("보내지 못했습니다: {e}"))?;
        let deadline = Instant::now() + timing.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            if !keep_going() {
                return Err("중단했습니다".into());
            }
            let _ = sock.set_read_timeout(Some(left.min(Duration::from_millis(300))));
            let Ok((n, from)) = sock.recv_from(buf) else { continue };
            let known = match *peer {
                Some(p) => from == p,
                None => from.ip() == server.ip(),
            };
            if !known {
                continue;
            }
            let packet = parse(&buf[..n]);
            if let Packet::Error(code, text) = &packet {
                return Err(format!("서버가 거절했습니다 (오류 {code}): {text}"));
            }
            if want(&packet) {
                *peer = Some(from);
                return Ok(n);
            }
        }
    }
    Err("서버가 답하지 않습니다 (주소, 포트, 방화벽을 확인하세요)".into())
}

/// 파일을 받는다. `tick`은 지금까지 받은 양을 받고, 거짓을 돌려주면 중단한다.
pub fn get(sock: &UdpSocket, server: SocketAddr, remote: &str, to: &mut dyn Write, timing: Timing, tick: &mut dyn FnMut(u64) -> bool) -> Result<u64, String> {
    let mut buf = [0u8; BLOCK + 64];
    let (mut peer, mut expected, mut total) = (None, 1u16, 0u64);
    let mut last = request(RRQ, remote);
    loop {
        let n = exchange(sock, server, &mut peer, &last, &|p| matches!(p, Packet::Data(b, _) if *b == expected), &mut buf, timing, &mut || tick(total))?;
        let Packet::Data(_, data) = parse(&buf[..n]) else { return Err("알 수 없는 답을 받았습니다".into()) };
        to.write_all(data).map_err(|e| format!("파일에 쓰지 못했습니다: {e}"))?;
        total += data.len() as u64;
        if total > MAX_GET {
            return Err("파일이 너무 큽니다 (1 GB 초과)".into());
        }
        last = head(ACK, expected);
        if data.len() < BLOCK {
            let _ = sock.send_to(&last, peer.unwrap_or(server));
            return Ok(total);
        }
        expected = expected.wrapping_add(1);
    }
}

/// 파일을 올린다.
pub fn put(sock: &UdpSocket, server: SocketAddr, remote: &str, from: &mut dyn Read, timing: Timing, tick: &mut dyn FnMut(u64) -> bool) -> Result<u64, String> {
    let mut buf = [0u8; BLOCK + 64];
    let (mut peer, mut block, mut total, mut done) = (None, 0u16, 0u64, false);
    let mut last = request(WRQ, remote);
    loop {
        exchange(sock, server, &mut peer, &last, &|p| matches!(p, Packet::Ack(b) if *b == block), &mut buf, timing, &mut || tick(total))?;
        if done {
            return Ok(total);
        }
        let mut chunk = vec![0u8; BLOCK];
        let mut filled = 0;
        while filled < BLOCK {
            match from.read(&mut chunk[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) => return Err(format!("파일을 읽지 못했습니다: {e}")),
            }
        }
        chunk.truncate(filled);
        block = block.wrapping_add(1);
        last = head(DATA, block);
        last.extend_from_slice(&chunk);
        total += filled as u64;
        done = filled < BLOCK;
    }
}

const HELP: &str = "명령\n  get <원격 파일> [저장할 이름]   서버에서 받아 전송 폴더에 저장\n  put <파일> [원격 이름]           전송 폴더의 파일을 서버로 올림\n  lls                              전송 폴더 목록\n  quit                             끝내기\n전송 도중 Ctrl+C로 중단합니다. 빈칸이 든 이름은 \"큰따옴표\"로 묶습니다.\n";

pub fn run(host: &str, port: u16, dir: &Path, rx: Receiver<Input>, out: Out) -> String {
    let mut con = Console::new(rx, out);
    let server = match super::resolve(host, port) {
        Ok(addrs) => addrs[0],
        Err(e) => return e,
    };
    con.out.note(&format!("TFTP 서버 {host}:{port}. TFTP는 암호도 암호화도 없으니 믿을 수 있는 망에서만 쓰세요. 명령은 help."));
    loop {
        let Some(line) = con.read_line("tftp> ", true) else { return "닫았습니다".into() };
        let args = words(&line);
        let Some(verb) = args.first() else { continue };
        let result = match (verb.as_str(), args.len()) {
            ("get", 2 | 3) => transfer(&mut con, server, dir, true, &args[1], args.get(2)),
            ("put", 2 | 3) => transfer(&mut con, server, dir, false, &args[1], args.get(2)),
            ("lls", _) => {
                con.out.text(&files::list_text(dir));
                Ok(())
            }
            ("help" | "?", _) => {
                con.out.text(HELP);
                Ok(())
            }
            ("quit" | "exit" | "bye", _) => return "끝냈습니다".into(),
            _ => Err("모르는 명령입니다. help를 입력하세요.".into()),
        };
        if let Err(e) = result {
            con.out.text(&format!("\x1b[31m{}\x1b[0m\n", plain(&e)));
        }
        if con.closed() {
            return "닫았습니다".into();
        }
    }
}

fn bind(server: SocketAddr) -> Result<UdpSocket, String> {
    UdpSocket::bind(if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).map_err(|e| format!("소켓을 열지 못했습니다: {e}"))
}

/// 진행 상황을 한 줄에 덮어쓰며 보이고, 사용자가 중단했는지 알려 준다.
pub fn ticker<'a>(con: &'a mut Console, label: &'a str) -> impl FnMut(u64) -> bool + 'a {
    let mut shown = 0u64;
    move |total| {
        if total >= shown + 262_144 {
            shown = total;
            con.out.bytes(format!("\r  {label} {}   ", size_text(total)).as_bytes());
        }
        !con.interrupted()
    }
}

fn transfer(con: &mut Console, server: SocketAddr, dir: &Path, download: bool, first: &str, second: Option<&String>) -> Result<(), String> {
    let sock = bind(server)?;
    let started = Instant::now();
    if download {
        check_remote(first)?;
        let name = second.map(String::as_str).unwrap_or_else(|| files::base_name(first));
        files::check_name(name)?;
        let path = files::fresh_path(dir, name);
        let mut file = std::fs::File::create(&path).map_err(|e| format!("파일을 만들지 못했습니다: {e}"))?;
        let result = get(&sock, server, first, &mut file, TIMING, &mut ticker(con, "받는 중"));
        drop(file);
        match result {
            Ok(total) => con.out.text(&format!("\r받았습니다: {} ({}, {:.1}초)\n", path.display(), size_text(total), started.elapsed().as_secs_f32())),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                con.out.text("\r");
                return Err(e);
            }
        }
    } else {
        let (mut file, size) = files::open_upload(dir, first)?;
        let remote = second.map(String::as_str).unwrap_or(first);
        check_remote(remote)?;
        let result = put(&sock, server, remote, &mut file, TIMING, &mut ticker(con, "올리는 중"));
        con.out.text("\r");
        let total = result?;
        con.out.text(&format!("올렸습니다: {remote} ({} / {}, {:.1}초)\n", size_text(total), size_text(size), started.elapsed().as_secs_f32()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::testing;

    const FAST: Timing = Timing { timeout: Duration::from_millis(150), retries: 4 };

    /// 시험용 서버. RRQ에는 `content`를 보내고(두 번째 블록의 첫 전송은 일부러 빠뜨린다),
    /// WRQ는 받아서 돌려준다. 실제 서버처럼 새 포트에서 답한다.
    fn server(content: Vec<u8>) -> (SocketAddr, std::thread::JoinHandle<Vec<u8>>) {
        let listen = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = listen.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let (n, client) = listen.recv_from(&mut buf).unwrap();
            let op = u16::from_be_bytes([buf[0], buf[1]]);
            assert!(buf[2..n].ends_with(b"\0octet\0"));
            let name = String::from_utf8_lossy(&buf[2..n - 7]).into_owned();
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            if name == "missing" {
                sock.send_to(&[&head(ERROR, 1)[..], b"File not found\x1b[2J\0"].concat(), client).unwrap();
                return Vec::new();
            }
            if op == RRQ {
                let mut dropped = false;
                for (i, chunk) in content.chunks(BLOCK).chain(content.len().is_multiple_of(BLOCK).then_some(&[][..])).enumerate() {
                    let block = i as u16 + 1;
                    let packet = [&head(DATA, block)[..], chunk].concat();
                    loop {
                        if block == 2 && !dropped {
                            dropped = true;
                        } else {
                            sock.send_to(&packet, client).unwrap();
                        }
                        // 클라이언트가 앞 블록의 ACK를 다시 보내면 버리고 이번 블록의 ACK를 기다린다.
                        match sock.recv_from(&mut buf) {
                            Ok((n, _)) if parse(&buf[..n]) == Packet::Ack(block) => break,
                            _ => {}
                        }
                    }
                }
                Vec::new()
            } else {
                let mut got = Vec::new();
                let mut block = 0u16;
                loop {
                    sock.send_to(&head(ACK, block), client).unwrap();
                    let (n, _) = sock.recv_from(&mut buf).unwrap();
                    if let Packet::Data(b, data) = parse(&buf[..n]) {
                        if b == block.wrapping_add(1) {
                            block = b;
                            got.extend_from_slice(data);
                            if data.len() < BLOCK {
                                sock.send_to(&head(ACK, block), client).unwrap();
                                return got;
                            }
                        }
                    }
                }
            }
        });
        (addr, handle)
    }

    fn sample(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn packets() {
        assert_eq!(request(RRQ, "a.cfg"), b"\0\x01a.cfg\0octet\0");
        assert_eq!(parse(&[0, 3, 0, 5, 1, 2]), Packet::Data(5, &[1, 2]));
        assert_eq!(parse(&[0, 4, 1, 0]), Packet::Ack(256));
        assert_eq!(parse(b"\0\x05\0\x02no\x07pe\0junk"), Packet::Error(2, "no?pe".into()));
        for junk in [&[][..], &[0], &[0, 3, 0], &[0, 9, 0, 0], &[0u8; 2000]] {
            assert!(matches!(parse(junk), Packet::Other | Packet::Data(..)));
        }
        assert_eq!(parse(&[&[0, 3, 0, 1][..], &[0u8; 513]].concat()), Packet::Other);
        assert!(check_remote("flash:/config.text").is_ok());
        assert!(check_remote("").is_err() && check_remote("a\0b").is_err());
    }

    #[test]
    fn get_with_a_lost_packet_and_exact_block_size() {
        for len in [0, 100, 512, 1024, 3000] {
            let (addr, handle) = server(sample(len));
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            let mut got = Vec::new();
            let total = get(&sock, addr, "x.bin", &mut got, FAST, &mut |_| true).unwrap();
            assert_eq!((total, got), (len as u64, sample(len)), "{len}");
            handle.join().unwrap();
        }
    }

    #[test]
    fn put_and_errors() {
        for len in [0, 511, 512, 2000] {
            let (addr, handle) = server(Vec::new());
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            let total = put(&sock, addr, "up.bin", &mut &sample(len)[..], FAST, &mut |_| true).unwrap();
            assert_eq!(total, len as u64);
            assert_eq!(handle.join().unwrap(), sample(len));
        }
        // 서버의 오류 글은 제어 문자를 지우고 보인다.
        let (addr, handle) = server(Vec::new());
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let err = get(&sock, addr, "missing", &mut Vec::new(), FAST, &mut |_| true).unwrap_err();
        assert_eq!(err, "서버가 거절했습니다 (오류 1): File not found?[2J");
        handle.join().unwrap();
        // 답이 없으면 끝난다. 중단하면 바로 끝난다.
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let quick = Timing { timeout: Duration::from_millis(30), retries: 1 };
        assert!(get(&sock, silent.local_addr().unwrap(), "x", &mut Vec::new(), quick, &mut |_| true).unwrap_err().contains("답하지"));
        assert_eq!(get(&sock, silent.local_addr().unwrap(), "x", &mut Vec::new(), FAST, &mut |_| false).unwrap_err(), "중단했습니다");
    }

    #[test]
    fn console_session() {
        let dir = testing::temp_dir("tftp");
        let (addr, handle) = server(sample(700));
        let (out, seen, tx, rx) = testing::session();
        tx.send(Input::Data(b"get /flash/boot.cfg\rlls\rget a ../x\rnope\rquit\r".to_vec())).unwrap();
        assert_eq!(run("127.0.0.1", addr.port(), &dir, rx, out), "끝냈습니다");
        handle.join().unwrap();
        assert_eq!(std::fs::read(dir.join("boot.cfg")).unwrap(), sample(700));
        let text = seen.text();
        assert!(text.contains("받았습니다") && text.contains("boot.cfg") && text.contains("700 B"), "{text}");
        assert!(text.contains("쓸 수 없습니다") && text.contains("모르는 명령"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
