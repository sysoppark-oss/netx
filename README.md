# 넷엑스 (NetX)

현업 네트워크·시스템 엔지니어가 쓰는 도구를 한곳에 모아 두고, 상황을 말하면 어떤 도구를 어떤 옵션으로 쓸지 제안받아 바로 실행하는 Windows용 통합 작업대입니다.

```
상황 입력 → 의도 파악 → 도구 우선순위 → 도구별 옵션 조합(이유·예상 결과) → [실행] → 결과 → 판정
```

- 사용자 등급 5단계: 일반사용자, 초급·중급·고급 엔지니어, 전산관리자(OP)
- 진단 도구 레지스트리(ping/fping, tcpdump, mtr, ss, dig, nc, curl, lsof, iperf3 + 확장)
- 넷엑스 터미널: 시리얼 콘솔, SSH, Telnet, SFTP/FTP, TFTP 서버
- 시각화 4종: 패킷 하이웨이, 라우팅 노선도, 요새 도면, 소나 레이더
- 스토리방화벽 연동(별도 창, 전체 기능)

기술 스택: Tauri(Rust) + React, Windows 10/11 + WSL2

설계 문서: [docs/DESIGN.md](docs/DESIGN.md)

v0.1 개발안: [docs/DEV_PLAN_v0.1.md](docs/DEV_PLAN_v0.1.md)

## 개발 환경

- 필요: Node.js 22, Rust(stable), Windows 10/11 (WebView2 기본 포함)
- 실행: `npm install` → `npm run tauri dev`
- 백엔드 테스트: `cd src-tauri && cargo test`
- 설치 파일: `main`에 푸시하면 GitHub Actions가 Windows 설치 파일(.exe, .msi)을 만듭니다. Actions 실행 화면의 Artifacts에서 내려받습니다.
