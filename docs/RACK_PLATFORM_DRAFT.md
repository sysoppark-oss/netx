> **초안(0.12판, 2026-11-12 — 0.11판 다음 판) — 확정 전.** 기준은 구글 드라이브 "05_개발/통합프로젝트" 폴더의 같은 이름 문서다.
> 이름(0장), "상용 인증서는 사지 않는다", NetX 라이선스(AGPL-3.0-only), "관제는 별도 모듈", **말씀 표시(11장: 항상 표시, 개역한글판 100곳, 헌정 문구·교회 안내, 11.9 언어 선택의 방향)**, **"판매하지 않는다"(8.4)** 만 확정이다. 말씀 표시는 자료 묶음(`word/`)의 README 규칙대로 구현해도 된다. 10장의 나머지 결정 사항을 기획자가 확정하기 전에는 **이 문서를 근거로 구현을 시작하지 않는다.**
> 지금 진행 중인 작업의 순서와 범위는 바뀌지 않는다.

# 안디옥(Antioch) 플랫폼 설계와 에피스틀(Epistle) 규격 초안

- 문서 버전: 0.12 (2026-11-12, 0.11판 다음 판). 0.12: 판매하지 않는다로 방침 변경(8.4, 10장 6-2·16번, 13.8), 설치 때 언어 선택과 언어별 말씀(11.9, 10장 22번), 영어는 World English Bible. 0.11: 말씀 표시 확정 사항 반영 — 항상 표시(끄는 설정·안내 창 없음), 성경전서 개역한글판(침례 표기) 100곳, 헌정 문구·교회 안내 확정(11장, 10장 9~12번). 자세한 근거는 「선교 구체화안」. 0.10: NAC 두 가지 확정 — 2·3단계는 미룸(폐기 아님), 소규모용 형태는 "중앙 서버 없음"(13.10, 10장). 다섯 번째 모듈 유해차단은 별도 문서로 안내(15장). 0.9: 제미나이 검증 통합 1차 답변 반영(12.9, 13.10, 14.9와 본문 해당 줄). 0.8: "AI 에이전트 안전망" 구상 추가(14장). 0.7: 관제는 별도 모듈로 확정(12.6), 네 번째 모듈 "NAC" 구상 추가(13장). 0.6: 세 번째 모듈 "관제" 구상 추가(12장). 0.5: NetX의 라이선스·배포 방침을 스토리방화벽과 동일하게 확정(8.4, 10장). 0.4: 공식 이름 확정 — 플랫폼 "안디옥(Antioch)", 통신 규격 "에피스틀(Epistle)" (0장). 이전 문서 이름: "랙 플랫폼 설계와 표준 API 규격". 0.2: 11장 "오늘의 말씀" 추가. 0.3: 서명 표현(8.3), 기존 장기 구상과의 관계(4.1), 라이선스와 판매(8.4) 정리, 2.3 정정
- 기준 위치: 구글 드라이브 "05_개발/통합프로젝트" 폴더. 저장소·프로젝트의 사본과 다르면 드라이브가 맞다
- 기획: 박정근
- 근거: `sysoppark-oss/storyfirewall` (`fe56c93`, v0.1.6), `sysoppark-oss/netx` (`267ecd1`) 저장소를 직접 읽고 작성
- 함께 보는 문서: `NetX_최종통합설계서.md` 8장(스토리방화벽 연동), 9.2(MCP 서버 공개)
- 표기: **[확정]** 기획자가 결정한 것, **[제안]** 이 문서의 권고, **[검증]** 노트북 실기 확인이 필요한 것

---

## 0. 이름 [확정 — 기획자 결정 2026-11-11]

| 대상 | 이름 | 표기 |
|---|---|---|
| 플랫폼 전체, 통합 화면 앱 | **안디옥** | `Antioch` (레지스트리 `SOFTWARE\Antioch`, 앱·저장소 `antioch`) |
| 프로그램 사이 통신 규격 | **에피스틀** | `Epistle 1.0` (크레이트 `epistle`) |
| 구조를 가리키는 말 | 랙, 슬롯, 섀시, 백플레인 | 보통명사로 그대로 쓴다. 메서드 접두사도 `rack.*` 그대로 |
| 모듈 | 스토리방화벽, NetX … | 이름 그대로. "안디옥에 꽂히는 모듈" |

**유래.** 사도 바울의 전도여행은 안디옥 교회에서 파송받아 떠났고, 1차·2차 여행 뒤 그곳으로 돌아와 보고했다(사도행전 13~14장, 18장). 모듈들이 각자 일하고 통합 화면으로 상태를 모아 오는 구조를 여기에 비겼다. 에피스틀은 바울이 교회들에 보낸 서신에서 왔다. 모듈끼리 주고받는 메시지가 서신이다.

**알아 둘 것.**
- "Epistle"은 이미 쓰는 곳이 있다: 선교사들이 후원자와 소통하는 서비스(epistle.org)와 같은 이름의 오픈소스 이메일 클라이언트. 이 문서에서는 **프로그램 안쪽의 규격 이름**으로만 쓰고, 사용자에게 보이는 제품 이름은 "안디옥"이다. 규격을 바깥에 따로 알리거나 판매할 때는 혼동이 없도록 "Antioch Epistle"처럼 붙여 쓴다.
- "Antioch"은 한 번의 웹 검색에서 같은 이름의 소프트웨어 제품이 눈에 띄지 않았다. 상표 조회는 하지 않았다. 판매를 시작하기 전에 한다.
- 이 문서의 본문에 남아 있는 "랙"은 구조를 가리키는 말이다.

---

## 1. 결론

1. **백플레인은 새로 고르지 않는다. 스토리방화벽이 이미 쓰는 "네임드 파이프 + 한 줄 JSON-RPC 2.0"을 랙 표준으로 올린다.** gRPC, 로컬 WebSocket, ZeroMQ는 쓰지 않는다. [제안]
2. **중앙 브로커(허브 프로세스)를 두지 않는다.** 모듈마다 자기 파이프를 열고, 레지스트리의 "슬롯 등록부"에 이름을 올린다. 섀시(통합 화면)는 등록부를 읽어 각 모듈에 직접 붙는다. [제안]
3. **모듈이 지킬 것은 공통 메서드 8개(`rack.*`)뿐이다.** 모듈 고유 기능(`policy.*`, `story.*` 등)은 그대로 둔다. 스토리방화벽은 기존 메서드를 하나도 바꾸지 않고 `rack.*`만 더한다.
4. **변경(차단, 종료 등)은 언제나 그 기능의 주인 모듈이 자기 확인 창으로 승인받는다.** 섀시나 다른 모듈은 "요청"만 할 수 있다. 스토리방화벽의 `change.stage → change.commit` 흐름을 랙 전체 규칙으로 삼는다.
5. 첫 구현은 **`epistle` 크레이트(규격 타입) → 스토리방화벽에 `rack.*` 추가 → 읽기 전용 섀시** 순서다.

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
| 스토리방화벽 "v46까지 진행" | 공개 판은 **v0.1.6** (2026-11-09). "46"은 판 번호가 아니라 제미나이 검증 의뢰 차수다(2026-11-11 현재 47차) |
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
| 백플레인 | **규격**(Epistle 규격)과 **등록부**. 프로세스가 아니다 |
| 장비(모듈) | 자기 파이프를 여는 독립 프로그램. 섀시 없이도 단독으로 동작한다 |
| 전원·상태 LED | `rack.health` |
| 장비 명판 | `rack.describe`(매니페스트) |

**브로커를 두지 않는 이유:** 브로커가 죽으면 전부 멈추고, 브로커가 모든 모듈의 권한을 대신 쥐게 되어 가장 위험한 프로세스가 된다. 모듈 수가 한 자릿수인 동안은 직접 연결이 단순하고 안전하다.

**단독 동작 원칙:** 스토리방화벽과 NetX는 섀시가 없어도 지금처럼 쓴다. 랙은 얹는 것이지 갈아타는 것이 아니다.

### 4.1 기존 장기 구상과의 관계

스토리방화벽 지침(CLAUDE.md)에는 이미 장기 구상이 있다: **"이야기 보안 플랫폼(공용 장부 위에 여러 프로그램을 얹는 구조). 플랫폼부터 만들지 않고, 제품 A의 그래프 엔진·이야기 생성기·정책 엔진을 처음부터 분리 가능하게 만든다."** 랙은 이 구상을 대신하는 새 계획이 아니라 그 한쪽 면이다.

| | 공용 장부 (기존 구상) | 랙 (이 문서) |
|---|---|---|
| 다루는 것 | 프로그램들이 **함께 쓰는 자료** (출처 그래프, 근거 이벤트) | 프로그램들이 **서로 말하는 방법** (장착, 통신, 인계, 승인) |
| 지금 상태 | 스토리방화벽 안의 그래프 엔진. 분리 가능하게 만드는 중 | 스토리방화벽의 서비스 ↔ 화면 규격이 원형 |

