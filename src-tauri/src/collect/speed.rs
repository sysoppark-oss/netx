//! 예약 수집의 네트워크 속도: 이 PC의 인터넷 내려받기·올리기 속도와 지연을 잰다.
//! 명령을 실행하지 않는다. 코드에 고정된 측정 주소(Cloudflare의 속도 측정 주소)와 HTTPS로 빈 자료를 주고받을 뿐이다.
//! 보내는 것은 0으로 채운 바이트뿐이고, 상대가 보낸 내용은 길이만 세고 버린다.

use std::time::{Duration, Instant};

use super::wire::Speed;

/// 측정 주소. 사용자 입력이나 관제 PC의 예약표로 바꿀 수 없다.
pub const BASE: &str = "https://speed.cloudflare.com";
pub const SERVER: &str = "speed.cloudflare.com";

const DOWN_FIRST: u64 = 10_000_000;
const DOWN_MORE: u64 = 25_000_000;
const UP_FIRST: usize = 5_000_000;
const UP_MORE: usize = 10_000_000;
/// 이보다 빨리 끝나면 너무 짧아 믿기 어려우므로 더 큰 것으로 한 번 더 잰다.
const TOO_SHORT: Duration = Duration::from_millis(1200);
const EACH: Duration = Duration::from_secs(25);
const PINGS: usize = 5;

fn mbps(bytes: u64, took: Duration) -> f64 {
    let secs = took.as_secs_f64().max(0.001);
    (bytes as f64 * 8.0 / 1_000_000.0 / secs * 10.0).round() / 10.0
}

async fn down(client: &reqwest::Client, base: &str, bytes: u64) -> Result<(u64, Duration), String> {
    let mut resp = client.get(format!("{base}/__down?bytes={bytes}")).send().await.map_err(|_| "측정 서버에 닿지 못했습니다")?;
    if !resp.status().is_success() {
        return Err("측정 서버가 거절했습니다".into());
    }
    // 첫 바이트가 온 때부터 잰다(연결 맺는 시간은 뺀다). 받은 것은 길이만 세고 버린다. 약속한 크기보다 더 오면 끊는다.
    let (mut got, mut start) = (0u64, None);
    while let Some(chunk) = resp.chunk().await.map_err(|_| "내려받다가 끊겼습니다")? {
        start.get_or_insert_with(Instant::now);
        got += chunk.len() as u64;
        if got > bytes.saturating_mul(2) {
            break;
        }
    }
    let took = start.map(|s| s.elapsed()).unwrap_or_default();
    if got < bytes / 2 {
        return Err("내려받은 양이 모자랍니다".into());
    }
    Ok((got, took))
}

async fn up(client: &reqwest::Client, base: &str, bytes: usize) -> Result<Duration, String> {
    let start = Instant::now();
    let resp = client.post(format!("{base}/__up")).header("content-type", "application/octet-stream").body(vec![0u8; bytes]).send().await.map_err(|_| "올리다가 끊겼습니다")?;
    if !resp.status().is_success() {
        return Err("측정 서버가 거절했습니다".into());
    }
    Ok(start.elapsed())
}

/// 지연, 내려받기, 올리기 순서로 잰다. 한 번에 15MB쯤, 빠른 회선에서는 50MB쯤 주고받는다.
pub async fn measure(base: &str) -> Result<Speed, String> {
    let client = reqwest::Client::builder().timeout(EACH).build().map_err(|_| "측정을 준비하지 못했습니다")?;
    let mut pings = Vec::new();
    for _ in 0..PINGS {
        let start = Instant::now();
        if let Ok(r) = client.get(format!("{base}/__down?bytes=0")).send().await
            && r.status().is_success()
        {
            pings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    if pings.is_empty() {
        return Err("측정 서버에 닿지 못했습니다".into());
    }
    // 첫 요청에는 연결 맺는 시간이 들어 있으므로 가장 짧은 값을 지연으로 삼는다.
    let latency_ms = (pings.iter().copied().fold(f64::MAX, f64::min) * 10.0).round() / 10.0;

    let (mut got, mut took) = down(&client, base, DOWN_FIRST).await?;
    let mut bytes = got;
    if took < TOO_SHORT {
        (got, took) = down(&client, base, DOWN_MORE).await?;
        bytes += got;
    }
    let down_mbps = mbps(got, took);

    let (mut sent, mut took) = (UP_FIRST, up(&client, base, UP_FIRST).await?);
    bytes += sent as u64;
    if took < TOO_SHORT {
        (sent, took) = (UP_MORE, up(&client, base, UP_MORE).await?);
        bytes += sent as u64;
    }
    let up_mbps = mbps(sent as u64, took);
    Ok(Speed { down_mbps, up_mbps, latency_ms, server: SERVER.into(), bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// 측정 주소를 흉내 내는 작은 서버: GET /__down?bytes=N 은 N바이트, POST /__up 은 본문을 다 읽고 답한다.
    async fn fake() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        if s.read(&mut b).await.unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(b[0]);
                    }
                    let head = String::from_utf8_lossy(&head).to_string();
                    let line = head.lines().next().unwrap_or("").to_string();
                    if line.starts_with("POST") {
                        let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                        let mut left = len;
                        let mut buf = vec![0u8; 65536];
                        while left > 0 {
                            let n = s.read(&mut buf).await.unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            left -= n.min(left);
                        }
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
                    } else {
                        let n: usize = line.split("bytes=").nth(1).and_then(|v| v.split(' ').next()).and_then(|v| v.parse().ok()).unwrap_or(0);
                        let _ = s.write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {n}\r\nconnection: close\r\n\r\n").as_bytes()).await;
                        let _ = s.write_all(&vec![7u8; n]).await;
                    }
                    let _ = s.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn measures_against_a_fake_server_and_fails_cleanly_when_unreachable() {
        let s = measure(&fake().await).await.unwrap();
        assert!(s.down_mbps > 0.0 && s.up_mbps > 0.0 && s.latency_ms >= 0.0);
        // 이 PC 안이라 금방 끝나므로 큰 것으로 한 번씩 더 잰다.
        assert_eq!(s.bytes, DOWN_FIRST + DOWN_MORE + (UP_FIRST + UP_MORE) as u64);
        assert_eq!(s.server, SERVER);
        assert!(measure("http://127.0.0.1:9").await.is_err());
    }

    #[test]
    fn speed_is_megabits_per_second() {
        assert_eq!(mbps(12_500_000, Duration::from_secs(1)), 100.0);
        assert_eq!(mbps(1_250_000, Duration::from_secs(2)), 5.0);
    }
}
