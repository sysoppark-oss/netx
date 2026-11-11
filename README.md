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

## 자동 업데이트

- 새 판 확인을 위해 GitHub에 접속하며 이 PC의 정보는 보내지 않습니다. 설치된 NetX는 시작 8초 뒤와 3시간마다 `raw.githubusercontent.com/sysoppark-oss/netx/<채널>-builds/manifest.txt`를 HTTPS로 읽습니다. 보내는 것은 그 파일을 달라는 요청뿐이고, PC 이름·설정·진단 결과는 보내지 않습니다(접속하는 쪽의 IP 주소는 다른 웹 접속과 마찬가지로 GitHub에 보입니다).
- 새 빌드가 있으면 시작할 때 내려받아 설치합니다(개발판 dev 채널, 정식판 main 채널 모두). 실행 중인 작업이 있으면 설치를 미루고 [지금 업데이트] 버튼으로 알립니다. 설정에서 자동 설치를 끄면 알림만 나옵니다.
- 설치 파일은 SHA-256과 업데이트 서명을 모두 확인한 뒤에만 실행합니다. 서명은 Tauri 서명 도구(minisign)로 만들며, 공개 키는 프로그램에 들어 있고 개인 키는 이 저장소의 비밀값에만 있습니다. 서명이 없거나 맞지 않으면 설치하지 않습니다.
- 직접 빌드한 실행 파일은 새 판을 확인하지 않습니다.

## 라이선스

[GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0-only). 스토리방화벽과 같은 라이선스입니다.

- 누구나 쓰고, 고치고, 다시 배포할 수 있습니다. 고친 판을 배포하거나 네트워크 서비스로 제공하면 그 소스도 같은 라이선스로 공개해야 합니다.
- NetX가 실행하는 외부 도구(ping, mtr, tcpdump, nmap 등)는 NetX에 들어 있지 않으며 각자의 라이선스를 따릅니다.

Copyright (C) 2026 박정근. 이 프로그램은 AGPL-3.0 으로 배포됩니다. 문의는 GitHub 이슈로 남겨 주세요.
