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
| 3. 실행 엔진 | 1차 완료·노트북 확인 (2026-11-10), 2차 진행 중 | 1차: WSL 실행·중지·제한, 도구 설치, 감사 로그. 2차: Windows 포트 조회. 남은 것: Rust 네이티브 진단, 프로세스 종료(Windows Hello) |
| 4. 레시피·화면 | 일부 선행 | 레시피 입력 → 검증된 제안 카드 표시까지 |
| 5. AI | 구현, 실제 모델 확인 대기 (2026-11-11) | 상황 → 제안, 브리핑·판정, 규칙 엔진, 설정 화면. 가짜 서버로 전 과정 확인. 실제 Ollama·온라인 모델로는 아직 확인하지 못함 |

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

### Phase 3 (1차)에서 정한 것

- **작업 순서:** `dev` 브랜치에서 개발 → 노트북에 설치해 확인 → `main`에 합친다.
- **실행 명령 형태:** `wsl.exe --cd <작업 폴더> [-u root] --exec timeout --signal=INT --kill-after=3 <초> stdbuf -oL -eL <도구> <인자...>`. 셸을 거치지 않는다.
- **root 실행:** sudoers를 고치지 않고 `wsl.exe -u root`를 쓴다. **[검증]** 노트북 확인 필요.
- **중지:** WSL 안의 `timeout` 프로세스에 INT 신호를 보낸다(`pkill -f`, 실행마다 고유한 시간 표식으로 구분). 도구가 통계를 출력하고 끝날 기회를 주고, 3초 안에 안 끝나면 강제 종료한다.
- **제한:** 단계당 120초, 출력 5,000줄 또는 2MB. 넘으면 중단한다.
- **실행 요청은 다시 검증한다:** 화면이 보낸 명령을 실행하지 않는다. 레시피 id와 입력값을 받아 백엔드가 계획을 다시 만들고 검증한 뒤 실행한다. 주의·위험 단계는 확인을 거쳐야 한다.
- **작업 폴더:** `%LOCALAPPDATA%\com.sysoppark.netx\work`. 캡처 파일이 여기에 저장된다. 감사 로그는 같은 위치의 `audit.log`.
- **도구 설치:** WSL에 없는 도구를 찾아 안내하고, [도구 설치]로 `apt-get`을 root로 실행한다(고정된 패키지 목록).
- **창 하나만 실행:** 다시 실행하면 이미 떠 있는 창을 앞으로 가져온다.
- **리눅스 개발 모드:** Windows가 아니면 도구를 직접 실행한다. 이 방식으로 실행·중지·시간 제한·출력 상한을 실제 프로세스로 테스트한다(단위 테스트 52개). 가상 화면에서 앱을 띄워 레시피 실행, 확인 절차, 중지까지 눌러 확인했다.

### 노트북 확인 결과 (Phase 3 1차, 2026-11-10)

- `wsl.exe -u root --exec`로 비밀번호 없이 root 실행됨(도구 설치, tcpdump). sudoers 설정은 필요 없다.
- 한글 사용자 폴더(`C:\Users\박미정\AppData\Local\...`)를 `--cd` 작업 폴더로 넘겨도 정상 동작.
- [도구 설치]로 Ubuntu에 fping, tcpdump, mtr, dig, iperf3 설치 완료 후 안내가 사라짐.
- 라우팅 지연 분석(mtr, ping → 8.8.8.8) 실행과 실시간 출력 정상.
- 트래픽 퀵 캡처: 확인 절차 → root 실행 → [중지] 시 tcpdump가 통계를 출력하고 종료.
- NSIS 설치 파일은 사용자 폴더(`%LOCALAPPDATA%\NetX`)에 설치되어 관리자 권한 창이 뜨지 않는다. 처음 한 번만 기존 MSI 설치를 제거해야 했다.
- 발견해서 고친 것: 홈 화면에서 오류가 보이지 않던 문제, 출력 창이 맨 아래로 따라가지 않던 문제.

### 개발판 전달 방식

`dev`에 푸시 → GitHub Actions가 설치 파일을 `dev-builds` 브랜치에 올림(스토리방화벽과 같은 방식) → 노트북 `C:\netx-build`로 받아 해시 확인 후 설치 → 화면 확인 → `main`에 합침.

### Phase 5에서 정한 것

- **연결 방식 두 가지:** 로컬 Ollama(`/api/chat`, `format`에 스키마)와 OpenAI 호환 온라인 API(`/chat/completions`, `response_format`에 스키마). 설정 화면에서 고른다. 기본은 "사용 안 함".
- **API 키:** Windows 자격 증명 저장소에 보관하고 화면으로 다시 보내지 않는다. 설정 파일(`settings.json`)에는 주소와 모델 이름만 둔다.
- **AI에게 보여 주는 도구 목록:** 금지·위험 옵션은 아예 넣지 않고, 일반·초급 등급에는 안전 옵션만 넣는다.
- **AI 계획 실행:** 백엔드가 AI 계획 원본을 보관하고, 화면은 계획 번호만 보낸다. 실행할 때 다시 검증한다(AI 계획은 스키마 밖 옵션 불가). AI가 금지 옵션을 넣으면 그 단계는 "차단" 카드로 보이고 전체 실행에서 빠진다.
- **브리핑·판정:** 실행이 끝나면 단계별 명령, 목적, 예상, 실제 출력을 보내 결론 한 문장, 판정(정상/문제 발견/판단 보류), 단계별 신호등, 다음 할 일을 받는다. 등급에 따라 말투를 바꾼다.
- **AI가 없거나 실패할 때:** 키워드로 레시피를 고르고 문장에서 주소·포트·대역을 뽑아 입력 칸을 채운다. 값이 다 있으면 바로 계획을 보여 준다.
- **내부 IP 가리기:** 온라인 API로 보내는 진단 출력에서 사설 IP를 가리고 돌아온 글에서 되돌린다. 상황 설명에 적은 주소는 가리지 않는다(대상을 알아야 계획을 만들 수 있다).
- **프롬프트 주입 대비:** 진단 출력은 자료로만 취급하라고 지시하고, AI의 답은 화면에 글로만 쓰인다. AI가 직접 실행하는 경로는 없다.
- **남은 것:** Anthropic API 형식 직접 지원, 음성 입력, 적응형 다음 단계 제안.