- **랙 1.0에는 공용 장부가 없다.** 모듈은 각자 자기 저장소를 갖고, 대상은 `entities`(6.6)라는 공용어로만 주고받는다. 한 모듈이 다른 모듈의 DB를 직접 읽지 않는다.
- **공용 장부가 생기면 그것도 슬롯에 꽂는 모듈 하나가 된다.** 그때 `entities`에 장부의 노드 ID를 가리키는 칸을 더한다(마이너 판 증가로 가능). 지금 규격을 바꿀 필요가 없다.
- **"플랫폼부터 만들지 않는다"는 원칙을 그대로 따른다.** 스토리방화벽의 현재 순서(기준판을 쓰며 고치는 짧은 순환, MVP 단계 순서)를 랙 작업이 앞지르지 않는다. 스토리방화벽 쪽 랙 작업은 읽기 전용 `rack.*` 추가(액션 플랜 2단계)까지가 첫 범위이고, 시작 시점은 스토리방화벽 일정에 맞춘다.
- **OCSF와의 관계:** 지침은 외부 연동·내보내기 때 OCSF로 변환하도록 정했다. `rack.event`는 같은 PC 안의 모듈끼리 주고받는 요약 형식이며 OCSF를 대신하지 않는다. PC 밖으로 내보낼 때는 지금 정한 대로 OCSF를 쓴다.
- **MCP와의 관계:** 지침의 "MCP 서버: 기본 읽기 전용, 차단·삭제·복원은 사용자 승인 필수"는 랙의 "변경은 주인 모듈이 승인받는다"(6.8)와 같은 규칙이다.
- [검증] 드라이브 기획 문서(이야기형 연결 분석 기획서, 제품A 개발 명세)의 장기 구상 장과 이 절이 어긋나지 않는지는 스토리방화벽 개발 세션의 검토가 필요하다. 이 문서는 지침의 한 줄만 근거로 썼다.

---

## 5. 슬롯 등록과 발견

### 5.1 등록부

| 모듈 종류 | 등록 위치 | 쓰는 주체 |
|---|---|---|
| 서비스형(스토리방화벽) | `HKLM\SOFTWARE\Antioch\Modules\<id>` | 설치 프로그램·서비스(관리자) |
| 사용자 앱형(NetX) | 설치 정보는 `HKLM\...\Modules\<id>`, 실행 중 파이프 이름은 `HKCU\SOFTWARE\Antioch\Running\<id>` | 설치 프로그램 / 실행 중인 앱 |

값: `Exe`(실행 파일 전체 경로), `PipeName`(실행 중일 때), `Api`(예: `1.0`), `Kind`(`service` | `app`).

- 파이프 이름은 스토리방화벽 방식 그대로 **시작마다 무작위**(`\\.\pipe\<id>-<128비트 hex>`)로 만들고, 첫 인스턴스를 만든 뒤에 공개한다.
- 스토리방화벽은 기존 `HKLM\SOFTWARE\StoryFirewall\PipeName`을 그대로 두고, 랙 등록부에 같은 값을 한 번 더 쓴다(기존 화면 호환).

### 5.2 접속 순서

1. 섀시가 등록부에서 모듈 목록을 읽는다.
2. `PipeName`이 있으면 접속한다. 없으면 "꺼짐"으로 표시하고, 사용자가 누르면 `Exe`를 실행한다.
3. **서버 확인:** 파이프 서버 프로세스의 실행 파일이 등록된 `Exe`와 같고 서명이 유효한지 본다(서비스형은 session 0 확인 추가). HKCU는 일반 권한 프로그램이 고칠 수 있으므로 이 확인을 빼면 안 된다.
4. `rack.hello` → `rack.describe` → `rack.subscribe`.

---

## 6. 표준 API 규격 초안 (Epistle 1.0)

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
   "params":{"client":"antioch","client_version":"0.1.0","api":[1,0]}}
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

## 7. Rust 인터페이스 초안 (`epistle` 크레이트)

스토리방화벽 `crates/api`와 같은 성격이다. 타입과 검증만 있고 전송·엔진에 의존하지 않아 리눅스에서 전부 시험한다.

