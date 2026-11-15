; NetX 설치 프로그램에 끼워 넣는 동작 (Tauri NSIS 훅).
; 근거: docs/RACK_PLATFORM_DRAFT.md 8.3장(확정) — 랙에 붙는 프로그램은 PC 전체(관리자) 설치이고,
; 설치 프로그램이 HKLM\SOFTWARE\Antioch\Modules\<id>에 실행 파일 경로를 적는다.

!macro NSIS_HOOK_POSTINSTALL
  ; 1) 랙 구성원 등록. HKLM은 관리자만 쓸 수 있다. 64비트 등록부에 쓴다.
  SetRegView 64
  WriteRegStr HKLM "SOFTWARE\Antioch\Modules\netx" "Exe" "$INSTDIR\netx.exe"
  WriteRegStr HKLM "SOFTWARE\Antioch\Modules\netx" "Api" "1.0"
  WriteRegStr HKLM "SOFTWARE\Antioch\Modules\netx" "Kind" "app"

  ; 2) 예전 사용자 폴더 설치판(%LOCALAPPDATA%\NetX)이 남아 있으면 그 판의 제거 프로그램으로 지운다.
  ;    /S는 조용히, _?=는 끝날 때까지 기다리게 한다. 설정·캡처·감사 로그(%LOCALAPPDATA%\com.sysoppark.netx)는 지우지 않는다.
  SetShellVarContext current
  ${If} ${FileExists} "$LOCALAPPDATA\NetX\uninstall.exe"
  ${AndIf} ${FileExists} "$LOCALAPPDATA\NetX\netx.exe"
  ${AndIf} "$LOCALAPPDATA\NetX" != "$INSTDIR"
    ExecWait '"$LOCALAPPDATA\NetX\uninstall.exe" /S _?=$LOCALAPPDATA\NetX'
    Delete "$LOCALAPPDATA\NetX\uninstall.exe"
    RMDir "$LOCALAPPDATA\NetX"
  ${EndIf}
  SetShellVarContext all
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  SetRegView 64
  DeleteRegKey HKLM "SOFTWARE\Antioch\Modules\netx"
  DeleteRegKey /ifempty HKLM "SOFTWARE\Antioch\Modules"
  DeleteRegKey /ifempty HKLM "SOFTWARE\Antioch"
!macroend
