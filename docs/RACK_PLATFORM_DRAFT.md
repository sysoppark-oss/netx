# 랙(Rack) 플랫폼 설계와 표준 API 규격 초안

> **초안(0.2판, 2026-11-11) — 확정 전.** "ai연구소" 프로젝트의 설계 세션에서 작성해 전달한 참조본이다.
> 10장의 결정 사항(서명, 라이선스, 섀시 형태, 이름 등)을 기획자가 확정하기 전에는 **이 문서를 근거로 구현을 시작하지 않는다.**
> 지금 진행 중인 작업의 순서와 범위는 바뀌지 않는다. 기준 원본은 프로젝트 문서 `claude/Rack_플랫폼_설계_표준API_초안.md`이다.

- 문서 버전: 0.2 (2026-11-11, 11장 "오늘의 말씀" 모듈 추가)
- 기획: 박정근
- 근거: `sysoppark-oss/storyfirewall` (`fe56c93`, v0.1.6), `sysoppark-oss/netx` (`267ecd1`) 저장소를 직접 읽고 작성
- 함께 보는 문서: `NetX_최종통합설계서.md` 8장(스토리방화벽 연동), 9.2(MCP 서버 공개)
- 표기: **[확정]** 기획자가 결정한 것, **[제안]** 이 문서의 권고, **[검증]** 노트북 실기 확인이 필요한 것

---

## 1. 결론

1. **백플레인은 새로 고르지 않는다. 스토리방화벽이 이미 쓰는 "네임드 파이프 + 한 줄 JSON-RPC 2.0"을 랙 표준으로 올린다.** gRPC, 로컬 WebSocket, ZeroMQ는 쓰지 않는다. [제안]
2. **중앙 브로커(허브 프로세스)를 두지 않는다.** 모듈마다 자기 파이프를 열고, 레지스트리의 "슬롯 등록부"에 이름을 올린다. 섀시(통합 화면)는 등록부를 읽어 각 모듈에 직접 붙는다. [제안]
3. **모듈이 지킬 것은 공통 메서드 8개(`rack.*`)뿐이다.** 모듈 고유 기능(`policy.*`, `story.*` 등)은 그대로 둔다. 스토리방화벽은 기존 메서드를 하나도 바꾸지 않고 `rack.*`만 더한다.
4. **변경(차단, 종료 등)은 언제나 그 기능의 주인 모듈이 자기 확인 창으로 승인받는다.** 섀시나 다른 모듈은 "요청"만 할 수 있다. 스토리방화벽의 `change.stage → change.commit` 흐름을 랙 전체 규칙으로 삼는다.
5. 첫 구현은 **`rack-proto` 크레이트(규격 타입) → 스토리방화벽에 `rack.*` 추가 → 읽기 전용 섀시** 순서다.

---

## 2. 저장소에서 확인한 현재 상태

### 2.1 스토리방화벽: 백플레인의 원형이 이미 있다

| 항목 | 현재 구현 | 위치 |
|---|---|---|
| 구조 | 서비스(관리자 권한, session 0) ↔ 화면(Tauri, 일반 권한) 분리 | `crates/svc`, `ui/src-tauri` |
| 규격 | JSON-RPC 2.0, 한 줄 JSON, 요청 1MB 상한 | `crates/api` (전송·엔진에 의존하지 않는 타입 전용 크레이트) |
| 전송 | 메시지 모드 네임드 파이프, 원격 접속 거부. 개발용은 TCP 루프백 | `svc/src/win/pipe.rs`, `ui/src-tauri/src/client.rs` |
| 가로채기 방지 | 시작마다 무작위 파이프 이름 → `HKLM\SOFTWARE\StoryFirewall\PipeName`에 공개, 클라이언트는 서버가 session 0인지 확인 | `svc/src/win/pipe_name.rs` |
| 인증 | 접속한 프로그램의 서명자 CN이 `StoryFirewall`인지 검사, 실패하면 연결 끊음. 변경 메서드는 관리자 그룹 검사 | `svc/src/win/authn.rs` |
| 권한 | 메서드마다 `Role::User`(조회) / `Role::Admin`(변경) | `api/src/methods.rs` |
| 판 협상 | `rpc.hello` → api 메이저가 다르면 `version_mismatch` | `api/src/rpc.rs` |
| 구독 | `connections.subscribe`, `events.subscribe` → 서버가 알림(`connections.tick`, `events.new`)을 밀어 줌 | `api/src/methods.rs` |
| 변경 절차 | `change.stage → change.get → change.commit → change.result`, 멱등 키 `request_id` | 같은 파일 |
| 메서드 | 약 55개 (`connections.*`, `story.*`, `policy.*`, `cut.*`, `emergency.*`, `blocked.recent`, `collector.health` 등) | 같은 파일 |
| 오류 코드 | 표준 + `-32001 unauthorized` ~ `-32006 version_mismatch` | `api/src/error.rs` |

### 2.2 NetX: 아직 바깥으로 열린 통로가 없다

- Tauri 2 + tokio. 화면 ↔ 백엔드는 Tauri 명령(`commands.rs`)만 쓴다.
- 설계서 10.4의 `bridge/`(스토리방화벽 딥링크·IPC, 토큰 검증)는 **아직 만들지 않았다.** 저장소에 파이프·딥링크 코드가 없다.
- 실행 엔진, 검증기(`security/validator.rs`), 감사 로그(`audit.rs`), 해설(`interpret.rs`)이 이미 모듈로 나뉘어 있어서, 바깥 요청을 받는 입구만 붙이면 된다.
- 저장소에 LICENSE 파일이 없다(8.4 참고).

### 2.3 전달받은 요청문에서 바로잡을 점

