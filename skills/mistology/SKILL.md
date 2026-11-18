---
name: mistology
description: 프로젝트 오답·정합성 온톨로지. 작업 시작·공유 헬퍼 작성 전과 에러 발생 시 연결된 오답·표준 규약(★표준)을 검색하고, 완료 전 impact로 결합 파급을 점검하며, 검증된 오답(new)이나 재사용 표준 규약(canon)을 남긴다. Use at task start, before creating shared helpers, on errors, before completion (`impact`), and after verifying fixes or canonical contracts.
---

# MISTOLOGY: 프로젝트 오답 · 정합성 온톨로지

이 프로젝트에서 실제로 틀렸던 지뢰, 재사용해야 할 표준 진입점(`★표준`), 코드 간 숨은 결합만 모은다. 노트 원본은 `<프로젝트 루트>/.mistology/notes/*.md`(git 커밋)이고, 검색·그래프 인덱스 `.mistology/index.db`는 git에서 빠지며 지워져도 자동 재생성된다.

## 1. 찾기 (`search -b`) & 완료 전 파급 점검 (`impact`)

- **작업 시작 · 새 헬퍼 작성 전(프리플라이트):** 건드릴 파일·심볼과 **수행하려는 기능어**를 한 번에 묶어 `-b`(`--brief`)로 검색한다.
  `mistology search -b "경로 정규화 로그인 토큰 src/auth/token.ts"`
  - `★표준`(`표준 규약`)이 나오면 새 함수를 중복 구현하지 말고 명시된 표준 파일·심볼을 재사용한다. 검색 결과가 비어 있으면 코드베이스에 기존 구현이 없는지 먼저 확인한 뒤 새로 작성한다.
  - **`정답`·`표준 규약`만 따르고**, `금지`·`피할 오답`·`피할 구규약`은 절대 반복하지 않는다.
  - `← ...`(또는 `[연결: ... 경유]`)가 붙은 항목은 결합된 이웃·연쇄 모듈이다. import가 없어도 **계약 호환성을 확인하고 관련 테스트를 실행한다**(필요 시 함께 수정).
- **에러가 났을 때:** 추측으로 고치기 전에 에러 메시지 원문을 그대로 넣어 `mistology search -b "<에러>"`로 검색한다.
- **작업 완료 · 커밋 전(파급 점검):** 파일을 수정한 뒤 끝내기 전에 `mistology impact`를 실행해 변경 파일과 양방향 1-hop으로 묶인 `[미수정 결합 파일]`·`[결합 심볼]`의 계약과 테스트가 빠지지 않았는지 확인한다.
- 일반 요약 카드는 `-b` 없이 `mistology search "..."`, 전문이 꼭 필요할 때만 `mistology search [-k N] --full "..."`를 쓴다.

## 2. 쓰기 (`new` · `canon` · `supersede`)

**검증된 사실만 기록한다.** 추측이나 일반론은 쓰지 않으며, 쓰기 전에 먼저 `search -b`로 중복 여부를 확인한다.

| 상황 | 할 일 |
|---|---|
| 실제 에러·실패를 겪고 수정해 검증까지 마쳤다 (기존 노트 없음) | `mistology new` |
| 다른 세션이 중복 구현하거나 어기면 충돌하는 **공유 표준 진입점(SSOT)·불변 계약**을 확립·검증했다 | `mistology canon` |
| 기존 오답노트나 표준 규약의 현재 정답/규약이 바뀌어 깨졌다 | `mistology supersede <노트 id>` |
| 규약은 그대로인데 파일 경로만 이동(rename)됐다 / 코드가 삭제돼 폐기됐다 | 해당 노트 frontmatter 경로만 갱신 / `status: retired`로 변경 |

### 새 오답 기록 (`new`)

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

### 표준 진입점 · 불변 계약 등록 (`canon`)

```bash
mistology canon \
  --intent "어떤 기능·역할을 수행할 때 이 규약을 써야 하는가" \
  --contract "반드시 재사용할 표준 함수·모듈과 지켜야 할 계약" \
  --verified "확인된 테스트나 실제 사용처 근거" \
  --forbid "금지할 중복 구현이나 위반 패턴" \
  --signature "대표 심볼명 핵심 도메인 구문" \
  --file src/paths/rebase.rs \
  --symbol rebase_cwd_paths \
  --coupled src/cli/entry.rs \
  --tag path
```

### 기존 정답 · 표준 규약 갱신 (`supersede`)

```bash
mistology supersede 2026-11-11-001 \
  --evidence "기존 정답/규약이 깨진 증거" \
  --why "바뀌어서 틀려짐: 원인" \
  --fix "새 정답 또는 새 표준 규약" \
  --verified "확인 근거" \
  --next "다음에는 ..." \
  --forbid "새 버전에도 금지할 패턴이 있으면 명시 (생략 시 이전 금지 미상속)" \
  --signature "새로 나온 에러 메시지나 식별 구문" \
  --coupled src/새로_밝혀진_결합파일.ts
```

### 규칙

- `new`의 `--signature`에는 에러 메시지 원문을 그대로 넣고, `canon`의 `--signature`에는 좁고 명확한 대표 심볼·식별 구문만 넣는다(`canon`은 실존 `--file`과 `--symbol` 최소 1개 필수).
- 직접 구현·수정한 파일은 `--file`, 핵심 함수·타입·키는 `--symbol`, 함께 계약을 맞춰야 하는 다른 파일·심볼은 `--coupled`에 넣는다.
- `--why`는 `처음부터 틀림:` 또는 `바뀌어서 틀려짐:`으로 시작한다. 노트를 쓰거나 바꾸면 `.mistology/notes/`를 커밋에 함께 포함한다.

## 3. 기존 프로젝트 초기 시딩 (요청 시)

- 사용자가 기존 저장소의 Git 이력으로 오답노트 초기 시딩을 요청하면, 최근 버그·회귀·되돌림 커밋(`fix`, `bug`, `revert`, `hotfix`)만 추려 커밋 메시지와 diff를 오래된 순으로 확인한다.
- **실제 틀렸던 원인(`--why`)·증거(`--evidence`)·에러 시그니처(`--signature`)와 함께 고쳐야 했던 결합 대상(`--coupled`)이 diff에서 명확한 사례만** `mistology new`로 기록하며, 이후 커밋에서 기존 해결책이 깨져 수정됐다면 `mistology supersede <id>`로 버전을 누적한다.
- 단순 기능 추가, 리팩터링, 스타일 수정이나 원인이 불명확한 추측성 이력은 절대 기록하지 않는다.
