---
name: mistology
description: 프로젝트 오답 온톨로지. 작업을 시작할 때와 에러가 났을 때 이 프로젝트의 연결된 오답 요약 카드를 검색하고, 내 실수와 정답이 둘 다 확인됐을 때(테스트 실패→수정 후 통과, 사용자 정정, 되돌리고 다른 방법으로 해결) 결합 관계(--file, --symbol, --coupled)와 함께 오답노트를 남긴다. Use at the start of a task, when an error occurs, and after confirming the fix for your own mistake.
---

# MISTOLOGY: 프로젝트 오답 온톨로지

이 프로젝트에서 실제로 틀렸던 것과 코드 간 숨은 결합만 모은다. 노트 원본은 `<프로젝트 루트>/.mistology/notes/*.md`(git 커밋)이고, 검색·그래프 인덱스 `.mistology/index.db`는 git에서 빠지며 지워져도 다음 검색 때 자동 재생성된다.

## 1. 찾기 (초압축 브리핑 `-b` · 요약 카드 우선)

- **작업 시작할 때(프리플라이트):** 건드릴 모든 파일·심볼과 작업 설명을 **한 번에** 묶어 `-b`(`--brief`)로 검색한다.
  `mistology search -b "로그인 토큰 수정 src/auth/token.ts"`
- **에러가 났을 때:** 고치기 전에 에러 메시지를 그대로 넣어 검색한다.
  `mistology search -b "TS2345: Argument of type 'string' is not assignable"`
- `-b` 출력은 단 1번 호출로 직접 노트와 1-hop·2-hop 단방향 결합 사슬(`← ...`)까지 **초압축 브리핑 최대 6개**(`정답`, `피할 오답`)만 내보낸다.
  - **`정답`(또는 기본 카드의 `현재 정답`·`다음에는`)만 따른다.** `피할 오답`은 이미 틀린 길이니 절대 반복하지 않는다.
  - `← ...`(또는 `[연결: ... 경유]`)가 붙은 항목은 내가 건드릴 파일과 결합된 이웃·연쇄 모듈의 지뢰다. import가 없어도 반드시 함께 확인·수정한다.
  - 문제 설명·다음 행동까지 담긴 일반 요약 카드가 필요하면 `-b` 없이 `mistology search "..."`(기본 최대 3개), 과거 스택트레이스 전문이 꼭 필요할 때만 `mistology search [-k N] --full "..."`(옵션은 검색어 앞에)을 쓴다.
- `관련 오답노트 없음`이면 바로 진행한다.

## 2. 쓰기 (고정 관계 스키마로 연결성 남기기)

**틀렸다는 증거와 정답의 확인 근거가 둘 다 있을 때만 쓴다.** 추측으로 쓰지 않는다.

쓰기 전에 에러 메시지와 파일 경로로 먼저 `search` 한다.

| 검색 결과 | 할 일 |
|---|---|
| 같은 문제 노트가 없다 | `mistology new` |
| 같은 문제인데, 그 노트의 현재 정답이 틀렸다 | `mistology supersede <노트 id>` |
| 같은 문제이고, 현재 정답대로 하니 풀렸다 | 쓰지 않는다 (이미 기록됨) |

### 새 문제 (`new`)

```bash
mistology new \
  --problem "무엇을 하려고 했나" \
  --tried "처음 시도한 (틀린) 방법" \
  --evidence "에러 메시지나 실패한 테스트 출력 그대로" \
  --why "처음부터 틀림: 원인" \
  --fix "지금 통하는 방법" \
  --verified "통과한 테스트, 사용자 확인 등" \
  --next "다음에 같은 상황에서 처음부터 할 행동 한 줄" \
  --signature "에러 메시지 그대로" \
  --file src/auth/token.ts \
  --symbol issueToken \
  --coupled src/middleware/session.ts \
  --tag auth
```

### 현재 정답이 틀렸을 때 (`supersede`)

```bash
mistology supersede 2026-11-11-001 \
  --evidence "지금 현재 정답이 틀린 증거" \
  --why "바뀌어서 틀려짐: 원인" \
  --fix "새 정답" \
  --verified "확인 근거" \
  --next "다음에는 ..." \
  --signature "새로 나온 에러 메시지" \
  --coupled src/새로_밝혀진_결합파일.ts
```

### 규칙

- `--signature`는 필수(`new`). 에러 메시지는 요약하지 말고 **그대로** 넣는다(에러 문자열이 없으면 함수명·설정 키).
- **연결성 기록(`--file`, `--symbol`, `--coupled`)**: 직접 수정한 파일은 `--file`, 핵심 함수·타입·환경변수는 `--symbol`, **"이걸 바꿀 때 같이 안 맞춰서 터진 다른 파일·심볼"**은 반드시 `--coupled`에 넣는다. 이 값이 노트들을 1-hop·2-hop 온톨로지로 묶어 준다 (`--file`·`--coupled` 파일은 `src/workers/webhook.py`처럼 저장소 루트 상대 경로를 권장하되 유일한 파일명도 자동 연결됨).
- `--why`는 `처음부터 틀림:` 또는 `바뀌어서 틀려짐:`으로 시작한다.
- 옛 방법으로 되돌리는 것도 `supersede`로 새 버전을 붙인다.
- 관련 코드가 사라져 더 이상 유효하지 않은 노트만 frontmatter의 `status: active`를 `status: retired`로 바꾼다.
- 노트를 쓰거나 바꾸면 `.mistology/notes/`의 파일을 작업 커밋에 함께 포함한다.

## 3. 기존 프로젝트 초기 시딩 (요청 시)

- 사용자가 기존 저장소의 Git 이력으로 오답노트 초기 시딩을 요청하면, 최근 버그·회귀·되돌림 커밋(`fix`, `bug`, `revert`, `hotfix`)만 추려 커밋 메시지와 diff를 오래된 순으로 확인한다.
- **실제 틀렸던 원인(`--why`)·증거(`--evidence`)·에러 시그니처(`--signature`)와 함께 고쳐야 했던 결합 대상(`--coupled`)이 diff에서 명확한 사례만** `mistology new`로 기록하며, 이후 커밋에서 기존 해결책이 깨져 수정됐다면 `mistology supersede <id>`로 버전을 누적한다.
- 단순 기능 추가, 리팩터링, 스타일 수정이나 원인이 불명확한 추측성 이력은 절대 기록하지 않는다.
