//! PC 사이 통로: 테일스케일 주소에서만 듣고, TLS 1.3 상호 인증 뒤에 한 줄 JSON을 주고받는다.
//!
//! 보내는 쪽 → 모음함:  pair / hi / alive / dropped / put      모음함 → 보내는 쪽:  답(pair.wait, pair.ok, pair.no, ok, ack, no)뿐.
//! 모음함이 먼저 보내는 줄은 없다. 상호 인증이 끝나고 지문이 짝으로 확인되기 전에는 어떤 자료도 처리하지 않는다.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use tokio_rustls::rustls::crypto::{CryptoProvider, verify_tls13_signature};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use tokio_rustls::rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use tokio_rustls::rustls::{self, ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use super::store::{Taken, Why};
use super::{Collector, Hub, Join, PAIR_WINDOW, key, lock, now_ms, tailnet, wire};

/// 동시에 받는 접속 수
const MAX_CONN: usize = 8;
/// 상호 인증에 줄 시간
const HANDSHAKE: Duration = Duration::from_secs(10);
/// 인증 뒤 첫 줄, 그리고 줄과 줄 사이에 기다리는 시간
const IDLE: Duration = Duration::from_secs(150);
/// 보내는 쪽이 살아 있음을 알리는 간격
const ALIVE_EVERY: Duration = Duration::from_secs(60);
/// 한 줄의 상한(한 건의 상한 + 봉투)
const MAX_LINE: usize = wire::MAX_ITEM_BYTES * 2 + 4096;
const ANSWER: Duration = Duration::from_secs(20);

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// 상대 인증서의 서명만 확인한다(그 개인 키를 가졌는지). 누구인지는 인증 뒤에 지문으로 가린다.
#[derive(Debug)]
struct AnyPeer(Arc<CryptoProvider>);

impl ClientCertVerifier for AnyPeer {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(&self, end: &CertificateDer<'_>, _chain: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, rustls::Error> {
        key::fingerprint(end.as_ref()).map(|_| ClientCertVerified::assertion()).ok_or_else(|| rustls::Error::General("인증서가 아닙니다".into()))
    }
    fn verify_tls12_signature(&self, _m: &[u8], _c: &CertificateDer<'_>, _d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.3만 씁니다".into()))
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// 저장된 지문과 같은 모음함만 받아들인다. 짝 맺는 중(`expect` 없음)에는 서명만 확인하고, 누구인지는 코드 증명과 확인 숫자로 가린다.
#[derive(Debug)]
struct Pinned {
    expect: Option<String>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(&self, end: &CertificateDer<'_>, _chain: &[CertificateDer<'_>], _name: &ServerName<'_>, _ocsp: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        let got = key::fingerprint(end.as_ref()).ok_or_else(|| rustls::Error::General("인증서가 아닙니다".into()))?;
        match &self.expect {
            Some(want) if !key::same(want, &got) => Err(rustls::Error::General("모음함의 지문이 저장된 것과 다릅니다".into())),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }
    fn verify_tls12_signature(&self, _m: &[u8], _c: &CertificateDer<'_>, _d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.3만 씁니다".into()))
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn own_cert(me: &key::Identity) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    (vec![CertificateDer::from(me.cert.clone())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(me.key.clone())))
}

fn server_config(me: &key::Identity) -> Result<ServerConfig, String> {
    let p = provider();
    let (chain, key) = own_cert(me);
    ServerConfig::builder_with_provider(p.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(Arc::new(AnyPeer(p)))
        .with_single_cert(chain, key)
        .map_err(|e| format!("기기 키를 쓰지 못했습니다: {e}"))
}

fn client_config(me: &key::Identity, expect: Option<String>) -> Result<ClientConfig, String> {
    let p = provider();
    let (chain, key) = own_cert(me);
    ClientConfig::builder_with_provider(p.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { expect, provider: p }))
        .with_client_auth_cert(chain, key)
        .map_err(|e| format!("기기 키를 쓰지 못했습니다: {e}"))
}

/// 받아도 되는 상대 주소인지: 테일스케일 대역만. (시험에서는 이 PC 안의 주소도 받는다.)
fn allowed_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => tailnet(v4) || (cfg!(test) && v4.is_loopback()),
        IpAddr::V6(_) => false,
    }
}

/// 이 PC의 테일스케일 주소(있으면)
pub fn tailnet_addr() -> Option<Ipv4Addr> {
    let nets = sysinfo::Networks::new_with_refreshed_list();
    let mut found: Vec<Ipv4Addr> = nets.values().flat_map(|d| d.ip_networks().iter().map(|n| n.addr)).filter_map(|a| if let IpAddr::V4(v4) = a { Some(v4) } else { None }).filter(|a| tailnet(*a)).collect();
    found.sort();
    found.into_iter().next()
}

/// 한 줄을 읽는다. 상한을 넘거나 끊기면 없음.
async fn read_line<R: AsyncRead + Unpin>(reader: &mut BufReader<R>, wait: Duration) -> Option<Value> {
    let mut buf = Vec::new();
    let mut limited = tokio::io::AsyncReadExt::take(&mut *reader, MAX_LINE as u64 + 1);
    let n = timeout(wait, limited.read_until(b'\n', &mut buf)).await.ok()?.ok()?;
    if n == 0 || buf.len() > MAX_LINE || buf.last() != Some(&b'\n') {
        return None;
    }
    serde_json::from_slice(&buf).ok()
}

async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, v: &Value) -> bool {
    let mut line = v.to_string();
    line.push('\n');
    timeout(ANSWER, async { writer.write_all(line.as_bytes()).await.is_ok() && writer.flush().await.is_ok() }).await.unwrap_or(false)
}