| 요청문 표현 | 저장소·문서 기준 사실 |
|---|---|
| 스토리방화벽 "v46까지 진행" | 공개 판은 **v0.1.6** (2026-11-09). "46"은 저장소에서 판 번호로 확인되지 않는다 |
| "NetCraft v2.0 (NetX), 네트워크 시뮬레이션 및 진단 도구" | NetX는 **도구 통합 작업대 + 상황 기반 제안 엔진**이다. 교육·시뮬레이션용 넷크래프트 구상은 설계서 12장에서 배제했다 |
| "겨자씨 CIMS의 Bento Box 위젯" | 이 프로젝트 문서와 지난 대화에서 찾지 못했다. Bento 화면은 **새 요구사항**으로 보고 6.5에 데이터 계약을 잡았다. 기존 코드가 있으면 그것에 맞춘다 |
| "gRPC 등 인터페이스 코드 초안" | 아래 3장의 이유로 gRPC를 권하지 않는다. 코드 초안은 JSON-RPC 기준으로 7장에 있다 |

---

## 3. IPC 방식 비교와 권고

| 기준 | 네임드 파이프 + JSON-RPC (현행) | gRPC (tonic) | 로컬 WebSocket | ZeroMQ |
|---|---|---|---|---|
| 이미 만든 것 | 서버·클라이언트·인증·시험 모두 있음 | 없음 | 없음 | 없음 |
| 상대 신원 확인 | OS가 접속한 프로세스 PID를 알려 줌 → 서명·관리자 검사 가능 | TCP로 하면 불가. 파이프 위에 올리려면 전송 계층을 직접 붙여야 함 | 불가(토큰을 따로 설계) | Windows에서는 사실상 TCP, 같은 문제 |
| 열리는 포트 | 없음 | TCP면 있음 | 있음. 브라우저가 `localhost`로 접근할 수 있어 웹페이지발 공격면이 생김 | 있음 |
| 화면(React)과의 궁합 | JSON 그대로 | protobuf ↔ JSON 변환 필요 | JSON 그대로 | 변환 필요 |
| 빌드 부담 | 없음(serde) | protoc, 코드 생성, 의존성 증가 | 보통 | C 라이브러리 또는 미성숙 순수 Rust 구현 |
| 서버 → 클라이언트 알림 | 있음(구독) | 스트리밍 | 있음 | 있음 |
| AI 에이전트 연결 | MCP가 같은 JSON-RPC 2.0 → 얇은 어댑터로 연결 | 변환 계층 필요 | 변환 계층 필요 | 변환 계층 필요 |

**권고 이유 세 가지.**

1. **방화벽 제품이 로컬 포트를 여는 것은 스스로 공격면을 만드는 일이다.** 파이프는 포트가 없고 원격 접속을 OS 수준에서 거부한다.
2. **"누가 꽂았는가"를 OS가 보증한다.** 랙의 핵심 위험은 가짜 모듈·가짜 섀시다. 파이프는 양쪽 모두 상대 프로세스의 실행 파일 서명을 확인할 수 있다. 스토리방화벽은 이 검사를 이미 한다.
3. **NetX 설계서 9.2의 "MCP 서버 공개"와 같은 봉투다.** 랙 규격을 JSON-RPC로 두면 나중에 AI 에이전트용 문을 여는 비용이 작다.

**gRPC가 맞는 경우(지금은 아님):** 다른 PC의 에이전트(설계서 9.2 "원격 관측소")와 통신할 때. 그때도 랙 내부 규격은 그대로 두고 원격 구간만 별도 게이트웨이로 붙인다.

**한계:** 패킷 단위 실시간 데이터(패킷 하이웨이 등)는 백플레인으로 보내지 않는다. 백플레인은 요약·상태·사건만 나르고, 무거운 화면은 모듈 자기 창에서 그린다(6.6 `rack.open`).

---

## 4. 랙 구조

```
┌──────────────── 섀시 (통합 화면, 일반 권한) ────────────────┐
│  Bento 위젯판 · 통합 사건 목록 · 모듈 상태등 · 모듈 창 열기   │
└───────┬───────────────────────┬───────────────────────┬────┘
        │ rack.* (JSON-RPC)     │                       │
   ┌────┴─────┐           ┌─────┴────┐            ┌─────┴────┐
   │ 슬롯 1    │◄─────────►│ 슬롯 2    │            │ 슬롯 N    │
   │스토리방화벽│  인계      │  NetX    │            │ (다음 모듈)│
   │ 서비스    │ (handoff) │ 사용자 앱 │            │          │
   └──────────┘           └──────────┘            └──────────┘
        ▲                       ▲
        └── 슬롯 등록부(레지스트리): 모듈 id, 파이프 이름, 실행 파일 ──┘
```

| 물리 랙 | 소프트웨어 랙 |
|---|---|
| 섀시 | 통합 화면 앱. 모듈을 찾고, 상태를 모으고, 위젯을 배치한다. 자기 기능은 없다 |
| 슬롯 | 레지스트리의 모듈 등록 항목 |
| 백플레인 | **규격**(Rack API)과 **등록부**. 프로세스가 아니다 |
| 장비(모듈) | 자기 파이프를 여는 독립 프로그램. 섀시 없이도 단독으로 동작한다 |
| 전원·상태 LED | `rack.health` |
| 장비 명판 | `rack.describe`(매니페스트) |

**브로커를 두지 않는 이유:** 브로커가 죽으면 전부 멈추고, 브로커가 모든 모듈의 권한을 대신 쥐게 되어 가장 위험한 프로세스가 된다. 모듈 수가 한 자릿수인 동안은 직접 연결이 단순하고 안전하다.

