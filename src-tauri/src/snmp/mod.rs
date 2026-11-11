//! SNMP v2c 읽기 전용 클라이언트. 장비에 값을 묻고(GET, GETNEXT) "OID → 값"을 돌려준다.
//!
//! 이 모듈은 나중에 공용 크레이트로 떼어 내 "관제" 프로그램이 함께 쓴다. 그래서
//! - NetX의 다른 모듈을 가져다 쓰지 않는다. 의존은 `std`와 `tokio`뿐이다.
//! - 커뮤니티 문자열은 인자로만 받는다. 설정이나 자격 증명 저장소를 읽지 않는다.
//! - 값을 바꾸는 요청(SET)은 만들지 않는다.
//! - 화면에 보여 줄 모양으로 바꾸는 일은 하지 않는다. 그것은 부르는 쪽의 일이다.
//!
//! 장비가 보낸 답은 믿지 않는다(`ber`): 크기 상한, 길이 대조, 요청 번호 대조를 거치고,
//! 어떤 입력에도 죽지 않는다. 답은 요청을 보낸 주소·포트에서 온 것만 받는다(연결된 UDP 소켓).

pub mod ber;

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

use tokio::net::UdpSocket;

pub use ber::Request;

/// 받을 응답의 최대 크기. 이보다 큰 답은 잘려 들어와 해석 단계에서 버려진다.
pub const MAX_RESPONSE_BYTES: usize = 8192;
/// SNMP 표준 포트
pub const PORT: u16 = 161;

/// OID. "1.3.6.1.2.1.1.5.0"처럼 점으로 이은 숫자들이다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid(Vec<u32>);

impl Oid {
    pub fn new(arcs: Vec<u32>) -> Self {
        Oid(arcs)
    }
    pub fn arcs(&self) -> &[u32] {
        &self.0
    }
    /// `root` 아래(같은 것은 제외)에 있으면 `root` 뒤에 붙은 마디들을 돌려준다.
    pub fn suffix_of(&self, root: &Oid) -> Option<&[u32]> {
        self.0.strip_prefix(root.arcs()).filter(|rest| !rest.is_empty())
    }
    /// 뒤에 마디를 더 붙인 OID
    pub fn child(&self, more: &[u32]) -> Oid {
        Oid(self.0.iter().chain(more).copied().collect())
    }
}

impl FromStr for Oid {
    type Err = Error;
    fn from_str(text: &str) -> Result<Self, Error> {
        let arcs: Option<Vec<u32>> = text.trim().trim_start_matches('.').split('.').map(|p| p.parse().ok()).collect();
        arcs.filter(|a| a.len() >= 2 && a.len() <= ber::MAX_OID_ARCS).map(Oid).ok_or(Error::Malformed("OID 형식이 아닙니다"))
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: Vec<String> = self.0.iter().map(u32::to_string).collect();
        f.write_str(&text.join("."))
    }
}

/// 장비가 돌려준 값
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    /// OCTET STRING. 글자일 수도, MAC 주소 같은 바이트일 수도 있다.
    Bytes(Vec<u8>),
    Oid(Oid),
    Ip([u8; 4]),
    /// Counter32, Gauge32, TimeTicks, Counter64
    Unsigned(u64),
    Null,
    /// 그런 항목이 없다(noSuchObject, noSuchInstance)
    NoSuch,
    /// 더 읽을 것이 없다(endOfMibView)
    EndOfMib,
}

