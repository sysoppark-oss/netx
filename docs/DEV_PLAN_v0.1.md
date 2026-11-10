# 넷엑스(NetX) v0.1 확정 개발안

- 문서 버전: 1.0 (2026-11-10)
- 기준 문서: [DESIGN.md](DESIGN.md) v1.1
- 경위: Gemini가 작성한 v0.1 개발안 초안을 Claude가 설계서와 대조해 검토하고, 보완 사항과 최신 기술 적용안을 반영해 확정했다.
- 표기: **[검증]** 은 노트북 실기 확인이 필요한 사항이다.

---

## 1. v0.1 목표와 범위

**목표:** 상황을 입력하거나 레시피를 누르면 제안 카드가 나오고, [실행]을 누르면 결과·브리핑·판정이 보인다.

| 포함 | 제외 (v0.2 이후) |
|---|---|
| 핵심 루프(상황 → 계획 → 실행 → 결과 → 판정) | 시각화 4종 (단, 결과 화면 자리만 마련) |
| 도구 레지스트리와 9종 스키마 | 넷엑스 터미널(시리얼, SSH, TFTP) |
| 보안 검증기 | 스토리방화벽 연동 |
| WSL2 실행 엔진 | 파이프라인 빌더 |
| **Rust 네이티브 기본 진단** (WSL 없이 동작) | 스냅샷 비교 |
| **Windows 포트·PID 조회** (`Get-NetTCPConnection`) | 확장 도구(nmap 등) |
| 사용자 등급 5종, Easy/Expert 전환 | 음성 입력 |
| 레시피 6종, 동적 옵션 패널 | |
| AI 실행 계획·브리핑·판정, 규칙 엔진(AI 없을 때) | |
| LLM 설정 화면, 감사 로그, 초기 설정 마법사 | |
| GitHub Actions Windows 자동 빌드 | |

---

## 2. Gemini 초안 대비 변경 사항

| 번호 | 초안 | 확정 | 이유 |
|---|---|---|---|
| 1 | 포트 점유 추적을 WSL의 ss/lsof로만 처리 | `Get-NetTCPConnection`을 v0.1에 포함, WSL 결과와 합쳐 표시 | WSL 안에서는 Windows 프로그램이 보이지 않아 레시피와 [강제 종료]가 동작하지 않음 |
| 2 | 안전 옵션만 스키마에 정의 | `forbidden` 위험 단계 추가, 차단 옵션 명시 | 정상 옵션만으로 명령 실행·파일 쓰기가 가능함 |
| 3 | 인자 배열 분리만 적용 | 옵션 인젝션 차단(`-`로 시작하는 값 거부, `--` 사용) | 값 칸에 옵션을 숨겨 넣는 공격 방어 |
| 4 | 실행 중지 방식 미정 | WSL 안의 `timeout`으로 감싸고, 중지는 WSL 내부 프로세스에 신호 | `wsl.exe`만 끝내면 WSL 안의 프로세스가 남을 수 있음 |
| 5 | sudoers NOPASSWD 안내 | `wsl.exe -u root --exec` 우선, 안 될 때만 sudoers | 설정 단계 단순화 **[검증]** |
| 6 | AI 브리핑·판정 단계 없음 | 브리핑과 판정(예상 결과 대조) 추가 | 설계서 핵심 루프의 마지막 단계 |
| 7 | AI 실패 시 대책 없음 | 규칙 엔진(키워드 → 레시피) 추가 | 네트워크 장애 상황에서 AI가 끊길 수 있음 |
| 8 | 레시피가 AI보다 뒤 | 레시피를 AI보다 먼저 구현 | AI 없이도 Phase 4 끝에 노트북에서 실제 사용 가능 |
| 9 | 테스트·빌드 계획 없음 | 검증기 단위 테스트, Actions 자동 빌드를 초기에 구축 | 보안 핵심부는 테스트가 기준 |
| 10 | (신규) | Rust 네이티브 기본 진단 | WSL 없는 PC, 일반사용자도 사용 가능 |
| 11 | (신규) | 구조화 출력(JSON 스키마 강제) | 소형 로컬 모델의 형식 오류 방지 |
| 12 | (신규) | Tauri 2 권한 설정으로 이중 방어 | 화면이 뚫려도 허용 밖 명령은 백엔드에 닿지 않음 |
| 13 | (신규) | Windows Hello 확인 | 프로세스 종료 등 되돌릴 수 없는 조치 보호 |