**단독 동작 원칙:** 스토리방화벽과 NetX는 섀시가 없어도 지금처럼 쓴다. 랙은 얹는 것이지 갈아타는 것이 아니다.

---

## 5. 슬롯 등록과 발견

### 5.1 등록부

| 모듈 종류 | 등록 위치 | 쓰는 주체 |
|---|---|---|
| 서비스형(스토리방화벽) | `HKLM\SOFTWARE\StoryRack\Modules\<id>` | 설치 프로그램·서비스(관리자) |
| 사용자 앱형(NetX) | 설치 정보는 `HKLM\...\Modules\<id>`, 실행 중 파이프 이름은 `HKCU\SOFTWARE\StoryRack\Running\<id>` | 설치 프로그램 / 실행 중인 앱 |

값: `Exe`(실행 파일 전체 경로), `PipeName`(실행 중일 때), `Api`(예: `1.0`), `Kind`(`service` | `app`).

- 파이프 이름은 스토리방화벽 방식 그대로 **시작마다 무작위**(`\\.\pipe\<id>-<128비트 hex>`)로 만들고, 첫 인스턴스를 만든 뒤에 공개한다.
- 스토리방화벽은 기존 `HKLM\SOFTWARE\StoryFirewall\PipeName`을 그대로 두고, 랙 등록부에 같은 값을 한 번 더 쓴다(기존 화면 호환).

### 5.2 접속 순서

1. 섀시가 등록부에서 모듈 목록을 읽는다.
2. `PipeName`이 있으면 접속한다. 없으면 "꺼짐"으로 표시하고, 사용자가 누르면 `Exe`를 실행한다.
3. **서버 확인:** 파이프 서버 프로세스의 실행 파일이 등록된 `Exe`와 같고 서명이 유효한지 본다(서비스형은 session 0 확인 추가). HKCU는 일반 권한 프로그램이 고칠 수 있으므로 이 확인을 빼면 안 된다.
4. `rack.hello` → `rack.describe` → `rack.subscribe`.

---

## 6. 표준 API 규격 초안 (Rack API 1.0)

### 6.1 전송과 봉투 [스토리방화벽 현행 그대로]

- 네임드 파이프, 메시지 모드, 원격 접속 거부. 한 메시지 = 한 줄 JSON(끝에 개행).
- JSON-RPC 2.0. 요청 1MB 상한. `params`는 객체.
- 메서드 이름: 영문·숫자·`.`·`_`. **`rack.` 접두사는 공통 규격 전용**이고, 모듈 고유 메서드는 자기 접두사를 쓴다.
- 변경 메서드에는 `request_id`(UUID) 필수 — 멱등 키.
- 목록 응답은 `cursor` 페이지, 한 쪽 최대 1,000건.
- 알림 빈도: 모듈당 초당 10건 이하. 넘으면 모듈이 묶어서 보낸다.

### 6.2 공통 메서드

| 메서드 | 권한 | 용도 |
|---|---|---|
| `rack.hello` | 누구나 | 판 협상, 접속자 신원·권한 통보 |
| `rack.describe` | read | 매니페스트(명판) |
| `rack.health` | read | 상태등 |
| `rack.widgets` | read | Bento 위젯 데이터 |
| `rack.subscribe` | read | 공통 사건 알림 구독 → 알림 `rack.event` |
| `rack.handoff` | operate | 다른 모듈·섀시가 넘기는 일감(인텐트) |
| `rack.action.stage` / `rack.action.commit` | operate / 주인 모듈의 확인 | 변경 요청과 확정 |
| `rack.open` | operate | 모듈 창을 특정 화면으로 열기 |

모듈은 지원하지 않는 메서드에 `-32601 method not found`로 답하면 된다. `rack.hello`, `rack.describe`, `rack.health` 세 개가 **최소 장착 조건**이다.

### 6.3 `rack.hello`

```json
→ {"jsonrpc":"2.0","id":1,"method":"rack.hello",
   "params":{"client":"rack-shell","client_version":"0.1.0","api":[1,0]}}
← {"jsonrpc":"2.0","id":1,
   "result":{"module":"storyfirewall","module_version":"0.1.6","api":[1,0],
             "granted":["read","operate"]}}
```

- 메이저가 다르면 `-32006 version_mismatch`. 마이너는 서버 ≥ 클라이언트면 통과(현행 규칙).
- `granted`는 모듈이 접속자의 서명·권한을 보고 정한다. 접속자가 스스로 권한을 주장하는 칸은 없다.

### 6.4 `rack.describe` (매니페스트)

```json
{
  "id": "storyfirewall",
  "title": "스토리방화벽",
  "kind": "service",
  "version": "0.1.6",
  "api": [1, 0],
  "summary": "이 PC의 연결을 이야기로 보여 주고 차단 규칙을 만든다",
  "capabilities": ["events", "widgets", "handoff", "actions", "open"],
  "namespaces": ["connections", "story", "policy", "cut", "emergency", "blocked"],
  "widgets": [
    {"id": "blocked_today", "title": "오늘 차단", "kind": "stat", "sizes": ["1x1", "2x1"], "refresh_sec": 10},
    {"id": "open_incidents", "title": "열린 사건", "kind": "list", "sizes": ["2x2"], "refresh_sec": 10}
  ],
  "intents_in":  ["firewall.block.request", "firewall.show.incident"],
  "intents_out": ["diagnose.target"],
  "actions": [
    {"id": "emergency.start", "title": "긴급 차단", "risk": "danger"}
  ]
}
```

- `namespaces`는 모듈 고유 메서드의 접두사 목록이다. 섀시는 고유 메서드를 **부르지 않는다**(모르는 것으로 취급). 고유 메서드는 그 모듈의 전용 화면과 약속한 상대만 쓴다.
- `risk`는 NetX의 4단계(`safe`, `caution`, `danger`, `forbidden`)를 랙 공통으로 쓴다.