```rust
//! epistle: Epistle 1.0 메시지 타입. 전송(파이프)·인증은 각 모듈이 맡는다.
use serde::{Deserialize, Serialize};

pub const EPISTLE_API: (u32, u32) = (1, 0);

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
| 등록부 기록 | `crates/svc/src/win/pipe_name.rs` | `HKLM\SOFTWARE\Antioch\Modules\storyfirewall`에도 공개 |
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

**전제 [확정 — 스토리방화벽 지침, 사용자 결정 2026-11-07]:** 상용 코드 서명 인증서(OV·EV)는 사지 않는다. 공개 배포판도 자체 서명이며 SmartScreen 경고는 받아들인다. 이 문서의 "서명"은 모두 **자체 서명**을 뜻하고, 인증서 구입을 전제로 한 안은 없다.

지금 스토리방화벽 서비스는 서명자 CN이 `StoryFirewall`인 프로그램만 받는다. 섀시와 NetX가 붙으려면 둘 중 하나가 필요하다.

- (가) 세 프로그램을 **같은 자체 서명 키(같은 서명자 이름)** 로 서명한다. 가장 단순하다. [제안]
- (나) 서비스가 허용 서명자 목록을 갖는다(`StoryFirewall`, `NetX`, `Antioch`). 프로그램마다 키를 따로 쓸 때의 방식이다.

자체 서명에서 따져야 할 것:

- **이 검사가 막는 것과 못 막는 것.** 서명자 확인은 "우리 키로 서명한 프로그램만 붙는다"를 보장한다. 공인 인증서가 아니므로 "우리가 누구인지"를 남에게 증명하지는 못한다(그건 SmartScreen 경고로 드러나는 부분이고 이미 받아들인 것).
- **키가 새면 누구나 랙 구성원 행세를 할 수 있다.** 서명 키는 지금처럼 사용자 PC와 저장소 비밀에만 두고, 개발 세션이 다루지 않는다. (가)는 키 하나가 세 프로그램을 모두 대표하므로 유출 피해가 더 넓다. (나)는 관리할 키가 늘어난다.
- **개발판과 공개판은 서로 붙지 않게 한다.** 스토리방화벽에는 시험 인증서 CN을 받는 `test-signer` 빌드 경로가 있고 배포 빌드에서는 켜지 않는다. NetX·섀시의 개발 빌드도 같은 시험 서명으로 맞춰, 개발판은 개발판끼리만 연결되게 한다.
- [검증] 서비스가 화면을 인증할 때 쓰는 서명 검사(오프라인 WinVerifyTrust + 서명자 CN)가 자체 서명 키에서 어떤 조건으로 "유효"가 되는지, 공개판 안내문의 "설치 파일은 이 릴리스 한 번만 쓰고 버린 자체 서명 키로 서명"과 어떤 관계인지 스토리방화벽 개발 세션과 노트북에서 확인한다. 릴리스마다 키가 바뀐다면 (가)는 "같은 릴리스 묶음끼리만 붙는다"는 뜻이 되어 세 프로그램의 배포 주기를 묶게 된다.
- NetX 개발 빌드에는 지금 서명 절차가 없다. 랙에 붙이기 전에 빌드 절차에 넣어야 한다.

### 8.4 라이선스와 배포 [확정 2026-11-12 — 판매하지 않는다]

**기획자 결정(2026-11-12): 모든 프로그램을 판매하지 않는다.** 스토리방화벽, NetX, 안디옥과 앞으로의 모듈 모두 AGPL-3.0-only로 소스를 공개하고 누구에게나 무료로 나눈다. 이전 판의 "최종 목표는 판매", 유료 범위, 이중 라이선스에 관한 내용은 효력이 없다. 아래 본문에서 판매를 전제로 한 문장(1번, 2번의 뒷부분, 마지막 줄)은 기록으로만 남긴다. 라이브러리와 자료를 고를 때의 기준은 "공개 저장소에 올려 누구나 다시 배포할 수 있는가"다.

#### 이전 판의 검토 기록

확인한 사실:

| | 스토리방화벽 | NetX |
|---|---|---|
| 라이선스 | AGPL-3.0-only (저장소에 명시) | **AGPL-3.0-only [확정 2026-11-11 "스토리방화벽과 동일"]** — LICENSE 추가함 |
| 저장소 | 코드·개발 빌드 공개 유지 (사용자 결정 2026-11-07) | 공개 (같은 방침) |
| 목표 | 판매하지 않는다. 무료 공개 [확정 2026-11-12] | 스토리방화벽과 동일 |

따져야 할 것:

1. **공개(AGPL)와 판매는 함께 갈 수 있다.** 저작권자는 자기 코드를 AGPL로 공개하면서 같은 코드를 다른 조건으로 팔 수 있다(이중 라이선스). 다만 **남의 기여를 받으면** 그 부분은 기여자의 저작물이라, 기여자 동의 없이는 다른 조건으로 팔 수 없다. 외부 기여를 받을 계획이면 받기 전에 동의 절차를 정해야 한다.
2. **두 프로그램의 방침은 같다 [확정 2026-11-11].** 스토리방화벽과 NetX 모두 소스를 AGPL-3.0-only로 공개하고, 누구나 무료로 받아 쓸 수 있으며, 최종 목표는 판매다. **무엇을 유료로 할지의 구체적인 범위**(예: 개인용은 무료, 기업용 기능·지원은 유료)는 스토리방화벽 지침에도 아직 적혀 있지 않아 두 프로그램 공통의 미정 사항으로 남는다. 정해지면 이 폴더에 공통 규칙으로 올린다.
3. **`epistle`(공용 규격)는 스토리방화벽에서 떼어 별도 저장소·별도 라이선스로 둔다.** NetX나 앞으로의 모듈이 스토리방화벽의 `api` 크레이트를 그대로 쓰면 그 배포물에 AGPL 조건이 따라온다. 규격은 널리 쓰일수록 좋으므로 허용적 라이선스(MIT 또는 Apache-2.0)를 제안한다. `epistle`의 의존성은 serde, serde_json, thiserror뿐이고 모두 MIT/Apache-2.0이라 상용 배포에 걸리는 것이 없다.
4. **NetX에 LICENSE를 넣었다(AGPL-3.0-only, 스토리방화벽과 같은 파일).** 두 프로그램이 같은 라이선스라 코드를 서로 자유롭게 옮겨 쓸 수 있다. 그래서 `epistle`을 AGPL로 두어도 이 두 프로그램 사이에는 문제가 없다. 그래도 3번의 허용적 라이선스 제안은 유지한다: 앞으로 남이 만든 모듈이 안디옥에 붙을 때 규격 크레이트 때문에 그 모듈 전체가 AGPL이 되는 일을 피하기 위해서다. 이 선택은 미정으로 남긴다.
5. NetX는 외부 도구(nmap, tshark 등)를 **실행**할 뿐 묶어 배포하지 않는다. 설치 마법사가 받아 오게 하는 지금 방식이면 그 도구들의 라이선스가 NetX 배포물에 옮겨 오지 않는다. 설치 파일에 동봉하게 되면 도구별로 다시 확인한다.

법률 판단은 제가 대신할 수 없다. 판매를 시작하기 전에 조건을 전문가에게 한 번 확인하시기 바란다.

---

## 9. 액션 플랜

| 단계 | 할 일 | 완료 기준 |
|---|---|---|
| 0 | 이 초안 확정: 8.3 서명, 8.4 라이선스, 10장 결정 | 결정 사항이 문서에 [확정]으로 바뀜 |
| 1 | `epistle` 크레이트: 7장 타입 + 스토리방화벽 `rpc.rs`·`page.rs` 이전, 직렬화·검증 시험, JSON 예시 고정 시험 | 리눅스에서 `cargo test` 통과. 스토리방화벽이 이 크레이트를 써서 기존 시험 그대로 통과 |
| 2 | 스토리방화벽에 `rack.hello`·`describe`·`health`·`widgets`·`subscribe` 추가(읽기만) | 시험용 클라이언트(`svc-dev`)로 다섯 메서드 응답 확인. 노트북에서 실제 서비스로 확인 |
| 3 | 읽기 전용 섀시 `antioch`(Tauri 2 + React): 등록부 읽기, 상태등, Bento 위젯 2~3개, 통합 사건 목록, "오늘의 말씀" 위젯(11장) | 노트북에서 스토리방화벽 한 대가 꽂힌 랙 화면이 실시간으로 갱신됨. 말씀 위젯이 인터넷 없이 뜸 |
| 4 | NetX `bridge/`: 파이프 서버 + `rack.*` 읽기 | 랙에 두 번째 장비가 뜸 |
| 5 | 인계 양방향: `diagnose.target`, `firewall.block.request` | 사건 카드 → NetX 제안 카드, 요새 도면 → 방화벽 확인 창. 둘 다 사용자가 누르기 전에는 아무것도 실행되지 않음 |
| 6 | `rack.action.*`, `rack.open`, 위젯 종류 확장 | 섀시에서 긴급 차단을 시작해 방화벽 UAC로 확정 |
| 이후 | 관제 모듈(12장, NetX 구성도 1-C가 끝난 뒤), NAC 모듈(13장, 관제 뒤), MCP 어댑터(랙 → AI 에이전트), 원격 관측소 게이트웨이 | 선택 |

단계 1~3은 스토리방화벽에 읽기 기능만 더하므로 기존 동작을 건드리지 않는다.

---

## 10. 결정이 필요한 사항

| # | 질문 | 이 문서의 기본값 |
|---|---|---|
| 1 | IPC 방식 | 네임드 파이프 + JSON-RPC 2.0 (현행 승격) |
| 2 | 브로커 유무 | 없음. 등록부 + 직접 연결 |
| 3 | 섀시를 새 앱으로 만들지, 스토리방화벽 화면을 키울지 | 새 앱 `antioch`. 스토리방화벽 화면은 방화벽 전용으로 유지 |
| 4 | 저장소 | 새 공개 저장소 하나에 `epistle` + `antioch` |
| 5 | 랙 구성원 서명 | (가) 같은 자체 서명 키. 인증서는 사지 않는다(확정). 릴리스마다 키가 바뀌는지 확인한 뒤 (가)/(나) 결정 |
| 6 | `epistle` 라이선스 | 스토리방화벽과 분리, MIT 또는 Apache-2.0 |
| 6-1 | NetX 라이선스 | **[확정 2026-11-11]** AGPL-3.0-only, 스토리방화벽과 동일 |
| 6-2 | 무료 배포와 판매의 범위 | **[확정 2026-11-12]** 모든 프로그램을 판매하지 않는다. AGPL 공개, 누구에게나 무료 (8.4). 2026-11-11의 "최종 목표 판매"를 바꾼 결정 |
| 7 | 플랫폼 이름 | **[확정 2026-11-11]** 플랫폼 "안디옥(Antioch)", 통신 규격 "에피스틀(Epistle)". 판매 전 상표 조회는 남음(0장) |
| 8 | "겨자씨 CIMS" Bento 화면 | 기존 코드가 있으면 위치를 알려 주시면 6.5의 위젯 계약을 거기에 맞춤 |
| 9 | 말씀 번역본(언어별) | **[확정 2026-11-11]** 한국어는 성경전서 개역한글판(침례 표기). 새번역·개역개정은 쓰지 않는다(문의도 하지 않음). 다른 언어는 사용 조건이 확인된 번역본만, 나중에 |
| 10 | 말씀 표시 기본값 | **[확정 2026-11-11]** 항상 표시. 끄는 설정 없음, 개인용·기관용 구분 없음, 첫 실행 안내·동의 창 없음 |
| 11 | 구절 목록 | **[확정 2026-11-11]** 100곳(140절), 여덟 주제. 자료 파일 `ko.json`(11.3). 네 곳은 종이 성경 대조가 남음 |
| 12 | "정보" 화면 헌정 문구, 교회 안내 | **[확정 2026-11-11]** 11.6. 모든 프로그램이 같은 문구. 프로그램별 한 줄·영상 채널 링크는 넣지 않음 |
| 13 | 관제 모듈을 NetX 안의 기능으로 둘지, 별도 프로그램으로 둘지 | **[확정 2026-11-11]** 별도 모듈. SNMP 수집 코드는 NetX와 공용 크레이트로 (12.6) |
| 14 | 관제 모듈의 이름 | 미정 (임시 id `watch`) |
| 15 | NAC 3단계의 차단 방식 | 미정. ARP 방식은 하지 않는다. 관리형 스위치가 없는 소규모에서는 3단계가 사실상 불가능하다 (13.6, 13.10) |
| 16 | 조직판(중앙 관리)을 유료로 할지 | **[확정 2026-11-12]** 유료로 하지 않는다. 판매하지 않는다 (8.4) |
| 17 | NAC 모듈의 이름 | 미정 (임시 id `nac`) |
| 18 | AI 에이전트 안전망의 형태 (세 구상 중 첫 번째 순서) | 별도 프로그램이 아니라 스토리방화벽의 기존 에이전트 감시 기능을 넓히는 것 (14.3). 스토리방화벽 개발 세션의 검토 필요 |
| 19 | 민감 폴더 열기 감시를 넣을지 | PoC로 누락률과 양을 확인한 뒤 결정. 넣어도 "보조 지표"로만 (14.4의 1번) |
| 20 | NAC의 형태: 조직 내부 중앙 서버 / 중앙 서버 없음 | **[확정 2026-11-11]** 기본은 "중앙 서버 없음"(13.10의 9번). 중앙 서버 형태는 전담자가 있는 조직이 요청할 때를 위해 문서에만 남긴다 |
| 22 | 말씀 언어 | **[확정 2026-11-12]** 설치 때 언어를 고르고 그 언어의 말씀을 보인다. 화면 언어와 말씀 언어는 따로 저장. 첫 범위는 한국어 + 영어(World English Bible) (11.9) |
| 21 | NAC 2·3단계를 계속 구상에 둘지 | **[확정 2026-11-11]** 폐기하지 않고 미룬다. 조건: 관제 모듈이 실제로 쓰이기 전에는 손대지 않는다 |

---

## 11. "오늘의 말씀" 모듈 [확정 2026-11-11 — 표시 방침·번역본·구절·문안. 크레이트 모양은 제안]

목적: 이 프로그램들은 맑은물속교회가 열방 선교를 위해 만들어 공개한다. 쓰는 사람이 일하는 틈에 말씀을 만나고, 누가 왜 만들었는지 알 수 있게 한다. 방식은 **드러내 놓고, 판단하는 순간은 방해하지 않게**다. 말씀은 항상 표시하며 끄는 설정을 두지 않는다(기획자 결정: 공개용 프로그램이고, 쓰는 것 자체를 동의로 본다).

### 11.1 형태

- 독립 프로세스가 아니라 **공용 크레이트 `antioch-word`** 로 만든다. 구절 자료와 고르는 규칙만 들어 있다. 네트워크 코드가 없다.
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
| "정보" 화면 | 헌정 문구(11.6), [교회 안내], [소스 코드] |

**넣지 않는 자리(고정 규칙):** 차단·종료 확인 창, 60초 유지 확인, UAC·Windows Hello 흐름, 경고 알림과 `rack.event`, 진단 결과 브리핑과 원본 출력, 감사 로그. 사용자가 판단해야 하는 순간과 기록에는 말씀을 섞지 않는다.

계속 흘러가는 자막은 넣지 않는다. 내보내는 파일에도 넣지 않는다.

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

- 성경 전체가 아니라 **선정한 구절 목록**을 싣는다. 한국어는 100곳(140절)으로 확정했다: 복음의 핵심 20, 초대 10, 지키시는 하나님 15, 위로 14, 말씀 10, 지혜 10, 열방 10, 새 생명 11. 구절마다 `group`(주제)과 `long`(긴 구절 여부)을 갖는다. 365곳 확장은 나중에 정한다.
- 자료 묶음(`ko.json`, `dedication.ko.md`, `church.ko.md`, 규칙 `README.md`)은 NetX 저장소 `word/`, 스토리방화벽 저장소 `docs/proposals/word/`에 같은 내용으로 있다. `antioch-word` 크레이트가 생기면 그 안으로 옮긴다.
- `id`는 언어와 무관한 공통 값이다(책 약어.장.절). 언어 파일마다 같은 `id` 목록을 갖는다 → 같은 날 어느 나라에서나 같은 구절이 그 나라 말로 나온다.
- **본문은 번역본 원문 파일에서 스크립트로 뽑는다.** 사람이든 AI든 기억으로 옮겨 적지 않는다. 뽑은 뒤 원문과 글자 단위로 대조하는 시험을 둔다.
- `license`와 `source`가 비어 있는 언어 파일은 빌드가 실패하게 한다. 사용 조건을 확인하지 않은 번역본이 배포물에 들어가지 않게 하는 장치다.
- 번역본 [확정]: 한국어는 **성경전서 개역한글판**. 대한성서공회 저작권 안내의 표에 저작재산권 보호기간이 2011-12-31에 끝난 것으로 적혀 있다(2026-11-11 열람). 마태복음 28:19는 기획자 결정으로 "침례"로 적고, 번역본 이름을 "성경전서 개역한글판(침례 표기)"로 밝힌다. 새번역·개역개정은 허락과 저작권료가 필요해 쓰지 않는다. 그 밖의 언어는 언어마다 자유 이용 번역본이 있는지 확인한 것만 추가하고, 없는 언어는 다른 언어로 대신하지 않고 표시를 생략한다.
- 본문 출처: 공개 저장소의 개역한글 자료 두 벌에서 프로그램으로 뽑아 서로 대조했다(70곳 일치, 30곳은 띄어쓰기·옛 표기·시편 표제 차이). 성서공회가 낸 원본 파일은 아니다. 글자가 달랐던 네 곳(이사야 55:1, 시편 121:7-8, 히브리서 4:12, 사도행전 1:8)은 종이 성경 대조가 남아 있다.

### 11.4 고르는 규칙

- **오늘의 말씀:** 날짜(사용자 PC의 현지 날짜)로 계산한다. 무작위가 아니다 → 인터넷 없이도 모든 프로그램이 같은 날 같은 구절을 보인다.
  - `day` = 현지 날짜가 1970-01-01부터 며칠째인지. `G` = 복음의 핵심 20곳, `O` = 나머지 80곳(파일 순서).
  - `day % 3 == 0`이면 `G[(day / 3) % 20]`, 아니면 `O[(day - day / 3 - 1) % 80]`. 사흘에 한 번은 복음의 핵심 구절이 나온다.
  - 확인 값: 2026-11-11 → `jhn.7.37-38`, 2026-11-12 → `rom.6.23`, 2026-12-25 → `jhn.14.27`. 전체 표는 자료 묶음의 `README.md`.
- **기다리는 화면·빈 화면·시작 화면:** `long`이 아닌 96곳에서 차례로 돌린다. 한 화면에서 10초보다 빨리 바꾸지 않는다.
- `long`인 구절(4곳)은 위젯의 "전체 보기"에서만 보인다. 본문을 잘라서 보여 주지 않는다.

### 11.5 설정 없음, 그리고 지키는 선 [확정]

- 말씀 표시를 켜고 끄는 설정을 만들지 않는다. 개인용·기관용 구분, 첫 실행 안내·동의 창도 없다. 말씀 언어는 설치 때 고른 값을 따르고 설정에서 바꿀 수 있다(11.9). 바꾸는 설정이지 끄는 설정이 아니다.
- **아무것도 밖으로 보내지 않는다.** 구절은 앱에 내장하고, 몇 번 보였는지 세거나 전송하지 않는다.
- 11.2의 "넣지 않는 자리"는 예외 없이 지킨다. 이것은 선교 방침이 아니라 안전 규칙이다.
- 기록: 설계 검토 때 "기관·사업장 PC에서는 끌 수 있어야 한다"는 우려가 나왔으나 기획자가 예외 없이 표시하기로 결정했다. 다시 묻지 않는다.

### 11.6 "정보" 화면 헌정 문구와 교회 안내 [확정 2026-11-11]

> "오직 성령이 너희에게 임하시면 너희가 권능을 받고 예루살렘과 온 유대와 사마리아와 땅끝까지 이르러 내 증인이 되리라 하시니라"
> — 사도행전 1:8 (성경전서 개역한글판)
>
> 이 프로그램은 기독교한국침례회 평창 맑은물속교회가 만들어 누구에게나 값없이 나눕니다.
>
> 맑은물속교회는 말씀의 맑은 물속에 깊이 잠기고, 복음의 순수함을 지키며, 그 생명을 세상으로 흘려보내도록 부름받은 교회입니다. 이 프로그램은 교회 담장을 넘어 흘러가는 물줄기 가운데 하나입니다.
>
> 성령께서 저희를 평창에서 사마리아와 땅끝까지 예수 그리스도의 증인으로 세우시기를 구하며, 이 프로그램을 그 길에 드립니다. 프로그램 곳곳의 성경 말씀이 그분을 아는 길이 되기를 바랍니다.
>
> 이 PC의 기록은 밖으로 보내지 않으며, 소스 코드는 공개되어 있습니다(AGPL-3.0).
>
> [교회 안내] [소스 코드]

- 모든 프로그램이 같은 문구를 쓴다. 프로그램마다 덧붙이는 한 줄은 없다.
- [교회 안내]의 문안은 자료 묶음의 `church.ko.md`다: 교회 이름(기독교한국침례회 평창 맑은물속교회 · LIVING WATERS CHURCH), 소개, 세 가지 부르심, 만든 이유, 연락 전자우편. 번지 주소와 설교 영상 채널은 넣지 않는다.
- 문구를 다시 타이핑하지 않고 파일을 그대로 넣는다.

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
// antioch-word 크레이트 (네트워크·파일 쓰기 없음)
pub struct Verse { pub id: &'static str, pub reference: &'static str, pub text: &'static str, pub group: &'static str, pub long: bool }
pub struct Edition { pub lang: &'static str, pub version: &'static str, pub license: &'static str }

pub enum Place { Widget, Waiting, Empty, Startup }
// 설정 구조체는 없다: 항상 표시한다 (11.5)

/// 그 언어의 번역본이 실려 있으면 돌려준다. 없으면 None (다른 언어로 대신하지 않는다)
pub fn edition(lang: &str) -> Option<&'static Edition>;
/// 오늘의 말씀: 현지 날짜로 결정 (같은 날 같은 구절)
pub fn today(lang: &str, local_date: (i32, u32, u32)) -> Option<&'static Verse>;
/// 기다리는 화면용: long 이 아닌 구절 가운데 n번째
pub fn rotate(lang: &str, n: usize) -> Option<&'static Verse>;
```

