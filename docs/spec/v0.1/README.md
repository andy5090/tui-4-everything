# t4e Spec v0.1 문서 묶음

## 문서 구조
- 00-overview.md: 제품 개요 / 정의 / 범위
- 01-goals-nongoals.md: 목표/비목표, 가정, 성공 기준
- 02-ux-ia.md: 화면 IA, 키바인딩, 사용자 플로우
- 03-catalog-registry.md: 카탈로그/팩/툴 매니페스트 스키마 & 설치 리졸버 규칙
- 04-starter-catalog.md: Starter v0.1 큐레이션(엔터테인먼트+파일+펀+에디터) 목록
- 05-workspaces-mux.md: tmux/zellij 워크스페이스 스펙 & 컴파일 규칙
- 06-agents.md: Claude Code / Codex CLI / OpenCode 옵션 설치 & UX/상태머신
- 07-security-safety.md: 리스크 배지, 스크립트 설치 확인, 시크릿 저장 정책
- 08-impl-checklist.md: Rust 모듈 구조, 상태머신, MVP 체크리스트

## 용어
- Tool: 단일 앱/커맨드 단위
- Pack: 툴의 큐레이션 묶음(원키 설치 단위)
- Workspace: tmux/zellij 분할 레이아웃 템플릿(원클릭 실행 단위)
- Registry: Tool/Pack/Workspace 메타데이터 저장소(로컬+원격)

## 구현 원칙
- v0.1은 "외부 멀티플렉서(tmux/zellij)" 기반 분할만 지원(내장 PTY 분할은 v0.2+)
- Starter는 일반 사용자 체감(엔터테인먼트/파일 관리) 위주
- 개발자/에이전트/IDE는 Search-only(옵션 설치)로 분리