### 6.5 `rack.health`와 `rack.widgets`

```json
// rack.health
{"state": "ok", "since": 1794355200, "notes": [],
 "checks": [{"id": "collector", "state": "ok"}, {"id": "wfp", "state": "ok"}]}
```

`state`: `ok` | `degraded` | `down` | `starting`. 스토리방화벽은 `collector.health`를 감싸서 답한다.

```json
// rack.widgets  params: {"ids": ["blocked_today", "open_incidents"]}
{"widgets": [
  {"id": "blocked_today", "kind": "stat", "value": 37, "unit": "건", "trend": "up",
   "state": "caution", "open": {"view": "blocked"}},
  {"id": "open_incidents", "kind": "list", "state": "danger",
   "items": [{"title": "처음 보는 프로그램이 해외로 접속", "sub": "updater.exe → 203.0.113.9:443",
              "state": "danger", "open": {"view": "incident", "ref": "4812"}}]}
]}
```

- 위젯 종류(`kind`)는 섀시가 그릴 줄 아는 6개로 시작한다: `stat`(숫자), `status`(상태등), `list`(목록), `spark`(작은 추이선), `timeline`(시간순), `quote`(인용문 — 11장).
- **모듈은 데이터만 주고, 그리기는 섀시가 한다.** 모듈이 HTML이나 스크립트를 보내는 길은 열지 않는다(화면 주입 방지).
- 문자열은 섀시가 글자로만 표시한다. 길이 상한: 제목 80자, 보조 200자.
- `open`은 그 항목을 눌렀을 때 `rack.open`에 넘길 값이다.

### 6.6 `rack.subscribe` → 알림 `rack.event`

```json
{"jsonrpc":"2.0","method":"rack.event","params":{
  "id": "storyfirewall:evt:90211",
  "ts": 1794355321,
  "module": "storyfirewall",
  "severity": "warn",
  "kind": "incident.opened",
  "title": "처음 보는 프로그램이 해외로 접속",
  "entities": [
    {"type": "process", "pid": 4321, "image": "C:\\Users\\...\\updater.exe"},
    {"type": "endpoint", "ip": "203.0.113.9", "port": 443, "proto": "tcp"}
  ],
  "open": {"view": "incident", "ref": "4812"},
  "suggest": [{"intent": "diagnose.target", "to": "netx", "label": "NetX로 정밀 진단"}]
}}
```

- `severity`: `info` | `notice` | `warn` | `critical`.
- `entities`의 공통 타입: `process`, `endpoint`, `host`, `domain`, `file`, `rule`, `device`. 모듈끼리 대상을 주고받는 공용어다.
- `suggest`가 설계서 8장의 "[NetX로 정밀 진단] 버튼"에 해당한다. 섀시도, 스토리방화벽 자기 화면도 같은 정보를 쓴다.
- `rack.open`: `{"view": "incident", "ref": "4812"}` → 모듈이 자기 창을 띄워 그 화면으로 간다. 무거운 시각화는 여기서 본다.

### 6.7 `rack.handoff` (모듈 간 인계)

```json
→ {"jsonrpc":"2.0","id":7,"method":"rack.handoff","params":{
     "request_id": "2f0c…",
     "intent": "diagnose.target",
     "from": "storyfirewall",
     "entities": [{"type": "endpoint", "ip": "203.0.113.9", "port": 443, "proto": "tcp"}],
     "note": "사건 #4812"}}
← {"jsonrpc":"2.0","id":7,"result":{"accepted": true, "opened": true}}
```

첫 판의 인텐트 두 가지:

| 인텐트 | 보내는 쪽 → 받는 쪽 | 받는 쪽 동작 |
|---|---|---|
| `diagnose.target` | 스토리방화벽·섀시 → NetX | 창을 띄우고 옴니박스에 대상을 채워 제안 카드를 낸다. **실행은 사용자가 누른다** |
| `firewall.block.request` | NetX·섀시 → 스토리방화벽 | 변경을 맡겨 두고(`change.stage`) 자기 확인 창을 띄운다. 60초 유지 확인·가드 그대로 |

- 인계받은 값은 **데이터**다. 받는 모듈은 자기 검증기를 그대로 통과시킨다(NetX: `security/validator.rs`, 스토리방화벽: 가드·dryrun).
- 인계만으로는 아무것도 실행·차단되지 않는다.

### 6.8 `rack.action.stage` / `rack.action.commit`

매니페스트 `actions`에 올린 동작을 섀시에서 시작하는 통로다.

- `stage` → 모듈이 무엇이 바뀌는지 요약(`summary` 줄 목록)과 `change_id`를 돌려준다.
- `commit` → **주인 모듈이 자기 방식으로 승인받는다**(스토리방화벽: UAC, NetX: 실행 확인·Windows Hello). 섀시의 버튼은 승인이 아니다.
- 스토리방화벽에서는 기존 `change.stage`/`change.commit`의 얇은 포장이다.

### 6.9 권한과 오류 코드

| 등급 | 뜻 | 줄 조건 |
|---|---|---|
| `read` | 상태·위젯·사건 조회 | 인증된 로컬 사용자 + 서명 확인된 랙 구성원 |
| `operate` | 인계, 창 열기, 변경 맡기기 | 위와 같음. 변경 확정은 주인 모듈 승인 |
| `admin` | 모듈 고유의 관리 메서드 | 모듈이 정함(스토리방화벽: 관리자 그룹). 랙 공통 메서드는 `admin`을 요구하지 않는다 |

오류 코드는 스토리방화벽 것을 그대로 쓰고 세 개를 더한다.