### 11.8 순서

1. ~~번역본 사용 조건 확인(한국어) → 구절 목록 선정~~ 끝남(2026-11-11). 자료 묶음을 두 저장소에 올렸다. 남은 것: 네 곳 종이 성경 대조.
2. `antioch-word` 크레이트와 단위 시험(날짜 결정 확인 값, `long` 제외, 없는 언어). 크레이트가 생기기 전에는 각 프로그램이 자료 묶음의 규칙대로 직접 구현한다.
3. 섀시 위젯(액션 플랜 3단계에 포함).
4. NetX 기다리는 화면·빈 화면 → 스토리방화벽 화면 순으로 적용. 각 프로그램의 "정보" 화면에 헌정 문구.

### 11.9 언어 선택 [확정 2026-11-12 — 방향. 저장 위치와 설치 화면 구현은 제안]

기획자 결정: 내려받아 설치할 때 사용할 언어를 고르게 하고, 고른 언어의 성경 말씀을 표시한다. 스토리방화벽에도 똑같이 적용한다.

1. **설치 화면 첫 장에서 언어를 고른다.** 기본 선택은 Windows 표시 언어다.
2. **목록에는 말씀 자료가 실린 언어만 나온다.** "없음" 항목은 없다. 어느 언어를 골라도 말씀이 표시된다.
3. **화면 언어와 말씀 언어를 따로 저장한다.** 화면이 번역되지 않은 언어를 고르면 화면은 영어(영어 화면도 없으면 한국어)로, 말씀은 고른 언어로 보인다.
4. **저장 위치 [제안]:** 설치 프로그램이 `HKLM\SOFTWARE\Antioch`에 `WordLang`, `UiLang` 값을 쓴다. 모든 프로그램이 이 값을 함께 읽으므로 두 번째 프로그램을 설치할 때는 미리 선택돼 있다. 사용자가 설정에서 바꾸면 `HKCU\SOFTWARE\Antioch`의 같은 이름 값에 쓴다(일반 권한으로 쓸 수 있는 자리).
5. **읽는 순서:** HKCU → HKLM → Windows 표시 언어 → 영어 → 한국어. 앞의 값이 이 프로그램에 실린 언어 목록에 없으면 다음으로 넘어간다. 이전 판의 "없는 언어는 표시를 생략한다"는 "항상 표시"와 맞지 않아 이렇게 바꾼다.
6. **레지스트리 값은 자료다.** 프로그램에 내장된 언어 목록과 글자 그대로 비교만 한다. 파일 경로나 명령으로 쓰지 않는다. HKCU는 다른 프로그램도 고칠 수 있기 때문이다.
7. **언어 파일의 조건:** 한국어 파일과 같은 `id` 100곳을 같은 순서로 갖는다. `license`와 `source`가 비어 있으면 빌드가 실패한다. `long`은 언어마다 따로 정한다(길이가 다르므로). "오늘의 말씀"은 `id`로 정하므로 어느 언어에서나 같은 날 같은 곳이 나온다.
8. **장·절 번호가 번역본마다 다를 수 있다**(예: 러시아어 Synodal의 시편 번호). 언어를 더할 때 번호 대응을 확인하고, 뽑은 본문은 그 언어를 아는 사람이 확인한다.
9. **헌정 문구와 교회 안내**도 언어별 파일로 둔다. 기획자가 확정한 언어만 싣고, 없는 언어는 영어 문안(영어도 확정 전이면 한국어)을 보인다. 기계 번역만으로 싣지 않는다.
10. **첫 범위:** 한국어 + 영어. 영어 번역본은 World English Bible(저작권 없음, eBible.org 안내 2026-11-12 열람. 이름은 상표라 본문을 고치면 쓸 수 없다). NIV는 저작권자의 허락이 필요하고, 본문을 공개 저장소에 올릴 수 없어 쓰지 않는다. KJV는 영국에서 왕실 권리가 남아 있어 차선으로 둔다.
11. **뒤로 미루는 것:** 오른쪽에서 왼쪽으로 쓰는 언어(아랍어, 히브리어), 세 번째 이후 언어.
12. [검증] Tauri의 NSIS 설치 파일이 언어 선택 창과 설치 중 레지스트리 쓰기를 지원하는 방식, 스토리방화벽 설치 파일에서 같은 방식이 되는지는 각 개발 세션이 공식 문서로 확인한 뒤 구현한다. 안드로이드판은 설치 화면이 없으므로 기기 언어를 따르고 설정에서 바꾼다.

