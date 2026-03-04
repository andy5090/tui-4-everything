# 08. Implementation Checklist (Rust + Ratatui)

## 8.1 기술 스택(v0.1)
- UI: ratatui + crossterm
- async: tokio
- storage: json (초기)
- yaml: serde_yaml
- logging: tracing

## 8.2 모듈 구조(권장)
- ui/
- catalog/
- installer/
- mux/
- runner/
- storage/
- security/

## 8.3 Installer Queue 상태머신(v0.1)
Idle → Queued → Installing → Success|Failed

## 8.4 tmux 컴파일 구현 핵심(v0.1 MUST)
- split-window 호출 시 `-P -F "#{pane_id}"`로 pane_id 저장
- 이후 target은 pane_id 사용
- 명령 주입은 `send-keys`

## 8.5 MVP 테스트 시나리오(v0.1)
1) Starter Pack 설치
2) 설치 로그 확인
3) Workspace 실행
4) Fun Desk 실행
5) Agents 설치 1개 이상 + Coding Desk 연결

## 8.6 v0.1 릴리즈 기준
- Starter tools 40개 이상 등록
- tmux workspaces 3개 이상 제공
- brew driver + apt driver 동작
- script install confirm UX 동작