| 코드 | 이름 | 뜻 |
|---|---|---|
| -32001 ~ -32006 | (현행) | unauthorized, guard_rejected, needs_ack, busy, not_confirmed_yet, version_mismatch |
| -32010 | unsupported_intent | 받지 않는 인텐트 |
| -32011 | module_unavailable | 대상 모듈이 꺼져 있거나 준비 중 |
| -32012 | rate_limited | 요청이 너무 잦음 |

---

## 7. Rust 인터페이스 초안 (`rack-proto` 크레이트)

스토리방화벽 `crates/api`와 같은 성격이다. 타입과 검증만 있고 전송·엔진에 의존하지 않아 리눅스에서 전부 시험한다.

```rust
//! rack-proto: Rack API 1.0 메시지 타입. 전송(파이프)·인증은 각 모듈이 맡는다.
use serde::{Deserialize, Serialize};

pub const RACK_API: (u32, u32) = (1, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Grant { Read, Operate, Admin }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State { Ok, Degraded, Down, Starting }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Info, Notice, Warn, Critical }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk { Safe, Caution, Danger, Forbidden }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello { pub client: String, pub client_version: String, pub api: (u32, u32) }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloReply {
    pub module: String,
    pub module_version: String,
    pub api: (u32, u32),
    pub granted: Vec<Grant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub title: String,
    pub kind: ModuleKind,
    pub version: String,
    pub api: (u32, u32),
    pub summary: String,
    pub capabilities: Vec<String>,
    pub namespaces: Vec<String>,
    pub widgets: Vec<WidgetSpec>,
    pub intents_in: Vec<String>,
    pub intents_out: Vec<String>,
    pub actions: Vec<ActionSpec>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleKind { Service, App, Builtin }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetSpec {
    pub id: String, pub title: String, pub kind: WidgetKind,
    pub sizes: Vec<String>, pub refresh_sec: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetKind { Stat, Status, List, Spark, Timeline, Quote }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSpec { pub id: String, pub title: String, pub risk: Risk }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub state: State, pub since: i64,
    #[serde(default)] pub notes: Vec<String>,
    #[serde(default)] pub checks: Vec<Check>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check { pub id: String, pub state: State }

/// 모듈끼리 주고받는 대상의 공용어
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Entity {
    Process  { pid: u32, #[serde(default)] image: Option<String> },
    Endpoint { ip: std::net::IpAddr, #[serde(default)] port: Option<u16>,
               #[serde(default)] proto: Option<String> },
    Host     { name: String },
    Domain   { name: String },
    File     { path: String, #[serde(default)] sha256: Option<String> },
    Rule     { owner: String, id: String },
    Device   { ip: std::net::IpAddr, #[serde(default)] mac: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRef { pub view: String, #[serde(default)] pub r#ref: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String, pub ts: i64, pub module: String,
    pub severity: Severity, pub kind: String, pub title: String,
    #[serde(default)] pub entities: Vec<Entity>,
    #[serde(default)] pub open: Option<OpenRef>,
    #[serde(default)] pub suggest: Vec<Suggest>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggest { pub intent: String, pub to: String, pub label: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handoff {
    pub request_id: String, pub intent: String, pub from: String,
    #[serde(default)] pub entities: Vec<Entity>,
    #[serde(default)] pub note: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffReply { pub accepted: bool, pub opened: bool }

/// 모듈이 구현하는 면. 전송·인증을 통과한 뒤 디스패처가 부른다.
pub trait RackModule {
    fn describe(&self) -> Manifest;
    fn health(&self) -> Health;
    fn widgets(&self, ids: &[String]) -> Result<serde_json::Value, RackError>;
    fn handoff(&self, caller: &Caller, h: Handoff) -> Result<HandoffReply, RackError>;
    fn open(&self, caller: &Caller, target: OpenRef) -> Result<(), RackError>;
}

/// 전송 계층이 채워 주는 접속자 정보. 접속자가 보낸 값이 아니다.
pub struct Caller { pub pid: u32, pub image: String, pub signer: Option<String>, pub granted: Vec<Grant> }

#[derive(Debug, thiserror::Error)]
pub enum RackError {
    #[error("권한 없음")]            Unauthorized,       // -32001
    #[error("받지 않는 인텐트")]      UnsupportedIntent,  // -32010
    #[error("모듈을 쓸 수 없음")]     ModuleUnavailable,  // -32011
    #[error("요청이 너무 잦음")]      RateLimited,        // -32012
    #[error("잘못된 값: {0}")]        InvalidParams(String), // -32602
}
```

봉투(`Request`/`Response`/`Notification`), 한 줄 파싱, 판 협상, 페이지는 스토리방화벽 `api/src/rpc.rs`, `page.rs`를 이 크레이트로 옮겨 양쪽이 함께 쓴다.

---

## 8. 모듈별 변경점

### 8.1 스토리방화벽 (추가만, 기존 메서드 불변)

| 무엇 | 어디 | 내용 |
|---|---|---|
| `rack.*` 메서드 등록 | `crates/api/src/methods.rs` | `Method`에 8개 추가, 권한은 `Role::User`(변경 확정은 기존 `change.commit`이 관리자·UAC를 요구) |
| 매니페스트·상태·위젯 | `crates/svc/src/rack.rs` (신규) | `rack.health` ← `collector.health`, 위젯 ← `blocked.recent`·`story.today`·`emergency.status` 집계 |
| 공통 사건 변환 | `crates/svc/src/dispatch.rs` 구독부 | `events.new`를 `rack.event` 형식으로도 내보냄. `entities`와 `suggest` 채우기 |
| 인텐트 수신 | `crates/svc/src/rack.rs` | `firewall.block.request` → `change.stage`에 맡기고 화면에 확인 요청 |
| 인증 확장 | `crates/svc/src/win/authn.rs` | 통과시킬 서명자를 `PRODUCT_SIGNER_CN` 하나에서 **랙 구성원 목록**으로(8.3) |
| 등록부 기록 | `crates/svc/src/win/pipe_name.rs` | `HKLM\SOFTWARE\StoryRack\Modules\storyfirewall`에도 공개 |
| [NetX로 정밀 진단] 버튼 | `ui/` | 사건 카드에서 NetX로 `rack.handoff(diagnose.target)` |
| api 판 | `api/src/rpc.rs` | `API_VERSION` (1,0) → (1,1). 마이너 증가라 기존 화면과 호환 |