---

## 12. 세 번째 모듈 "관제" 구상 [구상 — 기획자 제안 2026-11-11, 설계 전]

기획자 제안: 종합 SNMP 솔루션 또는 관제 센터 개념의 솔루션을 안디옥에 추가한다. 이 장은 범위와 경계만 잡은 구상이며, 화면·자료 형식 같은 상세 설계는 아직 하지 않았다.

### 12.1 자리

| | 스토리방화벽 | NetX | 관제 |
|---|---|---|---|
| 보는 범위 | 이 PC 한 대의 안쪽 | 사용자가 지정한 대상 | 네트워크의 장비 전체(스위치, AP, 공유기, 서버, 프린터) |
| 보는 때 | 상시 | 사람이 실행할 때 | 상시 |
| 하는 일 | 이야기로 보여 주고 차단 | 진단하고 추적 | 상태를 지켜보고 알림 |
| 형태 | 서비스 + 화면 | 사용자 앱 | 서비스 + 화면 |

안디옥의 Bento 위젯과 통합 사건 목록에 가장 많은 자료를 내는 모듈이 된다. 통합 화면이 실제 관제 화면이 되는 것은 이 모듈이 꽂힌 뒤다.

### 12.2 첫 범위

넣는 것 세 가지:

1. **장비 목록.** 사용자가 등록하거나, NetX 구성도에서 찾은 장비를 넘겨받는다.
2. **상태 감시.** ICMP 응답, SNMP `sysUpTime`, 인터페이스 상태(`ifOperStatus`), 인터페이스 트래픽(`ifHCInOctets`/`ifHCOutOctets`). 기본 주기 60초.
3. **알람.** 장비 무응답, 포트 down, 재부팅(가동 시간이 줄어듦), 트래픽 임계치 초과.
4. **승인 목록과 새 단말 알림.** 네트워크에 보이는 단말(MAC, IP, 이름)을 목록으로 만들고, 사용자가 승인한 목록에 없는 단말이 나타나면 알린다. 차단은 하지 않는다. 이것이 NAC의 1단계다(13.2).

첫 범위에서 빼는 것: SNMP Trap 수신, 장비 설정 변경(SNMP SET), syslog 수집, NetFlow/sFlow, 장비 설정 백업, 원격 여러 거점. 필요가 확인되면 뒤에 넣는다.

### 12.3 구조

- 수집은 Windows 서비스, 화면은 일반 권한 앱. 스토리방화벽과 같은 분리다.
- 저장은 SQLite. 원시 측정값은 짧게(예: 7일), 시간·하루 단위 요약은 길게 둔다. 보관 기한을 처음부터 정해 DB가 끝없이 자라지 않게 한다.
- **이 PC가 꺼져 있던 동안은 감시 공백이다.** 공백을 "장애 없음"으로 보이지 않게, 그래프와 사건 목록에 "감시 안 함" 구간으로 표시한다.
- 화면 조회는 실제 규모(장비 수십 대, 포트 수백 개, 측정값 수백만 줄)에서 2초 안에 응답하도록 요약 표를 주기 작업으로 만든다(스토리방화벽 지침의 같은 원칙).

### 12.4 에피스틀 연결

```json
{"id": "watch", "title": "(이름 미정) 관제", "kind": "service",
 "capabilities": ["events", "widgets", "handoff", "open"],
 "widgets": [
   {"id": "devices", "title": "장비 상태", "kind": "status", "sizes": ["1x1", "2x1"], "refresh_sec": 60},
   {"id": "alarms", "title": "열린 알람", "kind": "list", "sizes": ["2x2"], "refresh_sec": 30},
   {"id": "uplink", "title": "회선 트래픽", "kind": "spark", "sizes": ["2x1"], "refresh_sec": 60}
 ],
 "intents_in":  ["watch.add.device"],
 "intents_out": ["diagnose.target"]}
```

- 사건 종류: `device.down`, `device.up`, `device.rebooted`, `port.down`, `port.up`, `traffic.threshold`. 대상은 공용어 `device`(6.6)로 적는다. 포트를 가리키려면 `device`에 `port` 칸을 더한다(마이너 판 증가).
- 인계: 관제 → NetX `diagnose.target`(알람이 난 장비를 정밀 진단), NetX → 관제 `watch.add.device`(구성도에서 찾은 장비를 감시 목록에 넣기, 관제 쪽에서 사용자가 확인해야 등록됨), 관제·NetX → 스토리방화벽 `firewall.block.request`(기존 그대로).
- 세 모듈이 한 줄로 이어진다: **관제가 발견 → NetX가 진단 → 스토리방화벽이 차단.** 각 단계는 사용자가 눌러야 넘어간다.

### 12.5 보안

- **읽기만 한다.** SNMP GET/GETNEXT/GETBULK만 쓰고 SET은 구현하지 않는다. 관제 모듈이 장비 설정을 바꾸는 길을 열지 않는다.
- **SNMPv3(인증 + 암호화)를 기본으로 한다.** v2c는 커뮤니티 문자열이 평문으로 오가므로, 고를 때 그 사실을 알리고 허용한다(v3를 지원하지 않는 장비가 많다).
- 자격 증명(커뮤니티 문자열, v3 사용자·암호)은 사용자가 직접 넣고 Windows 자격 증명 저장소에 둔다. 화면으로 되돌려 보내지 않고, 로그·저장소·에피스틀 메시지에 싣지 않는다.
- 감시 대상은 사용자가 등록한 장비만이다. 대역을 훑어 자동으로 감시 목록에 넣지 않는다. 사설 대역이 아닌 주소는 등록할 때 한 번 더 확인받는다.
- **Trap 수신(UDP 162)은 포트를 여는 일이다.** 첫 범위에서 빼고, 넣을 때도 기본은 끔으로 한다.
- 장비에서 온 문자열(sysName, sysDescr, 인터페이스 이름)은 자료로만 다룬다. 화면에는 글자로만 표시하고, AI에 넘길 때 명령으로 취급하지 않는다(스토리방화벽 지침의 같은 원칙).
- 스토리방화벽의 가드에 관제의 폴링(ICMP, UDP 161)을 실수로 막지 않는 규칙이 필요하다.

### 12.6 NetX와의 경계 [확정 2026-11-11 — 별도 모듈]

NetX의 진행 중 작업 1-C(SNMP/LLDP로 스위치·AP 연결 관계 그리기)가 관제의 씨앗이다. 두 가지 길이 있다.

- (가) 관제를 NetX 안의 기능으로 둔다. 프로그램이 하나라 간단하지만, NetX는 사용자가 실행할 때만 도는 앱이라 상시 감시와 맞지 않는다.
- (나) 관제를 별도 모듈로 둔다. **[확정 — 기획자 결정 2026-11-11]** SNMP 클라이언트 코드는 NetX 1-C에서 먼저 만들어 공용 크레이트로 떼고, 관제 모듈이 그것을 가져다 쓴다. NetX는 "지금 한 번 조사", 관제는 "계속 지켜봄"으로 역할이 갈린다.