### 사용 후 요청 반영 1차 (2026-11-11)

노트북에서 써 본 뒤 나온 요청 9가지 가운데 아래를 먼저 반영했다.

- **트래픽 퀵 캡처:** 인터페이스를 WSL에서 읽어 목록으로 고른다(`any` 포함). 프로토콜(전체/TCP/UDP/ICMP/ARP), 포트 또는 범위(예: `21-30`), 상대 주소, 패킷 수를 고른다. 파일 이름은 `capture-(시각).pcap`.
- **웹 장애 진단:** `https://`를 생략해도 된다(`example.com`, `10.0.0.5:8443/admin`).
- **포트 점유 추적:** 포트 범위와 프로토콜(TCP/UDP/둘 다)을 고른다. Windows 조회, `ss`, `lsof` 모두 범위를 지원한다.
- **대역폭 측정:** 측정 시간을 고르고, 서버에 5초 안에 연결되지 않으면 바로 끝낸다.
- **진행 표시:** 실행 중인 단계에 경과 시간, 남은 시간(예상할 수 있을 때), 자동 중단 시간을 표시줄과 함께 보여 준다.
- **결과 해설(AI 없이):** 단계가 끝나면 출력을 해석해 신호등과 한 줄 결론, 핵심 수치를 보여 준다. ping, mtr, nc, curl, dig, fping, iperf3, Windows 포트 조회, ss, lsof, tcpdump를 해석한다. mtr은 끝까지 이어지는 손실이 시작되는 홉을 짚고, 중간 홉만의 손실은 원인으로 보지 않는다.
- **그래프:** 홉별 지연 막대(mtr), 응답 시간 선(ping), 속도 계기판과 초당 속도 선(iperf3), 장비별 응답 시간 막대(fping).

남은 요청(다음 차례): 계획에 사용자가 도구를 직접 추가하기, 명령어 직접 입력과 검증·예상 표시, 서브넷 스캔에 포트 스캔과 웹·터미널 바로 접속, 그래프 종류 확대.

## 사용 후 요청 반영 2차 (2026-11-11)

- 내 네트워크 표시줄: 어댑터, 내 IP/길이, 게이트웨이, DNS, 공인 IP, 지금 속도(↓↑), 앱을 켠 뒤 사용량. 항상 헤더 아래에 보인다.
  - 설정 조회는 고정된 PowerShell 스크립트(20초 캐시), 송수신량은 sysinfo, 공인 IP는 api.ipify.org(10분 캐시, 실패 시 ifconfig.me).
- 대역폭 측정: 공개 iperf3 서버 목록(도쿄, 홍콩, 싱가포르, LA, 프랑크푸르트)에서 고르거나 직접 입력. 서버 포트, 방향(다운로드/업로드) 추가. "서버 사용 중" 해설 추가.
- 입력 안내: 포트 형식 등 안내문을 입력 전부터 입력칸 아래에 표시(recipes의 `hint`).
- 트래픽 퀵 캡처 프로토콜 48종(L2~응용). 이름마다 코드에 고정된 필터 조각으로 바뀐다(recipe.rs `capture_filter`).
- 포트 점유 추적: SCTP, DCCP 추가(WSL의 ss만 실행, Windows와 lsof 단계는 계획에서 빠짐).
- 계획에 직접 추가: 도구를 고르면 사용 형식과 옵션 목록이 나오고, [검증]으로 위험도·옵션 풀이·예상 시간·예상 결과를 본 뒤 계획에 붙인다. 전체 실행과 개별 실행 모두 가능. 레시피 없이 쓰는 "직접 명령" 화면 추가.
  - 직접 적은 글은 셸에 넘기지 않고 낱말로 나눠 검증기를 거친다(manual.rs). 파이프·리다이렉션은 거부. 직접 추가한 단계는 항상 Source::Manual 규칙을 따른다.

## 사용 후 요청 반영 3차 (2026-11-11)