주의: 스토리방화벽 서비스는 "조회는 인증된 사용자, 변경은 관리자" 구조다. 섀시가 일반 권한으로 붙어도 `rack.*` 조회는 되고, 차단은 지금처럼 UAC를 거친다. **섀시 때문에 권한이 넓어지는 곳이 없어야 한다.**

### 8.2 NetX (입구 신설)

| 무엇 | 어디 | 내용 |
|---|---|---|
| 파이프 서버 | `src-tauri/src/bridge/` (설계서 10.4에 예정된 자리) | `tokio::net::windows::named_pipe`. 무작위 이름, 원격 거부, 인스턴스 만들기 권한 없는 ACL |
| 접속자 확인 | `bridge/authn.rs` | 접속 프로세스 서명 확인 → `granted` 결정 |
| `rack.*` 구현 | `bridge/rack.rs` | `describe`, `health`(WSL·도구 설치 상태는 `env_check.rs`), `widgets`(내 네트워크 `netinfo.rs`, 최근 진단 `audit.rs`) |
| `diagnose.target` 수신 | `bridge/rack.rs` → 화면 이벤트 | 창을 앞으로, 옴니박스에 대상 채움. 값은 `security/values.rs`로 형식 검사 |
| `firewall.block.request` 발신 | `prockill.rs` 옆 / 요새 도면 버튼 | 스토리방화벽 파이프로 인계 |
| 화면 명령 등록 | `lib.rs`, `build.rs`, `capabilities/default.json` | 기존 규칙대로 |

**설계서 8.1의 "서명한 일회용 토큰"은 필요 없어진다.** 딥링크는 누가 보냈는지 알 수 없어서 토큰이 필요했지만, 파이프는 OS가 접속자를 알려 주므로 서명 확인으로 대체된다. 딥링크(`netx://`)는 남기더라도 **대상 채우기만** 하고 전체 권한은 열지 않는다. 설계서 15장 5번 결정이 이것으로 정리된다.

현재 NetX의 진행 중 작업(구성도 보강 → 넷엑스 터미널)은 그대로 두고, 이 입구는 설계서 단계 계획의 v0.2 "스토리방화벽 연동"에 넣는다.

### 8.3 랙 구성원 신원 [검증 · 결정 필요]

지금 스토리방화벽 서비스는 서명자 CN이 `StoryFirewall`인 프로그램만 받는다. 섀시와 NetX가 붙으려면 둘 중 하나가 필요하다.

- (가) 세 프로그램을 **같은 게시자 인증서**로 서명한다. 가장 단순하다.
- (나) 서비스가 허용 서명자 목록을 갖는다(`StoryFirewall`, `NetX`, `StoryRack`).

어느 쪽이든 **개발 빌드 서명 방식**과 맞물린다. 공개 판 안내문에는 "코드 서명 인증서가 없고 설치 파일은 한 번 쓰고 버린 자체 서명 키로 서명"이라고 되어 있어, 서비스가 화면을 인증하는 서명이 실제로 어떤 키인지 노트북에서 확인해야 한다. NetX 개발 빌드는 지금 서명 절차가 없다.

### 8.4 라이선스 [결정 필요]

스토리방화벽은 AGPL-3.0-only이고 NetX에는 LICENSE가 없다. NetX가 스토리방화벽의 `api` 크레이트를 그대로 가져다 쓰면 NetX 배포물에도 AGPL 조건이 따라온다. `rack-proto`는 스토리방화벽에서 떼어 내 **별도 저장소, 별도 라이선스**로 두는 편이 깔끔하다. 저작권자가 한 사람이므로 떼어 낼 때 라이선스를 새로 정할 수 있다. 법률 판단은 제가 대신할 수 없으니, 배포 전에 조건을 한 번 확인하시기 바란다.

---

## 9. 액션 플랜

| 단계 | 할 일 | 완료 기준 |
|---|---|---|
| 0 | 이 초안 확정: 8.3 서명, 8.4 라이선스, 10장 결정 | 결정 사항이 문서에 [확정]으로 바뀜 |
| 1 | `rack-proto` 크레이트: 7장 타입 + 스토리방화벽 `rpc.rs`·`page.rs` 이전, 직렬화·검증 시험, JSON 예시 고정 시험 | 리눅스에서 `cargo test` 통과. 스토리방화벽이 이 크레이트를 써서 기존 시험 그대로 통과 |
| 2 | 스토리방화벽에 `rack.hello`·`describe`·`health`·`widgets`·`subscribe` 추가(읽기만) | 시험용 클라이언트(`svc-dev`)로 다섯 메서드 응답 확인. 노트북에서 실제 서비스로 확인 |
| 3 | 읽기 전용 섀시 `rack-shell`(Tauri 2 + React): 등록부 읽기, 상태등, Bento 위젯 2~3개, 통합 사건 목록, "오늘의 말씀" 위젯(11장) | 노트북에서 스토리방화벽 한 대가 꽂힌 랙 화면이 실시간으로 갱신됨. 말씀 위젯이 인터넷 없이 뜨고 설정에서 꺼짐 |
| 4 | NetX `bridge/`: 파이프 서버 + `rack.*` 읽기 | 랙에 두 번째 장비가 뜸 |
| 5 | 인계 양방향: `diagnose.target`, `firewall.block.request` | 사건 카드 → NetX 제안 카드, 요새 도면 → 방화벽 확인 창. 둘 다 사용자가 누르기 전에는 아무것도 실행되지 않음 |
| 6 | `rack.action.*`, `rack.open`, 위젯 종류 확장 | 섀시에서 긴급 차단을 시작해 방화벽 UAC로 확정 |
| 이후 | MCP 어댑터(랙 → AI 에이전트), 원격 관측소 게이트웨이 | 선택 |