NetX 설계서 9.2의 "감시 모드와 알림", "네트워크 지도", "회선 품질 일지"는 관제 모듈의 기능으로 옮긴다. NetX 1-C에서 SNMP 코드를 만들 때는 NetX의 다른 모듈(화면, 실행 엔진, 검증기)에 기대지 않는 독립 모듈로 짜서, 나중에 그대로 공용 크레이트로 뗄 수 있게 한다.

### 12.7 스토리방화벽 "제품 B"와의 관계

스토리방화벽 지침의 제품 B는 노트북을 사업장 스위치의 SPAN/TAP에 연결해 **패킷**으로 점검하는 앱이다. 관제는 장비에 **SNMP로 물어보는** 방식이라 수집 원리가 다르고 겹치지 않는다. 대상(사업장 네트워크)은 같으므로, 제품 B를 만들게 되면 안디옥의 또 다른 모듈이 되어 관제와 같은 장비 목록을 쓴다.

### 12.8 순서와 규모

- 제대로 된 NMS는 그 자체로 제품 하나 크기다. 12.2의 세 가지에서 시작하고, 쓰면서 넓힌다.
- 시작 시점은 NetX 1-C가 `main`에 올라간 뒤다. 그 전에는 설계만 한다.
- 라이선스는 다른 모듈과 같이 AGPL-3.0-only. SNMP는 NetX 1-C 방침대로 Rust로 직접 구현해 외부 라이브러리 의존을 두지 않는다.
- 기존 도구(Zabbix, LibreNMS, PRTG 등)와 겨루는 지점은 기능 수가 아니라 **"서버 없이 Windows PC 한 대에 설치해 바로 쓰는 가벼움"** 과 NetX·스토리방화벽으로 이어지는 흐름이다. 대상은 소규모 사업장, 교회, 학교, 선교지.

### 12.9 검증 통합 1차 반영 (2026-11-11)

제미나이 검증 답변에서 받아들인 것. 항목별 판단은 "통합 1차 검증 답변 반영" 문서에 있다.

1. **SNMP 해석기 방어(A1).** 중첩 깊이 상한, 길이 칸과 실제 남은 바이트의 대조, 응답 크기 상한. 보낸 요청과 출처 주소·포트·request-id가 맞지 않는 응답은 해석하기 전에 버린다. 어떤 입력에도 프로그램이 죽지 않아야 하고(색인 접근·`unwrap` 금지), 무작위 입력 시험을 단위 시험에 넣는다. NetX 1-C의 SNMP 모듈부터 적용한다.
2. **SNMPv3(A2).** 장비가 알려 준 boots·time보다 예전 값을 가진 응답은 버린다. 재시작 뒤에도 걸러낼 수 있게 마지막 값을 DB에 남긴다. v2c를 고를 때 경고: "읽기 전용이어도 같은 망의 공격자가 문자열을 얻으면 네트워크 구조를 읽어 갑니다."
3. **틀린 알람 방지(A3).** 절전에서 깨어난 뒤 두 주기는 알람을 내지 않는다. `sysUpTime`이 줄었는데 ICMP가 계속 살아 있었으면 "재부팅"이 아니라 "SNMP 에이전트 재시작"으로 표시한다. 포트는 번호(ifIndex)가 아니라 이름(ifName)으로 짝짓는다. 64비트 카운터를 먼저 쓴다. 64비트가 없는 빠른 포트는 폴링 주기 안에 카운터가 몇 번 넘쳤는지 알 수 없으므로 보정하지 않고 "측정 불가"로 표시한다.
4. **새 단말 알림(A4).** 무작위(사설) MAC은 주소의 "로컬 관리" 비트로 알아보고 한 묶음으로 알린다. 이 PC의 ARP 표로는 라우터 너머를 못 보므로, 등록한 공유기·L3 스위치의 SNMP ARP 표를 단말 목록의 기본 출처로 삼는다.
5. **저장(A5).** 측정값은 메모리에 모았다가 주기마다 한 번 묶어서 쓴다. 쓰기는 전용 스레드, 화면 조회는 읽기 전용 연결.
6. **예외 규칙(A6).** 스토리방화벽에 두는 관제용 예외는 "사용자가 등록한 장비 주소의 UDP 161과 ICMP"로만 좁힌다. 폴링을 한꺼번에 쏘지 않고 흩어 보낸다.
7. **한계 고지(A7).** "누가 회선을 다 쓰고 있나"는 SNMP 포트 통계만으로는 짚지 못한다는 것을 화면에 적고, NetX 진단으로 넘기는 버튼을 둔다.
8. **모듈 사이의 알림(D2).** 모듈끼리 전부 통과시키는 규칙은 두지 않는다. 대신 NetX가 스캔을 시작·종료할 때 에피스틀 사건을 내고, 스토리방화벽과 관제가 그 시간의 같은 대상에 대한 경보를 "NetX 스캔 중"으로 묶는다.

---

## 13. 네 번째 모듈 "NAC" 구상 [구상 — 기획자 제안 2026-11-11, 설계 전]

기획자 제안: 내부 단말의 네트워크 접근을 통제하는 NAC(Network Access Control) 개념의 솔루션을 추가한다. 별도 모듈이다. 이 장은 범위·단계·위험만 잡은 구상이다.

### 13.1 자리

"누가 이 네트워크에 들어올 수 있는가"를 정하는 모듈이다. 관제가 장비를 보고, NetX가 진단하고, 스토리방화벽이 PC를 지키고, NAC가 접속 자격을 정한다.

이미 가진 것을 쓴다: **스토리방화벽이 NAC의 에이전트가 된다.** PC의 상태를 알고, 문제가 있으면 스스로 통신을 끊는 기능(긴급 차단)이 이미 있다.

### 13.2 세 단계

| 단계 | 하는 일 | 차단 | 어디에 넣나 |
|---|---|---|---|
| 1. 누가 붙어 있나 | 모든 단말을 목록으로 만들고, 승인 목록에 없는 새 단말이 나타나면 알림 | 없음 | 관제 모듈의 기능 (12.2의 4번) |
| 2. PC 상태 점검 | 스토리방화벽이 설치된 PC가 자기 상태를 중앙에 보고하고, 문제가 있으면 스스로 격리 | PC가 자기 자신만 | NAC 모듈 + 스토리방화벽 조직판 |
| 3. 접속 통제 | 미승인·부적합 단말을 네트워크에서 막음 | 있음 | NAC 모듈 |

앞 단계를 실제로 써 본 뒤에 다음 단계로 간다.

### 13.3 2단계: 스토리방화벽을 에이전트로

- **보고하는 것(최소한):** 자기 식별 정보(기기 식별자, 현재 쓰는 어댑터의 IP·MAC·대역), 서비스가 돌고 있는가, 설치된 판, 열린 의심 사건의 수와 심각도, 긴급 차단 상태, 마지막 보고 시각. 식별 정보가 없으면 중앙이 어느 PC인지 알 수 없어 아무 조치도 못 한다(검증 B1).
- **보고하지 않는 것:** 연결 기록, 이야기 본문, 프로세스 목록, 접속한 도메인·주소, 파일 이름. 중앙은 "이 PC가 건강한가"만 알고 "이 PC가 무엇을 했는가"는 모른다. 자세한 내용은 그 PC의 스토리방화벽 화면에서만 본다.
- **자기 격리:** 조건(예: 심각한 사건이 열린 채 일정 시간 경과)에 해당하면 그 PC의 스토리방화벽이 긴급 차단을 건다. 중앙은 격리를 "권고"할 수 있고, 실행은 PC 쪽 스토리방화벽이 가입 때 동의한 정책에 따라 한다.
- **격리 상태의 통로(검증 B3 반영):** 들어오는 연결은 모두 막는다. 나가는 연결은 중앙·DHCP·DNS만 남긴다. 원격 관리는 **가입 때 지정한 관리 PC 주소에서 오는 것만** 남긴다(고칠 길은 있어야 한다). 파일 공유 같은 로컬 기본 서비스를 열어 두면 격리된 PC에서 옆 PC로 퍼지는 길이 되기 때문이다. 이것은 스토리방화벽의 기존 가드(원격 접속·로컬 기본 서비스를 막지 않음)와 다른 "격리 전용 규칙"이므로 스토리방화벽 개발 세션의 검토가 필요하다. [검증]

### 13.4 개인판과 조직판을 나눈다

스토리방화벽의 핵심 약속은 "이 PC의 기록을 밖으로 보내지 않는다"이고, 지침은 "외부 전송은 사용자 명시 동의 없이는 금지"다. 2단계는 PC 상태를 중앙으로 보내므로 이 약속과 부딪친다. 그래서:

- **개인판의 약속은 바꾸지 않는다.** 개인판에는 중앙 보고 기능을 켜는 길이 없다.
- **조직판은 따로 가입해야 동작한다.** 가입할 때 어느 조직이 관리하는지, 무엇을 보내는지, 어떤 조건에서 격리되는지를 보여 주고 동의를 받는다.
- 조직판 화면에는 "이 PC는 ○○이(가) 관리합니다"와 보내는 항목을 항상 볼 수 있게 둔다.

### 13.5 PC와 중앙 사이의 통신

에피스틀은 같은 PC 안의 통신 규격이다. PC와 중앙 사이는 별도 규격으로 설계한다.

