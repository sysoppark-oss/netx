import { useEffect, useRef, useState } from "react";
import { Terminal as Xterm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { WordCard } from "./components/Word";
import { attach, detach, onPreset, startListening, takePreset, termClose, termFolder, termOpen, termPorts, termResize, termWrite } from "./lib/term";
import type { TermKind, TermSpec } from "./lib/term";

/**
 * 넷엑스 터미널 창. 탭마다 접속 하나(시리얼, SSH, Telnet, FTP, TFTP).
 * 암호 칸은 없다. 암호는 접속한 뒤 터미널 안의 프롬프트에 직접 친다.
 * 말씀 표시는 접속이 하나도 없는 빈 화면에만 둔다(터미널 출력에는 섞지 않는다).
 */

const KINDS: { id: TermKind; label: string; port: number; hint: string; warn?: string }[] = [
  { id: "ssh", label: "SSH", port: 22, hint: "Windows에 들어 있는 OpenSSH로 접속합니다. 암호와 호스트 키 확인은 접속 뒤 터미널 안에서 합니다." },
  { id: "telnet", label: "Telnet", port: 23, hint: "오래된 장비용.", warn: "Telnet은 암호를 포함한 모든 글자가 암호화되지 않고 전달됩니다." },
  { id: "serial", label: "시리얼", port: 0, hint: "콘솔 케이블(USB-시리얼)로 스위치·라우터에 접속합니다. 보통 9600 또는 115200, 8-N-1." },
  { id: "ftp", label: "FTP", port: 21, hint: "파일은 NetX 전송 폴더 안에서만 주고받습니다. 암호는 접속 뒤 프롬프트에 입력합니다.", warn: "FTP는 암호를 포함한 모든 내용이 암호화되지 않고 전달됩니다." },
  { id: "tftp", label: "TFTP", port: 69, hint: "장비 설정·펌웨어를 주고받는 클라이언트입니다. 파일은 NetX 전송 폴더 안에서만 주고받습니다.", warn: "TFTP는 암호도 암호화도 없습니다. 믿을 수 있는 망에서만 쓰세요." },
];

const BAUDS = [300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

function titleOf(spec: TermSpec): string {
  const kind = KINDS.find((k) => k.id === spec.kind)?.label ?? spec.kind;
  if (spec.kind === "serial") return `${kind} ${spec.port}`;
  if (spec.kind === "ssh") return `${kind} ${spec.user}@${spec.host}`;
  return `${kind} ${spec.host}`;
}

/**
 * NetX의 보안 설정(CSP)은 스크립트가 만든 <style>을 막는다. xterm은 색과 글자 폭을 <style>로 넣으므로,
 * 그 내용을 브라우저의 스타일 객체(CSSOM)로 옮겨 적용한다. CSP는 그대로 둔다.
 */
function mirrorStyles(root: HTMLElement): () => void {
  const sheets = new Map<Element, { sheet: CSSStyleSheet; text: string }>();
  const drop = (sheet: CSSStyleSheet) => {
    document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== sheet);
  };
  const sync = () => {
    for (const el of root.querySelectorAll("style")) {
      let entry = sheets.get(el);
      if (!entry) {
        entry = { sheet: new CSSStyleSheet(), text: "" };
        sheets.set(el, entry);
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, entry.sheet];
      }
      const text = el.textContent ?? "";
      if (text !== entry.text) {
        entry.text = text;
        entry.sheet.replaceSync(text);
      }
    }
    for (const [el, entry] of sheets) {
      if (!el.isConnected) {
        drop(entry.sheet);
        sheets.delete(el);
      }
    }
  };
  const isStyle = (n: Node | null) => n?.nodeName === "STYLE";
  const watch = new MutationObserver((list) => {
    if (list.some((m) => isStyle(m.target) || isStyle(m.target.parentNode) || [...m.addedNodes, ...m.removedNodes].some(isStyle))) sync();
  });
  watch.observe(root, { childList: true, subtree: true, characterData: true });
  sync();
  return () => {
    watch.disconnect();
    for (const entry of sheets.values()) drop(entry.sheet);
  };
}

interface Tab {
  key: number;
  spec: TermSpec;
  state: "open" | "ended";
}

let nextKey = 1;