- 내장 포트 스캔(`portscan`, engine=builtin): 대역 또는 장비 하나에 TCP 연결만 시도해 열린 포트를 찾는다. WSL이 없어도 동작한다. 한 번에 포트 50개, (장비 × 포트) 40,000개까지. 위험도는 주의.
- 서브넷 장비 스캔: 확인할 포트 입력 추가(기본 22,23,80,443,3389,8080). 2단계로 포트 스캔을 실행한다.
- 결과에서 바로 접속: 열린 포트마다 [웹 열기](기본 브라우저), [터미널 접속](새 창에서 ssh 또는 telnet). 주소는 IP 형식, 사용자 이름은 정해진 글자만 받고, 암호는 NetX가 받지 않는다(commands.rs `open_target`).
- 그래프: 웹 진단에 단계별 걸린 시간(DNS, TCP 연결, TLS, 서버 응답 대기, 내려받기), Windows 포트 조회에 프로그램별 소켓 수, 포트 스캔에 장비별 열린 포트 수.
- iperf3: 서버가 연결을 끊는 경우(포트 사용 중)도 포트나 서버를 바꾸라고 안내.
- Expert 모드 명령 조립: 왼쪽에서 도구를 고르면 가운데에 사용 형식과 명령 입력칸이, 오른쪽에 옵션 패널이 나온다. 옵션을 누르면 명령에 붙고, [검증] → [계획에 추가] → 실행. Easy와 Expert는 계획을 따로 쓴다.
- 프로세스 종료: Windows 포트 조회 결과에 포트를 쓰는 프로그램 목록과 [종료] 버튼(중급 이상). Windows Hello로 본인 확인을 받고, Hello를 쓸 수 없는 PC에서는 프로세스 이름을 입력해야 한다. 시스템 프로세스와 NetX 자신은 종료하지 않는다. 종료는 감사 로그에 남는다(prockill.rs).
- 네트워크 구성도(레시피 `network_map`, 내장 도구 `netmap`): 대역을 비워 두면 내 네트워크 대역을 조사한다. 자주 쓰는 포트 13종에 TCP 연결을 시도하고(열림·거절 모두 "켜져 있음"), 이웃(ARP) 목록에서 MAC을 읽는다. 열린 포트와 MAC 제조사로 종류(공유기, PC, 프린터, 카메라, 휴대폰, 네트워크 장비, 서버)를 짐작해 인터넷 → 공유기 → 내부망 → 장비 그림으로 보여 준다. 장비를 누르면 MAC, 제조사, 열린 포트가 나온다. 한계: 같은 대역(L2)만 보이고, 스위치·AP 같은 중간 장비의 연결 관계는 알 수 없다(SNMP/LLDP 조사는 이후 과제).
- 구성도 장비 이름: 찾은 장비마다 NetBIOS 이름(UDP 137 node status)과 역방향 DNS(첫 DNS 서버에 PTR 질의)를 외부 프로그램 없이 직접 묻는다(장비당 1초, 전체 동시). 이름은 영문·숫자·`.`·`-`·`_`만 남겨 63자로 자른다. 출력은 `NODE <ip> <mac> <역할> <포트> <이름>` 6칸이고, 내 PC는 답이 없으면 운영체제가 아는 컴퓨터 이름을 쓴다. 그림의 장비 상자 IP 아래와 상세에 이름이 나온다(builtin.rs `lookup_names`).
- 구성도 그림 저장: [그림 저장]을 누르면 색을 직접 적은 독립 SVG를 작업 폴더에 `topology-<유닉스 시각>.svg`로 저장하고 경로를 보여 준다. 백엔드가 1MB 이하, `<svg` 시작, 스크립트·이벤트 속성·링크·foreignObject 없음을 다시 검사한다(commands.rs `save_topology`).
- 자동 업데이트(updater.rs): Actions가 푸시한 빌드를 `main-builds`·`dev-builds` 브랜치에 올리고, 설치된 NetX는 자기 채널의 `manifest.txt`를 시작 8초 뒤와 3시간마다 확인한다. 커밋이 다르고 더 나중에 빌드된 것이면 내려받아 SHA-256을 확인한 뒤 설치 프로그램을 `/P /R`로 실행한다(시작 직후에만 자동, 쓰는 도중에는 알림 줄의 [지금 업데이트]). 주소는 코드에 고정, HTTPS만, 리다이렉트 안 따름. 설정에서 자동 설치를 끌 수 있다. 별도 전자서명은 없다(믿는 근거는 GitHub 저장소와 HTTPS).
- 자동 업데이트 보완(2026-11-11): 정식판(main 채널)은 업데이트 서명 검증이 들어가기 전까지 설치하지 않고 알림만 띄운다(`updater::installs`, 상태 `notice`). 개발판(dev)은 그대로 자동 설치하되, 실행 중인 작업이 있으면(`engine::busy`, 내려받는 사이에 시작된 것 포함) 설치를 미루고 [지금 업데이트]로 알린다(`updater::decide`). README와 설정 화면에 "새 판 확인을 위해 GitHub에 접속하며 이 PC의 정보는 보내지 않는다"를 적었다. 다음 단계는 스토리방화벽과 같은 Tauri 업데이트 서명(공개 키 내장)으로 바꾸는 것이며, 서명 키는 사용자가 만들어 저장소 비밀값에 넣는다.
- 업데이트 서명 검증(2026-11-11): Actions가 설치 파일에 Tauri 서명 도구(minisign)로 서명해 `<설치 파일>.sig`를 `<채널>-builds`에 함께 올리고, 설치된 NetX는 SHA-256에 더해 서명을 내장 공개 키(`updater::PUBLIC_KEY`)로 확인한 뒤에만 설치한다(`updater::verify_signature`, `minisign-verify`). 개인 키는 저장소 비밀값 `TAURI_SIGNING_PRIVATE_KEY`에만 있고 암호는 없다. 서명 검증이 들어갔으므로 정식판(main 채널)도 자동 설치를 연다.
- 구성도 1-C, 스위치·AP 연결 관계(2026-11-11): 설정 화면에 SNMP 커뮤니티 문자열을 넣어 두면(`snmp-community`, Windows 자격 증명 저장소, 화면으로 되돌려 보내지 않음) `netmap`이 찾은 장비에 SNMP v2c(읽기 전용)로 묻는다. 답한 장비에서 sysName·sysDescr, LLDP 이웃 표, 브리지 MAC 표(비어 있으면 VLAN용 표), 포트 이름(ifName)을 읽어 "어느 장비가 어느 스위치의 어느 포트에 물렸는지"를 정한다(`builtin.rs`의 `snmp_survey`, `derive_links`): LLDP가 MAC 표보다 먼저, 공유기 MAC이 보이는 포트(위로 가는 포트)의 장비는 긋지 않음, 여러 스위치에 보이면 그 포트의 MAC 수가 가장 적은 곳. 출력에 `SNMP <ip> <MAC 표 줄 수> <LLDP 이웃 수> <설명>`과 `LINK <스위치> <포트> <장비> <lldp|fdb> <그 포트의 MAC 수>`가 더해지고, 구성도는 스위치마다 색이 다른 가로선 아래에 물린 장비를 놓고 포트를 적는다(화면용과 저장용이 `layoutOf`를 함께 씀). 내 PC의 MAC도 어댑터에서 읽어 표시한다. SNMP 패킷 만들기·읽기와 UDP 주고받기는 `src-tauri/src/snmp/`에 독립 모듈로 두었다(`std`·`tokio`만 의존, SET 없음, 응답 크기·길이·요청 번호 확인, 무작위 입력 시험). v3은 아직 없다.
- 말씀 표시(2026-11-11, 규칙은 `word/README.md`): `word/`의 세 파일을 고치지 않고 빌드에 넣어(`include_str!`, `src-tauri/src/word.rs`) 시작 화면, 기다리는 화면(진행 표시 아래), 빈 목록 화면(Expert 옵션 패널), "정보" 화면(헌정 문구와 [교회 안내])에 보인다. 본문 아래 장·절과 번역본 이름을 적고, 구절은 오늘의 시작 위치(`day % 96`)에서 20초마다 차례로 바꾼다(긴 구절 4곳 제외). "오늘의 말씀" 고르는 규칙과 README의 확인 값 7개를 단위 시험에 넣었다. 끄는 설정은 없고, 네트워크를 쓰지 않으며, 확인 창·경고·진단 결과·원본 출력·감사 로그·저장 그림에는 넣지 않는다. 교회 안내의 전자우편 주소는 글자로 보이고 누르면 메일 프로그램만 연다(`open_church_mail`, 주소는 문안 파일에서 읽음). 머리줄에 [정보] 단추를 더했다.
- 넷엑스 터미널(2026-11-11, `src-tauri/src/term/`, 화면 `src/Terminal.tsx`): 머리줄 [터미널]로 여는 별도 창(창 이름 `terminal`, 권한 `capabilities/terminal.json`은 터미널 명령과 말씀 표시만). 탭마다 접속 하나. SSH는 시스템 OpenSSH를 가상 터미널에서 실행(`pty.rs`, `portable-pty`), Telnet(`telnet.rs`, 옵션 협상은 ECHO·SGA·터미널 종류·창 크기만), 시리얼(`serial.rs`, `serialport`), FTP(`ftp.rs`, 수동 모드, 데이터 연결은 제어 연결과 같은 주소로만), TFTP 클라이언트(`tftp.rs`, RFC 1350)는 내장. 암호 칸이 없고(터미널 안 프롬프트에 직접 입력, FTP 암호는 되울리지 않음) 저장·기록하지 않는다. 감사 로그에는 종류·대상·사용자만(`audit::log_terminal`). 주소·포트·사용자·시리얼 포트 이름은 `Spec::check`로 검사한다. FTP·TFTP 파일은 전송 폴더(`work/transfer`) 안의 이름으로만 주고받고 덮어쓰지 않는다(`files.rs`). 서버가 보낸 글(FTP 응답·목록, TFTP 오류)은 제어 문자를 지우고 보인다. 스캔 결과의 [터미널 접속] 단추는 이 창을 연다(`open_terminal`의 preset). 접속이 열려 있으면 자동 업데이트 설치를 미룬다(`term::busy`). 본창을 닫으면 터미널 창도 닫힌다. 화면은 xterm.js 6.0.0(판 고정): `freezePrototype` 때문에 `vite.config.ts`의 `frozenPrototypeSafe`가 xterm의 `toString` 대입 한 곳을 바꾸고, CSP가 스크립트가 만든 `<style>`을 막으므로 `mirrorStyles`가 그 내용을 CSSOM으로 옮긴다. 터미널 화면은 터미널 창에서만 읽어 들인다(`main.tsx`). 없는 것: TFTP 서버, SFTP, 세션 기록 저장, 접속 목록 저장. 시험: 단위 22개(가짜 FTP·TFTP 서버, 가상 터미널 포함), 리눅스 화면은 `scripts/term_fakes.py`와 `NETX_TEST_SERIAL`로 확인.
- WSL 없는 기본 진단(2026-11-11, `src-tauri/src/basic/`): WSL이 없으면(`env_check`의 기본 진단) 스키마에 `builtin`을 함께 적은 리눅스 도구(ping, mtr, fping, dig, nc, curl)를 내장 기능으로 돌린다(`basic::engine_for`, 실행은 `engine`의 내장 갈래 → `basic::run`). 출력은 리눅스 도구와 같은 모양이라 `interpret`와 그래프를 그대로 쓴다. ping·mtr·fping은 ICMP(`icmp.rs`: Windows는 `IcmpSendEcho`, 관리자 권한 불필요, IPv4만. 리눅스 개발 환경은 시스템 ping을 한 번씩 실행), dig는 DNS 직접 질의(`dns.rs`, UDP, 잘리면 TCP), nc는 TCP 포트 확인(`-z`만), curl은 HTTP/1.1 요청과 TLS(`http.rs`, `tokio-rustls`, 단계별 시간과 인증서 만료일, 공개 인증 기관 목록으로 검증, `-k`). 지원하지 않는 옵션과 WSL 전용 도구(tcpdump, iperf3, ss, lsof) 단계는 계획에서 "전체 진단 필요"로 막아 보이고(`basic::fit_plan`, 계획을 만드는 모든 명령과 실행 직전에 적용) 나머지 단계는 실행된다. 서버가 보낸 글은 제어 문자를 지운다. 리눅스에서 기본 진단 화면을 보려면 `NETX_FORCE_BASIC=1`.
- 초기 설정 마법사(2026-11-11, `src/components/SetupWizard.tsx`): 첫 실행 때 한 번(`localStorage`의 `netx.setup.done`), 머리줄 환경 배지를 누르면 다시. 등급(고른 등급은 `netx.level`에 기억) → 진단 방식(기본 진단 설명, 전체 진단 준비: `wsl --install` 안내·[다시 확인]·[도구 설치]) → AI 연결(설정 열기). NetX가 관리자 권한을 얻거나 WSL을 설치하지 않는다. 말씀 표시는 넣지 않았다.
- Anthropic API 형식(2026-11-11, `planner/llm.rs`): 설정의 AI 연결에 "Anthropic"을 더했다(`Provider::Anthropic`). `POST {주소}/v1/messages`, 머리줄 `x-api-key`·`anthropic-version: 2023-06-01`. 정해진 형식으로만 답하게 하려고 스키마를 입력 형식으로 가진 도구 하나(`netx_answer`)를 반드시 쓰게 하고(`tool_choice`), 답은 `tool_use`의 `input`에서 꺼낸다. 온라인 방식(OpenAI 호환, Anthropic)에는 내부 IP 가리기를 똑같이 적용한다(`Provider::online`). 온라인 방식끼리 바꾸면서 새 키를 넣지 않으면 전에 쓰던 키를 지운다(다른 회사로 키가 나가지 않게). 모델 이름은 기본값 없이 사용자가 넣는다. 실제 Anthropic 서버로는 확인하지 못했다(가짜 서버로만).
- 시각화 4종(2026-11-11, 화면 `src/components/Viz.tsx`, 자료 `interpret.rs`의 `Chart::Route`·`Radar`·`Fortress`·`Highway`): 라우팅 노선도(mtr 결과: 홉을 역으로 이은 노선도, 손실이 이어지는 구간과 지연 큰 구간은 굵은 붉은 선이 뛰고 역을 누르면 해설), 소나 레이더(fping 결과: 도는 레이더 줄기 위에 응답한 장비를 점으로, 빠를수록 가운데·크게), 요새 도면(Windows 포트 점유 결과: 성벽의 문 = 포트, 이름표 = 프로그램, 이름 모를 프로그램은 붉게 깜박임. 종료는 기존 프로세스 목록에서), 패킷 하이웨이(캡처 결과: 단계가 끝나면 `pcap.rs`가 저장된 pcap 파일의 헤더만 읽어 상대별 흐름을 만들고, 내 PC와 상대를 잇는 선 위로 점이 움직인다. 녹색 암호화 통신, 청색 DNS, 붉은색 재전송·RST). 모두 결과 카드 안의 SVG와 SVG 자체 애니메이션(CSP 때문에 `<style>`을 끼워 넣지 않는다), 새 라이브러리 없음, 단계가 끝난 뒤의 자료로 그린다(실시간 아님). 기존 막대 그래프는 그 아래에 둔다. 소리와 [방화벽 차단 규칙 등록] 단추는 넣지 않았다(뒤의 것은 스토리방화벽 연동 때). 리눅스 화면 확인은 `PATH` 앞에 가짜 `mtr`·`fping`·`tcpdump`를 두고, 요새 도면은 `NETX_TEST_WINPORTS=<글 파일>`로 했다.
- 구성도 조사 옵션(2026-11-11, 사용자 승인): `netmap`에 `--ping`(ICMP로 켜진 장비 찾기와 응답 시간), `--mdns`(mDNS·UPnP로 종류와 이름), `--deep`(포트 13종 → 47종), `--ports 목록`, `--banner`(열린 포트의 첫 응답: SSH 판, 웹 서버·화면 제목. 주의 등급, 장비에 접속 흔적이 남는다), `--snmp`/`--no-snmp`, `--timeout ms`를 더했다(`builtin.rs`의 `MapOptions`, 덧붙임 조사는 `engine/probe.rs`). 결과는 `INFO <ip> <ping|mdns|upnp|banner> …` 줄로 나오고 구성도의 장비 상세에 핑·mDNS·UPnP·포트 첫 응답 칸으로 보인다(`Node.ping_ms`, `Node.notes`). 스스로 프린터라고 알린 장비는 프린터로 분류한다. 레시피 "네트워크 구성도"에 "조사 깊이"(빠르게 / 보통 = `--ping --mdns` / 정밀 = `--deep --ping --mdns --banner`, 기본은 보통)를 더했다. 장비가 보낸 글은 `probe::tidy`로 제어 문자를 지우고 80자로 자른다. TLS 포트(443 등)의 첫 응답과 OS 추정은 넣지 않았다.
- 노트북 확인과 손질(2026-11-11 밤): 넷엑스 터미널(창, 스캔 결과의 Telnet 단추, ConPTY의 ssh), 시각화 4종(실제 경로 추적·대역 핑·포트 점유·tcpdump 캡처), 구성도 정밀 조사(Windows ICMP 핑, 포트 47종, 포트 첫 응답), 설정의 Anthropic 단추를 노트북에서 눌러 확인하고 `main`에 합쳤다. 확인하면서 고친 것: 노선도 줄 끝 역의 글자 겹침, 요새 도면이 문 48개까지만 그릴 때 수신 대기 문을 먼저 남기고 나머지 수를 적음, 내 네트워크 표시줄이 "확인 불가"로 오래 남던 문제(화면이 몇 초마다 묻는 사이 PowerShell 조회가 겹겹이 쌓여 모두 제한 시간을 넘김 → `netinfo::Reading`으로 조회는 한 번에 하나만, 제한 시간 45초).
- 조사 깊이(빠르게/보통/정밀)를 모든 진단에 적용: 웹 장애(정밀: 핑, DNS 전체 응답, 인증서 검증·리다이렉트 추적), 경로·지연(횟수 5/10/30, 정밀: 1400바이트 핑), 서브넷 스캔(재시도 0/1/3, 포트 3종/고른 포트/47종), 포트 추적(Windows만/수신 대기/연결 중 소켓), 대역폭(5초 1줄/4줄/양방향), 패킷 캡처(96바이트/기본/전체 500개). 보통은 예전 동작과 같다.
- 캡처 분석 화면: 왼쪽 메뉴 [캡처 분석]과 캡처 결과의 [캡처 분석 열기]로 저장된 pcap을 연다. 요약(패킷 수, 시간, 프로토콜, 많이 통신한 상대), 문제 찾기(답 없는 접속 시도, DNS 실패·답 없음, 재전송, RST, ICMP 오류, 창 크기 0), 패킷 목록(거르기, 문제만 보기, 200줄씩), 패킷 상세(계층별 값, 16진수). 해석: 이더넷·VLAN·SLL·SLL2, ARP, IPv4·IPv6, TCP·UDP·ICMP·ICMPv6, DNS·DHCP·TLS(접속 이름)·HTTP·SSH. 작업 폴더의 파일만 이름으로 열고 읽기 전용(`src-tauri/src/capview.rs`, `src/components/CaptureView.tsx`). pcapng와 64MB 넘는 파일은 열지 않는다.
- 음성 입력: 설정에서 켜면(기본 꺼짐) 상황 입력칸 옆에 🎤 단추가 생기고, 누르면 Windows 음성 입력(Win+H)을 연다(`src-tauri/src/voice.rs`, 고정된 키만 보낸다). NetX는 마이크를 열지 않고 소리를 받지 않는다. Windows 음성 입력이 소리를 Microsoft로 보내므로 설정 자리에 그 내용을 적었다. PC 안에서 변환하는 방식(whisper.cpp)은 모델 크기(150~470MB) 때문에 뒤로 미뤘다.
- 말씀 구절 표시 모양을 통합프로젝트 「말씀 구절 표시 모양 (1판)」대로 바꿈: 본문은 기본 글자색 15px·줄 간격 1.7, 왼쪽 대표색 세로줄 3px, 장·절 줄은 대표색 12.5px, 낱말 중간에서 끊지 않음, 위 구분선 없음(`Word.tsx`의 `WordCard`). 본문과 문구는 건드리지 않았다. NetX는 어두운 화면 하나뿐이다.
- 스토리방화벽·안디옥 연동의 NetX 쪽 입구(`src-tauri/src/bridge/`, 초안 0.12판 8.2·6장 기준): 무작위 이름의 네임드 파이프(원격 거부), `HKCU\SOFTWARE\Antioch\Running\netx`에 이름 공개, 에피스틀 `rack.hello`·`describe`·`health`·`widgets`·`handoff(diagnose.target)`·`open`. 넘겨받은 대상은 형식 검사 뒤 입력칸에 채우고 알림만 보인다(실행 없음, 감사 로그 기록). 접속자 확인은 서명자 이름 대신 `HKLM\SOFTWARE\Antioch\Modules\<id>`의 `Exe` 경로 일치(스토리방화벽 공개판이 판마다 쓰고 버리는 키로 서명하기 때문). 등록되지 않은 접속자는 인사만 된다. 차단 요청 보내기(`firewall.block.request`)는 아직 없다.
- 설치 방식을 PC 전체 설치(Program Files, 관리자 승인)로 바꿈(`tauri.conf.json`의 `installMode: perMachine`). 설치 훅(`src-tauri/windows/hooks.nsh`)이 `HKLM\SOFTWARE\Antioch\Modules\netx`에 `Exe`·`Api`·`Kind`를 쓰고, 예전 사용자 폴더 설치판을 그 판의 제거 프로그램으로 지운다(설정·캡처·감사 로그는 남긴다). 제거할 때 등록부를 지운다. 업데이트는 [지금 업데이트]를 눌렀을 때 승인 창(UAC)이 뜬다.
- 조회한 결과 남기기: (1) 진단 메뉴마다 마지막 입력·계획·결과를 기억해 다른 메뉴에 갔다 와도 그대로 보인다(같은 메뉴를 다시 누르면 새로 시작). (2) [최근 진단]: 끝난 진단(레시피로 실행한 것)을 최근 30건·30MB까지 자료 폴더의 `history/`에 남기고, 고르면 그때의 계획과 결과로 다시 연다. 원본 출력은 건당 20만 자까지. 한 건씩 또는 모두 지울 수 있다(`src-tauri/src/history.rs`, `src/lib/history.ts`, `src/components/HistoryView.tsx`, `runs.ts`의 `restoreRun`). AI 제안·직접 명령으로 실행한 것과 터미널 내용은 기록하지 않는다.
- 최근 진단 기록 내보내기: [최근 진단]에서 한 건 또는 전부를 브라우저로 열리는 보고서(HTML) 한 파일로 작업 폴더에 저장한다(`netx-report-날짜-시각.html`, 덮어쓰지 않음). 단계별 명령·판정·핵심 수치·원본 출력이 들어가고, 스크립트는 없으며 기록 속 글자는 모두 바꿔 쓴다. [내부 주소 가리기]를 켜면 사설 주소를 [내부IP-번호]로 바꾼다. 감사 로그에 남긴다(`history.rs`의 `report`·`export`).
- 보고서에 그림 포함과 가져오기: 내보낸 보고서에 막대·선·계기·노선도·레이더·요새·하이웨이 그림(`report_fig.rs`, 기록의 그림 자료에서 스크립트 없는 HTML·SVG로 다시 그림)과 구성도 그림(기록할 때 화면이 넣어 둔 SVG, 저장 때와 같은 검사 통과분만)을 넣는다. 보고서 끝에 기록 자료를 주석(`<!--netx-data:base64-->`)으로 넣어, 다른 PC의 NetX가 [최근 진단] → [가져오기]로 그 파일을 골라 기록에 더할 수 있다("가져옴" 표시, 덮어쓰지 않음, 가린 보고서는 가린 채로). 감사 로그 `source: import`.

