# 05. Workspaces & Mux (tmux/zellij)

## 5.1 목표
- v0.1은 외부 멀티플렉서 백엔드 사용
- 사용자는 멀티플렉서 조작법을 사용하지 않고 t4e App View에서 앱을 조작
- MUST: tmux 3개 워크스페이스 제공
- SHOULD: zellij 2개 워크스페이스 제공(비차단)

## 5.2 Workspace 스키마(v0.1)
workspace:
- id, title
- mux: tmux|zellij
- tmux_view: windows|panes (optional, default: windows)
- session_name (optional)
- layout.panes[]:
  - id
  - split: root | <pane_id>
  - direction: left|right|up|down
  - size: "NN%"
  - cmd: string

## 5.3 기본 Workspace 템플릿(v0.1)
- Video Desk (tmux)
- Music Desk (tmux)
- Fun Desk (tmux)
- Files Desk (tmux)
- Coding Desk (tmux, optional)

## 5.4 tmux 컴파일 규칙(v0.1 MUST)
- 기본 `windows` 모드는 앱 하나당 tmux window 하나를 생성한다.
- 최초 window도 첫 앱에 재사용하며 빈 `main`/cwd window를 남기지 않는다.
- 앱 전환은 tmux window 이동(`C-b n/p`)으로 수행한다.
- `panes` 모드를 명시한 경우에만 아래 split 규칙을 사용한다.
- `split-window -P -F "#{pane_id}"`로 pane_id를 추적한다.
- 방향 매핑:
  - left/right => `split-window -h`
  - up/down => `split-window -v`
- `-p <NN>`로 percent split을 적용한다.
- 생성 직후 `send-keys`로 cmd를 주입한다.
- 마지막에 focus pane 선택.

## 5.5 t4e App View 규칙(v0.1 MUST)
- tmux session/window/pane은 사용자 UI에 노출하지 않는다.
- workspace 시작 후 App View를 자동으로 연다.
- t4e가 관리하는 pane만 화면 캡처, 키 입력, 종료 대상으로 허용한다.
- Tab/Shift-Tab 앱 전환, Alt+Backspace 백그라운드 복귀, Alt+Q 현재 앱 종료를 제공한다.
- Backspace와 Esc는 실행 중인 앱에 그대로 전달한다.
- 기본은 텍스트 선택 모드이며 Alt+M으로 마우스 조작 모드를 전환한다.
- 마우스 조작 모드는 목록 선택/스크롤, App View 탭 전환과 하단 제어 클릭을 지원한다.
- 그 외 키는 현재 앱에 전달하며 셸 문자열로 재해석하지 않는다.

## 5.6 zellij 컴파일 규칙(v0.1 SHOULD)
- `~/.config/t4e/workspaces/<id>.kdl` 생성
- `zellij --layout <file> --session <name>` 실행
- v0.1에서는 정적 템플릿만 제공.