fn kind(v: &Value) -> &str {
    v.get("t").and_then(Value::as_str).unwrap_or("")
}

// ================= 모음함(받는 쪽) =================

/// 테일스케일 주소에 묶어 듣는다. 주소가 없거나 묶지 못하면 듣지 않는다(다른 주소로 대신 듣지 않는다).
/// 주소가 바뀌거나 "모음함으로 쓰기"가 꺼지면 닫고, 다시 맞는 주소가 생기면 묶는다.
pub async fn run_listener(hub: Arc<Hub>) {
    let mut last_purge = 0u64;
    loop {
        let config = hub.config();
        if !config.receive {
            set_listening(&hub, None, None);
            tokio::select! { _ = hub.wake.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(30)) => {} }
            continue;
        }
        let now = now_ms();
        if now > last_purge + 3_600_000 {
            hub.inbox.purge(now, config.keep_days);
            last_purge = now;
        }
        let Some(addr) = tailnet_addr() else {
            set_listening(&hub, None, Some("이 PC에 테일스케일 주소가 없습니다. 테일스케일이 켜져 있는지 확인하세요.".into()));
            tokio::select! { _ = hub.wake.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(30)) => {} }
            continue;
        };
        let bind = SocketAddr::new(IpAddr::V4(addr), config.port);
        let listener = match TcpListener::bind(bind).await {
            Ok(l) => l,
            Err(e) => {
                set_listening(&hub, None, Some(format!("{bind}에서 듣지 못했습니다: {e}")));
                tokio::select! { _ = hub.wake.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(30)) => {} }
                continue;
            }
        };
        set_listening(&hub, Some(bind.to_string()), None);
        let serving = serve_on(hub.clone(), listener);
        tokio::pin!(serving);
        loop {
            hub.inbox.mark_listening(now_ms());
            tokio::select! {
                _ = &mut serving => break,
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                _ = hub.wake.notified() => {}
            }
            let c = hub.config();
            if !c.receive || c.port != config.port || tailnet_addr() != Some(addr) {
                break;
            }
        }
    }
}

fn set_listening(hub: &Hub, addr: Option<String>, error: Option<String>) {
    let mut s = lock(&hub.status);
    s.listening = addr;
    s.listen_error = error;
}