단계 1~3은 스토리방화벽에 읽기 기능만 더하므로 기존 동작을 건드리지 않는다.

---

## 10. 결정이 필요한 사항

| # | 질문 | 이 문서의 기본값 |
|---|---|---|
| 1 | IPC 방식 | 네임드 파이프 + JSON-RPC 2.0 (현행 승격) |
| 2 | 브로커 유무 | 없음. 등록부 + 직접 연결 |
| 3 | 섀시를 새 앱으로 만들지, 스토리방화벽 화면을 키울지 | 새 앱 `rack-shell`. 스토리방화벽 화면은 방화벽 전용으로 유지 |
| 4 | 저장소 | 새 공개 저장소 하나에 `rack-proto` + `rack-shell` |
| 5 | 랙 구성원 서명 | (가) 같은 게시자 인증서. 개발 빌드 서명 절차는 노트북 확인 뒤 정함 |
| 6 | `rack-proto` 라이선스 | 스토리방화벽과 분리 |
| 7 | 플랫폼 이름 | 임시로 "StoryRack" (레지스트리 키 이름에 들어가므로 단계 2 전에 확정) |
| 8 | "겨자씨 CIMS" Bento 화면 | 기존 코드가 있으면 위치를 알려 주시면 6.5의 위젯 계약을 거기에 맞춤 |
| 9 | 말씀 번역본(언어별) | 사용 조건이 확인된 번역본만 싣는다. 한국어는 성서공회 확인 뒤 결정, 영어는 KJV 또는 WEB |
| 10 | 말씀 표시 기본값 | 켜짐. 첫 실행 때 "설정에서 끌 수 있습니다" 한 줄 안내 |
| 11 | 구절 목록 | 기획자가 선정(11.3). 본문은 번역본 원문 파일에서 가져오고 손으로 옮겨 적지 않는다 |
| 12 | "정보" 화면 헌정 문구, 교회 안내 링크 주소 | 11.6 초안을 기획자가 고쳐 확정 |

---

## 11. "오늘의 말씀" 모듈 [제안]

목적: 이 프로그램들은 맑은물속교회가 열방 선교를 위해 만들어 공개한다. 쓰는 사람이 일하는 틈에 말씀을 만나고, 누가 왜 만들었는지 알 수 있게 한다. 방식은 **드러내 놓고, 방해하지 않고, 끌 수 있게**다.

### 11.1 형태

- 독립 프로세스가 아니라 **공용 크레이트 `rack-word`** 로 만든다. 구절 자료, 고르는 규칙, 설정 읽기만 들어 있다. 네트워크 코드가 없다.
- 쓰는 곳 두 가지:
  1. **섀시:** 내장 슬롯(`kind: "builtin"`)으로 장착해 Bento 위젯 한 칸을 낸다. 규격은 다른 모듈과 같은 `rack.describe` / `rack.widgets` 모양이지만 파이프를 거치지 않고 섀시 안에서 바로 답한다.
  2. **각 프로그램(스토리방화벽 화면, NetX):** 섀시 없이 단독으로 쓸 때도 같은 크레이트로 자기 화면의 정해진 자리에 표시한다.
- 서비스(관리자 권한)에는 넣지 않는다. 화면 쪽에만 둔다.

### 11.2 표시하는 자리와 표시하지 않는 자리

| 자리 | 동작 |
|---|---|
| 섀시 Bento 위젯 | 오늘의 말씀 한 구절. 누르면 본문 전체, 출처(번역본), 교회 안내가 열린다 |
| 기다리는 화면 | 스캔·진단·구성도 조사가 도는 동안 진행 표시 아래 한 구절 |
| 빈 화면 | "차단 기록 없음", "사건 없음" 같은 빈 목록 아래 |
| 시작 화면 | 프로그램이 뜨는 동안 |
| "정보" 화면 | 헌정 문구(11.6), 교회 안내 링크. 이 자리는 설정과 관계없이 항상 있다 |

**넣지 않는 자리(고정 규칙):** 차단·종료 확인 창, 60초 유지 확인, UAC·Windows Hello 흐름, 경고 알림과 `rack.event`, 진단 결과 브리핑과 원본 출력, 감사 로그. 사용자가 판단해야 하는 순간과 기록에는 말씀을 섞지 않는다.

계속 흘러가는 자막은 기본으로 넣지 않는다. 원하는 사용자를 위해 설정에 선택 항목(`ticker`)으로만 둔다.

### 11.3 구절 자료

```json
// verses/ko.json
{
  "lang": "ko",
  "version": "(번역본 이름)",
  "license": "(사용 근거: 공유 저작물 / 허락 문서 번호와 날짜)",
  "source": "(원문 파일 출처)",
  "verses": [
    {"id": "jhn.3.16", "ref": "요한복음 3:16", "text": "…"}
  ]
}
```

