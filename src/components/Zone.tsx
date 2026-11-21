// 대시보드의 구역 하나: 번호와 제목이 적힌 띠 + 내용. 구역마다 같은 모양을 쓴다.
import type { ReactNode } from "react";

export function Zone({
  code,
  title,
  note,
  right,
  bare,
  flush,
  className,
  children,
}: {
  code: string;
  title: string;
  /** 제목 옆에 적는 짧은 설명 */
  note?: string;
  /** 띠 오른쪽에 놓는 것(찾기 칸, 개수 등) */
  right?: ReactNode;
  /** 틀 없이 띠만 그린다(안의 카드가 저마다 틀을 가질 때) */
  bare?: boolean;
  /** 내용에 안쪽 여백을 두지 않는다(지도, 표) */
  flush?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={`${bare ? "" : "hud"} flex min-w-0 flex-col ${className ?? ""}`}>
      <header className={`flex flex-wrap items-center gap-x-2 gap-y-1 border-b border-accent/25 py-1.5 ${bare ? "" : "px-3"}`}>
        <span className="font-mono text-[10px] tracking-[0.18em] text-accent" aria-hidden>
          {code}
        </span>
        <h2 className="text-xs font-semibold tracking-wide">{title}</h2>
        {note && <span className="text-[11px] text-dim">{note}</span>}
        <span className="flex-1" />
        {right}
      </header>
      <div className={`min-h-0 min-w-0 flex-1 ${bare ? "pt-3" : flush ? "" : "p-3"}`}>{children}</div>
    </section>
  );
}
