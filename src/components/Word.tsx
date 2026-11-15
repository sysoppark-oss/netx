import { useEffect, useState } from "react";
import { bridgeStatus, getWord, openChurchMail, type BridgeStatus, setWordLang } from "../lib/backend";
import type { AppInfo, Verse, WordView } from "../lib/types";

/**
 * 말씀 표시 (규칙: word/README.md).
 * - 시작 화면, 기다리는 화면(진행 표시 아래), 빈 목록 화면, "정보" 화면에만 쓴다.
 * - 확인 창·승인 창, 경고·오류 알림, 진단 결과와 원본 출력, 내보내는 파일에는 넣지 않는다.
 * - 항상 표시한다(끄는 설정 없음). 본문은 자르지 않고 글자로만 보인다. 몇 번 보였는지 세지 않는다.
 */

/** 한 화면에서 구절을 바꾸는 간격. 10초보다 빨리 바꾸지 않는다. */
const ROTATE_MS = 20_000;

const dateKey = (d: Date) => `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;

// 모든 화면이 같은 순간에 같은 구절을 보이도록 자료와 시작 시각을 함께 쓴다.
let loaded: { key: string; view: Promise<WordView | null> } | null = null;
const startedAt = Date.now();

// 말씀 언어를 바꾸면 열려 있는 화면이 모두 다시 받는다.
const listeners = new Set<() => void>();

function reload() {
  loaded = null;
  listeners.forEach((f) => f());
}

function load(now: Date): Promise<WordView | null> {
  const key = dateKey(now);
  if (loaded?.key !== key) loaded = { key, view: getWord(now).catch(() => null) };
  return loaded.view;
}

/** 말씀 자료. 날짜가 바뀌면 다시 받는다. */
function useWord(): WordView | null {
  const [view, setView] = useState<WordView | null>(null);
  const [day, setDay] = useState(() => dateKey(new Date()));
  const [turn, setTurn] = useState(0);
  useEffect(() => {
    let alive = true;
    void load(new Date()).then((v) => alive && setView(v));
    return () => {
      alive = false;
    };
  }, [day, turn]);
  useEffect(() => {
    const bump = () => setTurn((n) => n + 1);
    listeners.add(bump);
    return () => {
      listeners.delete(bump);
    };
  }, []);
  useEffect(() => {
    const timer = setInterval(() => setDay(dateKey(new Date())), 60_000);
    return () => clearInterval(timer);
  }, []);
  return view;
}

/** 지금 보일 구절: 오늘의 시작 위치에서 차례로 돈다. */
function useWaitingVerse(): { verse: Verse; version: string } | null {
  const view = useWord();
  const [step, setStep] = useState(() => Math.floor((Date.now() - startedAt) / ROTATE_MS));
  useEffect(() => {
    const timer = setInterval(() => setStep(Math.floor((Date.now() - startedAt) / ROTATE_MS)), 1000);
    return () => clearInterval(timer);
  }, []);
  if (!view || view.waiting.length === 0) return null;
  return { verse: view.waiting[(view.start + step) % view.waiting.length], version: view.version };
}

/**
 * 본문, 그 아래 장·절과 번역본 이름.
 * 모양은 통합프로젝트의 「말씀 구절 표시 모양 (1판)」을 따른다: 본문은 기본 글자색으로 한 단계 크게,
 * 왼쪽에 대표색 세로줄(3px, 본문과 12px), 장·절 줄은 작은 대표색 글자. 위 구분선을 두지 않는다.
 * 글자에 상태 색(주의·위험·정상)과 투명도를 쓰지 않는다.
 */
export function WordCard({ className = "" }: { className?: string }) {
  const now = useWaitingVerse();
  if (!now) return null;
  return (
    <figure className={`max-w-[46em] border-l-[3px] border-accent pl-3 text-left ${className}`}>
      <blockquote className="text-[15px] leading-[1.7] break-keep whitespace-pre-line text-ink">{now.verse.text}</blockquote>
      <figcaption className="mt-1 text-[12.5px] text-accent">
        {now.verse.ref} · {now.version}
      </figcaption>
    </figure>
  );
}

/** 글 가운데 전자우편 주소만 누를 수 있게 한다. 누르면 메일 프로그램이 열리는 것까지만 한다. */
function WithMail({ text, email }: { text: string; email: string | null }) {
  const [failed, setFailed] = useState<string | null>(null);
  if (!email || !text.includes(email)) return <>{text}</>;
  const [before, ...rest] = text.split(email);
  return (
    <>
      {before}
      <button type="button" onClick={() => openChurchMail().catch((e) => setFailed(String(e)))} className="underline decoration-dotted hover:text-accent" title="메일 프로그램 열기">
        {email}
      </button>
      {rest.join(email)}
      {failed && <span className="ml-2 text-dim">({failed})</span>}
    </>
  );
}

/** 설정 화면의 "말씀 언어": 바꾸는 항목이다. 끄는 선택지는 없다(word/README.md "언어 선택"). */
export function WordLanguage() {
  const view = useWord();
  const [error, setError] = useState<string | null>(null);
  if (!view) return null;
  return (
    <div className="mt-5 border-t border-line pt-4 text-sm">
      <label className="flex items-center gap-3">
        <span>말씀 언어</span>
        <select
          value={view.lang}
          onChange={(e) => {
            setError(null);
            void setWordLang(e.target.value)
              .then(reload)
              .catch((err) => setError(String(err)));
          }}
          className="rounded-md border border-line bg-raised px-2 py-1"
        >
          {view.languages.map(([code, name]) => (
            <option key={code} value={code}>
              {name}
            </option>
          ))}
        </select>
        <span className="text-xs text-dim">{view.version}</span>
      </label>
      <p className="mt-1 text-xs leading-relaxed text-dim">
        화면 곳곳의 성경 말씀과 "정보" 화면의 문구를 고른 언어로 보입니다. 고르면 바로 바뀌고, 같은 PC의 다른 안디옥 프로그램도 이 값을 함께 씁니다.
      </p>
      {error && <p className="mt-1 text-xs text-danger">{error}</p>}
    </div>
  );
}

/** "정보" 화면: 프로그램 정보, 헌정 문구, [교회 안내] */
export function AboutDialog({ info, onClose }: { info: AppInfo | null; onClose: () => void }) {
  const view = useWord();
  const [church, setChurch] = useState(false);
  const [bridge, setBridge] = useState<BridgeStatus | null>(null);
  useEffect(() => {
    bridgeStatus()
      .then(setBridge)
      .catch(() => {});
  }, []);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <div className="max-h-full w-full max-w-xl overflow-y-auto rounded-lg border border-line bg-panel p-6">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold">정보</h2>
          <p className="font-mono text-xs text-dim">
            NetX v{info?.version ?? "…"}
            {info?.commit ? ` · ${info.commit}` : ""}
            {info?.channel ? ` · ${info.channel} 채널` : ""}
          </p>
        </div>

        {bridge && bridge.state !== "none" && (
          <p className="mt-2 text-xs text-dim">
            다른 프로그램과의 연동 입구:{" "}
            {bridge.state === "open" ? "열림" : bridge.state === "unchecked" ? "열렸지만 응답 확인 실패" : "닫힘"} · 등록된 실행 파일 {bridge.members}개
            {bridge.members === 0 ? " (등록된 실행 파일이 없어 아직 아무도 대상을 넘길 수 없습니다)" : ""}
          </p>
        )}

        {view && (
          <>
            <p className="mt-4 text-sm leading-relaxed whitespace-pre-line">{view.dedication}</p>
            <div className="mt-5 border-t border-line pt-4">
              <button type="button" onClick={() => setChurch((v) => !v)} className="rounded-md border border-line px-3 py-1.5 text-sm hover:border-accent">
                교회 안내 {church ? "▾" : "▸"}
              </button>
              {church && (
                <p className="mt-3 text-sm leading-relaxed whitespace-pre-line">
                  <WithMail text={view.church} email={view.email} />
                </p>
              )}
            </div>
          </>
        )}

        <div className="mt-6 flex justify-end">
          <button onClick={onClose} className="rounded-md border border-line px-4 py-2 text-sm text-dim">
            닫기
          </button>
        </div>
      </div>
    </div>
  );
}
