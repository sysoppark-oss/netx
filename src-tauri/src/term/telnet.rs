//! Telnet(내장). Windows에는 telnet 프로그램이 기본으로 없어서 직접 만든다.
//! 옵션 협상은 화면 되울림(ECHO), SGA, 터미널 종류, 창 크기만 받아들이고 나머지는 거절한다.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use super::{Input, Out};

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

/// 협상 글자가 끝없이 오가는 것을 막는 상한
const SB_MAX: usize = 64;

enum State {
    Data,
    Iac,
    Opt(u8),
    Sb,
    SbIac,
}

/// 받은 글자에서 Telnet 명령을 걸러 내고 답할 글자를 만든다.
pub struct Filter {
    state: State,
    sb: Vec<u8>,
    /// 옵션마다 이미 보낸 답(같은 답을 되풀이하지 않는다)
    said: [u8; 256],
    /// 상대가 친 글자를 되울려 주는지. 아니면 NetX가 화면에 직접 보인다.
    pub remote_echo: bool,
    pub naws: bool,
    pub size: (u16, u16),
}

impl Filter {
    pub fn new(size: (u16, u16)) -> Self {
        Filter { state: State::Data, sb: Vec::new(), said: [0; 256], remote_echo: false, naws: false, size }
    }

    fn say(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        let slot = &mut self.said[opt as usize];
        if *slot != verb {
            *slot = verb;
            reply.extend_from_slice(&[IAC, verb, opt]);
        }
    }

    pub fn feed(&mut self, input: &[u8], data: &mut Vec<u8>, reply: &mut Vec<u8>) {
        for &b in input {
            match self.state {
                State::Data => {
                    if b == IAC {
                        self.state = State::Iac;
                    } else {
                        data.push(b);
                    }
                }
                State::Iac => {
                    self.state = match b {
                        IAC => {
                            data.push(IAC);
                            State::Data
                        }
                        DO | DONT | WILL | WONT => State::Opt(b),
                        SB => {
                            self.sb.clear();
                            State::Sb
                        }
                        _ => State::Data,
                    }
                }
                State::Opt(verb) => {
                    self.state = State::Data;
                    match (verb, b) {
                        (WILL, ECHO) => {
                            self.remote_echo = true;
                            self.say(DO, b, reply);
                        }
                        (WILL, SGA) => self.say(DO, b, reply),
                        (WILL, _) => self.say(DONT, b, reply),
                        (WONT, _) => {
                            if b == ECHO {
                                self.remote_echo = false;
                            }
                            self.said[b as usize] = 0;
                        }
                        (DO, TTYPE) => self.say(WILL, b, reply),
                        (DO, NAWS) => {
                            self.naws = true;
                            self.say(WILL, b, reply);
                            reply.extend_from_slice(&naws(self.size));
                        }
                        (DO, _) => self.say(WONT, b, reply),
                        _ => {
                            if b == NAWS {
                                self.naws = false;
                            }
                            self.said[b as usize] = 0;
                        }
                    }
                }
                State::Sb => {
                    if b == IAC {
                        self.state = State::SbIac;
                    } else if self.sb.len() < SB_MAX {
                        self.sb.push(b);
                    }
                }
                State::SbIac => {
                    if b == SE {
                        // 터미널 종류를 물으면 xterm이라고 답한다.
                        if self.sb == [TTYPE, 1] {
                            reply.extend_from_slice(&[IAC, SB, TTYPE, 0]);
                            reply.extend_from_slice(b"xterm");
                            reply.extend_from_slice(&[IAC, SE]);
                        }
                        self.state = State::Data;
                    } else {
                        if b == IAC && self.sb.len() < SB_MAX {
                            self.sb.push(IAC);
                        }
                        self.state = State::Sb;
                    }
                }
            }
        }
    }
}

/// 창 크기를 알리는 글자
pub fn naws((cols, rows): (u16, u16)) -> Vec<u8> {
    let mut out = vec![IAC, SB, NAWS];
    for b in cols.to_be_bytes().into_iter().chain(rows.to_be_bytes()) {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
    out.extend_from_slice(&[IAC, SE]);
    out
}

/// 사용자가 친 글자를 Telnet 형식으로 바꾼다(Enter는 CR LF, 255는 두 번).
pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 2);
    for &b in input {
        match b {
            b'\r' => out.extend_from_slice(b"\r\n"),
            IAC => out.extend_from_slice(&[IAC, IAC]),
            _ => out.push(b),
        }
    }
    out
}

/// 상대가 되울려 주지 않을 때 화면에 직접 보일 글자
pub fn local_echo(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in input {
        match b {
            b'\r' => out.extend_from_slice(b"\r\n"),
            0x7f | 0x08 => out.extend_from_slice(b"\x08 \x08"),
            0x20..=0x7e | 0x80..=0xff => out.push(b),
            _ => {}
        }
    }
    out
}