export default function TerminalApp() {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<number | "new">("new");
  const [folder, setFolder] = useState<string | null>(null);

  function add(spec: TermSpec) {
    const key = nextKey++;
    setTabs((t) => [...t, { key, spec, state: "open" }]);
    setActive(key);
  }

  function remove(key: number) {
    const rest = tabs.filter((x) => x.key !== key);
    setTabs(rest);
    if (active === key) setActive(rest.length > 0 ? rest[rest.length - 1].key : "new");
  }

  // 스캔 결과의 [터미널 접속] 단추에서 넘어온 접속
  useEffect(() => {
    let alive = true;
    const take = () =>
      takePreset()
        .then((p) => {
          if (!alive || !p) return;
          if (p.kind === "ssh") {
            if (p.user) add({ kind: "ssh", host: p.host, port: p.port, user: p.user });
          } else if (p.kind === "ftp") add({ kind: "ftp", host: p.host, port: p.port, user: p.user });
          else add({ kind: p.kind, host: p.host, port: p.port });
        })
        .catch(() => {});
    take();
    const off = onPreset(take);
    return () => {
      alive = false;
      void off.then((f) => f());
    };
  }, []);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-1 border-b border-line bg-panel px-3 pt-2">
        <span className="mr-2 pb-2 text-sm font-bold text-accent">NetX 터미널</span>
        <div className="flex min-w-0 flex-1 items-end gap-1 overflow-x-auto">
          {tabs.map((t) => (
            <div
              key={t.key}
              className={`flex shrink-0 items-center gap-2 rounded-t-md border border-b-0 px-3 py-1.5 text-xs ${active === t.key ? "border-line bg-void text-ink" : "border-transparent text-dim hover:text-ink"}`}
            >
              <button type="button" onClick={() => setActive(t.key)} className="flex items-center gap-1.5">
                <span className={`h-1.5 w-1.5 rounded-full ${t.state === "open" ? "bg-safe" : "bg-dim"}`} />
                {titleOf(t.spec)}
              </button>
              <button type="button" onClick={() => remove(t.key)} title="접속을 닫습니다" className="text-dim hover:text-danger">
                ✕
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() => setActive("new")}
            className={`shrink-0 rounded-t-md border border-b-0 px-3 py-1.5 text-xs ${active === "new" ? "border-line bg-void text-ink" : "border-transparent text-dim hover:text-ink"}`}
          >
            + 새 접속
          </button>
        </div>
        <button
          type="button"
          onClick={() => termFolder().then(setFolder).catch((e) => setFolder(String(e)))}
          title={folder ?? "FTP·TFTP로 주고받는 파일이 있는 폴더를 엽니다"}
          className="mb-1.5 shrink-0 rounded border border-line px-2 py-1 text-xs text-dim hover:border-accent hover:text-accent"
        >
          전송 폴더 열기
        </button>
      </header>

      <div className="relative min-h-0 flex-1">
        {tabs.map((t) => (
          <Session
            key={t.key}
            spec={t.spec}
            visible={active === t.key}
            onState={(state) => setTabs((all) => all.map((x) => (x.key === t.key ? { ...x, state } : x)))}
          />
        ))}
        {active === "new" && <NewConnection onOpen={add} empty={tabs.length === 0} />}
      </div>
    </div>
  );
}

/** 접속 하나의 화면. 탭을 닫으면(언마운트) 접속도 닫는다. */
function Session({ spec, visible, onState }: { spec: TermSpec; visible: boolean; onState: (state: "open" | "ended") => void }) {
  const box = useRef<HTMLDivElement>(null);
  const term = useRef<Xterm | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const id = useRef<number | null>(null);
  const [ended, setEnded] = useState<string | null>(null);
  const [round, setRound] = useState(0);
  const stateRef = useRef(onState);
  stateRef.current = onState;

  // 화면(xterm)은 한 번만 만든다.
  useEffect(() => {
    const x = new Xterm({
      fontFamily: '"D2Coding", "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace',
      fontSize: 14,
      cursorBlink: true,
      scrollback: 5000,
      theme: { background: "#0b1120", foreground: "#e2e8f0", cursor: "#2dd4bf", selectionBackground: "#2dd4bf55" },
    });
    const f = new FitAddon();
    x.loadAddon(f);
    x.open(box.current!);
    const unmirror = mirrorStyles(box.current!);
    // Ctrl+V는 붙여넣기, 고른 글자가 있을 때 Ctrl+C(또는 Ctrl+Shift+C)는 복사. 그 밖에는 그대로 접속으로 보낸다.
    x.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey) return true;
      const key = e.key.toLowerCase();
      if (key === "v") return false;
      if (key === "c" && (e.shiftKey || x.hasSelection())) {
        void navigator.clipboard?.writeText(x.getSelection()).catch(() => {});
        x.clearSelection();
        return false;
      }
      return true;
    });
    x.onData((data) => {
      if (id.current != null) void termWrite(id.current, data);
    });
    x.onResize(({ cols, rows }) => {
      if (id.current != null) void termResize(id.current, cols, rows);
    });
    term.current = x;
    fit.current = f;
    const watch = new ResizeObserver(() => {
      if (box.current && box.current.offsetWidth > 0) f.fit();
    });
    watch.observe(box.current!);
    return () => {
      watch.disconnect();
      unmirror();
      x.dispose();
      term.current = null;
    };
  }, []);

  // 접속. [다시 접속]을 누르면 같은 화면에서 새로 연다.
  useEffect(() => {
    let alive = true;
    let opened: number | null = null;
    const x = term.current!;
    setEnded(null);
    stateRef.current("open");
    const finish = (message: string) => {
      if (!alive) return;
      id.current = null;
      x.write(`\r\n\x1b[2m[${message}]\x1b[0m\r\n`);
      setEnded(message);
      stateRef.current("ended");
    };
    startListening()
      .then(() => {
        fit.current?.fit();
        return termOpen(spec, x.cols, x.rows);
      })
      .then((n) => {
        opened = n;
        if (!alive) {
          void termClose(n);
          detach(n);
          return;
        }
        id.current = n;
        attach(n, { data: (bytes) => x.write(bytes), end: finish });
      })
      .catch((e) => finish(String(e)));
    return () => {
      alive = false;
      id.current = null;
      if (opened != null) {
        void termClose(opened);
        detach(opened);
      }
    };
  }, [round, spec]);

  useEffect(() => {
    if (!visible) return;
    fit.current?.fit();
    term.current?.focus();
  }, [visible]);

  return (
    <div className={`absolute inset-0 flex-col bg-void ${visible ? "flex" : "hidden"}`}>
      <div ref={box} className="min-h-0 flex-1 px-2 pt-2" />
      {ended != null && (
        <div className="flex items-center gap-3 border-t border-line bg-panel px-3 py-2 text-xs">
          <span className="min-w-0 flex-1 truncate text-dim">접속이 끝났습니다: {ended.split("\n")[0]}</span>
          <button type="button" onClick={() => setRound((r) => r + 1)} className="rounded border border-line px-3 py-1 hover:border-accent hover:text-accent">
            다시 접속
          </button>
        </div>
      )}
    </div>
  );
}