/// 열린 소켓에서 접속을 받는다. 접속 수와 인증 시간에 상한이 있다.
pub async fn serve_on(hub: Arc<Hub>, listener: TcpListener) {
    let Ok(config) = server_config(&hub.me) else { return };
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let slots = Arc::new(Semaphore::new(MAX_CONN));
    loop {
        let Ok((tcp, peer)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        };
        if !allowed_peer(peer.ip()) {
            continue;
        }
        let Ok(slot) = slots.clone().try_acquire_owned() else { continue };
        let (hub, acceptor) = (hub.clone(), acceptor.clone());
        tokio::spawn(async move {
            let _slot = slot;
            let Ok(Ok(tls)) = timeout(HANDSHAKE, acceptor.accept(tcp)).await else { return };
            let Some(id) = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).and_then(|c| key::fingerprint(c.as_ref())) else { return };
            let (r, mut w) = tokio::io::split(tls);
            session(&hub, &id, &peer.ip().to_string(), BufReader::new(r), &mut w).await;
            let _ = w.shutdown().await;
        });
    }
}

/// 인증이 끝난 한 접속. `id`는 상대 공개 키의 지문이다.
async fn session<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(hub: &Hub, id: &str, addr: &str, mut reader: BufReader<R>, writer: &mut W) {
    let Some(first) = read_line(&mut reader, HANDSHAKE).await else { return };
    if first.get("v").and_then(Value::as_u64) != Some(u64::from(wire::VERSION)) {
        return;
    }
    match kind(&first) {
        "pair" => {
            let name = first.get("name").and_then(Value::as_str).unwrap_or("");
            let proof = first.get("proof").and_then(Value::as_str).unwrap_or("");
            let Some((reply, _number, decision)) = hub.try_pair(id, name, addr, proof) else {
                let _ = write_line(writer, &json!({"t": "pair.no"})).await;
                return;
            };
            if !write_line(writer, &json!({"t": "pair.wait", "proof": reply, "name": hub.config().name, "port": hub.config().port})).await {
                hub.close_window();
                return;
            }
            // 사람이 [짝 맺기 승인]을 누르기를 기다린다. 창이 닫히거나 시간이 지나면 거절이다.
            let approved = timeout(PAIR_WINDOW, decision).await.ok().and_then(Result::ok).unwrap_or(false);
            let _ = write_line(writer, &json!({"t": if approved { "pair.ok" } else { "pair.no" }})).await;
        }
        "hi" => {
            if let Err(why) = hub.inbox.admitted(id) {
                let _ = write_line(writer, &json!({"t": "no", "why": why.code()})).await;
                return;
            }
            hub.inbox.seen(id, addr, now_ms());
            if !write_line(writer, &json!({"t": "hi", "v": wire::VERSION})).await {
                return;
            }
            while let Some(msg) = read_line(&mut reader, IDLE).await {
                let now = now_ms();
                let answer = match kind(&msg) {
                    "alive" => {
                        if hub.inbox.admitted(id).is_err() {
                            return;
                        }
                        hub.inbox.seen(id, addr, now);
                        json!({"t": "ok"})
                    }
                    "dropped" => {
                        let n = |k: &str| msg.get(k).and_then(Value::as_u64).unwrap_or(0);
                        hub.inbox.dropped(id, n("first"), n("last"), now);
                        json!({"t": "ok"})
                    }
                    "put" => {
                        let seq = msg.get("seq").and_then(Value::as_u64).unwrap_or(0);
                        let body = msg.get("body").and_then(Value::as_str).unwrap_or("");
                        match hub.inbox.take(id, seq, body, now) {
                            Ok(Taken::Stored | Taken::Again) => json!({"t": "ack", "seq": seq}),
                            Ok(Taken::Refused(why)) => {
                                let _ = write_line(writer, &json!({"t": "no", "seq": seq, "why": why.code()})).await;
                                // 지나간 순번은 보내는 쪽이 건너뛰고 이어 보낸다. 그 밖의 거절은 끊는다.
                                if why == Why::Old {
                                    continue;
                                }
                                return;
                            }
                            Err(_) => return,
                        }
                    }
                    // 모르는 줄(2단계의 의뢰 포함)은 받지 않는다.
                    _ => return,
                };
                if !write_line(writer, &answer).await {
                    return;
                }
            }
        }
        _ => {}
    }
}

