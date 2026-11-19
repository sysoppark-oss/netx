// 거점 관제의 연결 표시: 이 PC의 정보가 지금 어디로 나가고 있는지(거점), 거점이 몇 곳 붙어 있는지(관제 PC)를 늘 보인다.
// 보내기도 받기도 꺼져 있으면 아무것도 그리지 않는다. 이름은 이 화면에만 보이고 밖으로 나가지 않는다.
import { useEffect, useState } from "react";
import { collectBoard, collectInventory, collectOverview, timeText, type Board, type Overview } from "../lib/collect";

export type CollectTab = "dashboard" | "send";

const PILL = "flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-0.5 hover:bg-raised";

export function LinkPill({ onOpen }: { onOpen: (tab: CollectTab) => void }) {
  const [ov, setOv] = useState<Overview | null>(null);
  const [board, setBoard] = useState<Board | null>(null);
  const [collecting, setCollecting] = useState(false);

  useEffect(() => {
    let alive = true;
    const poll = async () => {
      const o = await collectOverview().catch(() => null);
      if (!alive || !o) return;
      setOv(o);
      if (o.config.receive) {
        const b = await collectBoard().catch(() => null);
        if (alive && b) setBoard(b);
      }
      if (o.config.send && o.config.collector) {
        const items = await collectInventory().catch(() => []);
        if (alive) setCollecting(items.some((i) => i.on));
      }
    };
    void poll();
    const t = window.setInterval(poll, 5000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);

  if (!ov) return null;
  const c = ov.config;
  const sending = c.send && c.collector;
  if (!sending && !c.receive) return null;

  const pills = [];
  if (sending && c.collector) {
    const hub = c.collector.name || "관제 PC";
    const stopped = !ov.status.linked && (ov.status.send_error ?? "").includes("멈췄");
    const tone = ov.status.linked ? "border-safe/50 text-safe" : stopped ? "border-danger/50 text-danger" : "border-caution/50 text-caution";
    const text = ov.status.linked ? `${c.name} → ${hub} 연결됨` : stopped ? `관제 PC(${hub})가 받기를 멈춤` : `${c.name} → ${hub} 끊김`;
    const title = [
      `관제 PC: ${hub} (${c.collector.addr})`,
      ov.status.linked ? "지금 연결돼 있습니다." : (ov.status.send_error ?? "연결을 다시 거는 중입니다."),
      ov.status.last_sent ? `마지막으로 보낸 때: ${timeText(ov.status.last_sent)}` : "아직 보낸 것이 없습니다.",
      collecting ? "진단 결과와 예약 수집(이 PC의 정보)이 나갑니다." : "진단 결과만 나갑니다.",
      "누르면 보내기 설정이 열립니다.",
    ].join("\n");
    pills.push(
      <button key="send" onClick={() => onOpen("send")} className={`${PILL} ${tone}`} title={title}>
        <span className={ov.status.linked ? "" : "animate-pulse"}>●</span>
        <span>{text}</span>
        {ov.waiting > 0 && <span>· {ov.waiting}건 대기</span>}
        {collecting && <span className="text-dim">· 수집 켬</span>}
      </button>,
    );
  }
  if (c.receive) {
    const sites = (board?.pcs ?? []).filter((p) => !p.own && p.state === "active");
    const live = sites.filter((p) => !p.quiet);
    const listening = !!ov.status.listening;
    const tone = !listening ? "border-danger/50 text-danger" : sites.length > 0 && live.length === sites.length ? "border-safe/50 text-safe" : live.length > 0 ? "border-caution/50 text-caution" : "border-line text-dim";
    const text = !listening ? "관제 PC: 듣지 못함" : sites.length === 0 ? "관제 PC: 거점 없음" : `거점 ${sites.length}곳 중 ${live.length}곳 연결됨`;
    const title = [
      listening ? `듣는 주소: ${ov.status.listening}` : (ov.status.listen_error ?? "테일스케일 주소가 없어 듣지 못합니다."),
      ...sites.map((p) => `${p.name || p.id.slice(0, 8)}: ${p.quiet ? "소식 없음" : "연결됨"} (마지막 소식 ${p.seen ? timeText(p.seen) : "없음"})`),
      "최근 15분 안에 소식이 있으면 연결된 것으로 봅니다. 누르면 대시보드가 열립니다.",
    ].join("\n");
    pills.push(
      <button key="recv" onClick={() => onOpen("dashboard")} className={`${PILL} ${tone}`} title={title}>
        <span>📡</span>
        <span>{text}</span>
      </button>,
    );
  }
  return <span className="sticky right-0 flex shrink-0 items-center gap-2 bg-void pl-3">{pills}</span>;
}
