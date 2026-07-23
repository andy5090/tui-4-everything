# 07. Security / Safety

## 7.1 Capability와 파생 Risk 배지
- SAFE: capability 없음
- LOW: NETWORK, ACCOUNT, FILE_READ
- HIGH: FILE_WRITE, DELETE
- DANGER: SYSTEM, COMMANDS, AUTONOMOUS
- 여러 capability가 있으면 가장 높은 risk를 표시

## 7.2 설치 안전 규칙(MUST)
- script install:
  - 명시적 확인
  - 원문 명령 프리뷰
- 패키지 매니저 설치:
  - 기본 무확인 가능
  - DANGER 앱은 추가 확인

## 7.3 시크릿 저장(v0.1 SHOULD)
- 가능한 경우 OS keychain/secret-service
- fallback은 로컬 암호화 파일
- 세션 환경변수 주입만 허용

## 7.4 민감 경로 기본 deny 목록(가이드)
- ~/.ssh
- ~/.aws
- ~/.config/gcloud
- ~/.kube
- ~/.gnupg
