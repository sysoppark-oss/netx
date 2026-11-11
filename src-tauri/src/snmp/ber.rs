//! SNMP 메시지의 BER 인코딩과 해석. 네트워크를 쓰지 않는 순수 함수만 있다.
//!
//! 만드는 것: v2c GET, GETNEXT 요청. 읽는 것: Response. SET은 만들지 않는다.
//! 장비가 보낸 바이트는 믿지 않는다. 길이와 개수를 모두 확인하고, 벗어나면 오류로 끝낸다.

use super::{Error, Oid, Response, Value, VarBind};

/// 요청 종류. 읽기 요청만 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Get,
    GetNext,
}

const TAG_INTEGER: u8 = 0x02;
const TAG_OCTETS: u8 = 0x04;
const TAG_NULL: u8 = 0x05;
const TAG_OID: u8 = 0x06;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_IP: u8 = 0x40;
const TAG_COUNTER32: u8 = 0x41;
const TAG_GAUGE32: u8 = 0x42;
const TAG_TICKS: u8 = 0x43;
const TAG_OPAQUE: u8 = 0x44;
const TAG_COUNTER64: u8 = 0x46;
const TAG_NO_OBJECT: u8 = 0x80;
const TAG_NO_INSTANCE: u8 = 0x81;
const TAG_END_OF_MIB: u8 = 0x82;
const TAG_GET: u8 = 0xa0;
const TAG_GETNEXT: u8 = 0xa1;
const TAG_RESPONSE: u8 = 0xa2;

/// SNMP v2c의 버전 번호
const VERSION_2C: i64 = 1;
/// 한 응답에서 읽을 변수의 최대 개수
const MAX_BINDINGS: usize = 64;
/// OID 한 개의 최대 마디 수 (RFC 2578)
pub const MAX_OID_ARCS: usize = 128;

// ------------------------------------------------------------------ 만들기