pub fn run(host: &str, port: u16, size: (u16, u16), tx: Sender<Input>, rx: Receiver<Input>, out: Out) -> String {
    out.note(&format!("{host}:{port}에 Telnet으로 접속합니다. Telnet은 암호를 포함한 모든 글자가 암호화되지 않고 전달됩니다."));
    let stream = match super::connect(host, port) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let _ = stream.set_nodelay(true);
    let (Ok(mut reader), Ok(writer)) = (stream.try_clone(), stream.try_clone()) else {
        return "접속을 준비하지 못했습니다".into();
    };
    let writer = Arc::new(Mutex::new(writer));
    let filter = Arc::new(Mutex::new(Filter::new(size)));

    {
        let (writer, filter, out) = (writer.clone(), filter.clone(), out.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let message = loop {
                match reader.read(&mut buf) {
                    Ok(0) => break "상대가 접속을 끊었습니다".to_string(),
                    Ok(n) => {
                        let (mut data, mut reply) = (Vec::with_capacity(n), Vec::new());
                        filter.lock().unwrap_or_else(|e| e.into_inner()).feed(&buf[..n], &mut data, &mut reply);
                        if !reply.is_empty() {
                            let _ = writer.lock().unwrap_or_else(|e| e.into_inner()).write_all(&reply);
                        }
                        out.bytes(&data);
                    }
                    Err(e) => break format!("접속이 끊겼습니다: {e}"),
                }
            };
            let _ = tx.send(Input::Eof(message));
        });
    }

    let send = |bytes: &[u8]| writer.lock().unwrap_or_else(|e| e.into_inner()).write_all(bytes);
    loop {
        match rx.recv() {
            Ok(Input::Data(bytes)) => {
                if !filter.lock().unwrap_or_else(|e| e.into_inner()).remote_echo {
                    out.bytes(&local_echo(&bytes));
                }
                if let Err(e) = send(&encode(&bytes)) {
                    let _ = stream.shutdown(Shutdown::Both);
                    return format!("보내지 못했습니다: {e}");
                }
            }
            Ok(Input::Resize(cols, rows)) => {
                let mut f = filter.lock().unwrap_or_else(|e| e.into_inner());
                f.size = (cols, rows);
                if f.naws {
                    let _ = send(&naws((cols, rows)));
                }
            }
            Ok(Input::Eof(message)) => return message,
            Ok(Input::Close) | Err(_) => {
                let _ = stream.shutdown(Shutdown::Both);
                return "접속을 닫았습니다".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(f: &mut Filter, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut data, mut reply) = (Vec::new(), Vec::new());
        f.feed(input, &mut data, &mut reply);
        (data, reply)
    }

    #[test]
    fn negotiation() {
        let mut f = Filter::new((80, 24));
        // 상대: ECHO·SGA를 하겠다, 터미널 종류·창 크기를 알려 달라, 그 밖의 것(LINEMODE 34, 환경 39)
        let (data, reply) = feed(&mut f, &[IAC, WILL, ECHO, IAC, WILL, SGA, IAC, DO, TTYPE, IAC, DO, NAWS, IAC, DO, 34, IAC, WILL, 39, b'h', b'i']);
        assert_eq!(data, b"hi");
        let mut want = vec![IAC, DO, ECHO, IAC, DO, SGA, IAC, WILL, TTYPE, IAC, WILL, NAWS];
        want.extend_from_slice(&[IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE]);
        want.extend_from_slice(&[IAC, WONT, 34, IAC, DONT, 39]);
        assert_eq!(reply, want);
        assert!(f.remote_echo && f.naws);
        // 같은 요청이 다시 와도 같은 답을 되풀이하지 않는다.
        assert!(feed(&mut f, &[IAC, WILL, ECHO, IAC, DO, 34]).1.is_empty());
        // 터미널 종류 질문
        let (_, reply) = feed(&mut f, &[IAC, SB, TTYPE, 1, IAC, SE]);
        assert_eq!(reply, [&[IAC, SB, TTYPE, 0][..], b"xterm", &[IAC, SE]].concat());
        // 되울림을 그만두겠다
        feed(&mut f, &[IAC, WONT, ECHO]);
        assert!(!f.remote_echo);
    }

    #[test]
    fn split_and_escaped_bytes() {
        let mut f = Filter::new((80, 24));
        // 명령이 두 조각으로 나뉘어 와도 된다.
        assert_eq!(feed(&mut f, &[b'a', IAC]), (b"a".to_vec(), vec![]));
        assert_eq!(feed(&mut f, &[WILL]), (vec![], vec![]));
        assert_eq!(feed(&mut f, &[ECHO, b'b']), (b"b".to_vec(), vec![IAC, DO, ECHO]));
        // 255 두 번은 글자 255 하나, 그 밖의 명령(NOP 241)은 버린다.
        assert_eq!(feed(&mut f, &[IAC, IAC, IAC, 241, b'c']).0, vec![255, b'c']);
        // 끝나지 않는 부협상도 버틴다.
        let mut junk = vec![IAC, SB];
        junk.extend(std::iter::repeat_n(7u8, 10_000));
        assert!(feed(&mut f, &junk).0.is_empty());
        assert_eq!(feed(&mut f, &[IAC, SE, b'd']).0, b"d");
    }

    #[test]
    fn random_input_never_panics() {
        let mut f = Filter::new((80, 24));
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2000 {
            let chunk: Vec<u8> = (0..37)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    // 명령 글자가 자주 나오게 한다.
                    if seed % 3 == 0 { 240 + (seed >> 8) as u8 % 16 } else { (seed >> 8) as u8 }
                })
                .collect();
            let (_, reply) = feed(&mut f, &chunk);
            assert!(reply.len() <= chunk.len() * 12);
        }
    }

    #[test]
    fn input_encoding() {
        assert_eq!(encode(b"ls\r"), b"ls\r\n");
        assert_eq!(encode(&[b'a', 255]), [b'a', 255, 255]);
        assert_eq!(naws((255, 24)), [IAC, SB, NAWS, 0, 255, 255, 0, 24, IAC, SE]);
        assert_eq!(local_echo(b"ab\x7f\r\x1b"), b"ab\x08 \x08\r\n");
    }
}
