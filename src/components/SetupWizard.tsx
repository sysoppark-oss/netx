import { useState } from "react";
import { USER_LEVELS } from "../lib/types";
import type { EnvironmentStatus, UserLevel } from "../lib/types";

/**
 * 초기 설정 마법사. 첫 실행 때 한 번 뜨고, 머리줄의 [기본 진단]/[전체 진단] 배지를 누르면 다시 열린다.
 * NetX가 관리자 권한을 얻거나 WSL을 대신 설치하지 않는다. 설치 명령은 사용자가 직접 실행한다.
 */

const DONE_KEY = "netx.setup.done";
const LEVEL_KEY = "netx.level";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // 저장하지 못해도 쓰는 데는 지장이 없다(다음에 마법사가 다시 뜰 뿐이다).
  }
}

export const setupDone = () => read(DONE_KEY) === "1";
export const markSetupDone = () => write(DONE_KEY, "1");
export const savedLevel = (): UserLevel | null => USER_LEVELS.find((l) => l.id === read(LEVEL_KEY))?.id ?? null;
export const saveLevel = (level: UserLevel) => write(LEVEL_KEY, level);

const STEPS = ["등급", "진단 방식", "AI 연결"];
const WSL_COMMAND = "wsl --install";

interface Props {
  env: EnvironmentStatus | null;
  level: UserLevel;
  missing: string[];
  installing: boolean;
  aiOn: boolean;
  onLevel: (level: UserLevel) => void;
  onRecheck: () => Promise<void>;
  onInstall: () => void;
  onOpenSettings: () => void;
  onClose: () => void;
}