const field = "rounded border border-line bg-raised px-2 py-1.5 font-mono text-sm text-ink outline-none focus:border-accent";

function NewConnection({ onOpen, empty }: { onOpen: (spec: TermSpec) => void; empty: boolean }) {
  const [kind, setKind] = useState<TermKind>("ssh");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [user, setUser] = useState("");
  const [ports, setPorts] = useState<string[] | null>(null);
  const [serial, setSerial] = useState({ port: "", baud: 9600, data_bits: 8, parity: "none", stop_bits: 1, flow: "none" });
  const [error, setError] = useState<string | null>(null);
  const info = KINDS.find((k) => k.id === kind)!;

  function pick(k: TermKind) {
    setKind(k);
    setError(null);
    const next = KINDS.find((x) => x.id === k)!;
    if (next.port) setPort(String(next.port));
    if (k === "serial") refresh();
  }

  function refresh() {
    termPorts()
      .then((list) => {
        setPorts(list);
        setSerial((s) => (list.includes(s.port) ? s : { ...s, port: list[0] ?? "" }));
      })
      .catch(() => setPorts([]));
  }

  function submit() {
    const p = Number(port);
    const h = host.trim();
    const u = user.trim();
    if (kind === "serial") {
      if (!serial.port) return setError("시리얼 포트를 고르세요. 케이블을 꽂은 뒤 [다시 찾기]를 누르세요.");
      return onOpen({ kind, ...serial });
    }
    if (!h) return setError("주소를 입력하세요.");
    if (!Number.isInteger(p) || p < 1 || p > 65535) return setError("포트는 1~65535입니다.");
    if (kind === "ssh") {
      if (!u) return setError("SSH 사용자 이름을 입력하세요.");
      return onOpen({ kind, host: h, port: p, user: u });
    }
    if (kind === "ftp") return onOpen({ kind, host: h, port: p, user: u || null });
    onOpen({ kind, host: h, port: p });
  }

  return (
    <div className="absolute inset-0 overflow-y-auto p-6">
      <form
        className="mx-auto max-w-xl rounded-lg border border-line bg-panel p-5"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <h1 className="text-base font-semibold">새 접속</h1>
        <div className="mt-3 flex overflow-hidden rounded-md border border-line text-sm">
          {KINDS.map((k) => (
            <button key={k.id} type="button" onClick={() => pick(k.id)} className={`flex-1 px-3 py-1.5 ${kind === k.id ? "bg-accent font-semibold text-void" : "bg-raised text-dim"}`}>
              {k.label}
            </button>
          ))}
        </div>
        <p className="mt-3 text-xs leading-relaxed text-dim">{info.hint}</p>
        {info.warn && <p className="mt-1 text-xs leading-relaxed text-caution">⚠ {info.warn}</p>}

        {kind === "serial" ? (
          <div className="mt-4 grid grid-cols-2 gap-3 text-xs text-dim">
            <label className="col-span-2 flex flex-col gap-1">
              포트
              <span className="flex gap-2">
                <select value={serial.port} onChange={(e) => setSerial({ ...serial, port: e.target.value })} className={`${field} flex-1`}>
                  {(ports ?? []).length === 0 && <option value="">{ports == null ? "찾는 중…" : "시리얼 포트가 없습니다"}</option>}
                  {(ports ?? []).map((p) => (
                    <option key={p}>{p}</option>
                  ))}
                </select>
                <button type="button" onClick={refresh} className="rounded border border-line px-3 hover:border-accent hover:text-accent">
                  다시 찾기
                </button>
              </span>
            </label>
            <label className="flex flex-col gap-1">
              속도 (bps)
              <select value={serial.baud} onChange={(e) => setSerial({ ...serial, baud: Number(e.target.value) })} className={field}>
                {BAUDS.map((b) => (
                  <option key={b}>{b}</option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              데이터 비트
              <select value={serial.data_bits} onChange={(e) => setSerial({ ...serial, data_bits: Number(e.target.value) })} className={field}>
                {[8, 7, 6, 5].map((b) => (
                  <option key={b}>{b}</option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              패리티
              <select value={serial.parity} onChange={(e) => setSerial({ ...serial, parity: e.target.value })} className={field}>
                <option value="none">없음</option>
                <option value="even">짝수</option>
                <option value="odd">홀수</option>
              </select>
            </label>
            <label className="flex flex-col gap-1">
              정지 비트
              <select value={serial.stop_bits} onChange={(e) => setSerial({ ...serial, stop_bits: Number(e.target.value) })} className={field}>
                <option>1</option>
                <option>2</option>
              </select>
            </label>
            <label className="col-span-2 flex flex-col gap-1">
              흐름 제어
              <select value={serial.flow} onChange={(e) => setSerial({ ...serial, flow: e.target.value })} className={field}>
                <option value="none">없음</option>
                <option value="rtscts">하드웨어 (RTS/CTS)</option>
                <option value="xonxoff">소프트웨어 (XON/XOFF)</option>
              </select>
            </label>
          </div>
        ) : (
          <div className="mt-4 grid grid-cols-[1fr_7rem] gap-3 text-xs text-dim">
            <label className="flex flex-col gap-1">
              주소 (IP 또는 호스트 이름)
              <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="예: 192.168.123.21" spellCheck={false} autoFocus className={field} />
            </label>
            <label className="flex flex-col gap-1">
              포트
              <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" spellCheck={false} className={field} />
            </label>
            {(kind === "ssh" || kind === "ftp") && (
              <label className="col-span-2 flex flex-col gap-1">
                {kind === "ssh" ? "사용자 이름" : "사용자 이름 (비워 두면 접속 뒤에 묻습니다)"}
                <input value={user} onChange={(e) => setUser(e.target.value)} placeholder="예: admin" spellCheck={false} className={field} />
              </label>
            )}
          </div>
        )}

        {error && <p className="mt-3 text-xs text-danger">{error}</p>}
        <div className="mt-5 flex items-center justify-between gap-3">
          <p className="text-xs text-dim">암호를 적는 칸은 없습니다. NetX는 암호를 저장하지 않습니다.</p>
          <button type="submit" className="shrink-0 rounded-md bg-accent px-5 py-2 text-sm font-semibold text-void">
            접속
          </button>
        </div>
      </form>
      {empty && <WordCard className="mx-auto mt-8 max-w-xl text-center" />}
    </div>
  );
}