- 2026-11-12 내 네트워크 막대를 빨리 채움: 시작 즉시 운영체제에 나가는 길의 내 주소만 물어(UDP connect, 패킷 없음) 내 IP·어댑터·서브넷을 표시. 게이트웨이·DNS·링크 속도(PowerShell)와 공인 IP는 뒤에서 조회하고 그동안 "확인 중…", 1초마다 다시 물음. 진단은 `netinfo::settled()`로 조회가 끝나기를 기다림.

- 2026-11-12 연동 입구의 구성원 확인이 등록부의 `ClientExe`도 읽음(설계 문서 0.15판 8.3의 1·2번, 첫 연동 확정). `Exe`와 같은 조건(관리자 전용 폴더 아래, 최종 경로 통째 비교). `bridge::MEMBER_VALUES`·`admitted`, 시험 `client_exe_counts_as_a_member_on_the_same_terms`. "정보" 화면의 숫자는 "등록된 실행 파일" 수로 표기. 저장소의 `docs/RACK_PLATFORM_DRAFT.md`는 아직 0.14판 사본.

- 2026-11-12 넘겨받은 진단 대상을 지금 열린 진단의 입력칸에도 채움(`src/lib/handoff.ts`의 `handoffInputs`): 라우팅 지연 분석의 목적지, 패킷 캡처의 상대 주소·포트, 웹 장애 진단의 주소(443은 https, 80은 http). 실행 중이면 건드리지 않고, 채우면 계획은 지워진다(실행은 사용자가 누름). 알림이 떠 있는 동안 왼쪽 메뉴에서 진단을 고를 때도 같은 칸이 채워진다. 대상을 받는 칸이 없는 진단(서브넷 스캔, 구성도, 포트 추적, 대역폭)은 그대로.

