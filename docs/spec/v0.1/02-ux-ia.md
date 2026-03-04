# 02. UX / IA

## 2.1 화면 구조(탭)
1) Home
- Starter Packs 카드(Install/Run)
- Recents / Favorites
- "Fun Demo(옵션)" 버튼: cmatrix 등 즉시 실행

2) Search
- 전체 카탈로그 퍼지 검색
- 필터 토글:
  - [x] Starter only (기본 ON)
  - [ ] Include Dev/Agents (기본 OFF)
  - [ ] Include Labs (기본 OFF)
- 검색 결과 리스트 + 우측 상세 패널(설명/배지/설치방법/리스크)

3) Packs
- Pack 리스트
- Pack 상세: 포함 툴 체크리스트(선택 설치 가능)
- Install Queue + 로그 패널

4) Workspaces
- 템플릿 리스트
- Run with: tmux / zellij (설정 기반 default)
- Workspace 상세: pane 구성 미리보기(텍스트)

5) Agents (Search-only에서도 접근 가능)
- Claude/Codex/OpenCode 카드
- 설치/업데이트/실행/"Coding Desk로 열기"

6) Settings
- 기본 mux: tmux|zellij|none
- 설치 채널 우선순위
- 스크립트 설치 확인 강제(기본 ON)
- 시크릿 저장소(가능 시 OS keychain)

## 2.2 키바인딩(v0.1)
- / : 검색 포커스
- Enter : 기본 액션(설치되어 있으면 Run, 아니면 Install prompt)
- i : Install
- u : Update
- d : Uninstall(가능한 채널에 한함)
- w : Workspace로 열기(해당 Tool 포함 템플릿 추천)
- f : Favorite 토글
- r : Refresh(카탈로그/설치상태)
- L : Log 보기(설치/실행 로그)
- ? : Help / Keymap

## 2.3 핵심 사용자 플로우

### Flow A: Starter Pack 설치
Home → Pack 선택 → Install → Queue 진행 → 완료 후 Run

### Flow B: Video Desk 실행
Workspaces → "Video Desk" → Run → tmux/zellij 세션 생성 → pane에 커맨드 실행

### Flow C: 재미 요소 즉시 실행
Home → Fun Demo → cmatrix 실행(또는 Fun Desk)

### Flow D: Agents 설치/실행
Search(Include Dev/Agents ON) 또는 Agents 탭 →
툴 선택 → Install(채널 선택/스크립트 확인) →
First-run 가이드 →
Coding Desk로 열기
