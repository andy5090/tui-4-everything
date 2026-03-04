# 06. Agents (Claude Code / Codex CLI / OpenCode)

## 6.1 원칙
- Agents는 HIGH risk 고정
- 기본 노출은 Search-only
- 설치는 위험 경고 필수
- script 설치는 명시적 확인 + 명령 프리뷰 필수

## 6.2 Agents 탭 UX (v0.1 MUST)
- 카드: Claude Code / Codex CLI / OpenCode
- 상태: Not installed / Installing / InstalledNeedsSetup / Ready / Failed
- 버튼: Install, Update, Run, Open Coding Desk, View Logs

## 6.3 상태 머신(v0.1)
- NotInstalled
- Installing
- InstalledNeedsSetup
- Ready
- Failed

## 6.4 설치 채널 제안(v0.1)
- Claude Code: macOS brew cask, Linux script
- Codex CLI: macOS brew cask 또는 npm, Linux npm global
- OpenCode: macOS brew/script, Linux script 우선 npm fallback

## 6.5 Coding Desk 연결
- Agent 실행 시 tmux/zellij workspace 열기
- right-bottom pane에 선택 agent 실행
- editor 우선순위: LazyVim > Helix > Micro

## 6.6 v0.1 한계
- 에이전트 내부 동작 강제 차단은 제공하지 않는다.
- HIGH 경고 + 민감 경로 주의 가이드 중심.