---

## 3. 화면 설계

| 영역 | 내용 |
|---|---|
| 초기 설정 마법사 (첫 실행 모달) | ① WSL2·Ubuntu 설치 확인 ② root 실행 방식 확인(`wsl -u root`) ③ 도구 일괄 설치 ④ AI 설정(온라인 API / Ollama 주소 / 사용 안 함) ⑤ 사용자 등급 선택. WSL이 없으면 "기본 진단 모드"로 계속 진행 가능 |
| 상단 헤더 | 옴니박스, 등급 선택(5단계), Easy/Expert 토글, AI 상태 표시(온라인/로컬/규칙 엔진), 설정 |
| 좌측 패널 | Easy: 레시피 6종 버튼. Expert: 9종 도구 아이콘 툴바 |
| 중앙 | 제안 카드 목록. 카드마다 도구, 명령 미리보기, 이유, 예상 결과, 위험도 배지, [이것만 실행], [옵션 수정]. 상단에 [전체 실행] |
| 우측 | 동적 옵션 패널(스키마 그룹별 토글·입력창, 옵션마다 한글 설명, 등급별 잠금 표시) |
| 하단 | 명령 미리보기, 실행 상태(진행·중지 버튼·경과 시간), AI 브리핑(신호등), 판정, 접이식 원본 출력 |
| 결과 시각화 자리 | v0.3 시각화 컴포넌트가 들어올 빈 슬롯. v0.1에서는 표 형태로 표시 |

### 등급별 화면 차이

| 등급 | 카드 표시 | 옵션 패널 | 원본 출력 |
|---|---|---|---|
| 일반사용자 | 문제와 해결 방법만 쉬운 말로. 명령 숨김 | 없음 | 숨김 |
| 초급 | 명령 + 옵션별 학습용 해설 | 안전 옵션만 | 접힘 |
| 중급 | 명령 + 요약 해설 | 전체(caution은 경고) | 접힘 |
| 고급 | 명령 위주, 간결 | 전체 + 직접 입력 | 펼침 |
| OP | 고급과 동일 + 감사 로그 열람 | 전체 + 직접 입력 | 펼침 |

---

## 4. 데이터 형식

### 4.1 실행 계획 (ExecutionPlan)

```json
{
  "intent": "intermittent_web_latency",
  "summary": "간헐적 웹 지연, 원인 구간 특정",
  "target": "example.com",
  "steps": [
    {
      "order": 1,
      "tool": "curl",
      "args": ["-o", "/dev/null", "-s", "-w", "dns:%{time_namelookup} tcp:%{time_connect} tls:%{time_appconnect} ttfb:%{time_starttransfer}", "--", "https://example.com"],
      "reason": "지연 구간을 단계별 시간으로 분해",
      "expect": "ttfb만 길면 L7, tcp가 길면 네트워크"
    }
  ]
}
```

- AI는 `risk`, `needs_root`, `engine`을 정하지 않는다. 검증기가 스키마에서 계산해 붙인다.
- AI 호출 시 이 형식의 JSON 스키마를 함께 넘겨 구조화 출력을 강제한다.
- `-o /dev/null`처럼 출력 버리기 용도의 경로는 스키마에서 허용값으로 따로 정의한다.

### 4.2 검증 결과 (ValidatedStep)

```json
{
  "order": 1,
  "tool": "curl",
  "engine": "builtin",
  "argv": ["..."],
  "risk": "safe",
  "needs_root": false,
  "confirm": "none",
  "blocked": false,
  "issues": []
}
```

