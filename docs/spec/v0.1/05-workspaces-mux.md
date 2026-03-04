# 05. Workspaces & Mux (tmux/zellij)

## 5.1 목표
- v0.1은 외부 멀티플렉서 백엔드 사용
- MUST: tmux 3개 워크스페이스 제공
- SHOULD: zellij 2개 워크스페이스 제공(비차단)

## 5.2 Workspace 스키마(v0.1)
workspace:
- id, title
- mux: tmux|zellij
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
- `split-window -P -F "#{pane_id}"`로 pane_id를 추적한다.
- 방향 매핑:
  - left/right => `split-window -h`
  - up/down => `split-window -v`
- `-p <NN>`로 percent split을 적용한다.
- 생성 직후 `send-keys`로 cmd를 주입한다.
- 마지막에 focus pane 선택.

## 5.5 zellij 컴파일 규칙(v0.1 SHOULD)
- `~/.config/t4e/workspaces/<id>.kdl` 생성
- `zellij --layout <file> --session <name>` 실행
- v0.1에서는 정적 템플릿만 제공.
