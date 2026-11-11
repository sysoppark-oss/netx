//! curl (기본 진단). HTTP/1.1 요청 하나를 직접 보내고 curl과 같은 모양으로 적는다.
//! 단계별 시간(DNS, 연결, TLS, 첫 응답)과 인증서 만료일을 함께 낸다.
//!
//! 하지 않는 것: HTTP/2, 프록시, 본문 보내기(POST), 인증, 쿠키. 그런 옵션은 실행 전에 "전체 진단 필요"로 막는다.
//! 인증서 검증은 프로그램에 든 공개 인증 기관 목록으로 한다(사내 인증 기관은 모른다. 그때는 `-k`).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use tokio_rustls::rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{self, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};

use super::args::Args;
use super::{Emit, clean, unsupported};

const HEAD_MAX: usize = 64 * 1024;
const BODY_MAX: usize = 512 * 1024;
const MAX_REDIRECTS: u32 = 5;

#[derive(Debug)]
pub struct Plan {
    url: url::Url,
    head: bool,
    follow: bool,
    verbose: bool,
    insecure: bool,
    include: bool,
    max_time: f64,
    connect_timeout: f64,
    write_out: Option<String>,
    agent: String,
}

impl Plan {
    pub fn parse(argv: &[String]) -> Result<Plan, String> {
        let a = Args::parse(argv, &["-m", "-w", "--connect-timeout", "-A"], &["-I", "-L", "-v", "-s", "-S", "-k", "-i", "-4", "--http1.1"])?;
        let text = a.one("주소")?;
        let text = if text.contains("://") { text.to_string() } else { format!("http://{text}") };
        let url = url::Url::parse(&text).map_err(|_| "주소가 올바르지 않습니다".to_string())?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(unsupported("http·https가 아닌 주소"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(unsupported("주소에 적은 사용자 이름·암호"));
        }
        let agent = a.value("-A").unwrap_or("NetX").to_string();
        if agent.chars().any(|c| c.is_control()) {
            return Err("-A 값에 쓸 수 없는 글자가 있습니다".into());
        }
        Ok(Plan {
            url,
            head: a.has("-I"),
            follow: a.has("-L"),
            verbose: a.has("-v"),
            insecure: a.has("-k"),
            include: a.has("-i"),
            max_time: a.number("-m", 60.0, 1.0, 600.0)?,
            connect_timeout: a.number("--connect-timeout", 10.0, 1.0, 300.0)?,
            write_out: a.value("-w").map(str::to_string),
            agent,
        })
    }
}

/// `-k`: 인증서를 검증하지 않는다(서명 확인은 그대로 한다).
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(&self, _end: &CertificateDer<'_>, _chain: &[CertificateDer<'_>], _name: &ServerName<'_>, _ocsp: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_config(insecure: bool) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(|e| e.to_string())?.with_root_certificates(roots).with_no_client_auth();
    if insecure {
        config.dangerous().set_certificate_verifier(Arc::new(AcceptAny(provider)));
    }
    Ok(config)
}

/// DER 한 덩이: (태그, 내용, 나머지)
fn tlv(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = data.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        (rest[..n].iter().fold(0usize, |acc, b| acc << 8 | *b as usize), &rest[n..])
    };
    (rest.len() >= len).then(|| (tag, &rest[..len], &rest[len..]))
}

/// 인증서의 유효 기간(시작, 끝)을 curl이 적는 모양("Nov 10 11:15:20 2099 GMT")으로 돌려준다.
pub fn validity(cert: &[u8]) -> Option<(String, String)> {
    let (_, cert, _) = tlv(cert)?;
    let (_, tbs, _) = tlv(cert)?;
    let (tag, _, mut rest) = tlv(tbs)?;
    // 맨 앞의 판 번호([0])는 없을 수도 있다.
    if tag == 0xa0 {
        rest = tlv(rest)?.2;
    }
    let rest = tlv(rest)?.2; // 서명 알고리즘
    let rest = tlv(rest)?.2; // 발급자
    let (_, period, _) = tlv(rest)?;
    let (tag1, from, period) = tlv(period)?;
    let (tag2, to, _) = tlv(period)?;
    Some((time_text(tag1, from)?, time_text(tag2, to)?))
}

