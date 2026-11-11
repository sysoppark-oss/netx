//! FTP·TFTP처럼 NetX가 직접 명령을 받는 접속의 줄 입력.
//! 터미널 창은 친 글자를 그대로 보내므로, 여기서 한 줄로 모으고 화면에 되울린다.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

use super::{Input, Out};

const LINE_MAX: usize = 1024;

#[derive(Debug, PartialEq)]
pub enum Event {
    Line(String),
    /// Ctrl+C
    Interrupt,
    /// 빈 줄에서 Ctrl+D
    End,
}

enum Esc {
    None,
    Start,
    Csi,
    Ss3,
}

/// 글자를 모아 한 줄을 만든다. 화살표 같은 제어 글자열은 버린다.
pub struct LineEdit {
    line: String,
    partial: Vec<u8>,
    esc: Esc,
}

fn width(c: char) -> usize {
    if (c as u32) >= 0x1100 { 2 } else { 1 }
}

impl LineEdit {
    pub fn new() -> Self {
        LineEdit { line: String::new(), partial: Vec::new(), esc: Esc::None }
    }

    /// 글자 하나를 받는다. `echo`에는 화면에 되울릴 글자가 쌓인다(`show`가 거짓이면 암호처럼 숨긴다).
    pub fn feed(&mut self, b: u8, show: bool, echo: &mut Vec<u8>) -> Option<Event> {
        match self.esc {
            Esc::Start => {
                self.esc = match b {
                    b'[' => Esc::Csi,
                    b'O' => Esc::Ss3,
                    _ => Esc::None,
                };
                return None;
            }
            Esc::Csi => {
                if (0x40..=0x7e).contains(&b) {
                    self.esc = Esc::None;
                }
                return None;
            }
            Esc::Ss3 => {
                self.esc = Esc::None;
                return None;
            }
            Esc::None => {}
        }
        match b {
            0x1b => self.esc = Esc::Start,
            b'\r' | b'\n' => {
                self.partial.clear();
                echo.extend_from_slice(b"\r\n");
                return Some(Event::Line(std::mem::take(&mut self.line)));
            }
            0x03 => {
                self.partial.clear();
                self.line.clear();
                echo.extend_from_slice(b"^C\r\n");
                return Some(Event::Interrupt);
            }
            0x04 if self.line.is_empty() => {
                echo.extend_from_slice(b"\r\n");
                return Some(Event::End);
            }
            0x7f | 0x08 => {
                self.partial.clear();
                if let Some(c) = self.line.pop() {
                    if show {
                        for _ in 0..width(c) {
                            echo.extend_from_slice(b"\x08 \x08");
                        }
                    }
                }
            }
            0x00..=0x1f => {}
            _ => {
                self.partial.push(b);
                match std::str::from_utf8(&self.partial) {
                    Ok(s) => {
                        if self.line.len() + s.len() <= LINE_MAX {
                            self.line.push_str(s);
                            if show {
                                echo.extend_from_slice(s.as_bytes());
                            }
                        }
                        self.partial.clear();
                    }
                    Err(e) if e.error_len().is_some() || self.partial.len() > 4 => self.partial.clear(),
                    Err(_) => {}
                }
            }
        }
        None
    }
}

/// 한 접속의 입력과 출력
pub struct Console {
    rx: Receiver<Input>,
    pub out: Out,
    edit: LineEdit,
    pending: VecDeque<u8>,
    closed: bool,
}

impl Console {
    pub fn new(rx: Receiver<Input>, out: Out) -> Self {
        Console { rx, out, edit: LineEdit::new(), pending: VecDeque::new(), closed: false }
    }

    /// 프롬프트를 보이고 한 줄을 받는다. 창이 닫히면 `None`.
    pub fn read_line(&mut self, prompt: &str, show: bool) -> Option<String> {
        self.out.text(prompt);
        loop {
            while let Some(b) = self.pending.pop_front() {
                let mut echo = Vec::new();
                let event = self.edit.feed(b, show, &mut echo);
                self.out.bytes(&echo);
                match event {
                    Some(Event::Line(line)) => return Some(line),
                    Some(Event::Interrupt) => self.out.text(prompt),
                    Some(Event::End) => return Some("quit".into()),
                    None => {}
                }
            }
            if self.closed {
                return None;
            }
            match self.rx.recv() {
                Ok(Input::Data(bytes)) => self.pending.extend(bytes),
                Ok(Input::Resize(..)) => {}
                Ok(Input::Close) | Ok(Input::Eof(_)) | Err(_) => self.closed = true,
            }
        }
    }

    /// 전송 도중 사용자가 Ctrl+C를 눌렀거나 창을 닫았는지. 미리 쳐 둔 다음 명령은 남겨 둔다.
    pub fn interrupted(&mut self) -> bool {
        let mut hit = self.closed;
        loop {
            match self.rx.try_recv() {
                Ok(Input::Data(bytes)) => {
                    if bytes.contains(&0x03) {
                        self.pending.clear();
                        hit = true;
                    } else {
                        self.pending.extend(bytes);
                    }
                }
                Ok(Input::Resize(..)) => {}
                Ok(Input::Close) | Ok(Input::Eof(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.closed = true;
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return hit,
            }
        }
    }

    pub fn closed(&self) -> bool {
        self.closed
    }
}

/// 명령 줄을 낱말로 나눈다. 빈칸이 든 이름은 큰따옴표로 묶는다.
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// 전송량 표기
pub fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_in(edit: &mut LineEdit, input: &[u8], show: bool) -> (Vec<Event>, String) {
        let (mut events, mut echo) = (Vec::new(), Vec::new());
        for &b in input {
            events.extend(edit.feed(b, show, &mut echo));
        }
        (events, String::from_utf8_lossy(&echo).into_owned())
    }

    #[test]
    fn line_editing() {
        let mut edit = LineEdit::new();
        // 지우기, 화살표(버림), 한글(지울 때 두 칸)
        let (events, echo) = type_in(&mut edit, "gex\x7ft 가\x7f\x1b[Aa.bin\r".as_bytes(), true);
        assert_eq!(events, [Event::Line("get a.bin".into())]);
        assert_eq!(echo, "gex\x08 \x08t 가\x08 \x08\x08 \x08a.bin\r\n");
        // 암호: 되울리지 않는다
        let (events, echo) = type_in(&mut edit, b"secret\x7f\r", false);
        assert_eq!(events, [Event::Line("secre".into())]);
        assert_eq!(echo, "\r\n");
        // Ctrl+C는 줄을 버리고, 빈 줄의 Ctrl+D는 끝
        let (events, _) = type_in(&mut edit, b"abc\x03\x04", true);
        assert_eq!(events, [Event::Interrupt, Event::End]);
        // 너무 긴 줄과 깨진 글자는 버틴다
        let mut long = vec![b'a'; 5000];
        long.extend_from_slice(&[0xff, 0xfe, b'\r']);
        let (events, _) = type_in(&mut edit, &long, true);
        assert!(matches!(&events[..], [Event::Line(l)] if l.len() == LINE_MAX));
    }

    #[test]
    fn splitting_words() {
        assert_eq!(words("  get  a.bin   b.bin "), ["get", "a.bin", "b.bin"]);
        assert_eq!(words(r#"put "my file.cfg" "" x"#), ["put", "my file.cfg", "", "x"]);
        assert!(words("   ").is_empty());
        assert_eq!((size_text(10), size_text(2048), size_text(3 * 1_048_576)), ("10 B".into(), "2.0 KB".into(), "3.0 MB".into()));
    }
}