- 성경 전체가 아니라 **선정한 구절 목록**을 싣는다(예: 365개). 무엇을 실을지는 기획자가 정한다.
- `id`는 언어와 무관한 공통 값이다(책 약어.장.절). 언어 파일마다 같은 `id` 목록을 갖는다 → 같은 날 어느 나라에서나 같은 구절이 그 나라 말로 나온다.
- **본문은 번역본 원문 파일에서 스크립트로 뽑는다.** 사람이든 AI든 기억으로 옮겨 적지 않는다. 뽑은 뒤 원문과 글자 단위로 대조하는 시험을 둔다.
- `license`와 `source`가 비어 있는 언어 파일은 빌드가 실패하게 한다. 사용 조건을 확인하지 않은 번역본이 배포물에 들어가지 않게 하는 장치다.
- 번역본 [검증]: 개역개정은 대한성서공회의 허락이 필요하다. 개역한글의 현재 사용 조건은 성서공회에 확인한다. 영어 KJV, WEB는 자유 이용 번역본이다. 그 밖의 언어는 언어마다 자유 이용 번역본이 있는지 확인한 것만 추가한다. 없는 언어는 영어로 대신하지 않고 표시를 생략한다(설정에서 다른 언어를 고를 수는 있다).

### 11.4 고르는 규칙

- **오늘의 말씀:** 날짜(사용자 PC의 현지 날짜)로 목록에서 하나를 정한다. 무작위가 아니라 계산이다 → 인터넷 없이도 모두 같은 구절을 본다.
- **기다리는 화면·빈 화면:** 목록에서 차례로 돌린다. 한 화면에서 10초보다 빨리 바꾸지 않는다.
- 긴 구절(예: 160자 초과)은 기다리는 화면에 쓰지 않고 위젯의 "전체 보기"에서만 보인다. 본문을 잘라서 보여 주지 않는다.

### 11.5 설정과 사용자 존중

| 설정 | 값 | 기본 |
|---|---|---|
| `word.enabled` | 켬 / 끔 | 켬 |
| `word.places` | `widget`, `waiting`, `empty`, `startup`, `ticker` 중 선택 | `ticker`만 빼고 전부 |
| `word.lang` | 화면 언어 따름 / 직접 선택 | 화면 언어 따름 |

- 첫 실행 때 한 번, 말씀이 표시된다는 것과 설정에서 끌 수 있다는 것을 한 줄로 알린다.
- **아무것도 밖으로 보내지 않는다.** 구절은 앱에 내장하고, 몇 번 보였는지·켰는지 껐는지를 세거나 전송하지 않는다. 교회 안내 링크는 사용자가 눌렀을 때만 기본 브라우저로 연다.
- 끈 사용자에게 다시 켜라고 권하지 않는다.

### 11.6 "정보" 화면 헌정 문구 [초안 — 기획자가 고쳐 확정]

> 이 프로그램은 맑은물속교회가 만들어 누구에게나 무료로 나눕니다.
> 이 PC의 기록은 밖으로 보내지 않으며, 소스 코드는 공개되어 있습니다.
> 프로그램 곳곳의 성경 구절은 설정에서 끌 수 있습니다.
> [교회 안내] [소스 코드]

왜 나누는지에 대한 신앙 고백 문장은 기획자의 말로 직접 쓰는 것이 맞아 초안에서는 비워 두었다.

### 11.7 규격 추가분

위젯 종류에 `quote`를 더한다(6.5, 7장에 반영).

```json
// rack.describe 의 내장 모듈 명판
{"id": "word", "title": "오늘의 말씀", "kind": "builtin", "version": "0.1.0", "api": [1, 0],
 "summary": "오늘의 성경 구절", "capabilities": ["widgets"], "namespaces": [],
 "widgets": [{"id": "today", "title": "오늘의 말씀", "kind": "quote", "sizes": ["2x1", "2x2"], "refresh_sec": 3600}],
 "intents_in": [], "intents_out": [], "actions": []}

// rack.widgets
{"widgets": [{"id": "today", "kind": "quote",
  "text": "(구절 본문)", "ref": "요한복음 3:16", "version": "(번역본 이름)",
  "open": {"view": "word", "ref": "jhn.3.16"}}]}
```

```rust
// rack-word 크레이트 (네트워크·파일 쓰기 없음)
pub struct Verse { pub id: &'static str, pub reference: &'static str, pub text: &'static str }
pub struct Edition { pub lang: &'static str, pub version: &'static str, pub license: &'static str }

pub enum Place { Widget, Waiting, Empty, Startup, Ticker }

pub struct WordSettings { pub enabled: bool, pub places: Vec<Place>, pub lang: Option<String> }

/// 그 언어의 번역본이 실려 있으면 돌려준다. 없으면 None (다른 언어로 대신하지 않는다)
pub fn edition(lang: &str) -> Option<&'static Edition>;
/// 오늘의 말씀: 현지 날짜로 결정 (같은 날 같은 구절)
pub fn today(lang: &str, local_date: (i32, u32, u32)) -> Option<&'static Verse>;
/// 기다리는 화면용: n번째. max_chars 를 넘는 구절은 건너뛴다
pub fn rotate(lang: &str, n: usize, max_chars: usize) -> Option<&'static Verse>;
/// 이 자리에 표시해도 되는가 (설정 반영)
pub fn allowed(s: &WordSettings, place: Place) -> bool;
```

### 11.8 순서

1. 번역본 사용 조건 확인(한국어) → 구절 목록 선정 → `verses/*.json` 생성 스크립트와 대조 시험.
2. `rack-word` 크레이트와 단위 시험(날짜 결정, 길이 제한, 설정, 없는 언어).
3. 섀시 위젯(액션 플랜 3단계에 포함).
4. NetX 기다리는 화면·빈 화면 → 스토리방화벽 화면 순으로 적용. 각 프로그램의 "정보" 화면에 헌정 문구.