- 2026-11-12 넘겨받음 알림 문구: 열린 진단에 대상을 받는 칸이 없으면 "서브넷 스캔, 구성도, 포트 추적, 대역폭은 넘겨받은 대상을 넣을 칸이 없어 그대로 둡니다."와 채워지는 진단 안내를 보인다.

- 2026-11-12 인계의 이름 꼬리표(사용자 지시, 통합프로젝트 「변경 제안 — diagnose.target 인계에 숫자 주소와 이름을 함께 싣기 (2판)」): endpoint의 `name`을 받아 형식이 맞으면(`is_domain`) `Handoff.name`으로 화면에 넘기고 알림 띠에 "주소 — 이름(으)로 접속한 것으로 기록된 주소"로만 보인다. 입력칸·명령·조회에는 숫자 주소만 쓴다. 모양이 틀린 이름은 이름만 버린다. 시험 `a_name_rides_along_as_a_label_and_is_never_the_target`.

- 2026-11-12 조회 의뢰를 바로 알아보게(사용자 결정 "라만 빼고 모두"): ① 띠를 머리줄 아래 전체 너비·대표색 바탕으로 옮기고 도착 뒤 6초 깜박임, 화면을 덮는 창 위에도 보임(`relative z-50`). ② 작업 표시줄 단추 깜박임(`request_user_attention`). ③ 머리줄 [받은 의뢰 n]과 최근 20건 목록(`src/lib/inbox.ts`, `localStorage`의 `netx.handoffs`, 되살릴 때 모양 검사), 고르면 다시 채움. ④ NetX가 앞에 있지 않을 때 Windows 알림 창(`tauri-plugin-notification`, MIT/Apache-2.0, 본문은 보낸 곳과 숫자 주소만 — `bridge::notice_text`). 알림음은 넣지 않음. 자동 실행 없음.