- 중앙 서버는 **조직이 자기 네트워크 안에 설치**한다(셀프 호스팅). 우리가 운영하는 클라우드 중앙은 두지 않는다.
- PC가 중앙으로 **나가는 연결만** 쓴다. 중앙이 PC로 들어오는 포트를 열지 않는다.
- TLS 상호 인증. 가입할 때 PC마다 기기 인증서를 발급하고, 폐기 수단을 둔다.
- 메시지 형식은 에피스틀의 `rack.health`·`rack.event`와 같은 모양을 재사용해 중앙의 화면이 안디옥 통합 화면과 같은 위젯을 쓰게 한다.

### 13.6 3단계: 차단 방식 비교

| 방식 | 필요한 것 | 장점 | 위험·한계 |
|---|---|---|---|
| 스위치 포트 제어 | 관리형 스위치, SNMP 쓰기 권한 또는 장비 API | 확실하게 막는다. 추가 서버가 없다 | 장비 설정을 바꾼다. 잘못 막으면 현장에 가야 한다. 벤더마다 방법이 다르다 |
| 802.1X + RADIUS | 802.1X 지원 스위치·AP, RADIUS 서버 | 표준 방식. 접속하는 순간에 판정한다 | 구축이 무겁다. 프린터 같은 미지원 단말의 예외 처리가 필요하다 |
| DHCP 기반 | DHCP 서버 제어 | 간단하다 | 고정 IP를 쓰면 그냥 뚫린다 |
| 공유기 관리 기능 | 공유기의 MAC 차단 목록, 관리자 암호 | 관리형 스위치가 없는 곳에서도 된다 | 벤더마다 비공식 방법이라 판이 바뀌면 깨진다. 공유기 관리자 암호를 보관해야 한다. 유선 단말끼리의 통신은 못 막는다 |
| ARP 기반 | 같은 대역에 놓인 PC 한 대 | 장비 지원이 필요 없다 | 원리가 ARP 스푸핑이다. 오작동하면 대역 전체가 끊긴다. 보안 장비가 공격으로 탐지한다. 라우터 너머는 못 막아 대역마다 한 대씩 필요하다. **하지 않는다(검증 B6)** |

- 스위치 포트를 막는 것은 장비 설정 변경이므로 **관제 모듈("읽기만 한다")이 아니라 NAC 모듈의 기능**으로 두고, 매번 사용자 승인과 되돌리기 수단(일정 시간 뒤 자동 해제 또는 유지 확인)을 둔다. 스토리방화벽의 "60초 안에 유지를 누르지 않으면 되돌린다"와 같은 원칙이다.
- 어느 방식을 쓸지는 정하지 않았다(10장 15번).

### 13.7 보안과 책임

- **관리 권한이 있는 네트워크에서만 쓴다.** 남의 네트워크에서 단말을 막는 것은 법적 문제가 될 수 있다. 설치·가입 때 이를 알린다.
- **NAC 중앙은 가장 위험한 표적이다.** 모든 PC를 격리할 수 있는 자리이기 때문이다. 격리 권고에는 서명을 붙이고, 한꺼번에 여러 대를 격리하는 요청에는 제동(정한 대수 이상이면 추가 확인)을 둔다.
- 단말 사용자에게 관리되고 있다는 사실을 알린다(13.4). 직원 PC의 상태를 수집하는 일에는 개인정보·근로 관련 규정이 걸릴 수 있다. 법률 판단은 제가 대신할 수 없으니 조직에 배포하기 전에 확인이 필요하다.
- 승인 목록의 기준이 MAC 주소뿐이면 MAC 위장에 뚫린다. 2단계의 기기 인증서가 있는 PC와 MAC만 아는 단말을 구별해 표시한다.

### 13.8 유료 범위의 후보 [효력 없음 — 2026-11-12 판매하지 않기로 결정]

여러 PC를 중앙에서 관리하는 기능은 개인이 아니라 조직이 쓴다. "개인판은 무료, 조직판(중앙 관리·NAC)은 유료"가 자연스러운 경계가 될 수 있다. 소스 공개(AGPL)는 그대로 두고 조직판의 설치 지원·유지 보수를 유료로 하는 방법도 있다. 정하지 않았다(10장 16번).

### 13.9 순서

관제 모듈 뒤다. 1단계는 관제와 함께 만들고, 2단계는 스토리방화벽이 지금의 기준판 개선을 마치고 안정된 뒤에 시작한다. 스토리방화벽 쪽 변경(조직판, 중앙 보고)은 스토리방화벽 개발 세션과 드라이브 기획 문서에서 먼저 검토한다.

### 13.10 검증 통합 1차 반영 (2026-11-11)

1. **보고 항목에 자기 식별 정보 추가(B1).** 13.3에 반영.
2. **보고가 끊긴 PC는 "미확인"(B2).** 끊김을 곧 격리로 보지 않는다. 휴가·전원 꺼짐과 구별할 수 없기 때문이다. 관제가 그 단말이 네트워크에 살아 있는 것을 보는데 보고만 없을 때 "에이전트가 멈춤(고의일 수 있음)"으로 알린다. 2단계에서는 알림까지만 할 수 있다.
3. **격리 전용 규칙(B3).** 13.3에 반영.
4. **기기 키는 TPM에 묶는다(B4).** 꺼낼 수 없는 키만 디스크 복제를 견딘다. TPM이 없는 PC는 "같은 인증서가 서로 다른 주소에서 동시에 보고함"을 잡아내는 것으로 대신한다. 한 번에 정한 비율 이상을 격리하려는 요청은 서버 본체에서의 추가 확인을 거친다.
5. **차단 방식(B5, B6).** 13.6에 반영. 관리형 스위치가 없는 소규모에서는 3단계가 사실상 불가능하고 2단계(스스로 격리)에 기대야 한다. ARP 방식은 하지 않는다.
6. **개인판에는 중앙 보고 코드를 넣지 않고 빌드한다(B7).** 실행 중에 켜고 끄는 스위치로 두지 않는다. 그리고 "기록을 밖으로 보내지 않는다"는 약속은 **공식 배포처의 서명된 빌드**에 대한 약속임을 명시한다. 소스가 공개돼 있어 누구나 보고 기능을 넣은 빌드를 만들 수 있기 때문이다. 설치 파일 해시 대조와 업데이트 서명 확인을 안내한다.
7. **법·규정 확인 항목(B8).** 조직이 배포 전에 확인할 것: 개인정보 보호법(수집·이용의 근거와 고지), 「근로자참여 및 협력증진에 관한 법률」 제20조 제1항 제14호("사업장 내 근로자 감시 설비의 설치"는 노사협의회 협의 사항 — 상시 근로자 30명 이상 사업장. 대법원 2018도1917은 설치의 주된 목적이 감시가 아니어도 해당할 수 있다고 했다), 정보통신망법. 법률 판단은 제가 대신할 수 없다.
8. **순서(D1) [확정 2026-11-11].** NAC 2·3단계는 폐기하지 않고 맨 뒤로 미룬다. 관제 모듈이 실제로 쓰이기 전에는 손대지 않는다. 구상을 남기는 이유는 나중에 조직용(유료) 기능을 정할 때 출발점이 되기 때문이다. 1단계(단말 목록, 새 단말 알림)는 관제의 기능이라 그대로 간다.
9. **기본 형태: 중앙 서버 없음(D4) [확정 2026-11-11].** 전담자가 없는 곳에 중앙 서버를 두면 패치되지 않은 채 방치돼 가장 약한 곳이 된다. 그래서 13.5의 중앙 서버를 두지 않는 형태를 기본으로 한다.
   - 각 PC의 스토리방화벽은 **스스로 판단해 스스로 격리**한다. 다른 곳의 명령을 받지 않는다.
   - 관제가 설치된 PC 한 대가 각 PC의 상태를 **모아 보기만** 한다. 격리 명령을 보내는 길은 만들지 않는다. 탈취돼도 조직 전체를 멈출 수 없다.
   - 검증 답변이 제안한 "PC끼리 방송으로 상태를 주고받는 분산 격리"는 받지 않았다. 방송을 듣는 포트를 열어야 하고, 위조된 격리 신호 하나로 전체가 멈출 수 있으며, 그것을 막으려면 결국 키 배포가 필요해 중앙의 문제가 되돌아온다.
   - 13.5의 중앙 서버 형태는 전담자가 있는 조직용으로 남긴다.

---

## 14. "AI 에이전트 안전망" 구상 [구상 — 기획자 제안 2026-11-11, 설계 전]

### 14.1 출발점

AI 에이전트를 적극적으로 쓰는 사용자의 고민에서 나왔다.

- 에이전트가 내 컴퓨터를 훤히 들여다보는 느낌이 든다.
- 지금은 묻고 허락받은 뒤 실행하지만, 편하자고 권한을 넓혀 주다 보면 나중에 보안 문제가 생기지 않을까 걱정된다.
- 쓰는 것, 보안, 기본적인 안전망 사이에서 어디에 선을 그어야 할지 어렵다.

풀어야 할 것은 "권한을 얼마나 주느냐"보다 **"잘못됐을 때 피해가 어디까지 가느냐를 줄이고, 무슨 일이 있었는지 사용자가 볼 수 있게 하는 것"** 이다. 운영체제(macOS냐 Windows냐)보다 에이전트에게 준 권한의 범위가 위험을 정한다. 에이전트는 사용자와 같은 권한으로 움직이기 때문이다.

### 14.2 이미 있는 것 (스토리방화벽 v0.1.5부터)