fn time_text(tag: u8, raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    let digits = text.strip_suffix('Z')?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (year, rest) = match (tag, digits.len()) {
        (0x17, 12) => {
            let yy: u32 = digits[..2].parse().ok()?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, &digits[2..])
        }
        (0x18, 14) => (digits[..4].parse().ok()?, &digits[4..]),
        _ => return None,
    };
    let month: usize = rest[..2].parse().ok()?;
    let name = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].get(month.checked_sub(1)?)?;
    let day: u32 = rest[2..4].parse().ok()?;
    Some(format!("{name} {day:>2} {}:{}:{} {year} GMT", &rest[4..6], &rest[6..8], &rest[8..10]))
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// curl의 종료 코드와 글
struct Fail(i32, String);

#[derive(Default)]
struct Times {
    dns: f64,
    connect: f64,
    tls: f64,
    first: f64,
}

struct Response {
    status: u16,
    head: Vec<String>,
    body: Vec<u8>,
    remote: SocketAddr,
    times: Times,
}

fn header<'a>(head: &'a [String], name: &str) -> Option<&'a str> {
    head.iter().skip(1).find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()))
}

/// 응답 머리를 읽는다. 머리 뒤에 함께 온 본문 앞부분을 같이 돌려준다.
async fn read_head(stream: &mut dyn Io) -> Result<(Vec<String>, Vec<u8>), Fail> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let lines = String::from_utf8_lossy(&buf[..at]).split("\r\n").map(clean).collect();
            return Ok((lines, buf[at + 4..].to_vec()));
        }
        if buf.len() > HEAD_MAX {
            return Err(Fail(56, "응답 머리가 너무 깁니다".into()));
        }
        match stream.read(&mut chunk).await {
            Ok(0) if buf.is_empty() => return Err(Fail(52, "Empty reply from server".into())),
            Ok(0) => return Err(Fail(56, "응답이 도중에 끊겼습니다".into())),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(Fail(56, format!("Recv failure: {e}"))),
        }
    }
}

/// 본문을 읽는다(조각 전송 포함). 상한을 넘으면 거기서 그친다.
async fn read_body(stream: &mut dyn Io, head: &[String], mut buf: Vec<u8>) -> Vec<u8> {
    let length: Option<usize> = header(head, "content-length").and_then(|v| v.parse().ok());
    let chunked = header(head, "transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let want = if chunked { BODY_MAX * 2 } else { length.unwrap_or(BODY_MAX).min(BODY_MAX) };
    let mut chunk = [0u8; 8192];
    while buf.len() < want && !(chunked && buf.windows(5).any(|w| w == b"0\r\n\r\n")) {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    if chunked {
        buf = dechunk(&buf);
    }
    buf.truncate(want.min(BODY_MAX));
    buf
}

/// 조각 전송(chunked) 본문을 이어 붙인다. 깨져 있으면 읽은 데까지만 돌려준다.
pub fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = raw.windows(2).position(|w| w == b"\r\n") {
        let size = std::str::from_utf8(&raw[..end]).ok().and_then(|l| usize::from_str_radix(l.split(';').next().unwrap_or("").trim(), 16).ok());
        let Some(size) = size.filter(|s| *s > 0 && *s <= BODY_MAX) else { break };
        let Some(data) = raw.get(end + 2..end + 2 + size) else {
            out.extend_from_slice(&raw[end + 2..]);
            break;
        };
        out.extend_from_slice(data);
        raw = raw.get(end + 2 + size + 2..).unwrap_or(&[]);
    }
    out
}

async fn request(plan: &Plan, url: &url::Url, emit: Emit<'_>) -> Result<Response, Fail> {
    let host = url.host_str().unwrap_or_default().to_string();
    let port = url.port_or_known_default().unwrap_or(80);
    let https = url.scheme() == "https";
    let started = Instant::now();
    let since = || started.elapsed().as_secs_f64();
    let mut times = Times::default();
    let note = |line: String| {
        if plan.verbose {
            emit(line);
        }
    };

    let lookup = host.trim_matches(|c| c == '[' || c == ']').to_string();
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((lookup.as_str(), port)).await.map(|a| a.collect()).unwrap_or_default();
    let addr = *addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).ok_or_else(|| Fail(6, format!("Could not resolve host: {host}")))?;
    times.dns = since();

    note(format!("*   Trying {addr}..."));
    let tcp = match tokio::time::timeout(Duration::from_secs_f64(plan.connect_timeout), TcpStream::connect(addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => return Err(Fail(7, format!("Failed to connect to {host} port {port}: Connection refused"))),
        Ok(Err(e)) => return Err(Fail(7, format!("Failed to connect to {host} port {port}: {e}"))),
        Err(_) => return Err(Fail(28, format!("Failed to connect to {host} port {port}: Connection timed out"))),
    };
    let _ = tcp.set_nodelay(true);
    times.connect = since();
    note(format!("* Connected to {host} ({}) port {port}", addr.ip()));

    let mut stream: Box<dyn Io> = if https {
        let mut config = tls_config(plan.insecure).map_err(|e| Fail(35, e))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let name = ServerName::try_from(lookup.clone()).map_err(|_| Fail(35, "TLS 서버 이름이 올바르지 않습니다".into()))?;
        let tls = match tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await {
            Ok(s) => s,
            Err(e) => {
                let text = e.to_string();
                let cert = text.to_lowercase().contains("certificate") || text.contains("UnknownIssuer") || text.contains("Expired");
                return Err(if cert { Fail(60, format!("SSL certificate problem: {text}")) } else { Fail(35, format!("TLS connect error: {text}")) });
            }
        };
        times.tls = since();
        if let Some((from, to)) = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).and_then(|c| validity(c.as_ref())) {
            note("* Server certificate:".into());
            note(format!("*  start date: {from}"));
            note(format!("*  expire date: {to}"));
            note(if plan.insecure { "*  SSL certificate verify skipped (-k)".into() } else { "*  SSL certificate verify ok.".into() });
        }
        Box::new(tls)
    } else {
        Box::new(tcp)
    };

    let path = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    };
    let host_header = if url.port().is_some() { format!("{host}:{port}") } else { host.clone() };
    let lines = [format!("{} {path} HTTP/1.1", if plan.head { "HEAD" } else { "GET" }), format!("Host: {host_header}"), format!("User-Agent: {}", plan.agent), "Accept: */*".into(), "Connection: close".into()];
    for line in &lines {
        note(format!("> {line}"));
    }
    note("> ".into());
    stream.write_all(format!("{}\r\n\r\n", lines.join("\r\n")).as_bytes()).await.map_err(|e| Fail(55, format!("Send failure: {e}")))?;

    let (head, rest) = read_head(stream.as_mut()).await?;
    times.first = since();
    let status = head.first().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).filter(|_| head[0].starts_with("HTTP/")).ok_or_else(|| Fail(1, "HTTP 응답이 아닙니다".into()))?;
    for line in &head {
        note(format!("< {line}"));
    }
    note("< ".into());
    let body = if plan.head || matches!(status, 204 | 304) { Vec::new() } else { read_body(stream.as_mut(), &head, rest).await };
    Ok(Response { status, head, body, remote: addr, times })
}