fn put_len(out: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes = (len as u32).to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        out.push(0x80 | (4 - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
}

fn put_tlv(out: &mut Vec<u8>, tag: u8, body: &[u8]) {
    out.push(tag);
    put_len(out, body.len());
    out.extend_from_slice(body);
}

/// 부호 있는 정수를 가장 짧은 2의 보수 바이트로
fn int_bytes(v: i64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let mut at = 0;
    while at < 7 && ((b[at] == 0x00 && b[at + 1] & 0x80 == 0) || (b[at] == 0xff && b[at + 1] & 0x80 != 0)) {
        at += 1;
    }
    b[at..].to_vec()
}

fn oid_bytes(oid: &Oid) -> Result<Vec<u8>, Error> {
    let arcs = oid.arcs();
    if arcs.len() < 2 || arcs.len() > MAX_OID_ARCS || arcs[0] > 2 || (arcs[0] < 2 && arcs[1] > 39) {
        return Err(Error::Malformed("OID 형식이 아닙니다"));
    }
    let first = arcs[0].checked_mul(40).and_then(|v| v.checked_add(arcs[1])).ok_or(Error::Malformed("OID 형식이 아닙니다"))?;
    let mut out = Vec::with_capacity(arcs.len() + 4);
    for arc in std::iter::once(first).chain(arcs[2..].iter().copied()) {
        let mut groups = [0u8; 5];
        let mut n = 0;
        let mut v = arc;
        loop {
            groups[n] = (v & 0x7f) as u8;
            n += 1;
            v >>= 7;
            if v == 0 {
                break;
            }
        }
        for i in (0..n).rev() {
            out.push(groups[i] | if i > 0 { 0x80 } else { 0 });
        }
    }
    Ok(out)
}

/// 읽기 요청 한 개를 만든다. 값 자리는 모두 NULL이다.
pub fn encode_request(kind: Request, community: &[u8], request_id: i32, oids: &[Oid]) -> Result<Vec<u8>, Error> {
    if oids.is_empty() || oids.len() > MAX_BINDINGS {
        return Err(Error::Malformed("한 번에 물을 수 있는 OID는 1개에서 64개입니다"));
    }
    let mut bindings = Vec::new();
    for oid in oids {
        let mut pair = Vec::new();
        put_tlv(&mut pair, TAG_OID, &oid_bytes(oid)?);
        put_tlv(&mut pair, TAG_NULL, &[]);
        put_tlv(&mut bindings, TAG_SEQUENCE, &pair);
    }
    let mut pdu = Vec::new();
    put_tlv(&mut pdu, TAG_INTEGER, &int_bytes(i64::from(request_id)));
    put_tlv(&mut pdu, TAG_INTEGER, &[0]); // error-status
    put_tlv(&mut pdu, TAG_INTEGER, &[0]); // error-index
    put_tlv(&mut pdu, TAG_SEQUENCE, &bindings);

    let mut msg = Vec::new();
    put_tlv(&mut msg, TAG_INTEGER, &int_bytes(VERSION_2C));
    put_tlv(&mut msg, TAG_OCTETS, community);
    put_tlv(&mut msg, if kind == Request::Get { TAG_GET } else { TAG_GETNEXT }, &pdu);

    let mut out = Vec::with_capacity(msg.len() + 4);
    put_tlv(&mut out, TAG_SEQUENCE, &msg);
    Ok(out)
}

// ------------------------------------------------------------------ 읽기

/// 바이트 열을 앞에서부터 읽어 나가는 읽개
struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.buf.is_empty()
    }

    /// (태그, 내용) 하나를 읽는다. 길이가 남은 바이트를 넘거나 길이 형식이 이상하면 오류.
    fn tlv(&mut self) -> Result<(u8, &'a [u8]), Error> {
        const CUT: Error = Error::Malformed("응답이 잘렸습니다");
        let (&tag, rest) = self.buf.split_first().ok_or(CUT)?;
        let (&first, rest) = rest.split_first().ok_or(CUT)?;
        let (len, rest) = if first < 0x80 {
            (usize::from(first), rest)
        } else {
            let n = usize::from(first & 0x7f);
            // 길이 미정(0x80)과 4바이트를 넘는 길이는 받지 않는다.
            if n == 0 || n > 4 || rest.len() < n {
                return Err(Error::Malformed("길이 형식이 올바르지 않습니다"));
            }
            (rest[..n].iter().fold(0usize, |acc, b| (acc << 8) | usize::from(*b)), &rest[n..])
        };
        if rest.len() < len {
            return Err(CUT);
        }
        let (body, tail) = rest.split_at(len);
        self.buf = tail;
        Ok((tag, body))
    }

    fn expect(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], Error> {
        match self.tlv()? {
            (t, body) if t == tag => Ok(body),
            _ => Err(Error::Malformed(what)),
        }
    }
}

fn read_int(body: &[u8]) -> Result<i64, Error> {
    if body.is_empty() || body.len() > 8 {
        return Err(Error::Malformed("정수 길이가 올바르지 않습니다"));
    }
    let start = if body[0] & 0x80 != 0 { -1i64 } else { 0 };
    Ok(body.iter().fold(start, |acc, b| (acc << 8) | i64::from(*b)))
}

