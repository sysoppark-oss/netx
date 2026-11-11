//! FTP 클라이언트(내장, 수동 모드). Windows의 ftp.exe는 수동 모드가 없어 방화벽 뒤에서 잘 안 된다.
//! 암호는 "암호:" 프롬프트에 직접 치고(화면에 보이지 않는다) 서버로만 보낸다. 저장·기록하지 않는다.
//! 데이터 연결은 항상 제어 연결과 같은 주소로만 건다(서버가 알려 준 다른 주소로 가지 않는다).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::console::{Console, size_text, words};
use super::tftp::ticker;
use super::{CONNECT_TIMEOUT, Input, Out, files, plain};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const DATA_TIMEOUT: Duration = Duration::from_secs(60);
const LINE_MAX: u64 = 4096;
const REPLY_LINES_MAX: usize = 200;
/// 목록 출력 상한
const LIST_MAX: u64 = 4 * 1024 * 1024;

fn read_line(r: &mut impl BufRead) -> Result<String, String> {
    let mut raw = Vec::new();
    let n = r.take(LINE_MAX).read_until(b'\n', &mut raw).map_err(|e| format!("서버의 답을 받지 못했습니다: {e}"))?;
    if n == 0 {
        return Err("서버가 접속을 끊었습니다".into());
    }
    Ok(String::from_utf8_lossy(&raw).trim_end_matches(['\r', '\n']).to_string())
}

/// 서버의 답 하나(여러 줄일 수 있다)를 읽어 번호와 글을 돌려준다.
pub fn read_reply(r: &mut impl BufRead) -> Result<(u16, String), String> {
    let first = read_line(r)?;
    let code: u16 = first.get(..3).filter(|c| c.bytes().all(|b| b.is_ascii_digit())).and_then(|c| c.parse().ok()).ok_or("FTP 서버의 답이 아닙니다")?;
    let mut text = first.clone();
    if first.as_bytes().get(3) == Some(&b'-') {
        let end = format!("{code} ");
        for _ in 0..REPLY_LINES_MAX {
            let line = read_line(r)?;
            text.push('\n');
            text.push_str(&line);
            if line.starts_with(&end) {
                return Ok((code, text));
            }
        }
        return Err("서버의 답이 너무 깁니다".into());
    }
    Ok((code, text))
}

/// `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`에서 포트만 꺼낸다(주소는 쓰지 않는다).
pub fn parse_pasv(text: &str) -> Option<u16> {
    let nums: Vec<u16> = text.get(3..)?.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse::<u16>().ok().filter(|n| *n <= 255)).collect::<Option<_>>()?;
    match nums[..] {
        [_, _, _, _, hi, lo] => Some(hi * 256 + lo).filter(|p| *p != 0),
        _ => None,
    }
}

/// `229 Entering Extended Passive Mode (|||port|)`
pub fn parse_epsv(text: &str) -> Option<u16> {
    let inner = text.split_once("(|||")?.1.split_once("|)")?.0;
    inner.parse().ok().filter(|p| *p != 0)
}

struct Ctrl {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    peer: IpAddr,
}

impl Ctrl {
    fn reply(&mut self) -> Result<(u16, String), String> {
        read_reply(&mut self.reader)
    }

    /// 명령을 보내고 답을 받는다. 인자에 줄바꿈이 있으면 보내지 않는다(다른 명령이 끼어드는 것을 막는다).
    fn cmd(&mut self, verb: &str, arg: Option<&str>) -> Result<(u16, String), String> {
        let line = match arg {
            Some(a) if a.len() > 1024 || a.chars().any(|c| matches!(c, '\r' | '\n' | '\0')) => return Err("쓸 수 없는 글자가 있습니다".into()),
            Some(a) => format!("{verb} {a}\r\n"),
            None => format!("{verb}\r\n"),
        };
        self.writer.write_all(line.as_bytes()).map_err(|e| format!("보내지 못했습니다: {e}"))?;
        self.reply()
    }

