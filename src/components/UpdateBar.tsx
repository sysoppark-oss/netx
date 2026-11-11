import { useEffect, useState } from "react";
import { applyUpdate, onUpdateStatus } from "../lib/backend";
import type { UpdateStatus } from "../lib/types";

/** 새 버전 알림 줄. 시작 직후에는 자동으로 설치되고, 쓰는 도중에 새 버전이 올라오면 버튼으로 설치한다. */
export function UpdateBar() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => onUpdateStatus(setStatus), []);
  if (!status) return null;

  async function apply() {
    setBusy(true);
    try {
      await applyUpdate();
    } catch (e) {
      setStatus({ state: "error", commit: "", message: String(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={`flex items-center gap-3 border-b border-line px-7 py-1.5 text-xs ${status.state === "error" ? "text-danger" : "text-accent"}`}>
      <span>
        {status.state === "error" ? `업데이트하지 못했습니다: ${status.message}` : status.message}
        {status.commit && <span className="ml-2 font-mono text-dim">{status.commit}</span>}
      </span>
      {status.state === "available" && (
        <button type="button" onClick={apply} disabled={busy} className="rounded border border-accent px-2 py-0.5 disabled:opacity-50">
          지금 업데이트
        </button>
      )}
      {(status.state === "available" || status.state === "error") && (
        <button type="button" onClick={() => setStatus(null)} className="ml-auto text-dim">
          닫기
        </button>
      )}
    </div>
  );
}
