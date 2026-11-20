import { useDialog } from "../lib/dialog";
import { useEffect, useRef, useState } from "react";
import { wslSetupClear, wslSetupRestart, wslSetupStart, wslSetupStatus, type WslSetup } from "../lib/backend";
import { USER_LEVELS } from "../lib/types";
import type { EnvironmentStatus, UserLevel } from "../lib/types";

/**
 * 초기 설정 마법사. 첫 실행 때 한 번 뜨고, 머리줄의 [기본 진단]/[전체 진단] 배지를 누르면 다시 열린다.
 * 전체 진단을 고르면 NetX가 Windows의 WSL 설치 명령을 띄운다(사용자 결정 2026-11-20). NetX가 관리자 권한을 얻지는 않는다:
 * 승인 창의 [예]는 사용자가 누른다. 다시 시작한 뒤에는 NetX가 한 번 스스로 떠서 Ubuntu 등록과 도구 설치까지 이어 간다.
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
  /** 처음 보일 단계(다시 시작한 뒤 이어 갈 때는 1) */
  initialStep?: number;
}

export function SetupWizard({ env, level, missing, installing, aiOn, onLevel, onRecheck, onInstall, onOpenSettings, onClose, initialStep = 0 }: Props) {
  const dialog = useDialog(onClose);
  const [step, setStep] = useState(initialStep);
  const [wsl, setWsl] = useState<WslSetup | null>(null);
  const [wslError, setWslError] = useState("");
  const [starting, setStarting] = useState(false);
  const [confirmRestart, setConfirmRestart] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const autoTools = useRef(false);

  // 진단 방식 단계에서는 설치가 어디까지 왔는지 5초마다 본다. 전체 진단이 되면 화면의 상태도 다시 읽는다.
  useEffect(() => {
    if (step !== 1) return;
    let alive = true;
    const poll = async () => {
      const now = await wslSetupStatus().catch(() => null);
      if (!alive || !now) return;
      setWsl((prev) => {
        if (now.stage === "ready" && prev && prev.stage !== "ready") void onRecheck();
        return now;
      });
    };
    void poll();
    const t = window.setInterval(poll, 5000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step]);

  // 사용자가 시작한 설치는 끝까지 이어 간다: 전체 진단이 되면 빠진 도구를 한 번 자동으로 깔고, 다 되면 표시를 지운다.
  useEffect(() => {
    if (!wsl?.active || env?.mode !== "full") return;
    if (missing.length > 0 && !installing && !autoTools.current) {
      autoTools.current = true;
      onInstall();
    } else if (missing.length === 0 && !installing) {
      void wslSetupClear().then(() => setWsl((w) => (w ? { ...w, active: false } : w)));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsl?.active, env?.mode, missing.length, installing]);

  async function startWsl() {
    setStarting(true);
    setWslError("");
    try {
      setWsl(await wslSetupStart());
    } catch (e) {
      setWslError(String(e));
    } finally {
      setStarting(false);
    }
  }

  async function restartPc() {
    setRestarting(true);
    setWslError("");
    try {
      await wslSetupRestart();
    } catch (e) {
      setWslError(String(e));
      setRestarting(false);
    }
  }

  async function stopWsl() {
    await wslSetupClear().catch(() => undefined);
    setConfirmRestart(false);
    setWsl(await wslSetupStatus().catch(() => null));
  }
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
    <div {...dialog} aria-label="NetX 초기 설정" className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6">
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
                  <p className="mt-4">전체 진단 준비됨. 모든 진단 도구를 쓸 수 있습니다.</p>
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
                    <h3 className="font-semibold">전체 진단 쓰기 (WSL 설치)</h3>
                    <p className="mt-1 text-xs text-dim">Windows의 리눅스 기능(WSL2)과 Ubuntu, 진단 도구를 NetX가 차례로 설치합니다. 패킷 캡처, 대역폭 측정, 서비스 확인(nmap)까지 쓸 수 있게 됩니다.</p>
                    <ol className="mt-2 list-decimal space-y-0.5 pl-5 text-xs">
                      <li className={wsl?.stage === "none" || !wsl ? "text-ink" : "text-dim"}>[설치 시작] → 승인 창에서 [예] → 검은 설치 창이 끝날 때까지 기다림(몇 분)</li>
                      <li className={wsl?.stage === "restart" ? "text-ink" : "text-dim"}>PC 다시 시작 (NetX가 스스로 다시 뜹니다)</li>
                      <li className={wsl?.stage === "resume" ? "text-ink" : "text-dim"}>Ubuntu와 진단 도구 설치 (자동, 리눅스 계정은 만들지 않습니다)</li>
                    </ol>
                    {wsl && wsl.stage !== "none" && <p className={`mt-2 text-xs ${wsl.stage === "unsupported" ? "text-danger" : "text-caution"}`}>{wsl.message}</p>}
                    {wslError && <p className="mt-2 text-xs text-danger">{wslError}</p>}
                    <div className="mt-2 flex flex-wrap items-center gap-2">
                      {(!wsl || wsl.stage === "none") && (
                        <button type="button" onClick={startWsl} disabled={starting || !wsl} className="rounded-md bg-accent px-3 py-1.5 text-sm font-semibold text-void disabled:opacity-50">
                          {starting ? "승인 창을 기다리는 중…" : "설치 시작"}
                        </button>
                      )}
                      {wsl?.stage === "restart" &&
                        (confirmRestart ? (
                          <>
                            <span className="text-xs text-caution">열어 둔 다른 프로그램의 작업을 저장하셨나요? 20초 뒤 다시 시작합니다.</span>
                            <button type="button" onClick={restartPc} disabled={restarting} className="rounded-md bg-caution px-3 py-1.5 text-sm font-semibold text-void disabled:opacity-50">
                              {restarting ? "다시 시작하는 중…" : "네, 다시 시작"}
                            </button>
                            <button type="button" onClick={() => setConfirmRestart(false)} disabled={restarting} className={button}>
                              아니요
                            </button>
                          </>
                        ) : (
                          <>
                            <button type="button" onClick={() => setConfirmRestart(true)} className="rounded-md bg-accent px-3 py-1.5 text-sm font-semibold text-void">
                              지금 다시 시작
                            </button>
                            <button type="button" onClick={startWsl} disabled={starting} className={button} title="승인 창을 놓쳤거나 설치 창이 바로 닫혔을 때 다시 띄웁니다">
                              설치 다시 띄우기
                            </button>
                          </>
                        ))}
                      {wsl?.stage === "resume" && (
                        <button type="button" onClick={startWsl} disabled={starting} className="rounded-md bg-accent px-3 py-1.5 text-sm font-semibold text-void disabled:opacity-50">
                          {starting ? "승인 창을 기다리는 중…" : "이어서 설치"}
                        </button>
                      )}
                      {wsl?.active && !restarting && (
                        <button type="button" onClick={stopWsl} className="text-xs text-dim hover:text-ink">
                          그만두기
                        </button>
                      )}
                    </div>
                    {wsl?.stage === "resume" && <p className="mt-2 text-xs text-dim">이어서 설치해도 전체 진단이 되지 않으면, PC의 가상화 기능(BIOS의 VT-x/AMD-V)이 꺼져 있을 수 있습니다.</p>}
                    <details className="mt-2 text-xs text-dim">
                      <summary className="cursor-pointer">직접 설치하려면</summary>
                      <p className="mt-1">
                        관리자 터미널에서{" "}
                        <code className="rounded bg-void px-1.5 py-0.5 font-mono text-ink">{WSL_COMMAND}</code>{" "}
                        <button type="button" onClick={copy} className="rounded border border-line px-1.5 hover:border-accent hover:text-accent">
                          {copied ? "복사됨" : "복사"}
                        </button>{" "}
                        을 실행하고 PC를 다시 시작한 뒤, 여기서 [다시 확인] → [도구 설치]를 누릅니다.
                      </p>
                    </details>
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
                <li>온라인(OpenAI 호환 API 또는 Anthropic API): API 키가 필요합니다. 키는 직접 입력하며 Windows 자격 증명 저장소에만 둡니다.</li>
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