    /// 데이터 연결을 연다(수동 모드).
    fn data(&mut self) -> Result<TcpStream, String> {
        let port = if self.peer.is_ipv4() {
            let (code, text) = self.cmd("PASV", None)?;
            (code == 227).then(|| parse_pasv(&text)).flatten().ok_or_else(|| format!("수동 모드를 열지 못했습니다: {}", plain(&text)))?
        } else {
            let (code, text) = self.cmd("EPSV", None)?;
            (code == 229).then(|| parse_epsv(&text)).flatten().ok_or_else(|| format!("수동 모드를 열지 못했습니다: {}", plain(&text)))?
        };
        let stream = TcpStream::connect_timeout(&SocketAddr::new(self.peer, port), CONNECT_TIMEOUT).map_err(|e| format!("데이터 연결을 열지 못했습니다: {e}"))?;
        let _ = stream.set_read_timeout(Some(DATA_TIMEOUT));
        let _ = stream.set_write_timeout(Some(DATA_TIMEOUT));
        Ok(stream)
    }

    /// 데이터 연결을 열고 전송 명령을 보낸다. 서버가 시작을 알리면(1xx) 연결을 돌려준다.
    fn start(&mut self, verb: &str, arg: Option<&str>) -> Result<TcpStream, String> {
        let data = self.data()?;
        let (code, text) = self.cmd(verb, arg)?;
        if !(100..200).contains(&code) {
            return Err(plain(&text));
        }
        Ok(data)
    }

    /// 전송이 끝난 뒤의 답(2xx여야 한다)
    fn finish(&mut self) -> Result<(), String> {
        let (code, text) = self.reply()?;
        if (200..300).contains(&code) { Ok(()) } else { Err(plain(&text)) }
    }

    fn list(&mut self, path: Option<&str>) -> Result<String, String> {
        let data = self.start("LIST", path)?;
        let mut raw = Vec::new();
        data.take(LIST_MAX).read_to_end(&mut raw).map_err(|e| format!("목록을 받지 못했습니다: {e}"))?;
        self.finish()?;
        Ok(plain(&String::from_utf8_lossy(&raw)))
    }

    fn get(&mut self, remote: &str, to: &mut dyn Write, tick: &mut dyn FnMut(u64) -> bool) -> Result<u64, String> {
        let mut data = self.start("RETR", Some(remote))?;
        let mut buf = vec![0u8; 65536];
        let mut total = 0u64;
        let result = loop {
            if !tick(total) {
                break Err("중단했습니다".to_string());
            }
            match data.read(&mut buf) {
                Ok(0) => break Ok(total),
                Ok(n) => {
                    if let Err(e) = to.write_all(&buf[..n]) {
                        break Err(format!("파일에 쓰지 못했습니다: {e}"));
                    }
                    total += n as u64;
                }
                Err(e) => break Err(format!("받다가 끊겼습니다: {e}")),
            }
        };
        drop(data);
        let finished = self.finish();
        result.and_then(|total| finished.map(|_| total))
    }

    fn put(&mut self, remote: &str, from: &mut dyn Read, tick: &mut dyn FnMut(u64) -> bool) -> Result<u64, String> {
        let mut data = self.start("STOR", Some(remote))?;
        let mut buf = vec![0u8; 65536];
        let mut total = 0u64;
        let result = loop {
            if !tick(total) {
                break Err("중단했습니다".to_string());
            }
            match from.read(&mut buf) {
                Ok(0) => break Ok(total),
                Ok(n) => {
                    if let Err(e) = data.write_all(&buf[..n]) {
                        break Err(format!("올리다가 끊겼습니다: {e}"));
                    }
                    total += n as u64;
                }
                Err(e) => break Err(format!("파일을 읽지 못했습니다: {e}")),
            }
        };
        let _ = data.shutdown(Shutdown::Both);
        drop(data);
        let finished = self.finish();
        result.and_then(|total| finished.map(|_| total))
    }
}

const HELP: &str = "명령\n  ls [경로]                        서버의 목록\n  cd <경로>, pwd                   서버의 폴더 이동, 현재 위치\n  get <원격 파일> [저장할 이름]    받아서 전송 폴더에 저장\n  put <파일> [원격 이름]           전송 폴더의 파일을 올림\n  lls                              전송 폴더 목록\n  quit                             끝내기\n전송 도중 Ctrl+C로 중단합니다. 빈칸이 든 이름은 \"큰따옴표\"로 묶습니다. 파일은 바이너리로 주고받습니다.\n";

