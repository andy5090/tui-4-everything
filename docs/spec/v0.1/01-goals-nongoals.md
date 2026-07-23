# 01. Goals / Non-goals

## 1.1 Goals (MUST)
1. Starter Pack 4개 이상 제공(엔터테인먼트 2개 이상 포함)
2. Pack 원키 설치(Multi-select + Queue)
3. 설치 엔진은 "패키지 매니저 우선" 전략을 적용
4. tmux 워크스페이스 3개 이상 제공 및 실행 성공
5. Agents(Claude/Codex/OpenCode)는 Search-only로 제공(옵션 설치 가능)
6. capability 기반 파생 리스크 배지(SAFE/LOW/HIGH/DANGER) 제공

## 1.2 Non-goals (v0.1에서 제외)
- t4e 자체 PTY 분할(내장 멀티플렉서)
- 에이전트의 파일 변경/커맨드 실행을 t4e가 기술적으로 강제 차단(툴 내부 동작)
- 추천 알고리즘/개인화 고도화
- 원격 레지스트리 서명/검증(체크섬 수준은 v0.2+ 권장)

## 1.3 Assumptions
- macOS는 Homebrew 사용자가 많다는 가정(brew 우선)
- Linux는 배포판별 패키지명이 다르므로 package_hints + search resolver 필요
- 엔터테인먼트 툴은 계정/인증(Spotify 등)이 필요할 수 있음 → UX 가이드 필수

## 1.4 Definition of Done (v0.1)
- macOS(brew)에서 Starter Packs 90% 이상 설치 성공
- Linux(apt 계열 1종 이상)에서 Starter Packs 60% 이상 설치 성공(리졸버 포함)
- tmux workspace 실행 3개 이상(동일 레이아웃 재현 가능)
- 설치 로그/에러 원인/재시도 UX 제공