- `confirm`: `none`, `click`(추가 확인), `hello`(Windows Hello).
- `blocked`가 `true`이면 카드에 이유를 표시하고 실행 버튼을 비활성화한다.

### 4.3 도구 스키마 (ToolSchema)

```json
{
  "tool": "nc",
  "title": "포트 연결 시험",
  "engines": ["wsl", "builtin"],
  "needs_root": false,
  "streaming": false,
  "end_of_options": false,
  "options": [
    {"flag": "-v", "type": "boolean", "label": "상세 출력", "risk": "safe", "group": "출력"},
    {"flag": "-z", "type": "boolean", "label": "연결만 확인(데이터 전송 안 함)", "risk": "safe", "group": "모드"},
    {"flag": "-w", "type": "number", "label": "연결 시간 제한(초)", "min": 1, "max": 60, "risk": "safe", "group": "제한"},
    {"flag": "-l", "type": "boolean", "label": "연결 수신 대기", "risk": "danger", "group": "모드"},
    {"flag": "-e", "type": "string", "label": "프로그램 실행", "risk": "forbidden"},
    {"flag": "-c", "type": "string", "label": "셸 명령 실행", "risk": "forbidden"}
  ],
  "positional": [
    {"name": "host", "type": "host", "label": "대상"},
    {"name": "port", "type": "port_or_range", "label": "포트"}
  ],
  "min_level": "junior"
}
```

- `engines`: 실행 가능한 엔진 목록. 앞쪽이 우선이며, WSL이 없으면 `builtin`으로 대체한다.
- `end_of_options`: 도구가 `--`를 지원하는지 여부.
- 값 타입: boolean, number, string, enum, enum_dynamic, path, host, url, port, port_or_range, cidr, bpf.

### 4.4 9종 도구 차단·주의 옵션

| 도구 | forbidden | danger / caution |
|---|---|---|
| ping/fping | — | `-f`(플러드) danger, `fping -g` 대역 스캔 caution |
| tcpdump | `-z`(파일 회전 시 프로그램 실행) | `-w` caution(작업 폴더 한정), `-C`/`-W` caution |
| mtr | — | `-i` 0.2초 미만 caution |
| ss | `-K`(소켓 강제 종료) | — |
| dig | `-f`(파일에서 질의 읽기), `-k`·`-y`(TSIG 키) | — |
| nc | `-e`, `-c` | `-l` danger, `-u` caution |
| curl | `-K`/`--config`, `-T`, `-F`, `--data-binary`, `--data-urlencode`, 쿠키·추적·인증서 파일 옵션, `@`로 시작하는 `-d`/`-H`/`-w` 값 | `-o`/`-O` caution(작업 폴더 한정, `/dev/null`은 안전), `-X`, `-d`, `-u`, `-x` caution |
| lsof | — | — |
| iperf3 | `-F`, `--logfile`, `-D`, `-I` | `-s`(서버 모드) danger, `-u` caution |

실제 스키마는 `tools/*.json`이 기준이다(Phase 2에서 작성, 도구 10개·옵션 292개). 표에 없는 옵션도 같은 기준(프로그램 실행 = forbidden, 파일 쓰기·수신 대기 = caution 이상)으로 분류한다.

### 4.5 레시피 (Recipe)

```json
{
  "id": "web_down",
  "title": "웹 장애 진단",
  "icon": "🌐",
  "inputs": [{"name": "url", "type": "url", "label": "URL"}],
  "keywords": ["웹", "홈페이지", "사이트", "접속", "502", "안 열려"],
  "steps": [
    {"tool": "dig", "args": ["+short", "{url.host}"]},
    {"tool": "nc", "args": ["-vz", "-w", "3", "{url.host}", "{url.port|443}"]},
    {"tool": "curl", "args": ["-Ivks", "--", "{url}"]}
  ],
  "parallel": true
}
```