fn show(con: &Console, text: &str) {
    con.out.note(&plain(text));
}

fn fail(con: &Console, text: &str) {
    con.out.text(&format!("\x1b[31m{}\x1b[0m\n", plain(text)));
}

pub fn run(host: &str, port: u16, user: Option<&str>, dir: &Path, rx: Receiver<Input>, out: Out) -> String {
    let mut con = Console::new(rx, out);
    con.out.note(&format!("{host}:{port}에 FTP로 접속합니다. FTP는 암호를 포함한 모든 내용이 암호화되지 않고 전달됩니다."));
    let stream = match super::connect(host, port) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let _ = stream.set_read_timeout(Some(CONTROL_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONTROL_TIMEOUT));
    let (Ok(peer), Ok(writer)) = (stream.peer_addr(), stream.try_clone()) else { return "접속을 준비하지 못했습니다".into() };
    let mut ctrl = Ctrl { reader: BufReader::new(stream), writer, peer: peer.ip() };
    let message = session(&mut con, &mut ctrl, user.map(str::trim).filter(|u| !u.is_empty()), dir).unwrap_or_else(|e| e);
    let _ = ctrl.writer.shutdown(Shutdown::Both);
    message
}

/// 로그인하고 명령을 받는다. 접속이 끝난 까닭을 돌려준다.
fn session(con: &mut Console, ctrl: &mut Ctrl, preset: Option<&str>, dir: &Path) -> Result<String, String> {
    let (code, text) = ctrl.reply()?;
    show(con, &text);
    if code != 220 {
        return Err("FTP 서버가 접속을 받지 않았습니다".into());
    }
    let mut preset = preset;
    let mut logged_in = false;
    for _ in 0..3 {
        let user = match preset.take() {
            Some(u) => u.to_string(),
            None => match con.read_line("사용자 이름 (익명 접속은 anonymous): ", true) {
                Some(u) => u.trim().to_string(),
                None => return Ok("닫았습니다".into()),
            },
        };
        if user.is_empty() || user.len() > 64 {
            continue;
        }
        let (mut code, mut text) = ctrl.cmd("USER", Some(&user))?;
        show(con, &text);
        if code == 331 {
            let Some(password) = con.read_line("암호: ", false) else { return Ok("닫았습니다".into()) };
            (code, text) = ctrl.cmd("PASS", Some(&password))?;
            show(con, &text);
        }
        if code == 230 {
            logged_in = true;
            break;
        }
    }
    if !logged_in {
        return Err("로그인하지 못했습니다".into());
    }
    let (code, text) = ctrl.cmd("TYPE", Some("I"))?;
    if code != 200 {
        show(con, &text);
    }
    con.out.note("로그인했습니다. 명령은 help.");

    loop {
        let Some(line) = con.read_line("ftp> ", true) else { return Ok("닫았습니다".into()) };
        let args = words(&line);
        let Some(verb) = args.first() else { continue };
        let result: Result<(), String> = match (verb.as_str(), args.len()) {
            ("ls" | "dir", 1 | 2) => ctrl.list(args.get(1).map(String::as_str)).map(|text| con.out.text(&text)),
            ("cd", 2) => ctrl.cmd("CWD", Some(&args[1])).map(|(_, text)| show(con, &text)),
            ("pwd", 1) => ctrl.cmd("PWD", None).map(|(_, text)| show(con, &text)),
            ("get", 2 | 3) => download(con, ctrl, dir, &args[1], args.get(2)),
            ("put", 2 | 3) => upload(con, ctrl, dir, &args[1], args.get(2)),
            ("lls", _) => {
                con.out.text(&files::list_text(dir));
                Ok(())
            }
            ("help" | "?", _) => {
                con.out.text(HELP);
                Ok(())
            }
            ("quit" | "exit" | "bye", _) => {
                if let Ok((_, text)) = ctrl.cmd("QUIT", None) {
                    show(con, &text);
                }
                return Ok("끝냈습니다".into());
            }
            _ => Err("모르는 명령입니다. help를 입력하세요.".into()),
        };
        if let Err(e) = result {
            fail(con, &e);
            // 서버가 끊긴 뒤라면 더 받을 수 없다.
            if e.contains("끊었습니다") {
                return Err("서버가 접속을 끊었습니다".into());
            }
        }
        if con.closed() {
            return Ok("닫았습니다".into());
        }
    }
}