// ================= 보내는 쪽 =================

async fn connect(hub: &Hub, addr: Ipv4Addr, port: u16, expect: Option<String>) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
    if !allowed_peer(IpAddr::V4(addr)) {
        return Err("모음함 주소가 테일스케일 주소(100.64.0.0/10)가 아닙니다".into());
    }
    let config = client_config(&hub.me, expect)?;
    let tcp = timeout(HANDSHAKE, TcpStream::connect((addr, port))).await.map_err(|_| "모음함에 닿지 않습니다(시간 초과)".to_string())?.map_err(|e| format!("모음함에 닿지 않습니다: {e}"))?;
    let name = ServerName::try_from("netx-collect").map_err(|e| e.to_string())?;
    timeout(HANDSHAKE, TlsConnector::from(Arc::new(config)).connect(name, tcp)).await.map_err(|_| "모음함과의 인증이 끝나지 않았습니다".to_string())?.map_err(|e| format!("모음함과 인증하지 못했습니다: {e}"))
}

/// 보내는 쪽 짝 맺기: 모음함 화면의 코드를 넣는다. 코드가 맞으면 확인 숫자를 돌려주고, 모음함의 승인은 뒤에서 기다린다(`hub.join`에 적힌다).
pub async fn join(hub: Arc<Hub>, addr: &str, port: u16, code: &str) -> Result<Join, String> {
    let ip: Ipv4Addr = addr.trim().parse().map_err(|_| "모음함의 테일스케일 주소(100.x.y.z)를 적어 주세요".to_string())?;
    let code = key::normal_code(code);
    if code.len() < 8 || code.len() > 32 {
        return Err("모음함 화면에 뜬 코드를 그대로 적어 주세요".into());
    }
    let tls = connect(&hub, ip, port, None).await?;
    let collector_id = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).and_then(|c| key::fingerprint(c.as_ref())).ok_or("모음함의 인증서를 읽지 못했습니다")?;
    let mine = hub.me.id();
    let (r, mut w) = tokio::io::split(tls);
    let mut reader = BufReader::new(r);
    let k = {
        let (code, mine, theirs) = (code.clone(), mine.clone(), collector_id.clone());
        tokio::task::spawn_blocking(move || key::pair_key(&code, &mine, &theirs)).await.map_err(|e| e.to_string())?
    };
    if !write_line(&mut w, &json!({"t": "pair", "v": wire::VERSION, "name": hub.config().name, "proof": key::proof(&k, "sender")})).await {
        return Err("모음함에 보내지 못했습니다".into());
    }
    let refused = "모음함이 받지 않았습니다. 모음함에서 [새 PC 짝 맺기]를 눌렀는지, 코드가 맞는지 확인하세요.";
    let answer = read_line(&mut reader, ANSWER).await.ok_or(refused)?;
    // 모음함도 코드를 안다는 증명을 내야 한다(코드를 모르는 가짜 모음함을 가린다).
    if kind(&answer) != "pair.wait" || !key::same(answer.get("proof").and_then(Value::as_str).unwrap_or(""), &key::proof(&k, "collector")) {
        return Err(refused.into());
    }
    let name = wire::text(answer.get("name").and_then(Value::as_str).unwrap_or(""), 40);
    let waiting = Join::Waiting { number: key::confirm_number(&mine, &collector_id), name: name.clone(), addr: ip.to_string() };
    *lock(&hub.join) = waiting.clone();
    tokio::spawn(async move {
        let done = read_line(&mut reader, PAIR_WINDOW + Duration::from_secs(5)).await;
        let _ = w.shutdown().await;
        // 기다리는 동안 사람이 취소했으면(상태가 바뀌었으면) 아무것도 저장하지 않는다.
        if !matches!(&*lock(&hub.join), Join::Waiting { .. }) {
            return;
        }
        let result = match done.as_ref().map(kind) {
            Some("pair.ok") => match hub.remember_collector(Collector { id: collector_id, name: name.clone(), addr: ip.to_string(), port }) {
                Ok(()) => Join::Done { name },
                Err(why) => Join::Failed { why },
            },
            _ => Join::Failed { why: "모음함에서 승인하지 않았습니다".into() },
        };
        *lock(&hub.join) = result;
    });
    Ok(waiting)
}