- 2026-11-14 의뢰받은 진단의 결과를 보낸 쪽에 자동으로 돌려줌(사용자 결정 "자동으로, 단계별 결과까지 모두". 통합프로젝트 「변경 제안 — 진단 의뢰의 결과를 보낸 쪽에 자동으로 돌려주기 (1판 초안)」 기준, **형식은 확정 전**): `bridge/result.rs`(의뢰 기억 20건, `shape`로 판정·요약·단계별 명령·상태·풀이·원본 출력, 단계당 64KB·전체 900KB에서 자름), 알림 `rack.result`(의뢰한 접속이 살아 있을 때), 메서드 `rack.result.get`(끊긴 뒤 의뢰 번호로 물음), 규격 판 1.1과 `capabilities`의 `results`. 접속 처리 루프를 `bridge::serve`(플랫폼 공통)로 옮겨 리눅스에서 시험. 결과를 기다리는 접속은 30분까지 둠. 화면: 넘겨받은 주소 그대로 실행한 레시피 진단이 끝나면 `bridge_report` → "진단 결과를 …에 보냈습니다" 알림, 감사 로그 `rack.result`(대상·받는 쪽·크기만).

- 2026-11-14 게이트웨이·DNS·링크 속도도 바로 표시(사용자 "계속 느리다"): Windows에서는 `ipconfig` 크레이트(MIT/Apache-2.0, 운영체제 API `GetAdaptersAddresses`)로 내 주소를 가진 어댑터의 게이트웨이(IPv4)·DNS(IPv4, 4개)·링크 속도를 읽는다(`netinfo::wiring`, `wiring_of`, `link_text`). 여기서 게이트웨이가 나오면 PowerShell 조회는 돌리지 않는다. 안 나오면(게이트웨이 없는 어댑터 등) 예전처럼 PowerShell이 뒤에서 채운다.
- 2026-11-14 보고서 만들기 1단계(`docs/REPORT_FEATURE_REQUEST_v1.md` 4.3의 1): 왼쪽 메뉴 [보고서 만들기] — 구역(내 네트워크·최근 진단·감사 로그)과 기간(오늘/7일/30일/직접), 가리기(기본 켬)를 골라 보고서(HTML) 한 파일을 만든다. 저장할 자리는 운영체제 저장 창(`tauri-plugin-dialog`, MIT/Apache-2.0, 백엔드에서만 띄움)에서 사용자가 고르고, 창을 닫으면 아무것도 남기지 않는다. 백엔드 `src-tauri/src/report.rs`(순수 함수 `build`, 가리기 `Masker`, 그림 검사 `safe_svg`), 명령 `report_make`·`report_cancel`·`report_open`, 화면 `src/components/ReportView.tsx`·`src/lib/report.ts`. 예전 "최근 진단 → 내보내기"(`history_export`)는 없애고 이 화면의 "최근 진단" 구역과 줄마다의 [이 건 보고서]로 흡수했다. 가져오기는 그대로이고 예전 보고서 파일과 새 보고서 모두 가져오기에 통과한다(시험). 가리기: 내부 IPv4(사설·링크 로컬·100.64.0.0/10)와 IPv6(fc00::/7, fe80::/10)는 `내부-번호`, MAC은 앞 세 자리만, PC 이름 `PC-1`, 사용자 이름 `사용자`, 이 PC의 공인 주소 `내 공인 IP`. 가리기는 글자를 바꿔 쓰기 전에, 기록 자료에서는 글자 값에만 한다(이름표와 `light`·`status` 같은 값은 그대로). 구성도 그림은 가져온 기록에도 들어 있으므로 꼬리표·속성 허용 목록(`safe_svg`)을 통과한 것만 싣는다. 보고서 머리에 CSP(`default-src 'none'`)를 넣고 무작위 입력 시험(`hostile_text_never_becomes_markup`)을 붙였다. 감사 로그 `source: "report"`. 남은 것: 2단계(구성도·캡처 분석·직접 명령), 3단계(터미널, 원자료 JSON `antioch.report/1`). 리눅스 시험 250개.