/// 부호 없는 정수(Counter, Gauge, TimeTicks). 앞에 0x00이 하나 붙어 9바이트까지 올 수 있다.
fn read_unsigned(body: &[u8]) -> Result<u64, Error> {
    let body = if body.len() > 1 && body[0] == 0 { &body[1..] } else { body };
    if body.is_empty() || body.len() > 8 {
        return Err(Error::Malformed("정수 길이가 올바르지 않습니다"));
    }
    Ok(body.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
}

fn read_oid(body: &[u8]) -> Result<Oid, Error> {
    const BAD: Error = Error::Malformed("OID 형식이 아닙니다");
    if body.is_empty() || body.last().is_some_and(|b| b & 0x80 != 0) {
        return Err(BAD);
    }
    let mut arcs: Vec<u32> = Vec::with_capacity(body.len() + 1);
    let mut v: u32 = 0;
    for b in body {
        if v > u32::MAX >> 7 {
            return Err(BAD);
        }
        v = (v << 7) | u32::from(b & 0x7f);
        if b & 0x80 == 0 {
            if arcs.is_empty() {
                let top = (v / 40).min(2);
                arcs.extend([top, v - top * 40]);
            } else {
                arcs.push(v);
            }
            v = 0;
            if arcs.len() > MAX_OID_ARCS {
                return Err(BAD);
            }
        }
    }
    Ok(Oid::new(arcs))
}

fn read_value(tag: u8, body: &[u8]) -> Result<Value, Error> {
    Ok(match tag {
        TAG_INTEGER => Value::Int(read_int(body)?),
        TAG_OCTETS | TAG_OPAQUE => Value::Bytes(body.to_vec()),
        TAG_NULL => Value::Null,
        TAG_OID => Value::Oid(read_oid(body)?),
        TAG_IP => Value::Ip(<[u8; 4]>::try_from(body).map_err(|_| Error::Malformed("IP 주소 길이가 올바르지 않습니다"))?),
        TAG_COUNTER32 | TAG_GAUGE32 | TAG_TICKS | TAG_COUNTER64 => Value::Unsigned(read_unsigned(body)?),
        TAG_NO_OBJECT | TAG_NO_INSTANCE => Value::NoSuch,
        TAG_END_OF_MIB => Value::EndOfMib,
        _ => return Err(Error::Malformed("모르는 값 종류입니다")),
    })
}

/// 응답 메시지를 읽는다. Response가 아니거나 형식이 어긋나면 오류.
pub fn decode_response(packet: &[u8]) -> Result<Response, Error> {
    let mut outer = Reader { buf: packet };
    let mut msg = Reader { buf: outer.expect(TAG_SEQUENCE, "SNMP 메시지가 아닙니다")? };
    if read_int(msg.expect(TAG_INTEGER, "버전이 없습니다")?)? != VERSION_2C {
        return Err(Error::Malformed("SNMP v2c 응답이 아닙니다"));
    }
    msg.expect(TAG_OCTETS, "커뮤니티 칸이 없습니다")?;
    let mut pdu = Reader { buf: msg.expect(TAG_RESPONSE, "응답(Response)이 아닙니다")? };
    let request_id = read_int(pdu.expect(TAG_INTEGER, "요청 번호가 없습니다")?)?;
    let error_status = read_int(pdu.expect(TAG_INTEGER, "오류 상태가 없습니다")?)?;
    let error_index = read_int(pdu.expect(TAG_INTEGER, "오류 위치가 없습니다")?)?;
    let mut list = Reader { buf: pdu.expect(TAG_SEQUENCE, "변수 목록이 없습니다")? };
    let mut bindings = Vec::new();
    while !list.done() {
        if bindings.len() >= MAX_BINDINGS {
            return Err(Error::Malformed("변수가 너무 많습니다"));
        }
        let mut pair = Reader { buf: list.expect(TAG_SEQUENCE, "변수 형식이 올바르지 않습니다")? };
        let oid = read_oid(pair.expect(TAG_OID, "변수 이름(OID)이 없습니다")?)?;
        let (tag, body) = pair.tlv()?;
        bindings.push(VarBind { oid, value: read_value(tag, body)? });
    }
    Ok(Response { request_id, error_status, error_index, bindings })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn oid(text: &str) -> Oid {
        text.parse().unwrap()
    }

    /// 시험용 응답 만들기: (OID, 값 태그, 값 바이트) 목록으로 Response 메시지를 만든다.
    pub fn response(request_id: i32, status: u8, items: &[(&Oid, u8, Vec<u8>)]) -> Vec<u8> {
        let mut bindings = Vec::new();
        for (o, tag, body) in items {
            let mut pair = Vec::new();
            put_tlv(&mut pair, TAG_OID, &oid_bytes(o).unwrap());
            put_tlv(&mut pair, *tag, body);
            put_tlv(&mut bindings, TAG_SEQUENCE, &pair);
        }
        let mut pdu = Vec::new();
        put_tlv(&mut pdu, TAG_INTEGER, &int_bytes(i64::from(request_id)));
        put_tlv(&mut pdu, TAG_INTEGER, &[status]);
        put_tlv(&mut pdu, TAG_INTEGER, &[0]);
        put_tlv(&mut pdu, TAG_SEQUENCE, &bindings);
        let mut msg = Vec::new();
        put_tlv(&mut msg, TAG_INTEGER, &[1]);
        put_tlv(&mut msg, TAG_OCTETS, b"public");
        put_tlv(&mut msg, TAG_RESPONSE, &pdu);
        let mut out = Vec::new();
        put_tlv(&mut out, TAG_SEQUENCE, &msg);
        out
    }

    /// 시험용 요청 읽기: (종류, 커뮤니티, 요청 번호, 첫 OID)
    pub fn read_request(packet: &[u8]) -> Option<(Request, Vec<u8>, i32, Oid)> {
        let mut outer = Reader { buf: packet };
        let mut msg = Reader { buf: outer.expect(TAG_SEQUENCE, "").ok()? };
        msg.expect(TAG_INTEGER, "").ok()?;
        let community = msg.expect(TAG_OCTETS, "").ok()?.to_vec();
        let (tag, body) = msg.tlv().ok()?;
        let kind = match tag {
            TAG_GET => Request::Get,
            TAG_GETNEXT => Request::GetNext,
            _ => return None,
        };
        let mut pdu = Reader { buf: body };
        let id = read_int(pdu.expect(TAG_INTEGER, "").ok()?).ok()? as i32;
        pdu.expect(TAG_INTEGER, "").ok()?;
        pdu.expect(TAG_INTEGER, "").ok()?;
        let mut list = Reader { buf: pdu.expect(TAG_SEQUENCE, "").ok()? };
        let mut pair = Reader { buf: list.expect(TAG_SEQUENCE, "").ok()? };
        Some((kind, community, id, read_oid(pair.expect(TAG_OID, "").ok()?).ok()?))
    }

    #[test]
    fn get_request_matches_known_bytes() {
        // sysDescr.0을 "public"으로 묻는 요청. 널리 알려진 표본 패킷과 바이트가 같아야 한다.
        let p = encode_request(Request::Get, b"public", 1, &[oid("1.3.6.1.2.1.1.1.0")]).unwrap();
        let want: &[u8] = &[
            0x30, 0x26, 0x02, 0x01, 0x01, 0x04, 0x06, b'p', b'u', b'b', b'l', b'i', b'c', 0xa0, 0x19, 0x02, 0x01, 0x01, 0x02, 0x01, 0x00, 0x02, 0x01, 0x00, 0x30, 0x0e, 0x30, 0x0c, 0x06, 0x08, 0x2b, 0x06, 0x01, 0x02, 0x01, 0x01,
            0x01, 0x00, 0x05, 0x00,
        ];
        assert_eq!(p, want);
        let next = encode_request(Request::GetNext, b"public", 1, &[oid("1.3.6.1.2.1.1.1.0")]).unwrap();
        assert_eq!(next[13], 0xa1);
        assert_eq!(read_request(&next).unwrap(), (Request::GetNext, b"public".to_vec(), 1, oid("1.3.6.1.2.1.1.1.0")));
    }

    #[test]
    fn oids_round_trip_including_large_arcs() {
        // LLDP 표는 "1.0.8802"로 시작한다(8802는 두 바이트로 적힌다).
        for text in ["1.0.8802.1.1.2.1.4.1.1.9.0.5.1", "1.3.6.1.2.1.17.4.3.1.2.224.55.23.244.184.97", "2.999.3", "1.3.6.1.4.1.4294967295", "0.0"] {
            let o = oid(text);
            assert_eq!(read_oid(&oid_bytes(&o).unwrap()).unwrap(), o, "{text}");
        }
        assert_eq!(&oid_bytes(&oid("1.0.8802.1")).unwrap(), &[0x28, 0xc4, 0x62, 0x01]);
        assert!(oid_bytes(&Oid::new(vec![1])).is_err());
        assert!(oid_bytes(&Oid::new(vec![3, 1])).is_err());
        assert!(oid_bytes(&Oid::new(vec![1, 40])).is_err());
        assert!(read_oid(&[]).is_err());
        assert!(read_oid(&[0x2b, 0x86]).is_err(), "마디가 끝나지 않은 OID");
        assert!(read_oid(&[0x2b, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]).is_err(), "32비트를 넘는 마디");
        assert!(read_oid(&[0x01; 200]).is_err(), "마디가 너무 많다");
    }

    #[test]
    fn integers_use_shortest_form() {
        for (v, bytes) in [(0i64, vec![0u8]), (127, vec![0x7f]), (128, vec![0x00, 0x80]), (-1, vec![0xff]), (-129, vec![0xff, 0x7f]), (0x1234_5678, vec![0x12, 0x34, 0x56, 0x78])] {
            assert_eq!(int_bytes(v), bytes, "{v}");
            assert_eq!(read_int(&bytes).unwrap(), v);
        }
        assert_eq!(read_int(&int_bytes(i64::MIN)).unwrap(), i64::MIN);
        assert_eq!(read_unsigned(&[0x00, 0xff, 0xff, 0xff, 0xff]).unwrap(), 4_294_967_295);
        assert!(read_int(&[]).is_err());
        assert!(read_int(&[0; 9]).is_err());
    }

    #[test]
    fn response_values_are_decoded() {
        let name = oid("1.3.6.1.2.1.1.5.0");
        let port = oid("1.3.6.1.2.1.17.4.3.1.2.0.17.34.51.68.85");
        let p = response(
            77,
            0,
            &[
                (&name, TAG_OCTETS, b"sw-office".to_vec()),
                (&port, TAG_INTEGER, vec![7]),
                (&name, TAG_TICKS, vec![0x01, 0x00]),
                (&name, TAG_IP, vec![192, 168, 123, 21]),
                (&name, TAG_OID, vec![0x2b, 0x06]),
                (&name, TAG_END_OF_MIB, vec![]),
                (&name, TAG_NO_INSTANCE, vec![]),
            ],
        );
        let r = decode_response(&p).unwrap();
        assert_eq!((r.request_id, r.error_status, r.bindings.len()), (77, 0, 7));
        assert_eq!(r.bindings[0], VarBind { oid: name.clone(), value: Value::Bytes(b"sw-office".to_vec()) });
        assert_eq!(r.bindings[1].value, Value::Int(7));
        assert_eq!(r.bindings[2].value, Value::Unsigned(256));
        assert_eq!(r.bindings[3].value, Value::Ip([192, 168, 123, 21]));
        assert_eq!(r.bindings[4].value, Value::Oid(oid("1.3.6")));
        assert_eq!(r.bindings[5].value, Value::EndOfMib);
        assert_eq!(r.bindings[6].value, Value::NoSuch);
        assert_eq!(decode_response(&response(-5, 2, &[])).unwrap().error_status, 2);
    }

    #[test]
    fn long_lengths_are_read() {
        // 200바이트 문자열: 길이가 0x81 0xC8 꼴로 적힌다.
        let name = oid("1.3.6.1.2.1.1.1.0");
        let p = response(1, 0, &[(&name, TAG_OCTETS, vec![b'x'; 200])]);
        assert_eq!(p[1], 0x81);
        assert_eq!(decode_response(&p).unwrap().bindings[0].value, Value::Bytes(vec![b'x'; 200]));
    }

    #[test]
    fn broken_packets_are_rejected_without_panicking() {
        let name = oid("1.3.6.1.2.1.1.5.0");
        let good = response(9, 0, &[(&name, TAG_OCTETS, b"sw".to_vec())]);
        assert!(decode_response(&good).is_ok());
        // 어디에서 잘려도 오류로 끝난다.
        for cut in 0..good.len() {
            assert!(decode_response(&good[..cut]).is_err(), "{cut}바이트에서 자른 것");
        }
        // 요청(GET)은 응답이 아니다.
        assert!(decode_response(&encode_request(Request::Get, b"public", 9, &[name.clone()]).unwrap()).is_err());
        // 길이 미정, 지나치게 큰 길이, 버전 다름, 모르는 값 종류
        assert!(decode_response(&[0x30, 0x80, 0x00, 0x00]).is_err());
        assert!(decode_response(&[0x30, 0x85, 1, 1, 1, 1, 1]).is_err());
        assert!(decode_response(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff, 0x02]).is_err());
        let mut v1 = good.clone();
        v1[4] = 0;
        assert!(decode_response(&v1).is_err());
        assert!(decode_response(&response(9, 0, &[(&name, 0x99, vec![1])])).is_err());
        // 아무 바이트나 넣어도 멈추지 않는다.
        for seed in 0u32..2000 {
            let junk: Vec<u8> = (0..64).map(|i| (seed.wrapping_mul(2_654_435_761).rotate_left(i) >> 3) as u8).collect();
            let _ = decode_response(&junk);
        }
        // 변수가 너무 많은 응답
        let many: Vec<(&Oid, u8, Vec<u8>)> = (0..65).map(|_| (&name, TAG_NULL, vec![])).collect();
        assert!(decode_response(&response(9, 0, &many)).is_err());
    }
}