fn download(con: &mut Console, ctrl: &mut Ctrl, dir: &Path, remote: &str, local: Option<&String>) -> Result<(), String> {
    let name = local.map(String::as_str).unwrap_or_else(|| files::base_name(remote));
    files::check_name(name)?;
    let path = files::fresh_path(dir, name);
    let mut file = std::fs::File::create(&path).map_err(|e| format!("파일을 만들지 못했습니다: {e}"))?;
    let started = Instant::now();
    let result = ctrl.get(remote, &mut file, &mut ticker(con, "받는 중"));
    drop(file);
    con.out.text("\r");
    match result {
        Ok(total) => {
            con.out.text(&format!("받았습니다: {} ({}, {:.1}초)\n", path.display(), size_text(total), started.elapsed().as_secs_f32()));
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

fn upload(con: &mut Console, ctrl: &mut Ctrl, dir: &Path, local: &str, remote: Option<&String>) -> Result<(), String> {
    let (mut file, size) = files::open_upload(dir, local)?;
    let remote = remote.map(String::as_str).unwrap_or(local);
    let started = Instant::now();
    let result = ctrl.put(remote, &mut file, &mut ticker(con, "올리는 중"));
    con.out.text("\r");
    let total = result?;
    con.out.text(&format!("올렸습니다: {} ({} / {}, {:.1}초)\n", plain(remote), size_text(total), size_text(size), started.elapsed().as_secs_f32()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::testing;
    use std::net::TcpListener;

    #[test]
    fn replies() {
        let mut r = &b"220 ready\r\n230-Welcome\r\n line two\r\n230 done\r\n"[..];
        assert_eq!(read_reply(&mut r).unwrap(), (220, "220 ready".into()));
        assert_eq!(read_reply(&mut r).unwrap(), (230, "230-Welcome\n line two\n230 done".into()));
        assert!(read_reply(&mut r).is_err());
        assert!(read_reply(&mut &b"SSH-2.0-OpenSSH\r\n"[..]).is_err());
        assert!(read_reply(&mut &b"22\r\n"[..]).is_err());
        // 끝나지 않는 여러 줄 답, 줄바꿈 없는 긴 줄
        let endless = "230-x\r\n".repeat(1000);
        assert!(read_reply(&mut endless.as_bytes()).is_err());
        let long = "2".repeat(100_000);
        assert!(read_reply(&mut long.as_bytes()).is_ok());
    }

    #[test]
    fn passive_ports() {
        assert_eq!(parse_pasv("227 Entering Passive Mode (192,168,0,1,19,137)."), Some(19 * 256 + 137));
        assert_eq!(parse_pasv("227 =10,0,0,1,0,21"), Some(21));
        assert_eq!(parse_pasv("227 Entering Passive Mode (192,168,0,1,19)"), None);
        assert_eq!(parse_pasv("227 (1,2,3,4,256,1)"), None);
        assert_eq!(parse_pasv("227 (1,2,3,4,0,0)"), None);
        assert_eq!(parse_pasv("227"), None);
        assert_eq!(parse_epsv("229 Entering Extended Passive Mode (|||50123|)"), Some(50123));
        assert_eq!(parse_epsv("229 (|||x|)"), None);
        assert_eq!(parse_epsv("229 (|||99999|)"), None);
    }

    /// 시험용 서버: 로그인(admin / pw 1), 목록, 받기, 올리기. 받은 명령을 모두 돌려준다.
    fn server() -> (u16, std::thread::JoinHandle<(Vec<String>, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut w = stream;
            let (mut log, mut uploaded) = (Vec::new(), Vec::new());
            let mut data: Option<TcpListener> = None;
            w.write_all(b"220-NetX test\r\n220 ready\r\n").unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let line = line.trim_end().to_string();
                log.push(line.clone());
                let (verb, arg) = line.split_once(' ').unwrap_or((&line, ""));
                let mut accept = || data.take().unwrap().accept().unwrap().0;
                match verb {
                    "USER" => w.write_all(b"331 password\r\n").unwrap(),
                    "PASS" => w.write_all(if arg == "pw 1" { b"230 ok\r\n" } else { b"530 no\r\n" }).unwrap(),
                    "TYPE" => w.write_all(b"200 ok\r\n").unwrap(),
                    "PWD" => w.write_all(b"257 \"/flash\"\r\n").unwrap(),
                    "PASV" => {
                        let l = TcpListener::bind("127.0.0.1:0").unwrap();
                        let p = l.local_addr().unwrap().port();
                        data = Some(l);
                        // 주소 칸에 엉뚱한 주소를 적어도 클라이언트는 제어 연결의 주소로 간다.
                        w.write_all(format!("227 Entering Passive Mode (10,9,8,7,{},{})\r\n", p / 256, p % 256).as_bytes()).unwrap();
                    }
                    "LIST" => {
                        w.write_all(b"150 here\r\n").unwrap();
                        accept().write_all(b"-rw- 12 a.cfg\r\n-rw- 5 \x1b[2Jevil\r\n").unwrap();
                        w.write_all(b"226 done\r\n").unwrap();
                    }
                    "RETR" if arg == "a.cfg" => {
                        w.write_all(b"150 here\r\n").unwrap();
                        accept().write_all(b"hostname sw1\n").unwrap();
                        w.write_all(b"226 done\r\n").unwrap();
                    }
                    "RETR" => w.write_all(b"550 no such file\r\n").unwrap(),
                    "STOR" => {
                        w.write_all(b"150 send\r\n").unwrap();
                        accept().read_to_end(&mut uploaded).unwrap();
                        w.write_all(b"226 done\r\n").unwrap();
                    }
                    "QUIT" => {
                        w.write_all(b"221 bye\r\n").unwrap();
                        break;
                    }
                    _ => w.write_all(b"500 what\r\n").unwrap(),
                }
            }
            (log, uploaded)
        });
        (port, handle)
    }

    #[test]
    fn console_session() {
        let dir = testing::temp_dir("ftp");
        std::fs::write(dir.join("new fw.bin"), b"\x00\x01firmware").unwrap();
        let (port, handle) = server();
        let (out, seen, tx, rx) = testing::session();
        // 첫 암호는 틀리고, 두 번째에 로그인한다. 암호에는 빈칸이 있다.
        let typed = "wrong\radmin\rpw 1\rpwd\rls\rget a.cfg\rget a.cfg\rget nothere\rget a.cfg ../../x\rput \"new fw.bin\" fw.bin\rput missing\rcd /flash\rquit\r";
        tx.send(Input::Data(typed.as_bytes().to_vec())).unwrap();
        assert_eq!(run("127.0.0.1", port, Some("admin"), &dir, rx, out), "끝냈습니다");
        let (log, uploaded) = handle.join().unwrap();
        assert_eq!(log[..5], ["USER admin", "PASS wrong", "USER admin", "PASS pw 1", "TYPE I"]);
        assert_eq!(uploaded, b"\x00\x01firmware");
        assert!(log.contains(&"STOR fw.bin".to_string()) && log.last().unwrap() == "QUIT");
        // 같은 이름으로 두 번 받으면 덮어쓰지 않고 새 이름으로 둔다. 실패한 받기는 파일을 남기지 않는다.
        assert_eq!(std::fs::read(dir.join("a.cfg")).unwrap(), b"hostname sw1\n");
        assert_eq!(std::fs::read(dir.join("a-1.cfg")).unwrap(), b"hostname sw1\n");
        assert!(!dir.join("nothere").exists());
        let text = seen.text();
        // 암호는 화면에 되울리지 않는다. 목록의 제어 문자는 지운다.
        assert!(!text.contains("wrong") && !text.contains("pw 1"), "{text}");
        assert!(text.contains("?[2Jevil") && !text.contains("\x1b[2J"), "{text}");
        assert!(text.contains("550 no such file") && text.contains("쓸 수 없습니다") && text.contains("없습니다"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