impl Value {
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }
    /// 음수가 아닌 정수 값
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(v) => u64::try_from(*v).ok(),
            Value::Unsigned(v) => Some(*v),
            _ => None,
        }
    }
    /// 값이 없다는 표시인지
    pub fn is_missing(&self) -> bool {
        matches!(self, Value::NoSuch | Value::EndOfMib | Value::Null)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarBind {
    pub oid: Oid,
    pub value: Value,
}

/// 해석한 응답 메시지
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub request_id: i64,
    pub error_status: i64,
    pub error_index: i64,
    pub bindings: Vec<VarBind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 요청이나 응답의 형식이 어긋났다
    Malformed(&'static str),
    /// 제한 시간 안에 맞는 답이 오지 않았다(SNMP가 꺼져 있거나 커뮤니티 문자열이 틀리면 이렇게 된다)
    Timeout,
    /// 보내거나 받지 못했다
    Io(String),
    /// 장비가 오류로 답했다(error-status)
    Status(i64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(why) => write!(f, "SNMP 형식 오류: {why}"),
            Error::Timeout => f.write_str("SNMP 응답이 없습니다"),
            Error::Io(why) => write!(f, "SNMP 통신 오류: {why}"),
            Error::Status(code) => write!(f, "장비가 SNMP 오류 {code}로 답했습니다"),
        }
    }
}

impl std::error::Error for Error {}

/// 장비 한 대와 주고받는 통로
pub struct Client {
    sock: UdpSocket,
    community: Vec<u8>,
    timeout: Duration,
    retries: u8,
    next_id: i32,
}

impl Client {
    /// `timeout`은 요청 한 번을 기다리는 시간, `retries`는 답이 없을 때 다시 보내는 횟수다.
    pub async fn connect(addr: SocketAddr, community: &str, timeout: Duration, retries: u8) -> Result<Client, Error> {
        let io = |e: std::io::Error| Error::Io(e.to_string());
        let local: SocketAddr = if addr.is_ipv4() { (Ipv4Addr::UNSPECIFIED, 0).into() } else { (Ipv6Addr::UNSPECIFIED, 0).into() };
        let sock = UdpSocket::bind(local).await.map_err(io)?;
        // 연결해 두면 이 주소·포트에서 온 것만 받는다.
        sock.connect(addr).await.map_err(io)?;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let port = u32::from(sock.local_addr().map(|a| a.port()).unwrap_or(0));
        let next_id = ((nanos ^ port.rotate_left(16)) & 0x3fff_ffff) as i32;
        Ok(Client { sock, community: community.as_bytes().to_vec(), timeout, retries, next_id })
    }

    /// 요청을 보내고 요청 번호가 같은 답을 기다린다. 번호가 다르거나 해석되지 않는 답은 버리고 계속 기다린다.
    async fn ask(&mut self, kind: Request, oids: &[Oid]) -> Result<Vec<VarBind>, Error> {
        self.next_id = if self.next_id >= 0x3fff_ffff { 1 } else { self.next_id + 1 };
        let id = self.next_id;
        let packet = ber::encode_request(kind, &self.community, id, oids)?;
        let mut buf = vec![0u8; MAX_RESPONSE_BYTES];
        for _ in 0..=self.retries {
            self.sock.send(&packet).await.map_err(|e| Error::Io(e.to_string()))?;
            let deadline = tokio::time::Instant::now() + self.timeout;
            loop {
                let got = match tokio::time::timeout_at(deadline, self.sock.recv(&mut buf)).await {
                    Err(_) => break,
                    // 닫힌 포트가 보낸 거절(ICMP)은 여기로 온다. 다시 보내 본다.
                    Ok(Err(_)) => break,
                    Ok(Ok(n)) => buf.get(..n).unwrap_or(&[]),
                };
                let Ok(reply) = ber::decode_response(got) else { continue };
                if reply.request_id != i64::from(id) {
                    continue;
                }
                return if reply.error_status == 0 { Ok(reply.bindings) } else { Err(Error::Status(reply.error_status)) };
            }
        }
        Err(Error::Timeout)
    }

    /// 여러 OID의 값을 한 번에 묻는다.
    pub async fn get(&mut self, oids: &[Oid]) -> Result<Vec<VarBind>, Error> {
        self.ask(Request::Get, oids).await
    }

    /// `oid` 바로 다음 항목을 묻는다.
    pub async fn get_next(&mut self, oid: &Oid) -> Result<VarBind, Error> {
        self.ask(Request::GetNext, std::slice::from_ref(oid)).await?.into_iter().next().ok_or(Error::Malformed("답에 변수가 없습니다"))
    }