- `keywords`는 AI가 없을 때 규칙 엔진이 옴니박스 입력과 맞춰 보는 데 쓴다.
- 레시피도 실행 전에 검증기를 똑같이 통과한다.

### 4.6 감사 로그 (AuditEntry)

```json
{"time": "2026-11-10T21:00:00+09:00", "level": "op", "source": "recipe:port_hunt", "tool": "kill", "argv": ["..."], "risk": "danger", "confirm": "hello", "result": "ok"}
```

---

## 5. Rust 모듈 인터페이스 (`src-tauri/src/`)

| 모듈 | 역할 | 주요 함수 |
|---|---|---|
| `registry` | `tools/*.json`, `recipes/*.json` 로드 | `load_all()`, `get_tool(name)`, `list_recipes()` |
| `security::validator` | 8단계 검증(허용 목록, 타입, 위험도, 경로, 인젝션, 등급, AI 불신) | `validate(plan, level) -> Vec<ValidatedStep>` |
| `engine::builtin` | Rust 네이티브 진단: ping(`surge-ping`), DNS(`hickory-resolver`), TCP 연결, HTTP 단계별 시간(`reqwest`), 인증서(`rustls`) | `run(step, tx)` |
| `engine::wsl` | `wsl.exe [-u root] --exec timeout --signal=INT <초> <도구> <인자...>` 실행, 줄 단위 스트리밍 | `run(step, tx)`, `stop(run_id)` |
| `engine::windows` | `Get-NetTCPConnection`, `Get-Process` 조회(PowerShell을 인자 배열로 호출, 출력은 JSON), 프로세스 종료 | `list_ports()`, `kill(pid)` |
| `engine::runner` | 엔진 선택, 동시 실행, 시간 제한, 출력량 상한, 실행 ID 관리 | `execute(steps) -> run_id`, `cancel(run_id)` |
| `planner::llm` | 온라인 API / Ollama 호출, 구조화 출력 | `plan(input, level)`, `brief(outputs)`, `judge(expect, outputs)` |
| `planner::rules` | AI가 없을 때 키워드로 레시피 선택 | `match_recipe(input)` |
| `planner::redact` | 온라인 전송 시 내부 IP·호스트명 가리기 | `redact(text)` |
| `audit` | 위험 조치 기록 | `log(entry)` |
| `setup` | WSL 확인, 도구 설치, root 실행 방식 확인 | `check_wsl()`, `install_tools()`, `check_root()` |
| `auth::hello` | Windows Hello 확인 요청 | `verify(reason) -> bool` |

### Tauri 명령 (화면 → 백엔드)

`plan_from_text`, `plan_from_recipe`, `validate_plan`, `execute`, `cancel`, `get_tools`, `get_recipes`, `setup_status`, `run_setup_step`, `get_settings`, `save_settings`, `read_audit`

- Tauri 2 권한 설정(capabilities)에 위 명령만 허용한다. 셸·파일 시스템 플러그인은 화면에 열지 않는다.

### 이벤트 (백엔드 → 화면)

`run:stdout`, `run:stderr`, `run:exit`, `run:timeout`, `plan:ready`, `brief:ready`, `judge:ready`

---

## 6. 구현 순서

| Phase | 내용 | 완료 기준 |
|---|---|---|
| 1. 기반 | Tauri 2 + React + TailwindCSS 뼈대, Tauri 권한 설정, GitHub Actions Windows 빌드(설치 파일 생성) | 노트북에서 빈 창이 뜨는 설치 파일을 Actions에서 받을 수 있다 |
| 2. 스키마·검증기 | 9종 스키마, 레시피 6종 JSON, `registry`, `security::validator`, 검증기 단위 테스트(차단 옵션·인젝션·등급 사례 포함) | 테스트가 Actions에서 통과한다 |
| 3. 실행 엔진 | `engine::builtin`, `engine::wsl`(timeout 감싸기, 중지), `engine::windows`(포트·PID), `runner`, 스트리밍 이벤트, 감사 로그 | 노트북에서 각 엔진으로 명령 하나씩 실행·중지된다 **[검증]** |
| 4. 레시피·화면 | 레시피 6종 버튼, 제안 카드, 동적 옵션 패널, 등급 5종, 결과 화면, 초기 설정 마법사, Windows Hello 확인 | AI 없이 레시피 6종이 노트북에서 동작한다 |
| 5. AI | `planner::llm`(구조화 출력, 브리핑, 판정), `planner::rules`, `redact`, LLM 설정 화면 | 옴니박스 입력 → 제안 카드 → 실행 → 브리핑·판정까지 한 바퀴 돈다 |

