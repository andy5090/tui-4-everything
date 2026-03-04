# 03. Catalog / Registry (Schema & Resolver)

## 3.1 Registry 파일 구조(v0.1 제안)
- registry/catalog.yaml
  - packs[]
  - tools[]
- registry/workspaces.yaml
  - workspaces[]

v0.1은 단일 YAML로 시작 가능.
v0.2+에서 tool/packs 분리 파일 및 원격 동기화 고려.

## 3.2 공통 필드 정의
### Tool
- id: string (unique)
- name: string
- category: entertainment|files|fun|edit|utility|agents|reading|ide
- tags: string[]
- audience: general|prosumer|developer|ops|security
- risk: SAFE|CAUTION|ADMIN|HIGH
- exposure: starter|search_only|labs (default: starter)
- run: { cmd: string }
- installers: Installer[]
- checks: Check[] (optional)
- notes/badges: optional

### Pack
- id, title, exposure
- tool_ids: string[]
- description(optional)

### Workspace
- id, title
- mux: tmux|zellij
- session_name (optional)
- layout.panes: Pane[]
- (optional) recommended_tools: string[]

## 3.3 Installer 스키마(v0.1)
Installer:
- platform: macos|linux
- method: brew|brew_cask|apt|dnf|pacman|pipx|npm_global|cargo|go|script|brew_or_pkg|pkg_or_brew|script_or_npm|...
- package_hints: string[]
- install_cmd: string (method가 script면 명시)
- requires_confirm: bool (script는 기본 true)
- fallback_*: optional (예: fallback_script, fallback_npm)

## 3.4 Resolver 규칙(v0.1 MUST)
목표: Linux 패키지명 차이를 자동 흡수.

### Resolver 전략
1) exact: package_hints 순서대로 설치 시도
2) search: 실패 시 패키지 매니저 검색
   - brew: brew search <hint>
   - apt: apt-cache search <hint> 또는 apt search
   - dnf: dnf search <hint>
   - pacman: pacman -Ss <hint>
3) 후보 정렬(간단): exact match > prefix > contains
4) 후보가 1개면 자동 진행, 2개 이상이면 UI에서 선택

### 안전 규칙
- script 설치는 항상 명시적 확인 + 명령 프리뷰
- agents(HIGH)는 기본적으로 search_only 노출

## 3.5 Check 스키마(v0.1)
- which: command -v <bin>
- version: <bin> --version (optional)
- custom: command string (optional)

## 3.6 상태 저장(v0.1)
- installed_tools: { tool_id: { channel, installed_at, last_ok_version? } }
- favorites: [ids...]
- recents: [{ id, kind, ts }...]
- settings: mux default, channel preference, safety toggles