export function SetupWizard({ env, level, missing, installing, aiOn, onLevel, onRecheck, onInstall, onOpenSettings, onClose }: Props) {
  const [step, setStep] = useState(0);
  const [checking, setChecking] = useState(false);
  const [copied, setCopied] = useState(false);
  const full = env?.mode === "full";

  async function recheck() {
    setChecking(true);
    try {
      await onRecheck();
    } finally {
      setChecking(false);
    }
  }

  function copy() {
    void navigator.clipboard
      ?.writeText(WSL_COMMAND)
      .then(() => setCopied(true))
      .catch(() => setCopied(false));
  }

  const button = "rounded-md border border-line px-3 py-1.5 text-sm hover:border-accent hover:text-accent disabled:opacity-50";
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6">
      <div className="flex max-h-full w-full max-w-2xl flex-col rounded-lg border border-line bg-panel">
        <div className="border-b border-line px-6 py-4">
          <h2 className="text-lg font-semibold">NetX 초기 설정</h2>
          <ol className="mt-2 flex gap-4 text-xs">
            {STEPS.map((name, i) => (
              <li key={name} className={i === step ? "font-semibold text-accent" : "text-dim"}>
                {i + 1}. {name}
              </li>
            ))}
          </ol>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 text-sm leading-relaxed">
          {step === 0 && (
            <>
              <p className="text-dim">쓰는 분에 맞춰 보이는 옵션과 설명의 깊이가 달라집니다. 나중에 머리줄의 [등급]에서 바꿀 수 있습니다.</p>
              <ul className="mt-3 space-y-2">
                {USER_LEVELS.map((l) => (
                  <li key={l.id}>
                    <label className={`flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2 ${level === l.id ? "border-accent bg-accent/10" : "border-line"}`}>
                      <input type="radio" name="level" checked={level === l.id} onChange={() => onLevel(l.id)} />
                      <span className="w-32 shrink-0 font-semibold">{l.label}</span>
                      <span className="text-xs text-dim">{l.hint}</span>
                    </label>
                  </li>
                ))}
              </ul>
            </>
          )}

          {step === 1 && (
            <>
              <p className={`rounded-md border px-3 py-2 ${full ? "border-safe/40 text-safe" : "border-caution/40 text-caution"}`}>
                지금 상태: <span className="font-semibold">{env ? (full ? "전체 진단" : "기본 진단") : "확인 중…"}</span>
                {env && <span className="ml-2 text-xs text-dim">{env.message}</span>}
              </p>

              {full ? (
                missing.length > 0 ? (
                  <div className="mt-4">
                    <p>
                      WSL에 아직 없는 도구가 있습니다: <span className="font-mono text-xs text-dim">{missing.join(", ")}</span>
                    </p>
                    <button onClick={onInstall} disabled={installing} title="WSL(Ubuntu)에서 apt로 진단 도구를 설치합니다. root 권한으로 실행됩니다." className={`${button} mt-2`}>
                      {installing ? "설치하는 중…" : "도구 설치"}
                    </button>
                  </div>
                ) : (
                  <p className="mt-4">모든 진단 도구를 쓸 수 있습니다. 더 할 일이 없습니다.</p>
                )
              ) : (
                <div className="mt-4 grid grid-cols-2 gap-4">
                  <section className="rounded-md border border-line p-3">
                    <h3 className="font-semibold">기본 진단으로 바로 쓰기</h3>
                    <p className="mt-1 text-xs text-dim">아무것도 설치하지 않습니다. NetX에 들어 있는 기능으로 진단합니다.</p>
                    <ul className="mt-2 list-disc space-y-0.5 pl-5 text-xs">
                      <li>핑, 경로 추적, 대역 핑</li>
                      <li>DNS 조회, 포트 확인, 웹 응답과 인증서 만료일</li>
                      <li>포트 스캔, 네트워크 구성도, Windows 포트 점유</li>
                      <li>넷엑스 터미널</li>
                    </ul>
                    <p className="mt-2 text-xs text-dim">안 되는 것: 패킷 캡처(tcpdump), 대역폭 측정(iperf3), 일부 세부 옵션. 해당 단계는 "전체 진단 필요"로 표시됩니다.</p>
                  </section>
                  <section className="rounded-md border border-line p-3">
                    <h3 className="font-semibold">전체 진단 준비 (WSL2)</h3>
                    <p className="mt-1 text-xs text-dim">Windows의 리눅스 기능(WSL2)에 진단 도구를 설치합니다. 설치는 직접 하셔야 합니다(관리자 권한, 재부팅 필요).</p>
                    <ol className="mt-2 list-decimal space-y-1 pl-5 text-xs">
                      <li>시작 단추를 오른쪽 클릭 → "터미널(관리자)" 또는 "Windows PowerShell(관리자)"</li>
                      <li>
                        아래 명령을 입력
                        <span className="mt-1 flex items-center gap-2">
                          <code className="rounded bg-void px-2 py-1 font-mono text-ink">{WSL_COMMAND}</code>
                          <button type="button" onClick={copy} className="rounded border border-line px-2 py-0.5 hover:border-accent hover:text-accent">
                            {copied ? "복사됨" : "복사"}
                          </button>
                        </span>
                      </li>
                      <li>PC를 다시 시작하고, 열리는 Ubuntu 창에서 사용자 이름과 암호를 만듭니다.</li>
                      <li>NetX를 켜고 여기서 [다시 확인] → [도구 설치]</li>
                    </ol>
                  </section>
                </div>
              )}
              <button onClick={recheck} disabled={checking} className={`${button} mt-4`}>
                {checking ? "확인하는 중…" : "다시 확인"}
              </button>
            </>
          )}

          {step === 2 && (
            <>
              <p>
                AI를 연결하면 상황을 글로 적었을 때 도구와 옵션을 제안하고, 실행 결과를 요약해 판정합니다. <span className="text-dim">연결하지 않아도 레시피와 직접 명령은 그대로 쓸 수 있습니다.</span>
              </p>
              <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-dim">
                <li>로컬(Ollama): 이 PC나 사내 PC에서 도는 모델. 내용이 밖으로 나가지 않습니다.</li>
                <li>온라인(OpenAI 호환 API): API 키가 필요합니다. 키는 직접 입력하며 Windows 자격 증명 저장소에만 둡니다.</li>
              </ul>
              <p className="mt-4">
                지금 상태: <span className={aiOn ? "font-semibold text-safe" : "font-semibold text-dim"}>{aiOn ? "연결됨" : "연결 안 됨"}</span>
              </p>
              <button onClick={onOpenSettings} className={`${button} mt-2`}>
                AI 설정 열기
              </button>
            </>
          )}
        </div>

        <div className="flex items-center justify-between border-t border-line px-6 py-3">
          <button onClick={onClose} className="text-sm text-dim hover:text-ink">
            건너뛰기
          </button>
          <div className="flex gap-2">
            {step > 0 && (
              <button onClick={() => setStep(step - 1)} className={button}>
                이전
              </button>
            )}
            {step < STEPS.length - 1 ? (
              <button onClick={() => setStep(step + 1)} className="rounded-md bg-accent px-4 py-1.5 text-sm font-semibold text-void">
                다음
              </button>
            ) : (
              <button onClick={onClose} className="rounded-md bg-accent px-4 py-1.5 text-sm font-semibold text-void">
                시작하기
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