---

## 7. 노트북 검증 목록

1. `wsl.exe -u root --exec tcpdump -D`가 비밀번호 없이 실행되는가
2. WSL 미러 네트워킹 모드에서 tcpdump가 호스트 인터페이스를 보는가
3. `wsl.exe`를 강제 종료했을 때 WSL 안의 `mtr`이 남는가, `timeout` 감싸기로 정리되는가
4. `Get-NetTCPConnection` 조회가 관리자 권한 없이 PID까지 나오는가
5. Windows Hello 확인 창이 Tauri 창 위에 정상적으로 뜨는가
6. Ollama(Tailscale 경유) 구조화 출력의 응답 시간

---

## 8. 미결정 사항 (DESIGN.md 15장과 동일)

- 온라인 LLM 서비스와 로컬 Ollama 모델 선택 (Phase 5 전까지)
- 9.2 추가 아이디어의 채택과 단계 배치
- 스토리방화벽 연동 수단 (v0.2 전까지)

---

## 9. 진행 기록

| Phase | 상태 | 비고 |
|---|---|---|
| 1. 기반 | 완료 (2026-11-10) | 노트북에서 창 실행 확인. WSL 미설치 PC에서 `wsl.exe`를 부르지 않도록 보완 |
| 2. 스키마·검증기 | 완료 (2026-11-10) | 아래 참고 |
| 3. 실행 엔진 | 예정 | 노트북에 WSL2 설치 필요 |
| 4. 레시피·화면 | 일부 선행 | 레시피 입력 → 검증된 제안 카드 표시까지 |
| 5. AI | 예정 | |

### Phase 2에서 정한 것

- **스키마 내장:** `tools/*.json`, `recipes/*.json`을 빌드할 때 실행 파일에 넣는다. 설치 후 파일을 바꿔 검증을 우회할 수 없다.
- **ping과 fping은 스키마를 따로 둔다**(실행 파일이 다르다). 도구 묶음으로는 9종, 스키마로는 10개다.
- **묶은 옵션 해석:** `-vz`, `-rwbzc 50`, `-sTCP:LISTEN`, `--opt=value`, dig의 `+time=2`를 풀어서 옵션마다 검사한다. 묶음 안에 금지 옵션이 숨어 있어도 차단된다.
- **위험도 적용 규칙:** 금지는 항상 차단. 위험은 중급 이상만. 주의는 중급 이상이거나 레시피에 들어 있을 때만.
- **스키마에 없는 옵션:** 고급·OP 등급이 직접 입력한 경우에만 위험 표시 후 허용한다. AI가 만든 계획과 레시피에서는 차단한다.
- **값에서 오는 위험도:** 사설 대역이 아닌 스캔은 위험, 포트 100개 초과 범위는 주의, `/16`(fping은 `/20`)보다 넓은 대역은 차단.
- **`--` 삽입:** 지원하는 도구는 위치 인자가 모두 뒤쪽에 있을 때 그 앞에 `--`를 넣는다. 9종 도구의 실제 인자 해석을 리눅스에서 확인했다.
- **포트 점유 추적 레시피**는 지금 WSL 내부(`ss`, `lsof`)만 본다. Windows 본체 조회(`Get-NetTCPConnection`)는 Phase 3에서 붙인다.
- **테스트:** 백엔드 단위 테스트 40개(값 검사, 금지 옵션, 옵션 인젝션, 등급, 레시피 전체 통과).