- 2026-11-14 보고서 만들기 2단계(`docs/REPORT_FEATURE_REQUEST_v1.md` 4.3의 2): 구역 `topology`·`capture`·`manual`. 구성도는 기간 안 진단 기록에서 가장 최근 구성도 조사 한 건의 그림(`safe_svg` 통과한 것만)·장비 표·포트 연결 표(`zone_topology`). 캡처 분석은 작업 폴더에서 기간 안에 저장한 `.pcap`을 최근 10개까지 `capview::open`으로 분석해 요약·찾은 문제·프로토콜·상위 대화(`zone_capture`, 패킷 내용 없음, 못 연 파일은 까닭만). 직접 명령은 실행 기록이 없어서 새로 남기기 시작: `src-tauri/src/manuallog.rs`가 자료 폴더 `manual.jsonl`에 단계마다 한 줄(명령, 상태, 종료 코드, 걸린 시간, 출력 앞 40줄·4,000바이트, 2MB 넘으면 앞 절반 버림), 명령 `manual_save`, 화면은 직접 명령·Expert 실행이 끝날 때 부른다(`App.tsx`). 보고서는 그 파일을 읽는다(`zone_manual`). 세 구역 모두 가리기·바꿔 쓰기·2,000줄 상한, 무작위 입력 시험에 포함. 남은 것: 3단계(터미널, 원자료 JSON). 리눅스 시험 254개.

