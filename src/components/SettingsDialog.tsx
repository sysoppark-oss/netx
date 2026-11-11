import { useState } from "react";
import { saveSettings, testLlm } from "../lib/backend";
import type { Provider, Settings, SettingsView } from "../lib/types";

const PRESETS: Record<Exclude<Provider, "none">, { base_url: string; model: string; hint: string }> = {
  ollama: {
    base_url: "http://127.0.0.1:11434",
    model: "llama3",
    hint: "Ollama가 돌아가는 PC의 주소입니다. 다른 PC라면 Tailscale 주소를 넣으세요. 인터넷이 끊겨도 동작합니다.",
  },
  openai: {
    base_url: "https://api.openai.com/v1",
    model: "",
    hint: "OpenAI 호환 방식의 API 주소입니다. 서비스가 안내하는 주소와 모델 이름을 넣으세요.",
  },
};

/** AI 연결 설정. API 키는 저장만 하고 다시 보여 주지 않는다. */
export function SettingsDialog({
  initial,
  onClose,
  onSaved,
}: {
  initial: SettingsView;
  onClose: () => void;
  onSaved: (view: SettingsView) => void;
}) {
  const [form, setForm] = useState<Settings>({
    provider: initial.provider,
    base_url: initial.base_url,
    model: initial.model,
    redact: initial.redact,
    manual_update: initial.manual_update,
  });
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(initial.has_key);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  function pickProvider(provider: Provider) {
    setStatus(null);
    if (provider === "none") return setForm({ ...form, provider });
    const preset = PRESETS[provider];
    // 다른 방식으로 바꾸면 주소와 모델을 그 방식의 기본값으로 채운다.
    const changed = provider !== form.provider;
    setForm({
      provider,
      base_url: changed || !form.base_url ? preset.base_url : form.base_url,
      model: changed || !form.model ? preset.model : form.model,
      redact: provider === "openai" ? true : form.redact,
      manual_update: form.manual_update,
    });
  }

  async function save(thenTest: boolean) {
    setBusy(true);
    setStatus(null);
    try {
      const view = await saveSettings(form, apiKey === "" ? null : apiKey);
      setHasKey(view.has_key);
      setApiKey("");
      onSaved(view);
      if (thenTest) setStatus({ ok: true, text: await testLlm() });
      else onClose();
    } catch (e) {
      setStatus({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  }

  async function removeKey() {
    setBusy(true);
    try {
      const view = await saveSettings(form, "");
      setHasKey(view.has_key);
      onSaved(view);
    } catch (e) {
      setStatus({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  }

  const input = "w-full rounded-md border border-line bg-raised px-3 py-2 text-sm outline-none focus:border-accent";
  const active = form.provider !== "none";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <div className="w-full max-w-lg rounded-lg border border-line bg-panel p-6">
        <h2 className="text-lg font-semibold">AI 연결 설정</h2>
        <p className="mt-1 text-xs text-dim">
          AI는 상황을 읽고 도구와 옵션을 제안하며, 실행 결과를 요약합니다. 연결하지 않아도 레시피와 키워드 찾기는 동작합니다.
        </p>

        <div className="mt-5 flex overflow-hidden rounded-md border border-line text-sm">
          {(
            [
              ["none", "사용 안 함"],
              ["ollama", "로컬 Ollama"],
              ["openai", "온라인 API"],
            ] as [Provider, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              onClick={() => pickProvider(id)}
              className={`flex-1 px-3 py-2 ${form.provider === id ? "bg-accent font-semibold text-void" : "bg-raised text-dim"}`}
            >
              {label}
            </button>
          ))}
        </div>

        {active && form.provider !== "none" && (
          <div className="mt-4 space-y-3">
            <p className="text-xs leading-relaxed text-dim">{PRESETS[form.provider].hint}</p>
            <label className="block text-xs text-dim">
              주소
              <input
                value={form.base_url}
                onChange={(e) => setForm({ ...form, base_url: e.target.value })}
                spellCheck={false}
                className={`${input} mt-1 font-mono text-ink`}
              />
            </label>
            <label className="block text-xs text-dim">
              모델 이름
              <input
                value={form.model}
                onChange={(e) => setForm({ ...form, model: e.target.value })}
                spellCheck={false}
                className={`${input} mt-1 font-mono text-ink`}
              />
            </label>

            {form.provider === "openai" && (
              <>
                <label className="block text-xs text-dim">
                  API 키 {hasKey && <span className="text-safe">(저장되어 있음)</span>}
                  <input
                    type="password"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder={hasKey ? "바꾸려면 새 키를 입력하세요" : "키를 입력하세요"}
                    autoComplete="off"
                    className={`${input} mt-1 font-mono text-ink`}
                  />
                </label>
                <p className="text-xs text-dim">
                  키는 Windows 자격 증명 저장소에 보관하며 화면에 다시 표시하지 않습니다.
                  {hasKey && (
                    <button onClick={removeKey} disabled={busy} className="ml-2 text-danger underline">
                      저장된 키 지우기
                    </button>
                  )}
                </p>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={form.redact}
                    onChange={(e) => setForm({ ...form, redact: e.target.checked })}
                  />
                  결과를 보낼 때 내부 IP 주소 가리기
                </label>
                <p className="text-xs leading-relaxed text-dim">
                  온라인 API를 쓰면 입력한 상황 설명과 진단 출력이 그 서비스로 전송됩니다. 내부 IP 가리기는 진단 출력에만
                  적용되고, 상황 설명에 적은 주소는 그대로 전송됩니다.
                </p>
              </>
            )}
          </div>
        )}

        <label className="mt-5 flex items-center gap-2 border-t border-line pt-4 text-sm">
          <input type="checkbox" checked={!form.manual_update} onChange={(e) => setForm({ ...form, manual_update: !e.target.checked })} />
          새 버전이 올라오면 시작할 때 자동으로 설치
        </label>
        <p className="mt-1 text-xs leading-relaxed text-dim">끄면 새 버전이 있다는 알림만 나오고, [지금 업데이트]를 눌러야 설치합니다.</p>

        {status && <p className={`mt-4 text-sm ${status.ok ? "text-safe" : "text-danger"}`}>{status.text}</p>}

        <div className="mt-6 flex justify-end gap-2">
          <button onClick={onClose} className="rounded-md border border-line px-4 py-2 text-sm text-dim">
            닫기
          </button>
          {active && (
            <button onClick={() => save(true)} disabled={busy} className="rounded-md border border-line px-4 py-2 text-sm disabled:opacity-50">
              {busy ? "확인 중…" : "저장하고 연결 시험"}
            </button>
          )}
          <button
            onClick={() => save(false)}
            disabled={busy}
            className="rounded-md bg-accent px-4 py-2 text-sm font-semibold text-void disabled:opacity-50"
          >
            저장
          </button>
        </div>
      </div>
    </div>
  );
}