/// 보내기가 켜져 있는 동안 보낼 함을 비운다. 닿지 않으면 쌓아 두었다가 다시 건다.
pub async fn run_sender(hub: Arc<Hub>) {
    let mut wait = Duration::from_secs(5);
    loop {
        let config = hub.config();
        let target = config.collector.clone().filter(|_| config.send);
        let Some(k) = target else {
            set_link(&hub, false, None);
            tokio::select! { _ = hub.wake.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(60)) => {} }
            continue;
        };
        match send_all(&hub, &k).await {
            Ok(()) => wait = Duration::from_secs(5),
            Err(why) => {
                set_link(&hub, false, Some(why));
                wait = (wait * 2).min(Duration::from_secs(600));
            }
        }
        tokio::select! { _ = hub.wake.notified() => {}, _ = tokio::time::sleep(wait) => {} }
    }
}

fn set_link(hub: &Hub, linked: bool, error: Option<String>) {
    let mut s = lock(&hub.status);
    s.linked = linked;
    s.send_error = error;
}

/// 한 번 접속해, 쌓인 것을 순번대로 보내고 접속을 붙든 채 새 것을 기다린다. 설정이 바뀌거나 끊기면 돌아온다.
async fn send_all(hub: &Hub, k: &Collector) -> Result<(), String> {
    let ip: Ipv4Addr = k.addr.parse().map_err(|_| "모음함 주소가 잘못 저장돼 있습니다".to_string())?;
    let tls = connect(hub, ip, k.port, Some(k.id.clone())).await?;
    let (r, mut w) = tokio::io::split(tls);
    let mut reader = BufReader::new(r);
    let gone = || "모음함과의 연결이 끊겼습니다".to_string();
    if !write_line(&mut w, &json!({"t": "hi", "v": wire::VERSION, "name": hub.config().name})).await {
        return Err(gone());
    }
    let hello = read_line(&mut reader, ANSWER).await.ok_or_else(gone)?;
    if kind(&hello) != "hi" {
        return Err(match hello.get("why").and_then(Value::as_str) {
            Some("stopped") => "모음함에서 이 PC의 짝을 멈췄습니다. 모음함에서 다시 켜야 합니다.".into(),
            Some("unknown") => "모음함이 이 PC를 짝으로 알지 못합니다. 짝을 다시 맺어야 합니다.".into(),
            _ => "모음함이 받지 않았습니다".to_string(),
        });
    }
    set_link(hub, true, None);
    loop {
        if hub.config().collector.as_ref() != Some(k) || !hub.config().send {
            return Ok(());
        }
        if let Some((first, last)) = hub.outbox.dropped() {
            if !write_line(&mut w, &json!({"t": "dropped", "first": first, "last": last})).await || read_line(&mut reader, ANSWER).await.as_ref().map(kind) != Some("ok") {
                return Err(gone());
            }
            hub.outbox.dropped_told();
        }
        while let Some((seq, line)) = hub.outbox.next() {
            if !write_line(&mut w, &json!({"t": "put", "seq": seq, "body": line})).await {
                return Err(gone());
            }
            let answer = read_line(&mut reader, ANSWER).await.ok_or_else(gone)?;
            match (kind(&answer), answer.get("seq").and_then(Value::as_u64)) {
                ("ack", Some(s)) if s == seq => {
                    hub.outbox.done(seq, now_ms());
                    lock(&hub.status).last_sent = now_ms();
                }
                ("no", _) => match answer.get("why").and_then(Value::as_str).unwrap_or("") {
                    // 이 건은 다시 보내도 받지 않는다.
                    "old" | "conflict" | "shape" | "too_big" => {
                        hub.outbox.give_up(seq);
                        if answer.get("why").and_then(Value::as_str) != Some("old") {
                            return Err("모음함이 한 건을 받지 않았습니다(모양 또는 크기)".into());
                        }
                    }
                    "too_fast" => return Err("모음함이 받는 양의 상한에 닿았습니다. 잠시 뒤 다시 보냅니다.".into()),
                    "full" => return Err("모음함에서 이 PC가 쓸 수 있는 저장 공간이 찼습니다.".into()),
                    "stopped" => return Err("모음함에서 이 PC의 짝을 멈췄습니다. 모음함에서 다시 켜야 합니다.".into()),
                    _ => return Err("모음함이 받지 않았습니다".into()),
                },
                _ => return Err(gone()),
            }
        }
        tokio::select! {
            _ = hub.wake.notified() => {}
            _ = tokio::time::sleep(ALIVE_EVERY) => {
                if !write_line(&mut w, &json!({"t": "alive"})).await || read_line(&mut reader, ANSWER).await.as_ref().map(kind) != Some("ok") {
                    return Err(gone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::store::Filter;
    use crate::collect::{PORT_DEFAULT, test_hub};

    fn item(target: &str) -> wire::Finished {
        wire::Finished { kind: "route_latency".into(), title: "라우팅 지연".into(), target: target.into(), verdict: "caution".into(), started: now_ms(), ended: now_ms(), ..Default::default() }
    }

    /// 모음함을 이 PC 안의 주소에 띄운다(시험에서만 허용된다).
    async fn collector(name: &str) -> (Arc<Hub>, u16) {
        let hub = test_hub(name);
        hub.set(false, true, true, name, 90).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve_on(hub.clone(), listener));
        (hub, port)
    }

    async fn until(what: &str, mut f: impl FnMut() -> bool) {
        for _ in 0..400 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("기다리던 일이 일어나지 않았습니다: {what}");
    }

    async fn paired(sender: &Arc<Hub>, home: &Arc<Hub>, port: u16) {
        let code = home.open_window().unwrap().code;
        let Join::Waiting { number, .. } = join(sender.clone(), "127.0.0.1", port, &code).await.unwrap() else { panic!("확인 숫자를 기다려야 한다") };
        until("승인 대기", || home.window_view().pending.is_some()).await;
        assert_eq!(home.window_view().pending.unwrap().number, number, "두 화면의 확인 숫자가 같아야 한다");
        home.decide(true).unwrap();
        until("짝 맺기 끝", || matches!(&*lock(&sender.join), Join::Done { .. })).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairs_then_sends_in_order_and_empties_the_outbox_only_on_ack() {
        let (home, port) = collector("평창").await;
        let sender = test_hub("용인");
        paired(&sender, &home, port).await;
        let k = sender.config().collector.unwrap();
        assert_eq!((k.id.as_str(), k.port, k.name.as_str()), (home.me.id().as_str(), port, "평창"));
        assert!(!sender.config().send, "짝을 맺어도 보내기는 사람이 켠다");

        sender.set(true, true, false, "용인", 90).unwrap();
        sender.submit(item("192.168.123.7"), None).unwrap();
        sender.submit(item("example.com"), None).unwrap();
        tokio::spawn(run_sender(sender.clone()));
        until("두 건 도착", || home.inbox.list(&Filter::default()).1 == 2).await;
        until("보낼 함 비움", || sender.outbox.waiting() == 0).await;
        let (rows, _) = home.inbox.list(&Filter::default());
        // 모음함에 보이는 이름은 승인할 때의 이름이다(나중에 보낸 쪽이 바꿔 말해도 바뀌지 않는다).
        assert!(rows.iter().all(|r| r.pc == sender.me.id() && r.pc_name == "용인-PC" && r.masked));
        assert!(rows.iter().any(|r| r.target == "example.com"));
        assert!(rows.iter().any(|r| r.target.starts_with("내부-")), "내부 주소는 가려서 나간다");
        assert!(sender.outbox.log().iter().all(|s| s.state == "sent"));
        assert!(lock(&sender.status).linked);

        // 붙어 있는 동안 새 건도 간다
        sender.submit(item("1.1.1.1"), None).unwrap();
        until("세 번째 건", || home.inbox.list(&Filter::default()).1 == 3).await;
        // 모음함이 짝을 멈추면 더 받지 않고, 보내는 쪽은 쌓아 둔다
        home.set_pc(&sender.me.id(), "stopped").unwrap();
        sender.submit(item("9.9.9.9"), None).unwrap();
        until("멈춤 알림", || lock(&sender.status).send_error.as_deref().is_some_and(|e| e.contains("멈췄"))).await;
        assert_eq!(home.inbox.list(&Filter::default()).1, 3);
        assert_eq!(sender.outbox.waiting(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wrong_code_closed_window_and_refusal_do_not_pair() {
        let (home, port) = collector("평창").await;
        let sender = test_hub("용인");
        // 창이 닫혀 있다
        assert!(join(sender.clone(), "127.0.0.1", port, "ABCDEFGHJK").await.is_err());
        // 틀린 코드
        home.open_window().unwrap();
        assert!(join(sender.clone(), "127.0.0.1", port, "ZZZZZZZZZZ").await.is_err());
        assert_eq!(home.window_view().wrong, 1);
        // 맞는 코드지만 사람이 거절
        let code = home.window_view().code;
        assert!(matches!(join(sender.clone(), "127.0.0.1", port, &format!(" {} ", code.to_lowercase())).await.unwrap(), Join::Waiting { .. }));
        until("승인 대기", || home.window_view().pending.is_some()).await;
        home.decide(false).unwrap();
        until("거절 전달", || matches!(&*lock(&sender.join), Join::Failed { .. })).await;
        assert!(sender.config().collector.is_none());
        assert!(home.inbox.admitted(&sender.me.id()).is_err());
        // 테일스케일 밖 주소로는 걸지 않는다
        assert!(join(sender.clone(), "8.8.8.8", port, "ABCDEFGHJK").await.unwrap_err().contains("테일스케일"));
        assert!(join(sender.clone(), "collector.example", port, "ABCDEFGHJK").await.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stranger_or_an_impostor_gets_nothing() {
        let (home, port) = collector("평창").await;
        let sender = test_hub("용인");
        paired(&sender, &home, port).await;
        sender.set(true, false, false, "용인", 90).unwrap();

        // 짝이 아닌 PC: 인증은 되지만 hi에서 거절당하고 자료는 저장되지 않는다
        let stranger = test_hub("남");
        stranger.remember_collector(Collector { id: home.me.id(), name: "평창".into(), addr: "127.0.0.1".into(), port }).unwrap();
        stranger.set(true, false, false, "남", 90).unwrap();
        stranger.submit(item("a"), None).unwrap();
        let err = send_all(&stranger, &stranger.config().collector.unwrap()).await.unwrap_err();
        assert!(err.contains("짝으로 알지 못합니다"), "{err}");
        assert_eq!(home.inbox.list(&Filter::default()).1, 0);
        assert_eq!(stranger.outbox.waiting(), 1);

        // 모음함 주소에 다른 키를 가진 가짜가 앉아 있으면 보내지 않는다(자료가 새지 않는다)
        let (fake, fake_port) = collector("가짜").await;
        fake.inbox.pair(&sender.me.id(), "용인", "127.0.0.1", false, now_ms()).unwrap();
        let mut k = sender.config().collector.unwrap();
        k.port = fake_port;
        sender.remember_collector(k.clone()).unwrap();
        sender.submit(item("secret.example"), None).unwrap();
        let err = send_all(&sender, &k).await.unwrap_err();
        assert!(err.contains("인증하지 못했습니다"), "{err}");
        assert_eq!(fake.inbox.list(&Filter::default()).1, 0);
        assert_eq!(sender.outbox.waiting(), 1);
    }

    /// 인증된 짝이 보내는 이상한 줄들: 죽지 않고, 저장하지 않고, 모음함이 먼저 말하지 않는다.
    #[tokio::test]
    async fn session_survives_garbage_and_never_speaks_first() {
        let home = test_hub("평창");
        home.set(false, true, true, "평창", 90).unwrap();
        let id = "f".repeat(64);
        home.inbox.pair(&id, "용인", "100.64.0.2", false, now_ms()).unwrap();
        let good = wire::encode(&item("a"), &id, 1, false, "t").unwrap();
        let cases: Vec<Vec<String>> = vec![
            vec!["not json".into()],
            vec![r#"{"t":"hi","v":2}"#.into()],
            vec![r#"{"t":"hi"}"#.into()],
            vec![r#"{"t":"put","v":1,"seq":1,"body":"x"}"#.into()],
            vec![r#"{"t":"hi","v":1}"#.into(), r#"{"t":"ask","target":"1.1.1.1"}"#.into()],
            vec![r#"{"t":"hi","v":1}"#.into(), r#"{"t":"put","seq":"1","body":7}"#.into()],
            vec![r#"{"t":"hi","v":1}"#.into(), r#"{"t":"dropped","first":-4,"last":"x"}"#.into(), "[1,2".into()],
            vec![r#"{"t":"hi","v":1}"#.into(), format!("{{\"t\":\"put\",\"seq\":1,\"body\":\"{}\"}}", "가".repeat(MAX_LINE))],
            vec![r#"{"t":"hi","v":1}"#.into(), json!({"t": "put", "seq": 1, "body": good}).to_string(), json!({"t": "put", "seq": 1, "body": good}).to_string(), r#"{"t":"alive"}"#.into()],
        ];
        let last = cases.len() - 1;
        for (i, lines) in cases.into_iter().enumerate() {
            let (mut near, far) = tokio::io::duplex(MAX_LINE * 8);
            let (r, mut w) = tokio::io::split(far);
            // 아무것도 보내기 전에는 모음함이 먼저 쓰지 않는다
            let mut peek = [0u8; 1];
            let quiet = timeout(Duration::from_millis(30), tokio::io::AsyncReadExt::read(&mut near, &mut peek));
            {
                let run = session(&home, &id, "100.64.0.2", BufReader::new(r), &mut w);
                tokio::pin!(run);
                tokio::select! { _ = &mut run => panic!("첫 줄 전에 끝났다"), q = quiet => assert!(q.is_err(), "모음함이 먼저 말했다") }
                for line in &lines {
                    near.write_all(line.as_bytes()).await.unwrap();
                    near.write_all(b"\n").await.unwrap();
                }
                near.shutdown().await.unwrap();
                timeout(Duration::from_secs(10), &mut run).await.unwrap_or_else(|_| panic!("{i}번째에서 끝나지 않았다"));
            }
            drop(w);
            let mut out = String::new();
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut near, &mut out).await;
            assert_eq!(home.inbox.list(&Filter::default()).1, u64::from(i == last), "{i}번째");
            if i == last {
                let answers: Vec<Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
                assert_eq!(answers.iter().map(kind).collect::<Vec<_>>(), vec!["hi", "ack", "ack", "ok"]);
            } else {
                assert!(out.lines().all(|l| matches!(kind(&serde_json::from_str::<Value>(l).unwrap()), "hi" | "ok" | "no")), "{i}번째: {out}");
            }
        }
        assert_eq!(home.config().port, PORT_DEFAULT);
    }
}
