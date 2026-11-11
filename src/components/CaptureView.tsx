// 캡처 분석 화면: 저장해 둔 캡처 파일을 열어 요약, 문제 찾기, 패킷 목록, 패킷 상세를 본다.
// 진단 결과 화면이므로 말씀 표시를 넣지 않는다. 읽기 전용이다.
import { useEffect, useRef, useState } from "react";
import {
  captureFolder,
  captureList,
  captureOpen,
  capturePacket,
  captureRows,
  type Analysis,
  type CaptureFile,
  type PacketDetail,
  type PacketPage,
} from "../lib/capture";

const PAGE = 200;

const MARK_LABEL: Record<string, string> = {
  retransmit: "재전송",
  reset: "연결 끊음",
  unanswered: "답 없음",
  "dns-error": "DNS 실패",
  "dns-unanswered": "DNS 답 없음",
  unreachable: "ICMP 오류",
  "zero-window": "버퍼 가득",
};

const MARK_TONE: Record<string, string> = {
  retransmit: "text-caution",
  reset: "text-caution",
  unanswered: "text-danger",
  "dns-error": "text-danger",
  "dns-unanswered": "text-caution",
  unreachable: "text-caution",
  "zero-window": "text-caution",
};

function bytesText(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

function whenText(secs: number): string {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  const two = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

export function CaptureView({ initial, onClose }: { initial: string; onClose: () => void }) {
  const [files, setFiles] = useState<CaptureFile[] | null>(null);
  const [name, setName] = useState(initial);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [mark, setMark] = useState("");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<PacketPage | null>(null);
  const [detail, setDetail] = useState<PacketDetail | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  // 늦게 온 응답이 새 조건의 화면을 덮지 않게 한다.
  const ask = useRef(0);

  useEffect(() => {
    captureList()
      .then((list) => {
        setFiles(list);
        setName((now) => now || list[0]?.name || "");
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    setAnalysis(null);
    setPage(null);
    setDetail(null);
    setError(null);
    setText("");
    setMark("");
    setOffset(0);
    if (!name) return;
    let live = true;
    captureOpen(name)
      .then((a) => live && setAnalysis(a))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [name]);

  useEffect(() => {
    if (!analysis) return;
    const mine = ++ask.current;
    const timer = setTimeout(() => {
      captureRows(analysis.name, text, mark, offset, PAGE)
        .then((p) => mine === ask.current && setPage(p))
        .catch((e) => mine === ask.current && setError(String(e)));
    }, 200);
    return () => clearTimeout(timer);
  }, [analysis, text, mark, offset]);

  function filterBy(nextText: string, nextMark: string) {
    setText(nextText);
    setMark(nextMark);
    setOffset(0);
    setDetail(null);
  }

  function showPacket(no: number) {
    if (!analysis) return;
    capturePacket(analysis.name, no)
      .then(setDetail)
      .catch((e) => setError(String(e)));
  }

  const maxPackets = Math.max(...(analysis?.protocols.map((p) => p.packets) ?? [1]), 1);

  return (
    <div className="fixed inset-0 z-40 flex flex-col bg-void">
      <header className="flex flex-wrap items-center gap-3 border-b border-line bg-panel px-4 py-2">
        <h1 className="text-base font-semibold">📂 캡처 분석</h1>
        <select
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="min-w-64 rounded border border-line bg-raised px-2 py-1 font-mono text-xs text-ink outline-none focus:border-accent"
          aria-label="캡처 파일"
        >
          {!files?.length && <option value="">저장된 캡처 없음</option>}
          {name && files && !files.some((f) => f.name === name) && <option value={name}>{name}</option>}
          {files?.map((f) => (
            <option key={f.name} value={f.name}>
              {whenText(f.modified)} · {bytesText(f.size)} · {f.name}
            </option>
          ))}
        </select>
        <button
          onClick={() => captureFolder().then(setFolder).catch((e) => setError(String(e)))}
          title="캡처 파일이 있는 폴더를 엽니다. Wireshark로 열어 보고 싶을 때 씁니다."
          className="rounded border border-line px-2.5 py-1 text-xs hover:bg-raised"
        >
          폴더 열기
        </button>
        {folder && <span className="truncate font-mono text-xs text-dim">{folder}</span>}
        <span className="flex-1" />
        <button onClick={onClose} className="rounded border border-line px-3 py-1 text-xs hover:bg-raised">
          닫기
        </button>
      </header>

      {error && <p className="border-b border-danger/40 bg-danger/10 px-4 py-2 text-sm text-danger">{error}</p>}

      {files && files.length === 0 && !name ? (
        <div className="flex flex-1 items-center justify-center p-8 text-center text-sm text-dim">
          <p>
            저장된 캡처가 없습니다.
            <br />
            왼쪽 메뉴의 "트래픽 퀵 캡처"로 먼저 패킷을 캡처하세요.
          </p>
        </div>
      ) : !analysis ? (
        <div className="flex flex-1 items-center justify-center text-sm text-dim">{error ? "" : "캡처 파일을 읽는 중입니다…"}</div>
      ) : (
        <div className="flex min-h-0 flex-1">
          {/* 요약 */}
          <aside className="w-80 shrink-0 space-y-4 overflow-y-auto border-r border-line bg-panel p-3 text-xs">
            <dl className="grid grid-cols-2 gap-x-3 gap-y-1.5">
              <dt className="text-dim">패킷</dt>
              <dd className="font-mono">{analysis.packets.toLocaleString()}개</dd>
              <dt className="text-dim">크기</dt>
              <dd className="font-mono">{bytesText(analysis.bytes)}</dd>
              <dt className="text-dim">걸린 시간</dt>
              <dd className="font-mono">{analysis.seconds.toFixed(1)}초</dd>
              <dt className="text-dim">시작</dt>
              <dd className="font-mono">{whenText(analysis.started)}</dd>
              <dt className="text-dim">잡은 곳</dt>
              <dd>{analysis.link}</dd>
            </dl>

            <section>
              <h2 className="mb-1.5 font-semibold">문제 찾기</h2>
              {analysis.findings.length === 0 ? (
                <p className="rounded border border-safe/40 bg-safe/10 px-2 py-1.5 leading-relaxed">
                  🟢 눈에 띄는 문제가 없습니다. 재전송, 연결 거부, 답 없는 접속, DNS 실패가 보이지 않습니다.
                </p>
              ) : (
                <ul className="space-y-1.5">
                  {analysis.findings.map((f) => (
                    <li key={f.mark}>
                      <button
                        onClick={() => filterBy("", mark === f.mark ? "" : f.mark)}
                        title="누르면 이 문제에 해당하는 패킷만 봅니다."
                        className={`w-full rounded border px-2 py-1.5 text-left leading-relaxed hover:bg-raised ${
                          f.light === "red" ? "border-danger/50 bg-danger/10" : "border-caution/50 bg-caution/10"
                        } ${mark === f.mark ? "ring-1 ring-accent" : ""}`}
                      >
                        <span className="font-semibold">
                          {f.light === "red" ? "🔴" : "🟡"} {f.title}
                        </span>
                        <span className="mt-0.5 block text-dim">{f.text}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {analysis.notes.map((n, i) => (
                <p key={i} className="mt-1.5 leading-relaxed text-dim">
                  ※ {n}
                </p>
              ))}
            </section>

            <section>
              <h2 className="mb-1.5 font-semibold">프로토콜</h2>
              <ul className="space-y-1">
                {analysis.protocols.map((p) => (
                  <li key={p.label}>
                    <button onClick={() => filterBy(p.label.includes(" ") ? "" : p.label.toLowerCase(), "")} className="flex w-full items-center gap-2 text-left hover:text-accent">
                      <span className="w-16 shrink-0 truncate font-mono">{p.label}</span>
                      <span className="h-2 flex-1 overflow-hidden rounded bg-raised">
                        <span className="block h-full rounded bg-accent/70" style={{ width: `${(p.packets / maxPackets) * 100}%` }} />
                      </span>
                      <span className="w-14 shrink-0 text-right font-mono text-dim">{p.packets.toLocaleString()}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>

            {analysis.talks.length > 0 && (
              <section>
                <h2 className="mb-1.5 font-semibold">많이 통신한 상대</h2>
                <ul className="space-y-1">
                  {analysis.talks.map((t) => (
                    <li key={`${t.a} ${t.b}`}>
                      <button onClick={() => filterBy(`${t.a} ${t.b}`, "")} className="w-full text-left hover:text-accent">
                        <span className="block truncate font-mono">
                          {t.a} ↔ {t.b}
                        </span>
                        <span className="text-dim">
                          {t.packets.toLocaleString()}개 · {bytesText(t.bytes)}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            )}
          </aside>

          {/* 패킷 목록과 상세 */}
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="flex flex-wrap items-center gap-3 border-b border-line px-3 py-2 text-xs">
              <input
                value={text}
                onChange={(e) => filterBy(e.target.value, mark)}
                placeholder="거르기: 주소, 포트, 프로토콜, 글자 (예: tcp 443 192.168.0.1)"
                spellCheck={false}
                className="w-96 max-w-full rounded border border-line bg-raised px-2 py-1 font-mono text-ink outline-none focus:border-accent"
              />
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={mark === "any"} onChange={(e) => filterBy(text, e.target.checked ? "any" : "")} />
                문제만 보기
              </label>
              {mark && mark !== "any" && (
                <button onClick={() => filterBy(text, "")} className="rounded border border-accent/60 px-2 py-0.5 text-accent hover:bg-raised">
                  {MARK_LABEL[mark] ?? mark} ✕
                </button>
              )}
              <span className="flex-1" />
              {page && (
                <span className="flex items-center gap-2 text-dim">
                  {page.total === 0 ? "맞는 패킷 없음" : `${(offset + 1).toLocaleString()}~${(offset + page.rows.length).toLocaleString()} / ${page.total.toLocaleString()}개`}
                  <button disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))} className="rounded border border-line px-2 py-0.5 text-ink hover:bg-raised disabled:opacity-40">
                    이전
                  </button>
                  <button
                    disabled={offset + PAGE >= page.total}
                    onClick={() => setOffset(offset + PAGE)}
                    className="rounded border border-line px-2 py-0.5 text-ink hover:bg-raised disabled:opacity-40"
                  >
                    다음
                  </button>
                </span>
              )}
            </div>

            <div className="min-h-0 flex-1 overflow-auto">
              <table className="w-full border-collapse font-mono text-xs">
                <thead className="sticky top-0 bg-panel text-left text-dim">
                  <tr>
                    <th className="px-2 py-1.5 text-right font-normal">번호</th>
                    <th className="px-2 py-1.5 text-right font-normal">시각(초)</th>
                    <th className="px-2 py-1.5 font-normal">보낸 곳</th>
                    <th className="px-2 py-1.5 font-normal">받는 곳</th>
                    <th className="px-2 py-1.5 font-normal">프로토콜</th>
                    <th className="px-2 py-1.5 text-right font-normal">길이</th>
                    <th className="px-2 py-1.5 font-normal">설명</th>
                  </tr>
                </thead>
                <tbody>
                  {page?.rows.map((r) => (
                    <tr
                      key={r.no}
                      onClick={() => showPacket(r.no)}
                      className={`cursor-pointer border-t border-line/50 hover:bg-raised ${detail?.no === r.no ? "bg-raised" : ""}`}
                    >
                      <td className="px-2 py-1 text-right text-dim">{r.no}</td>
                      <td className="px-2 py-1 text-right text-dim">{r.time.toFixed(4)}</td>
                      <td className="max-w-48 truncate px-2 py-1">{r.src}</td>
                      <td className="max-w-48 truncate px-2 py-1">{r.dst}</td>
                      <td className="px-2 py-1 text-accent">{r.proto}</td>
                      <td className="px-2 py-1 text-right text-dim">{r.len}</td>
                      <td className="px-2 py-1 whitespace-nowrap">
                        {r.mark && <span className={`mr-2 font-sans font-semibold ${MARK_TONE[r.mark] ?? "text-caution"}`}>[{MARK_LABEL[r.mark] ?? r.mark}]</span>}
                        {r.info}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {detail && (
              <div className="flex h-72 shrink-0 border-t border-line bg-panel text-xs">
                <div className="w-1/2 overflow-auto p-3">
                  <div className="mb-2 flex items-center gap-3">
                    <h2 className="font-semibold">패킷 {detail.no}</h2>
                    <span className="text-dim">
                      {detail.len}바이트{detail.captured < detail.len ? ` (저장된 것은 앞 ${detail.captured}바이트)` : ""}
                    </span>
                    <span className="flex-1" />
                    <button onClick={() => setDetail(null)} className="rounded border border-line px-2 py-0.5 hover:bg-raised">
                      접기
                    </button>
                  </div>
                  {detail.layers.map((l, i) => (
                    <section key={i} className="mb-2">
                      <h3 className="font-semibold text-accent">{l.name}</h3>
                      <dl className="mt-0.5 grid grid-cols-[9rem_1fr] gap-x-2 gap-y-0.5 pl-2">
                        {l.fields.map((f, j) => (
                          <div key={j} className="contents">
                            <dt className="text-dim">{f.name}</dt>
                            <dd className="font-mono break-all">{f.value}</dd>
                          </div>
                        ))}
                      </dl>
                    </section>
                  ))}
                </div>
                <pre className="w-1/2 overflow-auto border-l border-line bg-void p-3 font-mono leading-5">
                  {detail.hex.join("\n")}
                  {detail.cut ? "\n… (뒤는 줄였습니다)" : ""}
                </pre>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
