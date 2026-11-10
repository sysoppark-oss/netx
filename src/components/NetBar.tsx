import { useEffect, useRef, useState } from "react";
import { networkInfo } from "../lib/backend";
import type { NetInfo } from "../lib/types";

const POLL_MS = 3000;

function rate(bytesPerSec: number): string {
  const bits = bytesPerSec * 8;
  if (bits >= 1e9) return `${(bits / 1e9).toFixed(2)} Gbps`;
  if (bits >= 1e6) return `${(bits / 1e6).toFixed(1)} Mbps`;
  if (bits >= 1e3) return `${(bits / 1e3).toFixed(0)} Kbps`;
  return `${bits.toFixed(0)} bps`;
}

function size(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

/** 늘 보이는 내 네트워크 정보: 어댑터, 내 주소, 게이트웨이, DNS, 공인 IP, 지금 속도와 누적 사용량. */
export function NetBar() {
  const [info, setInfo] = useState<NetInfo | null>(null);
  const [speed, setSpeed] = useState<{ down: number; up: number } | null>(null);
  const first = useRef<NetInfo | null>(null);
  const last = useRef<NetInfo | null>(null);

  useEffect(() => {
    let alive = true;
    const poll = async () => {
      const now = await networkInfo().catch(() => null);
      if (!alive || !now) return;
      const prev = last.current;
      // 어댑터가 바뀌면 누적 기준을 다시 잡는다.
      if (!first.current || first.current.iface !== now.iface) {
        first.current = now;
        setSpeed(null);
      } else if (prev && now.rx_bytes !== null && now.tx_bytes !== null && prev.rx_bytes !== null && prev.tx_bytes !== null) {
        const secs = (now.at_ms - prev.at_ms) / 1000;
        if (secs > 0.5) {
          setSpeed({
            down: Math.max(0, now.rx_bytes - prev.rx_bytes) / secs,
            up: Math.max(0, now.tx_bytes - prev.tx_bytes) / secs,
          });
        }
      }
      last.current = now;
      setInfo(now);
    };
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  if (!info) return null;
  const base = first.current;
  const used =
    base && info.rx_bytes !== null && info.tx_bytes !== null && base.rx_bytes !== null && base.tx_bytes !== null
      ? { down: Math.max(0, info.rx_bytes - base.rx_bytes), up: Math.max(0, info.tx_bytes - base.tx_bytes) }
      : null;

  const Item = ({ label, value, title }: { label: string; value: string | null; title?: string }) => (
    <span className="flex shrink-0 items-baseline gap-1.5" title={title}>
      <span className="text-dim">{label}</span>
      <span className={`font-mono ${value ? "text-ink" : "text-dim"}`}>{value ?? "확인 불가"}</span>
    </span>
  );

  return (
    <div className="flex items-center gap-x-5 gap-y-1 overflow-x-auto border-b border-line bg-void px-5 py-1.5 text-xs whitespace-nowrap">
      <span className="shrink-0 font-semibold text-accent">내 네트워크</span>
      <Item label="어댑터" value={info.iface ? `${info.iface}${info.link ? ` (${info.link})` : ""}` : null} />
      <Item label="내 IP" value={info.ip ? `${info.ip}${info.prefix !== null ? `/${info.prefix}` : ""}` : null} />
      <Item label="게이트웨이" value={info.gateway} />
      <Item label="DNS" value={info.dns.length > 0 ? info.dns.slice(0, 2).join(", ") : null} title={info.dns.join(", ")} />
      <Item
        label="공인 IP"
        value={info.public_ip}
        title="밖에서 본 내 주소입니다. api.ipify.org에 물어서 확인하며 10분마다 다시 확인합니다."
      />
      <span className="ml-auto flex shrink-0 items-baseline gap-3 font-mono" title="이 어댑터로 지금 주고받는 속도">
        <span className="text-safe">↓ {speed ? rate(speed.down) : "…"}</span>
        <span className="text-caution">↑ {speed ? rate(speed.up) : "…"}</span>
      </span>
      {used && (
        <span className="flex shrink-0 items-baseline gap-1.5" title="NetX를 켠 뒤로 이 어댑터가 주고받은 양">
          <span className="text-dim">사용량</span>
          <span className="font-mono text-ink">
            ↓ {size(used.down)} · ↑ {size(used.up)}
          </span>
        </span>
      )}
    </div>
  );
}