/// `-w` 형식: `\n` 같은 표기와 `%{이름}` 변수를 바꾼다.
fn write_out(format: &str, vars: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(i) = rest.find(['\\', '%']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(body) = tail.strip_prefix("%{") {
            if let Some(end) = body.find('}') {
                out.push_str(&vars(&body[..end]));
                rest = &body[end + 1..];
                continue;
            }
        }
        let mut chars = tail.chars();
        let first = chars.next().unwrap_or('\\');
        match (first, chars.next()) {
            ('\\', Some('n')) => out.push('\n'),
            ('\\', Some('t')) => out.push('\t'),
            ('\\', Some('r')) => {}
            ('\\', Some('\\')) => out.push('\\'),
            ('%', Some('%')) => out.push('%'),
            (c, _) => {
                out.push(c);
                rest = &tail[c.len_utf8()..];
                continue;
            }
        }
        rest = &tail[2..];
    }
    out.push_str(rest);
    out
}

pub async fn run(plan: Plan, emit: Emit<'_>) -> Result<i32, String> {
    let started = Instant::now();
    let work = async {
        let mut url = plan.url.clone();
        let mut redirects = 0;
        loop {
            let response = request(&plan, &url, emit).await?;
            let next = header(&response.head, "location").filter(|_| plan.follow && matches!(response.status, 301 | 302 | 303 | 307 | 308)).and_then(|to| url.join(to).ok());
            match next {
                Some(to) if matches!(to.scheme(), "http" | "https") => {
                    redirects += 1;
                    if redirects > MAX_REDIRECTS {
                        return Err(Fail(47, format!("Maximum ({MAX_REDIRECTS}) redirects followed")));
                    }
                    url = to;
                }
                _ => return Ok((response, url, redirects)),
            }
        }
    };
    let outcome = match tokio::time::timeout(Duration::from_secs_f64(plan.max_time), work).await {
        Ok(r) => r,
        Err(_) => Err(Fail(28, format!("Operation timed out after {} milliseconds", (plan.max_time * 1000.0) as u64))),
    };
    let (response, url, redirects) = match outcome {
        Ok(v) => v,
        Err(Fail(code, text)) => {
            emit(format!("curl: ({code}) {}", clean(&text)));
            return Ok(code);
        }
    };
    if plan.head || plan.include {
        for line in &response.head {
            emit(line.clone());
        }
        emit(String::new());
    }
    for line in String::from_utf8_lossy(&response.body).lines().take(2000) {
        emit(clean(line));
    }
    if let Some(format) = &plan.write_out {
        let t = &response.times;
        let total = started.elapsed().as_secs_f64();
        let text = write_out(format, &|name| match name {
            "time_namelookup" => format!("{:.6}", t.dns),
            "time_connect" => format!("{:.6}", t.connect),
            "time_appconnect" => format!("{:.6}", t.tls),
            "time_pretransfer" => format!("{:.6}", t.tls.max(t.connect)),
            "time_starttransfer" => format!("{:.6}", t.first),
            "time_total" => format!("{total:.6}"),
            "http_code" | "response_code" => response.status.to_string(),
            "remote_ip" => response.remote.ip().to_string(),
            "remote_port" => response.remote.port().to_string(),
            "num_redirects" => redirects.to_string(),
            "url_effective" => url.to_string(),
            _ => String::new(),
        });
        for line in text.split('\n') {
            emit(line.to_string());
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::super::ping::tests::{collect, s};
    use super::*;
    use crate::interpret::{Light, interpret};
    use tokio::net::TcpListener;

    #[test]
    fn options() {
        let p = Plan::parse(&s(&["-I", "-v", "-k", "-s", "-m", "10", "-w", "x", "https://example.com/a?b=1"])).unwrap();
        assert!(p.head && p.verbose && p.insecure && !p.follow && p.max_time == 10.0);
        assert_eq!(Plan::parse(&s(&["example.com"])).unwrap().url.as_str(), "http://example.com/");
        for bad in [vec!["-X", "POST", "http://a"], vec!["-d", "x=1", "http://a"], vec!["-o", "f", "http://a"], vec!["-u", "a:b", "http://a"], vec!["ftp://a/"], vec!["http://user:pw@a/"], vec!["-A", "x\ny", "http://a"], vec![], vec!["--http2", "https://a"]] {
            assert!(Plan::parse(&s(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn certificate_dates() {
        // 시험용 인증서 뼈대: Certificate{ tbs{ [0]판, 일련번호, 알고리즘, 발급자, 기간{UTCTime, GeneralizedTime} } }
        let seq = |tag: u8, body: &[u8]| [&[tag, body.len() as u8][..], body].concat();
        let period = seq(0x30, &[seq(0x17, b"200101000000Z"), seq(0x18, b"20991110111520Z")].concat());
        let tbs = seq(0x30, &[seq(0xa0, &[2, 1, 2]), seq(0x02, &[1]), seq(0x30, &[]), seq(0x30, &[]), period.clone()].concat());
        assert_eq!(validity(&seq(0x30, &tbs)).unwrap(), ("Jan  1 00:00:00 2020 GMT".into(), "Nov 10 11:15:20 2099 GMT".into()));
        // 판 번호가 없는 옛 형식
        let old = seq(0x30, &[seq(0x02, &[1]), seq(0x30, &[]), seq(0x30, &[]), period].concat());
        assert!(validity(&seq(0x30, &old)).is_some());
        // 깨진 것은 버틴다
        let good = seq(0x30, &tbs);
        for cut in 0..good.len() {
            let _ = validity(&good[..cut]);
        }
        assert!(validity(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]).is_none());
        assert!(time_text(0x17, b"201301000000Z").is_none());
        assert!(time_text(0x17, b"2001010000Z").is_none());
    }

    #[test]
    fn write_out_and_chunks() {
        let vars = |n: &str| if n == "http_code" { "200".to_string() } else { String::new() };
        assert_eq!(write_out("\\nnetx code=%{http_code} 100%% %{nope}\\n", &vars), "\nnetx code=200 100% \n");
        assert_eq!(write_out("a%b\\q %{open", &vars), "a%b\\q %{open");
        assert_eq!(dechunk(b"5\r\nhello\r\n6;x=1\r\n world\r\n0\r\n\r\n"), b"hello world");
        assert_eq!(dechunk(b"5\r\nhel"), b"hel");
        assert_eq!(dechunk(b"zz\r\nhello"), b"");
        assert_eq!(dechunk(b"ffffffffff\r\nx"), b"");
    }

    /// 가짜 웹 서버: 경로에 따라 답이 다르다.
    async fn server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut c, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = c.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let first = req.lines().next().unwrap_or("").to_string();
                    let reply = if first.contains(" /moved ") {
                        "HTTP/1.1 301 Moved Permanently\r\nLocation: /page\r\nContent-Length: 0\r\n\r\n".to_string()
                    } else if first.contains(" /bad ") {
                        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n".to_string()
                    } else if first.contains(" /loop ") {
                        "HTTP/1.1 302 Found\r\nLocation: /loop\r\n\r\n".to_string()
                    } else if first.starts_with("HEAD ") {
                        "HTTP/1.1 200 OK\r\nServer: fake\x1b[2J\r\nContent-Length: 11\r\n\r\n".to_string()
                    } else {
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nhello \r\n5\r\nworld\r\n0\r\n\r\n".to_string()
                    };
                    let _ = c.write_all(reply.as_bytes()).await;
                });
            }
        });
        port
    }

    async fn curl(args: &[&str]) -> (i32, Vec<String>) {
        let (lines, emit) = collect();
        let code = run(Plan::parse(&s(args)).unwrap(), &emit).await.unwrap();
        let out = lines.lock().unwrap().clone();
        (code, out)
    }

    #[tokio::test]
    async fn requests_against_a_fake_server() {
        let port = server().await;
        let base = format!("http://127.0.0.1:{port}");
        // 레시피와 같은 인자
        let (code, out) = curl(&["-I", "-v", "-k", "-s", "-m", "10", "-w", "\\nnetx_times dns=%{time_namelookup} connect=%{time_connect} tls=%{time_appconnect} ttfb=%{time_starttransfer} total=%{time_total}\\n", &format!("{base}/page")]).await;
        assert_eq!(code, 0);
        assert!(out.contains(&"> HEAD /page HTTP/1.1".to_string()) && out.contains(&"< HTTP/1.1 200 OK".to_string()) && out.contains(&"HTTP/1.1 200 OK".to_string()), "{out:?}");
        // 서버가 보낸 제어 문자는 지운다.
        assert!(out.contains(&"Server: fake?[2J".to_string()), "{out:?}");
        assert!(out.iter().any(|l| l.starts_with("netx_times dns=0.") && l.contains(" tls=0.000000 ")), "{out:?}");
        let r = interpret("curl", &out, crate::engine::StepStatus::Done, Some(0));
        assert_eq!(r.light, Light::Green, "{}", r.text);
        assert_eq!(r.charts.len(), 1);

        // 본문(조각 전송), 넘겨주기 따라가기
        assert_eq!(curl(&["-s", &format!("{base}/page")]).await, (0, vec!["hello world".to_string()]));
        let (_, out) = curl(&["-s", "-L", "-w", "%{http_code} %{num_redirects} %{url_effective}", &format!("{base}/moved")]).await;
        assert_eq!(out, ["hello world".to_string(), format!("200 1 {base}/page")]);
        let (_, out) = curl(&["-s", "-I", &format!("{base}/moved")]).await;
        assert_eq!(out[0], "HTTP/1.1 301 Moved Permanently");
        assert_eq!(curl(&["-s", "-L", &format!("{base}/loop")]).await.0, 47);
        let (_, out) = curl(&["-I", &format!("{base}/bad")]).await;
        assert_eq!(interpret("curl", &out, crate::engine::StepStatus::Done, Some(0)).light, Light::Red);
    }

    #[tokio::test]
    async fn failures_use_curl_codes() {
        let closed = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let (code, out) = curl(&["-s", &format!("http://127.0.0.1:{closed}/")]).await;
        assert_eq!(code, 7);
        assert!(interpret("curl", &out, crate::engine::StepStatus::Done, Some(7)).text.contains("거부"), "{out:?}");
        let (code, out) = curl(&["-s", "http://nope.invalid/"]).await;
        assert_eq!((code, out[0].as_str()), (6, "curl: (6) Could not resolve host: nope.invalid"));
        // 아무 말 없이 끊는 서버, 답하지 않는 서버
        let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = silent.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (c, _) = silent.accept().await.unwrap();
            drop(c);
            let (_held, _) = silent.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        assert!(matches!(curl(&["-s", &format!("http://127.0.0.1:{port}/")]).await.0, 52 | 56));
        let (code, out) = curl(&["-s", "-m", "1", &format!("http://127.0.0.1:{port}/")]).await;
        assert_eq!(code, 28);
        assert!(interpret("curl", &out, crate::engine::StepStatus::Done, Some(28)).text.contains("응답이 없습니다"), "{out:?}");
        // http 주소에 TLS로 말을 걸면 TLS 오류
        let port = server().await;
        assert_eq!(curl(&["-s", "-k", &format!("https://127.0.0.1:{port}/")]).await.0, 35);
    }
}