- **AI 에이전트 감시:** PC의 AI 도구(데스크톱 앱, 코딩 도우미, MCP 서버)를 도구별로 모아 어디와 통신하고, 무엇을 실행하고, 어떤 파일을 만들었는지 보여 준다. 판정하지 않고 사실 목록만 낸다(`crates/graph/src/agent.rs`, API `agents.list`).
- **에이전트 외부 통신 차단:** 그 에이전트 실행 파일의 인터넷 연결을 1시간·오늘·계속 중 고른 기간 막는다(승인 창 + 60초 유지). 같은 PC 안(로컬 MCP 서버)과 사설망은 막지 않는다.
- **알려진 한계(코드와 안내문에 적힌 것):** 실행 파일 이름과 명령줄의 패키지 이름으로 알아본다. 이름을 바꾼 사본, 명령줄을 받지 못한 프로세스는 놓친다. node·python 같은 공용 실행기와 스토어 앱 경로는 차단에 한계가 있다.

### 14.3 형태 [제안]

**새 프로그램을 만들지 않는다.** 스토리방화벽의 위 기능을 넓히고, 안디옥 통합 화면에 위젯으로 드러낸다. 에이전트가 한 일을 보려면 프로세스·연결·파일 기록이 필요한데 그것은 스토리방화벽이 이미 모으고 있다. 같은 수집기를 하나 더 만드는 것은 낭비다.

기준은 스토리방화벽의 드라이브 기획서(에이전트 감시는 기획서 14판 20-5)다. 이 장은 그쪽에 올리는 제안이며, 스토리방화벽 개발 세션의 검토를 거쳐야 한다.

### 14.4 넓힐 것 네 가지

1. **보이게 (확장).** 에이전트별 "오늘 한 일"을 이야기로: 접속한 곳, 실행한 프로그램, 만든 파일. 지금 수집은 파일 **만들기**까지다. 사용자가 고른 민감 폴더(인증서, SSH 키, 브라우저 프로필, 지정한 문서 폴더)에 한해 파일 **열기**를 기록할 수 있는지 PoC로 확인한다. "읽기"가 아니라 "열기"를 보는 이유는 메모리 맵으로 읽어도 파일을 여는 것은 거치기 때문이다(검증 C2). 볼륨을 통째로 읽는 우회는 잡지 못하므로 화면에 **"보조 지표"** 로 표시한다. [검증]
2. ~~권한 설정 점검~~ **뺀다(검증 C4).** 에이전트 도구의 설정은 사용자 전역, 프로젝트별, 환경 변수, 조직 정책, 명령줄 인자가 겹쳐 적용되고 형식도 자주 바뀐다. 한 곳만 읽고 "좁게 설정됨"이라고 표시했는데 실제로는 넓게 열려 있으면 표시가 없는 것보다 위험하다. 판정 없이 "찾은 설정 파일의 위치"만 나열하는 것은 뒤로 미뤄 두고 다시 검토한다.
3. **묶어 두기 (네트워크).** 에이전트별로 허용 접속지 목록을 정하고 벗어나면 막는다. 기존 정책 DSL과 WFP로 한다. 공용 실행기(node, python)로 도는 에이전트는 실행 파일 경로만으로 구별되지 않는다는 한계를 화면에 적는다. 에이전트가 띄운 자식 프로세스(PowerShell, curl, git 등)에도 같은 규칙을 적용한다. 화면 문구는 "묶어 둠"이 아니라 **"접속지를 좁힘 — 허용한 곳을 거쳐 나가는 것은 막지 못합니다"** 로 한다(검증 C3).
4. **안전 습관 안내와 설정 도우미.** 작업 폴더 분리, 비밀 분리, 백업, 에이전트 전용 계정. 화면 안내로 시작한다. 전용 계정·폴더 권한을 대신 설정해 주는 도우미는 Windows 설정을 바꾸는 일이므로 매번 승인을 받는다.

### 14.5 하지 않는 것과 못 하는 것

- **에이전트의 행동을 멈춰 세우고 묻지 않는다.** 스토리방화벽의 정체성은 "사후 분석 + 만든 규칙의 실시간 집행"이고, 멈춰 세우고 묻는 방식은 범위 밖이다(지침). 행동마다 묻는 것은 에이전트 도구 자신의 승인 기능이 할 일이다. 이 모듈은 미리 정한 규칙을 집행하고, 일어난 일을 보여 준다.
- **파일 접근은 막지 못한다.** WFP는 네트워크만 다룬다. 파일 접근을 막으려면 커널 드라이버가 필요한데, 드라이버 없는 구조가 1순위다. 파일 쪽 보호는 운영체제의 권한(전용 계정, 폴더 권한)에 맡기고, 이 모듈은 그 설정을 돕고 결과를 보여 준다.
- **에이전트가 클라우드로 보낸 내용은 보지 못한다.** 암호화된 내용은 보지 않는다(지침). 볼 수 있는 것은 "어디로, 얼마나, 언제"다.
- **판정하지 않는다.** 사실 목록이 기본이다. 경보는 규칙으로만 낸다(예: 민감 폴더를 읽은 직후 처음 보는 주소로 많은 양을 보냄).

### 14.6 위험

- **설정 파일을 읽는 것 자체가 사생활 문제다.** 에이전트 도구의 폴더에는 대화 기록도 있다. 설정 파일만 읽고 대화 기록은 열지 않는다. 읽은 값도 밖으로 보내지 않는다.
- **에이전트에서 온 문자열은 자료다.** 에이전트가 만든 파일 이름, 명령줄, MCP 서버 이름을 화면에 글자로만 표시하고, AI 서술 기능에 넘길 때 명령으로 취급하지 않는다.
- **에이전트가 이 안전망을 끄려 할 수 있다.** 일반 권한의 에이전트는 서비스를 멈추지 못한다. 그러나 사용자를 설득해 승인 창을 누르게 할 수는 있다. 정책 해제·서비스 중지의 승인 창에는 무엇이 꺼지는지를 분명히 적는다.
- **맥북 사용자는 지금 쓸 수 없다.** 스토리방화벽은 Windows용이고 macOS는 미정이다. 14.4의 4번(안전 습관 안내)은 운영체제와 무관하므로 별도 안내 자료로 먼저 낼 수 있다.

### 14.7 에피스틀 연결

- 위젯: `agents`(목록 — 오늘 움직인 에이전트와 접속지 수), `agent_perms`(상태등 — 권한 설정 점검 결과).
- 사건 종류: `agent.new_destination`(처음 보는 접속지), `agent.sensitive_read`(민감 폴더 읽기, 14.4의 1번이 가능할 때), `agent.policy_blocked`(허용 목록 밖 접속을 막음).
- 인계: 사건에서 NetX `diagnose.target`으로 접속지 조사.

### 14.8 순서

스토리방화벽의 기준판 개선이 먼저다. 그다음 세 구상의 순서는 **AI 에이전트 가시성 확대(이 장의 1번) → 관제 → NAC** 다(검증 D1). 4번의 안내 부분은 수집기를 건드리지 않아 언제든 할 수 있다. 1번의 열기 감시와 3번은 PoC와 스토리방화벽 개발 세션의 검토 뒤에 정한다.

### 14.9 검증 통합 1차 반영 (2026-11-11)

1. **WSL·컨테이너 안은 대상 밖(C1).** 화면에 적는다. 프로세스 트리가 끊기는 실행(작업 스케줄러, 서비스 등록)은 스토리방화벽이 이미 기록하는 "자리잡기" 사건(예약 작업·자동 실행 등록)을 에이전트와 이어 붙여 찾는 방법을 검토한다. [검증] 검증 답변이 제안한 "로컬 포트로 구별"은 받지 않았다. MCP 서버는 대부분 포트를 열지 않는다.
2. **민감 폴더는 "열기"를 보조 지표로(C2).** 14.4의 1번에 반영.
3. **허용 접속지는 "좁힘"(C3).** 14.4의 3번에 반영. 주소가 바뀌는 대상은 DNS 조회 기록과 짝지어 따라가되 정확하지 않다고 적는다.
4. **권한 설정 점검은 뺀다(C4).** 14.4의 2번에 반영.
5. **해제 승인 창의 고정 경고(C5).** 에이전트 관련 차단을 풀거나 서비스를 멈추는 승인 창에 항상 보인다: "AI 에이전트가 이 해제를 권했다면, 에이전트가 외부 지시에 조종되고 있을 수 있습니다."
6. **버전 관리 권유와 백업 표시(C6).** 안내(4번)에 작업 폴더를 Git 아래 두는 방법을 넣고, 백업이 켜져 있는지를 사실로 보여 준다.
7. **권한 도우미(C7).** 기존 허용 권한을 고치지 않는다. 전용 계정을 쓰는 것 자체가 주된 보호이고(다른 일반 계정은 내 사용자 폴더를 열지 못한다), 사용자 폴더 밖의 공용 위치에만 에이전트 계정에 대한 "거부"를 더한다. 도우미는 암호를 보관하지 않는다.

---

## 15. 다섯 번째 모듈 "유해차단" [별도 문서]

유해 사이트·프로그램에서 이 PC를 쓰는 사람을 보호하는 모듈이다. 별도 모듈(서비스 + 화면)이며 임시 id는 `filter`다. 개발안, 검증 의뢰서, 답변 반영은 이 문서에 넣지 않고 따로 둔다: 통합프로젝트 폴더의 "유해차단_모듈_개발안"과 "검증의뢰서_통합2차_유해차단". 이 설계 문서와 부딪치는 곳(전제 "로컬 포트를 열지 않는다"에 대한 예외 요청 등)은 그 문서가 적고 있다.