- 2026-11-14 보고서 만들기 3단계(의뢰서 4.3의 3): 구역 `terminal`(감사 로그의 터미널 접속 기록에서 세션 목록 — 종류, 대상, 사용자, 시각. 기본 체크 해제, 체크하면 경고. **출력은 싣지 않는다: NetX는 터미널에 오간 글자를 저장하지 않는다**)과 원자료 JSON(요청의 `data`, 보고서 옆 `<이름>-data.json`, 머리는 2.6의 `antioch.report/1`, 구역마다 `items` — 모양은 `docs/REPORT_DATA_v1.md`, 가리면 원자료도 가린 채, JSON을 못 쓰면 보고서도 지움). 이로써 NetX 쪽 구역(4.1)은 모두 들어갔다. 리눅스 시험 256개.

- 2026-11-14 영어 말씀(World English Bible) 표시: `word/en.json`·`dedication.en.md`·`church.en.md`를 `include_str!`로 그대로 넣음(`word.rs`의 `PACKS`, 글자 수정 없음). 언어 목록 한국어·English. 고르는 순서는 `word/README.md`대로 `choose`(HKCU `SOFTWARE\Antioch\WordLang` → HKLM 같은 값 → Windows 표시 언어(화면의 `navigator.language`) → en → ko), 값은 목록과 글자 비교만. 설정 화면 "말씀 언어"(`Word.tsx`의 `WordLanguage`, 명령 `set_word_lang` → HKCU에 저장, 끄는 선택지 없음). 긴 구절은 언어마다(영어 7곳, 기다리는 화면 93곳). 오늘의 말씀은 두 언어에서 같은 날 같은 곳(README 확인 값 시험을 두 언어로). **설치 화면 첫 장의 언어 선택과 HKLM 쓰기는 넣지 않음**(자동 업데이트가 언어 창에서 멈출 수 있어 Tauri NSIS 문서 확인이 먼저). 화면 언어(UiLang)는 한국어 화면뿐이라 쓰지 않음. 리눅스 시험 259개.