    /// `root` 아래의 항목을 차례로 읽는다. `max`개를 넘기지 않는다.
    /// 장비가 `root` 밖의 항목이나 앞선 항목을 돌려주면(끝없이 도는 장비) 거기서 멈춘다.
    pub async fn walk(&mut self, root: &Oid, max: usize) -> Result<Vec<VarBind>, Error> {
        let mut found: Vec<VarBind> = Vec::new();
        let mut at = root.clone();
        while found.len() < max {
            let next = match self.get_next(&at).await {
                Ok(v) => v,
                // v1식으로 "더 없음"을 오류 2(noSuchName)로 알리는 장비가 있다.
                Err(Error::Status(2)) => break,
                // 읽다가 끊기면 읽은 데까지 돌려준다.
                Err(_) if !found.is_empty() => break,
                Err(e) => return Err(e),
            };
            if next.value.is_missing() || next.oid.suffix_of(root).is_none() || next.oid <= at {
                break;
            }
            at = next.oid.clone();
            found.push(next);
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn oid(text: &str) -> Oid {
        text.parse().unwrap()
    }

    #[test]
    fn oid_text_and_order() {
        assert_eq!(oid("1.3.6.1.2.1.1.5.0").to_string(), "1.3.6.1.2.1.1.5.0");
        assert_eq!(oid(".1.3.6").arcs(), [1, 3, 6]);
        for bad in ["", "1", "1.x.3", "1..3", "1.3.-1", "1.3.99999999999"] {
            assert!(bad.parse::<Oid>().is_err(), "{bad}");
        }
        let root = oid("1.3.6.1.2.1.17.4.3.1.2");
        assert_eq!(root.child(&[0, 17, 34]).suffix_of(&root), Some(&[0u32, 17, 34][..]));
        assert_eq!(root.suffix_of(&root), None);
        assert_eq!(oid("1.3.6.1.2.1.17.4.3.1.3.1").suffix_of(&root), None);
        // 사전식 순서: 표를 읽을 때 "앞으로만 간다"를 확인하는 데 쓴다.
        assert!(oid("1.3.6.1.2") < oid("1.3.6.1.2.0") && oid("1.3.6.1.2.9") < oid("1.3.6.1.10"));
    }

    /// 시험용 가짜 장비. `table`의 값으로 GET과 GETNEXT에 답한다. 커뮤니티가 다르면 답하지 않는다.
    /// `quirk`: 0 정상, 1 끝에서 오류 2로 답함, 2 같은 항목을 계속 돌려줌, 3 요청 번호가 틀린 답을 먼저 보냄
    pub async fn fake_agent(table: BTreeMap<Oid, (u8, Vec<u8>)>, community: &'static str, quirk: u8) -> SocketAddr {
        let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = sock.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((n, from)) = sock.recv_from(&mut buf).await else { return };
                let Some((kind, comm, id, asked)) = ber::tests::read_request(&buf[..n]) else { continue };
                if comm != community.as_bytes() {
                    continue;
                }
                let hit = match kind {
                    Request::Get => table.get_key_value(&asked),
                    Request::GetNext if quirk == 2 => table.iter().next(),
                    Request::GetNext => table.range((std::ops::Bound::Excluded(asked.clone()), std::ops::Bound::Unbounded)).next(),
                };
                let reply = match hit {
                    Some((o, (tag, body))) => ber::tests::response(id, 0, &[(o, *tag, body.clone())]),
                    None if quirk == 1 => ber::tests::response(id, 2, &[(&asked, 0x05, vec![])]),
                    None if kind == Request::Get => ber::tests::response(id, 0, &[(&asked, 0x81, vec![])]),
                    None => ber::tests::response(id, 0, &[(&asked, 0x82, vec![])]),
                };
                if quirk == 3 {
                    let _ = sock.send_to(&ber::tests::response(id.wrapping_add(7), 0, &[(&asked, 0x04, b"wrong".to_vec())]), from).await;
                    let _ = sock.send_to(b"\x30\x03junk", from).await;
                }
                let _ = sock.send_to(&reply, from).await;
            }
        });
        addr
    }

    fn sample() -> BTreeMap<Oid, (u8, Vec<u8>)> {
        let mut t = BTreeMap::new();
        t.insert(oid("1.3.6.1.2.1.1.1.0"), (0x04, b"Managed Switch 8-port".to_vec()));
        t.insert(oid("1.3.6.1.2.1.1.5.0"), (0x04, b"sw-office".to_vec()));
        t.insert(oid("1.3.6.1.2.1.17.4.3.1.2.0.17.34.51.68.85"), (0x02, vec![3]));
        t.insert(oid("1.3.6.1.2.1.17.4.3.1.2.136.54.108.0.0.1"), (0x02, vec![8]));
        t.insert(oid("1.3.6.1.2.1.17.4.3.1.3.0.17.34.51.68.85"), (0x02, vec![3]));
        t
    }

    const QUICK: Duration = Duration::from_millis(300);

    #[tokio::test]
    async fn get_and_walk_against_a_fake_agent() {
        let addr = fake_agent(sample(), "public", 0).await;
        let mut c = Client::connect(addr, "public", QUICK, 1).await.unwrap();
        let got = c.get(&[oid("1.3.6.1.2.1.1.5.0")]).await.unwrap();
        assert_eq!(got[0].value.as_bytes(), Some(&b"sw-office"[..]));
        assert_eq!(c.get(&[oid("1.3.6.1.2.1.1.9.0")]).await.unwrap()[0].value, Value::NoSuch);
        // 표 읽기: 뿌리 아래 두 줄만, 옆 표(…1.3)로 넘어가지 않는다.
        let root = oid("1.3.6.1.2.1.17.4.3.1.2");
        let rows = c.walk(&root, 100).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].oid.suffix_of(&root), Some(&[0u32, 17, 34, 51, 68, 85][..]));
        assert_eq!(rows[1].value.as_u64(), Some(8));
        assert_eq!(c.walk(&root, 1).await.unwrap().len(), 1, "개수 상한");
        assert!(c.walk(&oid("1.3.6.1.2.1.99"), 100).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn wrong_community_or_silent_device_times_out() {
        let addr = fake_agent(sample(), "secret", 0).await;
        let mut c = Client::connect(addr, "public", Duration::from_millis(120), 1).await.unwrap();
        assert_eq!(c.get(&[oid("1.3.6.1.2.1.1.5.0")]).await, Err(Error::Timeout));
        // 듣는 프로그램이 없는 포트
        let closed = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap().local_addr().unwrap();
        let mut c = Client::connect(closed, "public", Duration::from_millis(120), 0).await.unwrap();
        assert_eq!(c.get(&[oid("1.3.6.1.2.1.1.5.0")]).await, Err(Error::Timeout));
    }

    #[tokio::test]
    async fn odd_agents_do_not_trap_the_walk() {
        let root = oid("1.3.6.1.2.1.17.4.3.1");
        // 끝을 오류 2로 알리는 장비
        let mut c = Client::connect(fake_agent(sample(), "public", 1).await, "public", QUICK, 0).await.unwrap();
        assert_eq!(c.walk(&root, 100).await.unwrap().len(), 3);
        // 같은 항목만 돌려주는 장비: 끝없이 돌지 않는다.
        let mut c = Client::connect(fake_agent(sample(), "public", 2).await, "public", QUICK, 0).await.unwrap();
        assert!(c.walk(&oid("1.3.6.1.2.1.1"), 100).await.unwrap().len() <= 1);
        // 요청 번호가 틀린 답과 깨진 답이 먼저 와도 맞는 답을 고른다.
        let mut c = Client::connect(fake_agent(sample(), "public", 3).await, "public", QUICK, 0).await.unwrap();
        assert_eq!(c.get(&[oid("1.3.6.1.2.1.1.5.0")]).await.unwrap()[0].value.as_bytes(), Some(&b"sw-office"[..]));
    }
}
