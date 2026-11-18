use mistology::note::{self, NewFields};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const SIG: &str = "TS2345: Argument of type 'string' is not assignable";

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mistology-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(&dir).unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn svec(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn make_default_note(root: &Path) -> PathBuf {
    let sigs = svec(&[SIG]);
    let files = svec(&["src/api/client.ts"]);
    let symbols = svec(&["client.get"]);
    let coupled = svec(&["src/api/types.ts"]);
    let tags = svec(&["typescript"]);
    note::new_note(
        root,
        NewFields {
            problem: "API 클라이언트에 id를 넘기다 빌드가 실패",
            tried: "문자열 id를 그대로 넘김",
            evidence: SIG,
            why: "처음부터 틀림: client.get은 number id를 받는다",
            fix: "Number(id)로 변환해서 넘김",
            verified: "npm test 통과",
            next_step: "id는 넘기기 전에 number로 바꾼다",
            signatures: &sigs,
            files: &files,
            symbols: &symbols,
            coupled: &coupled,
            tags: &tags,
            force: false,
        },
    )
    .unwrap()
}

#[test]
fn new_writes_wrong_v1_current_v2_and_ontology_fields() {
    let t = TempDir::new();
    let today = note::today_iso();
    let path = make_default_note(t.path());
    assert_eq!(path, t.path().join(format!(".mistology/notes/{today}-001.md")));
    let (meta, body) = note::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(meta.id, format!("{today}-001"));
    assert_eq!(meta.current, "v2");
    assert_eq!(meta.status, "active");
    assert_eq!(meta.tags, svec(&["typescript"]));
    assert_eq!(meta.signatures, svec(&[SIG]));
    assert_eq!(meta.files, svec(&["src/api/client.ts"]));
    assert_eq!(meta.symbols, svec(&["client.get"]));
    assert_eq!(meta.coupled, svec(&["src/api/types.ts"]));
    assert!(body.contains(&format!("## v1 (오답) {today}\n- 한 것: 문자열 id를 그대로 넘김\n- 틀린 증거: {SIG}\n")));
    assert!(body.contains(&format!("## v2 (현재 정답) {today}\n- 한 것: Number(id)로 변환해서 넘김\n")));
}

#[test]
fn ids_increment_and_never_reuse_deleted_gap() {
    let t = TempDir::new();
    let today = note::today_iso();
    let first = make_default_note(t.path());
    let sig2 = svec(&["second error"]);
    note::new_note(t.path(), NewFields {
        problem: "p", tried: "t", evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &sig2, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
    }).unwrap();
    fs::remove_file(first).unwrap();
    let sig3 = svec(&["third error"]);
    let third = note::new_note(t.path(), NewFields {
        problem: "p", tried: "t", evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &sig3, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
    }).unwrap();
    assert_eq!(third.file_name().unwrap().to_str().unwrap(), format!("{today}-003.md"));
}

#[test]
fn parse_and_validation_guard_edge_cases() {
    let t = TempDir::new();
    let tricky = "He said \"no\": a, [b] - c";
    let sigs = vec![tricky.to_string(), format!("  {tricky}  ")];
    let p = note::new_note(t.path(), NewFields {
        problem: "문제 첫 줄\n## v9 (현재 정답) 가짜",
        tried: "첫 줄\r\n## v9 (현재 정답) 가짜",
        evidence: "e",
        why: "w",
        fix: "해법 1줄\n해법 2줄",
        verified: "v",
        next_step: "다음 1줄\n다음 2줄",
        signatures: &sigs, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
    }).unwrap();
    let raw = fs::read_to_string(&p).unwrap();
    assert!(raw.contains("- 한 것: 첫 줄\n  ## v9 (현재 정답) 가짜\n"));
    let (meta, body) = note::parse(&raw).unwrap();
    assert_eq!(meta.signatures, vec![tricky.to_string()]);
    let sum = note::extract_summary(&body);
    assert_eq!(sum.problem, "문제 첫 줄 ## v9 (현재 정답) 가짜");
    assert_eq!(sum.fix, "해법 1줄 해법 2줄");
    assert_eq!(sum.next_step, "다음 1줄 다음 2줄");
    assert_eq!(sum.pitfalls, "v1: 첫 줄 ## v9 (현재 정답) 가짜");
    assert!(note::is_valid_id(&meta.id));
    assert!(!note::is_valid_id("2026-11-11-1"));
    assert!(!note::is_valid_id("../../etc/passwd"));
    assert_eq!(
        fs::read_to_string(t.path().join(".mistology/.gitignore")).unwrap(),
        "index.db*\nevents.jsonl*\n*.tmp\n"
    );

    for bad in [
        "## 문제 상황\n그냥 메모\n",
        "---\nid: 1\ncurrent: v2\nstatus: retiree\n---\n본문\n",
        "---\nid: 1\ncurrent: v0\nstatus: active\n---\n본문\n",
        "---\nid: 1\ncurrent: v2\nstatus: active\ntags: [a, b]\n---\n본문\n",
        "---\nid: 1\ncurrent: v2\nstatus: active\nsignatures:\n  - 123\n---\n본문\n",
        "---\nid: 1\ncurrent: v2\nstatus: active\nsignatures:\n- \"TS2345: x\"\n---\n본문\n",
    ] {
        assert!(note::parse(bad).is_err(), "should reject: {bad}");
    }
    let blank = svec(&["   "]);
    assert!(note::new_note(t.path(), NewFields {
        problem: "p", tried: "t", evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &blank, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
    }).is_err());
}

#[test]
fn duplicate_signature_is_refused_unless_forced() {
    let t = TempDir::new();
    let today = note::today_iso();
    make_default_note(t.path());
    let dup = vec![format!("  {SIG}\n")];
    let err = note::new_note(t.path(), NewFields {
        problem: "p", tried: "t", evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &dup, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
    }).unwrap_err();
    assert!(err.contains(&format!("{today}-001")));
    let forced = note::new_note(t.path(), NewFields {
        problem: "p", tried: "t", evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &dup, files: &[], symbols: &[], coupled: &[], tags: &[], force: true,
    }).unwrap();
    assert_eq!(forced.file_name().unwrap().to_str().unwrap(), format!("{today}-002.md"));

    // retired 노트의 signature는 새 노트 생성을 막지 않는다
    for name in [format!("{today}-001.md"), format!("{today}-002.md")] {
        let p = t.path().join(".mistology/notes").join(name);
        let retired = fs::read_to_string(&p).unwrap().replace("status: active", "status: retired");
        fs::write(&p, retired).unwrap();
    }
    let after_retire = note::new_note(t.path(), NewFields {
        problem: "p", tried: "t", evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &dup, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
    }).unwrap();
    assert_eq!(after_retire.file_name().unwrap().to_str().unwrap(), format!("{today}-003.md"));
}

#[test]
fn extract_summary_and_find_root_work() {
    let t = TempDir::new();
    fs::create_dir(t.path().join(".git")).unwrap();
    let nested = t.path().join("crates/core/src");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(note::find_root(&nested), t.path());

    let path = make_default_note(t.path());
    let (_, body) = note::parse(&fs::read_to_string(path).unwrap()).unwrap();
    let sum = note::extract_summary(&body);
    assert_eq!(sum.problem, "API 클라이언트에 id를 넘기다 빌드가 실패");
    assert_eq!(sum.fix, "Number(id)로 변환해서 넘김");
    assert_eq!(sum.next_step, "id는 넘기기 전에 number로 바꾼다");
    assert_eq!(sum.pitfalls, "v1: 문자열 id를 그대로 넘김");
}

#[test]
fn supersede_preserves_history_and_merges_ontology_links() {
    let t = TempDir::new();
    let path = make_default_note(t.path());
    let id = path.file_stem().unwrap().to_str().unwrap().to_string();
    // 끝 개행이 지워진 파일이어도 줄이 붙지 않아야 한다
    let trimmed = fs::read_to_string(&path).unwrap().trim_end_matches('\n').to_string();
    fs::write(&path, &trimmed).unwrap();

    let sigs = svec(&[SIG, "TypeError: NaN"]);
    let symbols = svec(&["parseSlug"]);
    let coupled = svec(&["middleware/auth.ts"]);
    let out = note::supersede(t.path(), &id, note::SupersedeFields {
        evidence: "TypeError: NaN",
        why: "바뀌어서 틀려짐: slug 도입",
        fix: "slug 문자열을 그대로 넘김",
        verified: "npm test 통과",
        next_step: "api/types.ts에서 id 타입 확인",
        signatures: &sigs,
        files: &[],
        symbols: &symbols,
        coupled: &coupled,
        tags: &[],
    }).unwrap();
    assert_eq!(out, path);
    assert!(fs::read_dir(t.path().join(".mistology/notes")).unwrap().all(|e| {
        e.unwrap().path().extension().and_then(|s| s.to_str()) == Some("md")
    }));

    let (meta, body) = note::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(meta.current, "v3");
    assert_eq!(meta.signatures, svec(&[SIG, "TypeError: NaN"]));
    assert_eq!(meta.files, svec(&["src/api/client.ts"]));
    assert_eq!(meta.symbols, svec(&["client.get", "parseSlug"]));
    assert_eq!(meta.coupled, svec(&["src/api/types.ts", "middleware/auth.ts"]));
    assert_eq!(meta.tags, svec(&["typescript"]));
    assert_eq!(note::SUPERSEDED, "오답, 한때 정답");
    assert!(body.contains("## v2 (오답, 한때 정답)"));
    assert!(body.contains("- 다음에는: id는 넘기기 전에 number로 바꾼다\n- 틀린 증거: TypeError: NaN\n"));
    assert_eq!(body.matches("(현재 정답)").count(), 1);

    let sum = note::extract_summary(&body);
    assert_eq!(sum.fix, "slug 문자열을 그대로 넘김");
    assert_eq!(sum.pitfalls, "v1: 문자열 id를 그대로 넘김 / v2: Number(id)로 변환해서 넘김");
}

#[test]
fn supersede_chain_grows_v2_to_v4() {
    let t = TempDir::new();
    let path = make_default_note(t.path());
    let id = path.file_stem().unwrap().to_str().unwrap().to_string();
    for (ev, fx) in [("err2\n## v9 (가짜) 2026-11-11", "fix2\n## v9 (가짜) 2026-11-11"), ("err3", "세 번째 해법")] {
        note::supersede(t.path(), &id, note::SupersedeFields {
            evidence: ev, why: "바뀌어서 틀려짐: 재발", fix: fx, verified: "v", next_step: "n",
            signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
        }).unwrap();
    }
    let (meta, body) = note::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(meta.current, "v4");
    assert_eq!(body.matches("(오답, 한때 정답)").count(), 2);
    assert!(body.contains("- 한 것: 세 번째 해법"));
}

#[test]
fn supersede_rejects_bad_id_retired_or_tampered_note() {
    let t = TempDir::new();
    for bad in ["2026-11-11-404", "../../etc/passwd"] {
        assert!(note::supersede(t.path(), bad, note::SupersedeFields {
            evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
            signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
        }).is_err());
    }
    let path = make_default_note(t.path());
    let id = path.file_stem().unwrap().to_str().unwrap().to_string();
    let original = fs::read_to_string(&path).unwrap();
    assert!(note::supersede(t.path(), &id, note::SupersedeFields {
        evidence: " \r\n ", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
    }).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let tampered = original.replace("(현재 정답)", "(오답)");
    fs::write(&path, &tampered).unwrap();
    assert!(note::supersede(t.path(), &id, note::SupersedeFields {
        evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
    }).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), tampered);

    let retired = tampered.replace("(오답)", "(현재 정답)").replace("status: active", "status: retired");
    fs::write(&path, retired).unwrap();
    assert!(note::supersede(t.path(), &id, note::SupersedeFields {
        evidence: "e", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
    }).is_err());
}

#[test]
fn search_connects_one_hop_coupled_notes_above_noise() {
    let t = TempDir::new();
    let n1 = note::new_note(t.path(), NewFields {
        problem: "토큰 발급 포맷 변경 후 인증 실패",
        tried: "token.ts만 수정",
        evidence: "401 missing cookie",
        why: "처음부터 틀림: 세션 미들웨어와 쿠키 키가 결합됨",
        fix: "token.ts와 session.ts를 함께 수정",
        verified: "cargo test 통과",
        next_step: "token.ts 수정 시 session.ts 쿠키 설정도 확인",
        signatures: &svec(&["401 missing cookie"]),
        files: &svec(&["src/auth/token.ts", "package.json"]),
        symbols: &svec(&["issueToken"]),
        coupled: &svec(&["src/middleware/session.ts"]),
        tags: &svec(&["auth"]),
        force: false,
    }).unwrap();

    // n2에는 "token.ts"라는 문자열이 전혀 없지만 n1의 coupled 관계를 통해 1-hop으로 연결된다
    let n2 = note::new_note(t.path(), NewFields {
        problem: "세션 미들웨어 TTL 단위 실수",
        tried: "밀리초 단위로 넣음",
        evidence: "Session expired too late",
        why: "처음부터 틀림: Redis EX는 초 단위",
        fix: "초 단위 정수로 설정",
        verified: "통합 테스트 통과",
        next_step: "REDIS_TTL은 반드시 초 단위로 넣는다",
        signatures: &svec(&["Session expired too late"]),
        files: &svec(&["src/middleware/session.ts"]),
        symbols: &svec(&["REDIS_TTL"]),
        coupled: &[],
        tags: &svec(&["redis"]),
        force: false,
    }).unwrap();

    // n3는 공통 파일(package.json)과 src/*.ts 토큰만 겹치는 무관한 노트 (오답 횟수가 더 많아도 file↔file로는 1-hop 확장되지 않아야 함)
    let n3 = note::new_note(t.path(), NewFields {
        problem: "결제 영수증 포맷 오류",
        tried: "소수점 버림",
        evidence: "Rounding mismatch",
        why: "처음부터 틀림: 반올림 필요",
        fix: "반올림 적용",
        verified: "결제 테스트 통과",
        next_step: "금액은 반올림 함수 사용",
        signatures: &svec(&["Rounding mismatch"]),
        files: &svec(&["src/billing/invoice.ts", "package.json"]),
        symbols: &[],
        coupled: &[],
        tags: &[],
        force: false,
    }).unwrap();
    let n3_id = n3.file_stem().unwrap().to_str().unwrap().to_string();
    note::supersede(t.path(), &n3_id, note::SupersedeFields {
        evidence: "Rounding mismatch 2", why: "바뀌어서 틀려짐: 통화 단위", fix: "반올림 2", verified: "v", next_step: "n",
        signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
    }).unwrap();

    let hits = mistology::index::search(t.path(), "토큰 발급 수정 src/auth/token.ts", 2);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].path, n1);
    assert_eq!(hits[0].via, None);
    assert_eq!(hits[1].path, n2);
    assert_eq!(hits[1].via.as_deref(), Some("src/middleware/session.ts"));

    let card0 = mistology::index::format_card(t.path(), &hits[0]);
    assert!(card0.starts_with(&format!("# .mistology/notes/{}.md [v2 | 오답 1회]\n", hits[0].id)));
    assert!(card0.contains("- 현재 정답: token.ts와 session.ts를 함께 수정\n"));
    assert!(card0.contains("- 피할 오답: v1: token.ts만 수정\n"));
    assert!(card0.contains("- 연결: files=[src/auth/token.ts, package.json] symbols=[issueToken] coupled=[src/middleware/session.ts]"));

    let card1 = mistology::index::format_card(t.path(), &hits[1]);
    assert!(card1.starts_with(&format!(
        "# .mistology/notes/{}.md [v2 | 오답 1회 | 연결: src/middleware/session.ts 경유]\n",
        hits[1].id
    )));
    assert!(card1.ends_with("- 연결: files=[src/middleware/session.ts] symbols=[REDIS_TTL]"));

    // 동점일 때 최신 노트(ranked[0])를 기준으로 1-hop 확장해야 한다
    let t_tie = TempDir::new();
    let older = note::new_note(t_tie.path(), NewFields {
        problem: "동점토큰 이슈", tried: "t", evidence: "TIE_A", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &svec(&["TIE_A"]), files: &[], symbols: &[], coupled: &svec(&["other_unlinked"]), tags: &[], force: false,
    }).unwrap();
    let newer = note::new_note(t_tie.path(), NewFields {
        problem: "동점토큰 이슈", tried: "t", evidence: "TIE_B", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &svec(&["TIE_B"]), files: &[], symbols: &[], coupled: &svec(&["shared_target"]), tags: &[], force: false,
    }).unwrap();
    let linked = note::new_note(t_tie.path(), NewFields {
        problem: "연결된 이웃", tried: "t", evidence: "NEIGHBOR_ERR", why: "w", fix: "f", verified: "v", next_step: "n",
        signatures: &svec(&["NEIGHBOR_ERR"]), files: &[], symbols: &svec(&["shared_target"]), coupled: &[], tags: &[], force: false,
    }).unwrap();
    let tie_hits = mistology::index::search(t_tie.path(), "동점토큰", 3);
    assert_eq!(tie_hits.len(), 3);
    assert_eq!(tie_hits[0].path, newer);
    assert_eq!(tie_hits[1].path, older);
    assert_eq!(tie_hits[2].path, linked);
    assert_eq!(tie_hits[2].via.as_deref(), Some("shared_target"));
}

#[test]
fn search_ranks_exact_word_and_frequent_mistakes_first() {
    let t = TempDir::new();
    let today = note::today_iso();
    for code in [429, 428, 42, 427] {
        let sig = svec(&[&format!("ERR{code}: something failed")]);
        note::new_note(t.path(), NewFields {
            problem: "빌드가 실패", tried: "t", evidence: &sig[0], why: "w", fix: "f", verified: "v", next_step: "n",
            signatures: &sig, files: &[], symbols: &[], coupled: &[], tags: &[], force: false,
        }).unwrap();
    }
    let exact = mistology::index::search(t.path(), "ERR42 something failed", 1);
    assert_eq!(exact[0].path.file_name().unwrap().to_str().unwrap(), format!("{today}-003.md"));

    // 자주 틀린(버전이 많은) 노트가 같은 조건에서 먼저 나온다
    let shaky_id = format!("{today}-004");
    for i in 0..3 {
        note::supersede(t.path(), &shaky_id, note::SupersedeFields {
            evidence: &format!("e{i}"), why: "w", fix: "f", verified: "v", next_step: "n",
            signatures: &[], files: &[], symbols: &[], coupled: &[], tags: &[],
        }).unwrap();
    }
    let ranked = mistology::index::search(t.path(), "빌드", 1);
    assert_eq!(ranked[0].id, shaky_id);
}

#[test]
fn search_handles_nfd_template_noise_and_retired_notes() {
    use unicode_normalization::UnicodeNormalization;
    let t = TempDir::new();
    assert!(mistology::index::search(t.path(), "TS2345", 3).is_empty());
    assert!(!t.path().join(".mistology").exists());

    let path = make_default_note(t.path());
    fs::write(t.path().join(".mistology/notes/broken.md"), "그냥 메모 TS2345\n").unwrap();

    let nfd_q: String = "빌드".nfd().collect();
    assert_eq!(mistology::index::search(t.path(), &nfd_q, 3)[0].path, path);
    assert!(mistology::index::search(t.path(), "결제 상태 확인 현재 정답 문제 상황 다음에는", 3).is_empty());
    assert!(mistology::index::search(t.path(), "a ? _", 3).is_empty());
    assert!(mistology::index::search(t.path(), "TS2345", 0).is_empty());

    let retired = fs::read_to_string(&path).unwrap().replace("status: active", "status: retired");
    fs::write(&path, retired).unwrap();
    assert!(mistology::index::search(t.path(), "TS2345", 3).is_empty());
}

#[test]
fn search_skips_unchanged_reindex_and_rebuilds_corrupt_db() {
    let t = TempDir::new();
    let path = make_default_note(t.path());
    assert_eq!(mistology::index::search(t.path(), "TS2345", 1)[0].path, path);

    let db_path = t.path().join(".mistology/index.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute("UPDATE cards SET problem = 'sentinel'", []).unwrap();
    }
    // 파일 mtime이 그대로면 디스크의 .md를 다시 읽지 않고 SQLite의 cards만 읽는다
    assert_eq!(mistology::index::search(t.path(), "TS2345", 1)[0].problem, "sentinel");

    // 다른 프로세스가 잠시 DB 락을 잡고 있어도 busy_timeout으로 기다리며 index.db를 삭제하지 않는다
    let db_for_lock = db_path.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let locker = std::thread::spawn(move || {
        let lock_conn = rusqlite::Connection::open(&db_for_lock).unwrap();
        lock_conn.execute_batch("BEGIN EXCLUSIVE; UPDATE cards SET status = status;").unwrap();
        ready_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(60));
        lock_conn.execute_batch("COMMIT").unwrap();
    });
    ready_rx.recv().unwrap();
    assert_eq!(mistology::index::search(t.path(), "TS2345", 1)[0].problem, "sentinel");
    locker.join().unwrap();

    // index.db가 깨져도 마크다운 원본에서 자동 복구된다
    fs::write(&db_path, b"corrupted").unwrap();
    assert_eq!(mistology::index::search(t.path(), "TS2345", 1)[0].problem, "API 클라이언트에 id를 넘기다 빌드가 실패");
}

#[test]
fn cli_prints_compact_card_by_default_and_full_with_flag() {
    let t = TempDir::new();
    fs::create_dir(t.path().join(".git")).unwrap();
    let sub = t.path().join("src/nested");
    fs::create_dir_all(&sub).unwrap();
    let out_new = mistology::run_cli(&sub, [
        "mistology", "new",
        "--problem", "음수 옵션 파싱 실패",
        "--tried", "--strict",
        "--evidence", "-Werror failed",
        "--why", "처음부터 틀림: 하이픈 값",
        "--fix", "allow_hyphen_values 설정",
        "--verified", "cargo test 통과",
        "--next", "에러 문자열은 하이픈으로 시작할 수 있음",
        "--signature", "-Werror failed",
        "--file", "src/lib.rs",
        "--symbol", "-Werror",
        "--coupled", "skills/mistology/SKILL.md",
    ]).unwrap();
    let today = note::today_iso();
    assert_eq!(out_new, format!(".mistology/notes/{today}-001.md\n"));

    let card = mistology::run_cli(&sub, ["mistology", "search", "-Werror failed"]).unwrap();
    assert!(card.contains(&format!("# .mistology/notes/{today}-001.md [v2 | 오답 1회]")));
    assert!(card.contains("- 현재 정답: allow_hyphen_values 설정"));
    assert!(card.contains("symbols=[-Werror] coupled=[skills/mistology/SKILL.md]"));
    assert!(!card.contains("## 문제 상황"));

    let full = mistology::run_cli(&sub, ["mistology", "search", "--full", "-Werror failed"]).unwrap();
    assert!(full.contains("## 문제 상황\n음수 옵션 파싱 실패"));
}

#[test]
fn cli_reports_empty_search_and_supersede_path() {
    let t = TempDir::new();
    fs::create_dir(t.path().join(".git")).unwrap();
    let help = mistology::run_cli(t.path(), ["mistology", "--help"]).unwrap();
    assert!(help.contains("프로젝트 오답 온톨로지"));
    assert!(mistology::run_cli(t.path(), ["mistology", "search"]).is_err());
    assert_eq!(mistology::run_cli(t.path(), ["mistology", "search", "nothing"]).unwrap(), "관련 오답노트 없음\n");

    let path = make_default_note(t.path());
    let id = path.file_stem().unwrap().to_str().unwrap().to_string();
    let out = mistology::run_cli(t.path(), [
        "mistology", "supersede", &id,
        "--evidence", "e2", "--why", "바뀌어서 틀려짐: x", "--fix", "f2", "--verified", "v2", "--next", "n2",
    ]).unwrap();
    assert_eq!(out, format!(".mistology/notes/{id}.md\n"));
}

#[test]
fn search_handles_korean_josa_subwords_basename_coupling_and_abstention() {
    let t = TempDir::new();

    // Note 1: bare filename in coupled ("webhook.py") + "invalid" in signature + incidental body word ("헤더")
    let n_orders = note::new_note(t.path(), NewFields {
        problem: "주문 상태 부분환불 전이 시 중복 차감",
        tried: "이벤트 ID 확인 없이 차감하고 헤더만 기록",
        evidence: "DoubleRefundError: invalid duplicate refund evt_1",
        why: "처음부터 틀림: 웹훅 재시도 시 동일 이벤트가 재전달됨",
        fix: "applied_events와 parsePayloadDigest에서 sync_lock으로 멱등성 보장",
        verified: "환불 단위 테스트 통과",
        next_step: "주문 환불 수정 시 webhook.py도 함께 확인",
        signatures: &svec(&["DoubleRefundError: invalid duplicate refund evt_1"]),
        files: &svec(&["src/orders/state.py"]),
        symbols: &svec(&["transitionOrder"]),
        coupled: &svec(&["webhook.py"]),
        tags: &svec(&["orders"]),
        force: false,
    }).unwrap();

    // Note 2: full path ("src/workers/webhook.py") created AFTER Note 1 + PascalCase/camelCase identifiers
    let n_webhook = note::new_note(t.path(), NewFields {
        problem: "웹훅 서명 타임스탬프 허용 오차 초과",
        tried: "페이로드만 HMAC 검증",
        evidence: "WebhookReplayError: timestamp skew 950s exceeds 300s",
        why: "처음부터 틀림: 구형 요청 재전송 가능",
        fix: "verifyWebhookSig에서 300초 허용 오차 검증",
        verified: "웹훅 테스트 통과",
        next_step: "MAX_WEBHOOK_SKEW_SEC 확인",
        signatures: &svec(&["WebhookReplayError: timestamp skew 950s exceeds 300s"]),
        files: &svec(&["src/workers/webhook.py"]),
        symbols: &svec(&["verifyWebhookSig"]),
        coupled: &[],
        tags: &svec(&["webhook"]),
        force: false,
    }).unwrap();

    // Note 3: retired note sharing English boilerplate ("invalid", "Error") with Note 1
    let n_retired = note::new_note(t.path(), NewFields {
        problem: "레거시 XML CP949 인코딩 오류",
        tried: "CP949 직렬화",
        evidence: "XmlEncodingError: invalid CP949 byte",
        why: "처음부터 틀림: 다국어 문자",
        fix: "UTF-8 전환 후 폐기",
        verified: "폐기 확인",
        next_step: "JSON 사용",
        signatures: &svec(&["XmlEncodingError: invalid CP949 byte"]),
        files: &svec(&["src/export/legacy_xml.py"]),
        symbols: &svec(&["exportLegacyXml"]),
        coupled: &[],
        tags: &svec(&["legacy"]),
        force: false,
    }).unwrap();
    let ret_raw = fs::read_to_string(&n_retired).unwrap().replace("status: active", "status: retired");
    fs::write(&n_retired, ret_raw).unwrap();

    // 1) Korean josa stemming ("웹훅에서", "타임스탬프가") + mixed English+Hangul josa ("WebhookReplayError가")
    let josa_hits = mistology::index::search(t.path(), "웹훅에서 타임스탬프가 어긋남", 3);
    assert_eq!(josa_hits.first().map(|h| &h.path), Some(&n_webhook));
    let mixed_josa = mistology::index::search(t.path(), "WebhookReplayError가 발생", 3);
    assert_eq!(mixed_josa.first().map(|h| &h.path), Some(&n_webhook));

    // 2) PascalCase/camelCase subword expansion ("replay" matches WebhookReplayError) + Hangul-attached ASCII identifiers
    let sub_hits = mistology::index::search(t.path(), "replay attack", 3);
    assert_eq!(sub_hits.first().map(|h| &h.path), Some(&n_webhook));
    let ko_ident_hits = mistology::index::search(t.path(), "payload digest lock", 3);
    assert_eq!(ko_ident_hits.first().map(|h| &h.path), Some(&n_orders));

    // 2b) Apostrophe contractions & Rust lifetimes around a path must not swallow query words
    let contraction_hits = mistology::index::search(
        t.path(),
        "can't replay timestamp on src/unknown/file.py because skew doesn't match",
        3,
    );
    assert_eq!(contraction_hits.first().map(|h| &h.path), Some(&n_webhook));
    let lifetime_hits = mistology::index::search(
        t.path(),
        "<'a> replay timestamp on src/unknown/file.py with &'b str",
        3,
    );
    assert_eq!(lifetime_hits.first().map(|h| &h.path), Some(&n_webhook));

    // 2c) rebase_cwd_paths resolves ../ relative paths and subpackage files that share a root filename
    fs::write(t.path().join("package.json"), "{}").unwrap();
    fs::create_dir_all(t.path().join("apps/web")).unwrap();
    fs::write(t.path().join("apps/web/package.json"), "{}").unwrap();
    fs::create_dir_all(t.path().join("packages/auth/src")).unwrap();
    fs::write(t.path().join("packages/auth/src/jwt.ts"), "").unwrap();
    let ko_long = "한글경로검증".repeat(20);
    let abs_dotdot = format!("{}/apps/web/../../packages/auth/src/jwt.ts", t.path().display());
    let rebased = note::rebase_cwd_paths(
        t.path(),
        &t.path().join("apps/web"),
        &svec(&["package.json", "../../packages/auth/src/jwt.ts", &abs_dotdot, &ko_long, "missing/file.ts"]),
    );
    assert_eq!(
        rebased,
        svec(&[
            "apps/web/package.json",
            "packages/auth/src/jwt.ts",
            "packages/auth/src/jwt.ts",
            &ko_long,
            "missing/file.ts"
        ])
    );

    // 3) Unique bare filename ("webhook.py") canonicalizes to "src/workers/webhook.py" for 1-hop coupling
    let coupled_hits = mistology::index::search(t.path(), "주문 상태 부분환불 수정 src/orders/state.py", 3);
    assert_eq!(coupled_hits.len(), 2);
    assert_eq!(coupled_hits[0].path, n_orders);
    assert_eq!(coupled_hits[1].path, n_webhook);
    assert_eq!(coupled_hits[1].via.as_deref(), Some("src/workers/webhook.py"));

    // 4) True-negative abstention: short ASCII ("web" != "webhook"), all-noise path query, and single incidental body word ("헤더")
    assert!(mistology::index::search(t.path(), "파일 수정 src/app/test.py", 3).is_empty());
    assert_eq!(
        mistology::index::search(t.path(), "파일 수정 src/orders/state.py", 3).first().map(|h| &h.path),
        Some(&n_orders)
    );
    assert!(mistology::index::search(t.path(), "정적 파일 캐시 제어 추가 src/web/static_assets.py", 3).is_empty());
    assert!(mistology::index::search(t.path(), "헬스체크 응답 헤더 추가 src/health/ping.py", 3).is_empty());
    assert!(mistology::index::search(t.path(), "XmlEncodingError: invalid CP949 byte", 3).is_empty());

    // 5) Old schema version (user_version = 0) smoothly upgrades to current SCHEMA_VERSION and rebuilds
    let db_path = t.path().join(".mistology/index.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("DELETE FROM notes_fts; PRAGMA user_version = 0;").unwrap();
    }
    let rebuilt = mistology::index::search(t.path(), "replay attack", 3);
    assert_eq!(rebuilt.first().map(|h| &h.path), Some(&n_webhook));
    let ver: i32 = rusqlite::Connection::open(&db_path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ver, mistology::index::SCHEMA_VERSION);
}

#[test]
fn multi_file_query_expands_each_seed_without_hub_file_leakage() {
    let t = TempDir::new();
    for rel in [
        "services/fx/currency.py",
        "services/ledger/settlement.py",
        "services/gateway/webhook.py",
        "services/gateway/replay_cache.py",
        "services/auth/api_keys.py",
        "services/gateway/rate_limit.py",
    ] {
        let p = t.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "# stub\n").unwrap();
    }

    // Dedicated note 1: currency.py (forward-coupled to settlement.py!)
    let n_fx = note::new_note(
        t.path(),
        NewFields {
            problem: "Support 0 and 3 decimal ISO-4217 currencies",
            tried: "Hardcoded JPY scale",
            evidence: "ValueError: Precision overflow for BHD",
            why: "처음부터 틀림: binary zero-decimal assumption",
            fix: "Use CURRENCY_EXPONENTS map in currency.py",
            verified: "fx unit tests passed",
            next_step: "Register exponent in CURRENCY_EXPONENTS",
            signatures: &svec(&["Precision overflow for BHD"]),
            files: &svec(&["services/fx/currency.py"]),
            symbols: &svec(&["CURRENCY_EXPONENTS"]),
            coupled: &svec(&["services/ledger/settlement.py"]),
            tags: &svec(&["fx"]),
            force: false,
        },
    )
    .unwrap();

    // Coupled neighbor 1: settlement.py (no mention of currency.py — ONLY reachable via 1-hop!)
    let n_settlement = note::new_note(
        t.path(),
        NewFields {
            problem: "Prevent 100x underpayment on non-2-decimal settlement payouts",
            tried: "Left divisor table incomplete",
            evidence: "AssertionError: 97.5 != 9750",
            why: "처음부터 틀림: decoupled LEDGER_MINOR_DIVISORS fallback",
            fix: "Update LEDGER_MINOR_DIVISORS in settlement.py",
            verified: "settlement unit tests passed",
            next_step: "Sync LEDGER_MINOR_DIVISORS",
            signatures: &svec(&["97.5 != 9750"]),
            files: &svec(&["services/ledger/settlement.py"]),
            symbols: &svec(&["LEDGER_MINOR_DIVISORS"]),
            coupled: &[],
            tags: &svec(&["ledger"]),
            force: false,
        },
    )
    .unwrap();

    // Dedicated note 2: webhook.py (coupled via shared symbol WebhookReplayCache)
    let n_webhook = note::new_note(
        t.path(),
        NewFields {
            problem: "Enforce two-sided webhook timestamp tolerance",
            tried: "One-sided now_ts - ts > tolerance_sec",
            evidence: "SecurityAuditError: future timestamp accepted",
            why: "처음부터 틀림: negative delta bypassed check",
            fix: "Use abs(now_ts - ts) <= tolerance_sec",
            verified: "webhook security tests passed",
            next_step: "Always check abs(now_ts - ts)",
            signatures: &svec(&["SecurityAuditError: future timestamp accepted"]),
            files: &svec(&["services/gateway/webhook.py"]),
            symbols: &svec(&["verify_webhook"]),
            coupled: &svec(&["WebhookReplayCache"]),
            tags: &svec(&["webhook"]),
            force: false,
        },
    )
    .unwrap();

    // Coupled neighbor 2: replay_cache.py (defines symbol WebhookReplayCache, no mention of webhook.py!)
    let n_replay = note::new_note(
        t.path(),
        NewFields {
            problem: "Pass custom tolerance_sec into WebhookReplayCache",
            tried: "Left WebhookReplayCache at default 300s",
            evidence: "WebhookDroppedError: 450s batch webhook dropped",
            why: "처음부터 틀림: hardcoded DEFAULT_REPLAY_WINDOW_SEC = 300",
            fix: "Initialize WebhookReplayCache(window_sec=tolerance_sec)",
            verified: "600s batch replay test passed",
            next_step: "Forward tolerance_sec to WebhookReplayCache",
            signatures: &svec(&["WebhookDroppedError: 450s"]),
            files: &svec(&["services/gateway/replay_cache.py"]),
            symbols: &svec(&["WebhookReplayCache"]),
            coupled: &[],
            tags: &svec(&["gateway"]),
            force: false,
        },
    )
    .unwrap();

    // Unrelated subsystem: rate_limit.py -> coupled to api_keys.py
    let n_ratelimit = note::new_note(
        t.path(),
        NewFields {
            problem: "Prevent rate-limit reset on secret rotation",
            tried: "Keyed bucket by raw_key hash",
            evidence: "RateLimitBypassError",
            why: "처음부터 틀림: secret suffix changes on rotation",
            fix: "Key SlidingWindowRateLimiter by immutable key_id",
            verified: "rotation limit test passed",
            next_step: "Always key rate limiter by key_id",
            signatures: &svec(&["RateLimitBypassError"]),
            files: &svec(&["services/gateway/rate_limit.py"]),
            symbols: &svec(&["SlidingWindowRateLimiter"]),
            coupled: &svec(&["services/auth/api_keys.py"]),
            tags: &svec(&["auth"]),
            force: false,
        },
    )
    .unwrap();

    // Broad 3-file cross-cutting hub note (touches api_keys.py, currency.py, webhook.py)
    let _n_py39 = note::new_note(
        t.path(),
        NewFields {
            problem: "Python 3.9 union annotation TypeError",
            tried: "str | None without future import",
            evidence: "TypeError: unsupported operand type(s) for |",
            why: "처음부터 틀림: Python 3.9 runtime union",
            fix: "Add from __future__ import annotations",
            verified: "py39 test suite passed",
            next_step: "Include future annotations on Python 3.9",
            signatures: &svec(&["unsupported operand type(s) for |"]),
            files: &svec(&[
                "services/auth/api_keys.py",
                "services/fx/currency.py",
                "services/gateway/webhook.py",
            ]),
            symbols: &svec(&["annotations"]),
            coupled: &[],
            tags: &svec(&["python39"]),
            force: false,
        },
    )
    .unwrap();

    // Case A: Asymmetric FTS-heavy seed_0 (< -50) + pure-entity seed_1 (-21.6) must still expand BOTH 1-hop neighbors with via=Some(...)
    let hits = mistology::index::search(
        t.path(),
        "Support 3-decimal ISO-4217 Precision overflow for BHD in services/fx/currency.py and services/gateway/webhook.py",
        4,
    );
    let hit_paths: Vec<PathBuf> = hits.iter().map(|h| h.path.clone()).collect();

    assert!(
        !hit_paths.contains(&n_ratelimit),
        "must not leak across unqueried hub file api_keys.py into rate_limit.py: {hit_paths:?}"
    );
    assert!(hit_paths.contains(&n_fx), "must return direct note for currency.py: {hit_paths:?}");
    assert!(hit_paths.contains(&n_webhook), "must return direct note for webhook.py: {hit_paths:?}");
    let settlement_hit = hits.iter().find(|h| h.path == n_settlement).expect("must expand settlement.py via 1-hop");
    assert_eq!(settlement_hit.via.as_deref(), Some("services/ledger/settlement.py"));
    let replay_hit = hits
        .iter()
        .find(|h| h.path == n_replay)
        .unwrap_or_else(|| panic!("must expand replay_cache.py via 1-hop: {hit_paths:?}"));
    assert_eq!(replay_hit.via.as_deref(), Some("webhookreplaycache"));

    // Case B: Querying an unindexed path alongside hub note's exact error must NOT fan out across hub's unqueried files
    fs::create_dir_all(t.path().join("services/reporting")).unwrap();
    fs::write(t.path().join("services/reporting/csv_export.py"), "# stub\n").unwrap();
    let unindexed_hits = mistology::index::search(
        t.path(),
        "Python 3.9 union annotation TypeError unsupported operand in services/reporting/csv_export.py",
        3,
    );
    let unindexed_paths: Vec<PathBuf> = unindexed_hits.iter().map(|h| h.path.clone()).collect();
    assert!(
        !unindexed_paths.contains(&n_ratelimit),
        "hub note as seed_0 must not leak into rate_limit.py when a different path was queried: {unindexed_paths:?}"
    );
}

#[test]
fn directed_two_hop_coupling_expands_chain_without_sibling_uturn() {
    let t = TempDir::new();
    for rel in [
        "services/fx/currency.py",
        "services/ledger/settlement.py",
        "services/reporting/csv_export.py",
        "services/gateway/webhook.py",
        "services/gateway/replay_cache.py",
        "services/gateway/raw_body.py",
    ] {
        let p = t.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "# stub\n").unwrap();
    }

    let n_fx = note::new_note(
        t.path(),
        NewFields {
            problem: "Support 0 and 3 decimal ISO-4217 currencies",
            tried: "Hardcoded `if currency == \"JPY\": scale = 1`",
            evidence: "AssertionError: 500000 != 5000",
            why: "처음부터 틀림: JPY only",
            fix: "Added `ZERO_DECIMAL_CURRENCIES` set",
            verified: "krw test",
            next_step: "Check zero-decimal set",
            signatures: &svec(&["AssertionError: 500000 != 5000"]),
            files: &svec(&["services/fx/currency.py"]),
            symbols: &svec(&["CURRENCY_EXPONENTS"]),
            coupled: &[],
            tags: &svec(&["fx"]),
            force: false,
        },
    )
    .unwrap();
    let fx_id = n_fx.file_stem().unwrap().to_str().unwrap().to_string();
    note::supersede(
        t.path(),
        &fx_id,
        note::SupersedeFields {
            evidence: "ValueError: Precision overflow for BHD",
            why: "바뀌어서 틀려짐: 3-decimal currencies need 1000 scale",
            fix: "Use `CURRENCY_EXPONENTS` map with `Decimal(10) ** exp` in `services/fx/currency.py`",
            verified: "bhd test",
            next_step: "새 통화 추가 시 반드시 `CURRENCY_EXPONENTS`에 소수점 지수를 등록할 것",
            signatures: &svec(&["Precision overflow for BHD"]),
            files: &svec(&["services/fx/currency.py"]),
            symbols: &svec(&["CURRENCY_EXPONENTS"]),
            coupled: &[],
            tags: &svec(&["fx"]),
        },
    )
    .unwrap();

    let n_settlement = note::new_note(
        t.path(),
        NewFields {
            problem: "Prevent 100x underpayment on settlement payouts",
            tried: "Updated currency.py alone",
            evidence: "AssertionError: 97.5 != 9750",
            why: "처음부터 틀림: settlement has independent divisor table",
            fix: "Register `Decimal(10) ** exp` in `LEDGER_MINOR_DIVISORS` inside `services/ledger/settlement.py`",
            verified: "settlement test",
            next_step: "Update LEDGER_MINOR_DIVISORS whenever currency.py changes",
            signatures: &svec(&["97.5 != 9750"]),
            files: &svec(&["services/ledger/settlement.py"]),
            symbols: &svec(&["LEDGER_MINOR_DIVISORS"]),
            coupled: &svec(&["services/fx/currency.py"]),
            tags: &svec(&["ledger"]),
            force: false,
        },
    )
    .unwrap();

    let n_csv = note::new_note(
        t.path(),
        NewFields {
            problem: "Preserve 3-decimal precision in settlement CSV exports",
            tried: "Quantized to 0.01",
            evidence: "AssertionError: m1,BHD,1.24 != m1,BHD,1.235",
            why: "처음부터 틀림: default 2-decimal rounding truncated BHD",
            fix: "Register decimal places in `REPORT_DECIMALS` inside `services/reporting/csv_export.py`",
            verified: "csv test",
            next_step: "Update REPORT_DECIMALS whenever settlement.py changes",
            signatures: &svec(&["m1,BHD,1.24 != m1,BHD,1.235"]),
            files: &svec(&["services/reporting/csv_export.py"]),
            symbols: &svec(&["REPORT_DECIMALS"]),
            coupled: &svec(&["services/ledger/settlement.py"]),
            tags: &svec(&["reporting"]),
            force: false,
        },
    )
    .unwrap();

    let n_webhook = note::new_note(
        t.path(),
        NewFields {
            problem: "Enforce two-sided webhook timestamp tolerance",
            tried: "Checked `now_ts - ts > tolerance_sec`",
            evidence: "SecurityAuditError: future timestamp accepted",
            why: "처음부터 틀림: future ts gave negative delta",
            fix: "Use `abs(now_ts - ts) <= tolerance_sec` in `services/gateway/webhook.py`",
            verified: "webhook test",
            next_step: "Use two-sided abs(now_ts - ts) check",
            signatures: &svec(&["SecurityAuditError: future timestamp accepted"]),
            files: &svec(&["services/gateway/webhook.py"]),
            symbols: &svec(&["verify_webhook"]),
            coupled: &[],
            tags: &svec(&["webhook"]),
            force: false,
        },
    )
    .unwrap();

    let n_replay = note::new_note(
        t.path(),
        NewFields {
            problem: "Sync WebhookReplayCache window with tolerance_sec",
            tried: "Hardcoded 300s replay cache",
            evidence: "WebhookDroppedError: 450s batch dropped",
            why: "처음부터 틀림: default 300s window",
            fix: "Pass `WebhookReplayCache(window_sec=tolerance_sec)` in `services/gateway/replay_cache.py`",
            verified: "replay test",
            next_step: "Sync WebhookReplayCache window_sec",
            signatures: &svec(&["WebhookDroppedError: 450s batch dropped"]),
            files: &svec(&["services/gateway/replay_cache.py"]),
            symbols: &svec(&["WebhookReplayCache"]),
            coupled: &svec(&["services/gateway/webhook.py"]),
            tags: &svec(&["gateway"]),
            force: false,
        },
    )
    .unwrap();

    let n_raw = note::new_note(
        t.path(),
        NewFields {
            problem: "Preserve trailing CRLF bytes on raw HTTP frames",
            tried: "Called body.rstrip(b\"\\r\\n\")",
            evidence: "WebhookSignatureMismatchError",
            why: "처음부터 틀림: HMAC computed over wire CRLF",
            fix: "Keep `preserve_wire_bytes: bool = True` in `services/gateway/raw_body.py`",
            verified: "crlf test",
            next_step: "한글다중바이트안전경고절대원본바이트끝의줄바꿈문자를제거하거나재직렬화하지말것한글다중바이트안전경고추가검증문자열끝",
            signatures: &svec(&["WebhookSignatureMismatchError"]),
            files: &svec(&["services/gateway/raw_body.py"]),
            symbols: &svec(&["extract_wire_body"]),
            coupled: &svec(&["services/gateway/webhook.py"]),
            tags: &svec(&["http"]),
            force: false,
        },
    )
    .unwrap();

    // 1) Forward 2-hop: currency.py -> settlement.py -> csv_export.py in a single k=3 search!
    let fwd_hits = mistology::index::search(t.path(), "services/fx/currency.py", 3);
    assert_eq!(fwd_hits.len(), 3);
    assert_eq!(fwd_hits[0].path, n_fx);
    assert_eq!(fwd_hits[0].via, None);
    assert_eq!(fwd_hits[1].path, n_settlement);
    assert_eq!(fwd_hits[1].via.as_deref(), Some("services/fx/currency.py"));
    assert_eq!(fwd_hits[2].path, n_csv);
    assert_eq!(
        fwd_hits[2].via.as_deref(),
        Some("services/fx/currency.py → services/ledger/settlement.py")
    );

    // 2) Backward 2-hop: csv_export.py -> settlement.py -> currency.py
    let bwd_hits = mistology::index::search(t.path(), "services/reporting/csv_export.py", 3);
    assert_eq!(bwd_hits.len(), 3);
    assert_eq!(bwd_hits[0].path, n_csv);
    assert_eq!(bwd_hits[1].path, n_settlement);
    assert_eq!(bwd_hits[1].via.as_deref(), Some("services/ledger/settlement.py"));
    assert_eq!(bwd_hits[2].path, n_fx);
    assert_eq!(
        bwd_hits[2].via.as_deref(),
        Some("services/ledger/settlement.py → services/fx/currency.py")
    );

    // 3) No sibling U-turn: replay_cache.py -> webhook.py MUST NOT bounce back out to sibling raw_body.py
    let sib_hits = mistology::index::search(t.path(), "services/gateway/replay_cache.py", 4);
    let sib_paths: Vec<PathBuf> = sib_hits.iter().map(|h| h.path.clone()).collect();
    assert_eq!(sib_paths, vec![n_replay.clone(), n_webhook.clone()]);

    // 4) Coupled-only entity hits retain `via = Some("services/gateway/webhook.py")`
    let wh_hits = mistology::index::search(t.path(), "services/gateway/webhook.py", 3);
    assert_eq!(wh_hits.len(), 3);
    assert_eq!(wh_hits[0].path, n_webhook);
    assert_eq!(wh_hits[0].via, None);
    for coupled_hit in &wh_hits[1..] {
        assert!(coupled_hit.path == n_replay || coupled_hit.path == n_raw);
        assert_eq!(coupled_hit.via.as_deref(), Some("services/gateway/webhook.py"));
    }

    // 5) Brief mode (`-b` / `--brief`): surfaces all 6 multi-file + 1-hop + 2-hop obligations in 1 compact call
    let brief_out = mistology::run_cli(
        t.path(),
        [
            "mistology",
            "search",
            "-b",
            "Release review for services/fx/currency.py and services/gateway/webhook.py",
        ],
    )
    .unwrap();
    let card_out = mistology::run_cli(
        t.path(),
        [
            "mistology",
            "search",
            "-k",
            "6",
            "Release review for services/fx/currency.py and services/gateway/webhook.py",
        ],
    )
    .unwrap();

    for marker in [
        "CURRENCY_EXPONENTS",
        "LEDGER_MINOR_DIVISORS",
        "REPORT_DECIMALS",
        "abs(now_ts - ts)",
        "WebhookReplayCache(window_sec=tolerance_sec)",
        "preserve_wire_bytes",
        "- 피할 오답:",
        "ZERO_DECIMAL_CURRENCIES",
    ] {
        assert!(brief_out.contains(marker), "brief_out missing {marker}:\n{brief_out}");
    }
    assert!(
        brief_out.len() * 100 < card_out.len() * 55,
        "brief_out ({} B) should be <55% of card_out ({} B):\n{brief_out}",
        brief_out.len(),
        card_out.len()
    );
}

#[test]
fn review_regressions_hub_backward_slash_tokens_and_pitfall_polarity() {
    let t = TempDir::new();
    for rel in [
        "services/fx/currency.py",
        "services/ledger/settlement.py",
        "services/reporting/csv_export.py",
        "services/gateway/webhook.py",
        "services/gateway/ingress.py",
        "services/gateway/replay_cache.py",
        "services/gateway/raw_body.py",
        "services/tax/vat.py",
        "services/auth/api_keys.py",
    ] {
        let p = t.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "# stub\n").unwrap();
    }

    let n_fx = note::new_note(
        t.path(),
        NewFields {
            problem: "Support KRW/BHD currency exponents",
            tried: "Hardcoded JPY",
            evidence: "Precision overflow",
            why: "처음부터 틀림: binary scale",
            fix: "Use `CURRENCY_EXPONENTS`",
            verified: "fx test",
            next_step: "한글긴다음행동설명문자열절단검증용새통화추가시반드시정확한소수점지수를사전에등록하고정산테이블과리포트자리수까지모두함께점검해야합니다끝부분잘림확인문자열추가",
            signatures: &svec(&["Precision overflow"]),
            files: &svec(&["services/fx/currency.py"]),
            symbols: &svec(&["CURRENCY_EXPONENTS"]),
            coupled: &[],
            tags: &svec(&["fx"]),
            force: false,
        },
    )
    .unwrap();

    let n_settlement = note::new_note(
        t.path(),
        NewFields {
            problem: "Settlement divisor sync",
            tried: "Updated fx alone",
            evidence: "97.5 != 9750",
            why: "처음부터 틀림: decoupled divisor",
            fix: "Update `LEDGER_MINOR_DIVISORS`",
            verified: "settlement test",
            next_step: "Sync LEDGER_MINOR_DIVISORS",
            signatures: &svec(&["97.5 != 9750"]),
            files: &svec(&["services/ledger/settlement.py"]),
            symbols: &svec(&["LEDGER_MINOR_DIVISORS"]),
            coupled: &svec(&["services/fx/currency.py"]),
            tags: &svec(&["ledger"]),
            force: false,
        },
    )
    .unwrap();

    let n_csv = note::new_note(
        t.path(),
        NewFields {
            problem: "CSV export 3-decimal rounding",
            tried: "Default 0.01",
            evidence: "1.24 != 1.235",
            why: "처음부터 틀림: default 2 decimals",
            fix: "Update `REPORT_DECIMALS`",
            verified: "csv test",
            next_step: "Sync REPORT_DECIMALS",
            signatures: &svec(&["1.24 != 1.235"]),
            files: &svec(&["services/reporting/csv_export.py"]),
            symbols: &svec(&["REPORT_DECIMALS"]),
            coupled: &svec(&["services/ledger/settlement.py"]),
            tags: &svec(&["csv"]),
            force: false,
        },
    )
    .unwrap();

    // 2-file primary note (`webhook.py` + `ingress.py`) vs two 1-file coupled neighbors (`replay_cache.py`, `raw_body.py`)
    let n_webhook_2file = note::new_note(
        t.path(),
        NewFields {
            problem: "Two-sided webhook timestamp check across webhook and ingress",
            tried: "One-sided subtraction",
            evidence: "Future timestamp accepted",
            why: "처음부터 틀림: negative delta",
            fix: "Use `abs(now_ts - ts) <= tolerance_sec`",
            verified: "webhook test",
            next_step: "Check abs(now_ts - ts)",
            signatures: &svec(&["Future timestamp accepted"]),
            files: &svec(&["services/gateway/webhook.py", "services/gateway/ingress.py"]),
            symbols: &svec(&["verify_webhook"]),
            coupled: &[],
            tags: &svec(&["webhook"]),
            force: false,
        },
    )
    .unwrap();

    let n_replay = note::new_note(
        t.path(),
        NewFields {
            problem: "Sync WebhookReplayCache window",
            tried: "Updated `tolerance_sec` without passing `window_sec=tolerance_sec` to `WebhookReplayCache()`",
            evidence: "WebhookDroppedError: 450s dropped",
            why: "처음부터 틀림: default 300s",
            fix: "Pass `window_sec=tolerance_sec` to `WebhookReplayCache`",
            verified: "replay test",
            next_step: "Forward window_sec",
            signatures: &svec(&["WebhookDroppedError: 450s dropped"]),
            files: &svec(&["services/gateway/replay_cache.py"]),
            symbols: &svec(&["WebhookReplayCache"]),
            coupled: &svec(&["services/gateway/webhook.py"]),
            tags: &svec(&["gateway"]),
            force: false,
        },
    )
    .unwrap();

    let n_raw = note::new_note(
        t.path(),
        NewFields {
            problem: "Preserve wire CRLF bytes",
            tried: "Stripped ``\\r\\n`` bytes",
            evidence: "WebhookSignatureMismatchError: CRLF mismatch",
            why: "처음부터 틀림: altered wire digest",
            fix: "Keep `preserve_wire_bytes=True`",
            verified: "raw test",
            next_step: "Preserve wire bytes",
            signatures: &svec(&["WebhookSignatureMismatchError: CRLF mismatch"]),
            files: &svec(&["services/gateway/raw_body.py"]),
            symbols: &svec(&["extract_wire_body"]),
            coupled: &svec(&["services/gateway/webhook.py"]),
            tags: &svec(&["http"]),
            force: false,
        },
    )
    .unwrap();

    // VAT note with `/ 100.0` inside backticks + 4 backtick spans in v2 where `ROUND_HALF_UP` is 4th!
    let n_vat = note::new_note(
        t.path(),
        NewFields {
            problem: "EU/UK VAT rounding compliance",
            tried: "Used Python float `round(net_minor * float(vat_rate_pct) / 100.0)`",
            evidence: "Float drift",
            why: "처음부터 틀림: binary float",
            fix: "Switched `calculate_vat_minor` in `services/tax/vat.py` to `Decimal` with `ROUND_HALF_UP`",
            verified: "float test",
            next_step: "Use Decimal",
            signatures: &svec(&["VAT_ERR"]),
            files: &svec(&["services/tax/vat.py"]),
            symbols: &svec(&["calculate_vat_minor"]),
            coupled: &[],
            tags: &svec(&["vat"]),
            force: false,
        },
    )
    .unwrap();
    let vat_id = n_vat.file_stem().unwrap().to_str().unwrap().to_string();
    note::supersede(
        t.path(),
        &vat_id,
        note::SupersedeFields {
            evidence: "EU/UK HMRC vector failed",
            why: "바뀌어서 틀려짐: half-up bias",
            fix: "Use `Decimal.quantize(Decimal(\"1\"), rounding=ROUND_HALF_EVEN)` in `services/tax/vat.py`",
            verified: "hmrc test",
            next_step: "Keep ROUND_HALF_EVEN",
            signatures: &svec(&["VAT_HMRC_ERR"]),
            files: &svec(&["services/tax/vat.py"]),
            symbols: &svec(&["calculate_vat_minor"]),
            coupled: &[],
            tags: &svec(&["vat"]),
        },
    )
    .unwrap();

    // 3-file cross-cutting hub note created LAST (highest ID tie-breaker!)
    let n_hub_py39 = note::new_note(
        t.path(),
        NewFields {
            problem: "Python 3.9 union TypeError",
            tried: "Wrote `str | None` without `from __future__ import annotations`",
            evidence: "TypeError: unsupported operand",
            why: "처음부터 틀림: py39 union",
            fix: "Add `from __future__ import annotations`",
            verified: "py39 test",
            next_step: "Add future annotations",
            signatures: &svec(&["unsupported operand"]),
            files: &svec(&[
                "services/auth/api_keys.py",
                "services/fx/currency.py",
                "services/gateway/webhook.py",
            ]),
            symbols: &svec(&["annotations"]),
            coupled: &[],
            tags: &svec(&["python39"]),
            force: false,
        },
    )
    .unwrap();

    // Issue 1: 3-file hub note MUST NOT leak into Backward 1-hop or 2-hop!
    let bwd_2hop = mistology::index::search(t.path(), "services/reporting/csv_export.py", 4);
    let bwd_paths: Vec<PathBuf> = bwd_2hop.iter().map(|h| h.path.clone()).collect();
    assert!(
        !bwd_paths.contains(&n_hub_py39),
        "3-file hub must not leak into Backward 2-hop: {bwd_paths:?}"
    );
    assert_eq!(bwd_paths, vec![n_csv.clone(), n_settlement.clone(), n_fx.clone()]);

    let bwd_1hop = mistology::index::search(t.path(), "services/gateway/replay_cache.py", 3);
    let bwd_1hop_paths: Vec<PathBuf> = bwd_1hop.iter().map(|h| h.path.clone()).collect();
    assert!(
        !bwd_1hop_paths.contains(&n_hub_py39),
        "3-file hub must not leak into Backward 1-hop: {bwd_1hop_paths:?}"
    );
    assert_eq!(bwd_1hop_paths[0], n_replay);
    assert_eq!(bwd_1hop_paths[1], n_webhook_2file);

    // Issue 2: 2-file primary owner (`n_webhook_2file`) MUST rank #1 above coupled-only neighbors (`n_replay`, `n_raw`) and neighbors keep `via`!
    let wh_hits = mistology::index::search(t.path(), "services/gateway/webhook.py", 3);
    assert_eq!(wh_hits[0].path, n_webhook_2file, "2-file primary owner must rank #1");
    assert_eq!(wh_hits[0].via, None);
    for nbr in &wh_hits[1..] {
        assert!(nbr.path == n_replay || nbr.path == n_raw);
        assert_eq!(nbr.via.as_deref(), Some("services/gateway/webhook.py"));
    }

    // Issue 3: Non-path slash tokens ("KRW/BHD", "HTTP/1.1") MUST NOT poison `queried_files` and block Forward 1-hop/2-hop!
    let slash_hits = mistology::index::search(t.path(), "Support KRW/BHD currency exponents over HTTP/1.1", 3);
    let slash_paths: Vec<PathBuf> = slash_hits.iter().map(|h| h.path.clone()).collect();
    assert_eq!(
        slash_paths,
        vec![n_fx.clone(), n_settlement.clone(), n_csv.clone()],
        "KRW/BHD and HTTP/1.1 must not block Forward 1-hop/2-hop expansion"
    );

    // Issue 4: Direct error-signature match on `n_raw` alongside `webhook.py` MUST NOT overwrite `n_raw.via` with `Some(...)`
    let sig_hits = mistology::index::search(
        t.path(),
        "WebhookSignatureMismatchError: CRLF mismatch in services/gateway/webhook.py",
        3,
    );
    let raw_direct = sig_hits.iter().find(|h| h.path == n_raw).unwrap();
    assert_eq!(
        raw_direct.via, None,
        "direct signature hit on n_raw must retain via=None"
    );

    // Issue 5 & 6: `format_brief` pitfall division `/ 100.0`, 4th span `ROUND_HALF_UP`, `without` negation, multi-byte truncation, and CLI flags
    let vat_brief = mistology::run_cli(t.path(), ["mistology", "search", "-b", "EU/UK VAT services/tax/vat.py"]).unwrap();
    assert!(
        vat_brief.contains("`round(net_minor * float(vat_rate_pct) / 100.0)`"),
        "must not split division `/ 100.0` mid-backtick:\n{vat_brief}"
    );
    assert!(
        vat_brief.contains("`ROUND_HALF_UP`"),
        "must retain 4th distinct anti-pattern span `ROUND_HALF_UP`:\n{vat_brief}"
    );
    let pitfall_line = vat_brief.lines().find(|l| l.starts_with("- 피할 오답:")).unwrap();
    assert!(
        !pitfall_line.contains("`Decimal`"),
        "must not list active fix token `Decimal` as pitfall:\n{pitfall_line}"
    );

    let py39_brief = mistology::run_cli(
        t.path(),
        ["mistology", "search", "-b", "Python 3.9 unsupported operand"],
    )
    .unwrap();
    let py39_pitfall = py39_brief.lines().find(|l| l.starts_with("- 피할 오답:")).unwrap();
    assert!(
        py39_pitfall.contains("without"),
        "must preserve `without` negation rather than listing bare fix as pitfall:\n{py39_pitfall}"
    );

    // Short fix + >72-char multi-byte Hangul next_step exercises `truncate_chars` safely
    let fx_brief = mistology::run_cli(t.path(), ["mistology", "search", "-b", "-k", "1", "services/fx/currency.py"]).unwrap();
    assert!(fx_brief.contains("…)\n") || fx_brief.ends_with("…)\n"), "must truncate >72-char Hangul next_step cleanly:\n{fx_brief}");

    // Empty `-b` and `-b --full` conflict
    let empty_brief = mistology::run_cli(t.path(), ["mistology", "search", "-b", "XmlEncodingError CP949"]).unwrap();
    assert_eq!(empty_brief, "관련 오답노트 없음\n");
    assert!(mistology::run_cli(t.path(), ["mistology", "search", "-b", "--full", "vat.py"]).is_err());
}

#[test]
fn setup_installs_embedded_skill_to_detected_agent_homes_and_heals_broken_symlinks() {
    let fake_home = TempDir::new();
    let home = fake_home.path();

    // Detected agent roots: `.claude` and `.gemini` exist; `.codex` does NOT exist yet.
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    fs::create_dir_all(home.join(".gemini/config/skills")).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        // 1. Dangling symlink under `.claude/skills/mistology` (simulating wiped `/tmp` clone!)
        symlink(
            home.join("wiped-tmp-clone/skills/mistology"),
            home.join(".claude/skills/mistology"),
        )
        .unwrap();

        // 2. Valid live developer symlink under `.gemini/config/skills/mistology`
        let live_dev_skill = home.join("dev-checkout/skills/mistology");
        fs::create_dir_all(&live_dev_skill).unwrap();
        fs::write(live_dev_skill.join("SKILL.md"), "# CUSTOM DEV LINK\n").unwrap();
        symlink(&live_dev_skill, home.join(".gemini/config/skills/mistology")).unwrap();

        // 3. Wrong-type symlink: `.agents/skills/mistology` pointing to a regular FILE instead of a dir!
        fs::create_dir_all(home.join(".agents/skills")).unwrap();
        let stray_file = home.join("stray_file.txt");
        fs::write(&stray_file, "not a dir").unwrap();
        symlink(&stray_file, home.join(".agents/skills/mistology")).unwrap();

        // 4. Live ancestor symlink: `.codex/skills -> external-dotfiles/skills` (must NEVER write into external-dotfiles!)
        let ext_dotfiles = home.join("external-dotfiles/skills");
        fs::create_dir_all(&ext_dotfiles).unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        symlink(&ext_dotfiles, home.join(".codex/skills")).unwrap();

        // 5. Dangling ancestor symlink: `.cursor/skills -> wiped-tmp/skills` (must NOT crash setup with EEXIST!)
        fs::create_dir_all(home.join(".cursor")).unwrap();
        symlink(home.join("wiped-tmp/skills"), home.join(".cursor/skills")).unwrap();
    }

    let out = mistology::run_cli(
        home,
        ["mistology", "setup", "--home", home.to_str().unwrap()],
    )
    .unwrap();

    // Universal `.agents/skills/mistology/SKILL.md` is always installed (and wrong-type file symlink healed!)
    let shared_skill = fs::read_to_string(home.join(".agents/skills/mistology/SKILL.md")).unwrap();
    assert!(shared_skill.contains("name: mistology"), "embedded SKILL.md must be written");
    assert!(shared_skill.contains("초기 시딩"), "embedded SKILL.md must include git seeding section");

    #[cfg(unix)]
    {
        // External dotfiles behind `.codex/skills` ancestor symlink must remain untouched!
        assert!(
            !home.join("external-dotfiles/skills/mistology/SKILL.md").exists(),
            "must never write into external repo through ancestor symlink:\n{out}"
        );
        assert!(out.contains("상위 심볼릭 링크"), "must report ancestor symlink status:\n{out}");

        // Dangling `.claude/skills/mistology` symlink must be healed into a real directory + SKILL.md
        let claude_meta = fs::symlink_metadata(home.join(".claude/skills/mistology")).unwrap();
        assert!(!claude_meta.file_type().is_symlink(), "broken symlink must be replaced with real dir");
        let claude_skill = fs::read_to_string(home.join(".claude/skills/mistology/SKILL.md")).unwrap();
        assert!(claude_skill.contains("name: mistology"));

        // Valid live `.gemini/config/skills/mistology` symlink must be preserved untouched without `--force`
        let gemini_meta = fs::symlink_metadata(home.join(".gemini/config/skills/mistology")).unwrap();
        assert!(gemini_meta.file_type().is_symlink(), "live dev symlink must be preserved without --force");
        let dev_content = fs::read_to_string(home.join("dev-checkout/skills/mistology/SKILL.md")).unwrap();
        assert_eq!(dev_content, "# CUSTOM DEV LINK\n", "must not clobber target of live dev symlink");
        assert!(out.contains("유지"), "output should report preserved symlink:\n{out}");

        // Even with `--all --force`, ancestor symlink target (`external-dotfiles`) must NEVER be mutated,
        // while direct symlink (`.gemini/config/skills/mistology`) is replaced cleanly without touching `dev-checkout`!
        mistology::run_cli(
            home,
            ["mistology", "setup", "--home", home.to_str().unwrap(), "--all", "--force"],
        )
        .unwrap();
        assert!(
            !home.join("external-dotfiles/skills/mistology/SKILL.md").exists(),
            "--force must still never write through an ancestor symlink into external-dotfiles!"
        );
        assert_eq!(
            fs::read_to_string(home.join("dev-checkout/skills/mistology/SKILL.md")).unwrap(),
            "# CUSTOM DEV LINK\n",
            "--force must replace the symlink itself, never overwrite the external target repo!"
        );
        let gemini_forced = fs::read_to_string(home.join(".gemini/config/skills/mistology/SKILL.md")).unwrap();
        assert!(gemini_forced.contains("name: mistology"));
    }
}

#[test]
fn setup_project_mode_installs_workspace_skill() {
    let repo = TempDir::new();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join(".cursor")).unwrap();
    let sub = repo.path().join("packages/api");
    fs::create_dir_all(&sub).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::create_dir_all(repo.path().join(".claude/skills")).unwrap();
        symlink(
            repo.path().join("missing-tmp/mistology"),
            repo.path().join(".claude/skills/mistology"),
        )
        .unwrap();
    }

    let out = mistology::run_cli(&sub, ["mistology", "setup", "--project"]).unwrap();
    let proj_skill = repo.path().join(".agents/skills/mistology/SKILL.md");
    assert!(proj_skill.exists(), "must install workspace skill at git repo root:\n{out}");
    assert!(
        repo.path().join(".cursor/skills/mistology/SKILL.md").exists(),
        "--project must also detect existing .cursor workspace dir:\n{out}"
    );
    #[cfg(unix)]
    assert!(
        repo.path().join(".claude/skills/mistology/SKILL.md").is_file(),
        "--project must heal dangling workspace skill symlink:\n{out}"
    );
    let body = fs::read_to_string(proj_skill).unwrap();
    assert!(body.contains("name: mistology"));
}

#[test]
fn flight_recorder_logs_jsonl_heals_gitignore_and_rotates() {
    // 1. Plain directory without `.mistology` must NEVER get `.mistology/events.jsonl` created on search/doctor
    let plain = TempDir::new();
    let _ = mistology::run_cli(plain.path(), ["mistology", "search", "nonexistent"]).unwrap();
    let _ = mistology::run_cli(
        plain.path(),
        ["mistology", "doctor", "--home", plain.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(
        !plain.path().join(".mistology").exists(),
        "search/doctor must not create .mistology in an uninitialized directory"
    );

    // 2. Repo with `.git` and subdirectories logs relative `cwd`, heals legacy `.gitignore`, and records hits + errors
    let repo = TempDir::new();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    let sub = repo.path().join("packages/api");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("handler.ts"), "export const handle = () => {};\n").unwrap();

    let created = mistology::run_cli(
        &sub,
        [
            "mistology",
            "new",
            "--problem",
            "API 타임아웃 누락",
            "--tried",
            "기본 fetch 호출",
            "--evidence",
            "TimeoutError: request hung",
            "--why",
            "처음부터 틀림: AbortSignal 미지정",
            "--fix",
            "AbortSignal.timeout(5000) 지정",
            "--verified",
            "timeout 테스트 통과",
            "--next",
            "외부 호출 시 AbortSignal 필수",
            "--signature",
            "TimeoutError: request hung",
            "--file",
            "handler.ts",
        ],
    )
    .unwrap();
    let note_id = Path::new(created.trim())
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Simulate legacy v0.1.0 `.mistology/.gitignore` missing `events.jsonl*`
    let gi_path = repo.path().join(".mistology/.gitignore");
    fs::write(&gi_path, "index.db*\n*.tmp\n").unwrap();

    let stack_q = format!(
        "File \"{}/packages/api/handler.ts\", line 9: TimeoutError",
        repo.path().display()
    );
    let bad_note_arg = format!("{}/bad-id", repo.path().display());
    let _ = mistology::run_cli(&sub, ["mistology", "search", "-b", &stack_q]).unwrap();
    let _ = mistology::run_cli(&sub, ["mistology", "supersede", &bad_note_arg, "--evidence", "e", "--why", "바뀌어서 틀려짐: w", "--fix", "f", "--verified", "v", "--next", "n"]);

    let healed_gi = fs::read_to_string(&gi_path).unwrap();
    assert!(
        healed_gi.contains("events.jsonl*"),
        "legacy .mistology/.gitignore must self-heal with events.jsonl*:\n{healed_gi}"
    );

    let events_path = repo.path().join(".mistology/events.jsonl");
    let raw_events = fs::read_to_string(&events_path).unwrap();
    let repo_abs = repo.path().to_string_lossy();
    let repo_canon = fs::canonicalize(repo.path()).unwrap().to_string_lossy().into_owned();
    assert!(
        !raw_events.contains(repo_abs.as_ref()) && !raw_events.contains(&repo_canon),
        "events.jsonl must never leak logical or canonical host repo path:\n{raw_events}"
    );

    let parsed: Vec<serde_json::Value> = raw_events
        .lines()
        .map(|l| serde_json::from_str(l).expect("valid jsonl line"))
        .collect();
    assert_eq!(parsed.len(), 3, "expected 3 logged CLI events (new, search, failed supersede)");
    assert_eq!(parsed[0]["cmd"], "new");
    assert_eq!(parsed[0]["cwd"], "packages/api");
    assert_eq!(parsed[0]["note"], note_id);
    assert!(parsed[0]["err"].is_null());

    assert_eq!(parsed[1]["cmd"], "search");
    assert_eq!(parsed[1]["mode"], "brief");
    assert_eq!(parsed[1]["hits"][0]["id"], note_id);
    assert!(parsed[1]["hits"][0]["score"].as_f64().unwrap() > 0.0);

    assert_eq!(parsed[2]["cmd"], "supersede");
    assert!(parsed[2]["err"].as_str().unwrap().contains("노트 id 형식이 아님"));

    // 3. Exceeding 256 KiB rotates `events.jsonl` -> `events.jsonl.1`
    let filler = format!("{}\n", "{\"padding\":\"x\"}".repeat(18_000));
    assert!(filler.len() > 262_144);
    fs::write(&events_path, &filler).unwrap();

    let _ = mistology::run_cli(repo.path(), ["mistology", "search", "TimeoutError"]).unwrap();
    let rotated = repo.path().join(".mistology/events.jsonl.1");
    assert!(rotated.is_file(), "oversized events.jsonl must rotate to events.jsonl.1");
    assert!(fs::metadata(&rotated).unwrap().len() > 262_144);
    let fresh_len = fs::metadata(&events_path).unwrap().len();
    assert!(
        fresh_len > 0 && fresh_len < 4096,
        "rotated fresh events.jsonl should contain only the latest event (len={fresh_len})"
    );
}

#[test]
fn search_explain_shows_plan_scores_hops_and_cutoffs() {
    let t = TempDir::new();
    let mk = |sig: &str, problem: &str, files: &[&str], coupled: &[&str]| {
        note::new_note(
            t.path(),
            NewFields {
                problem,
                tried: "이전 시도",
                evidence: sig,
                why: "처음부터 틀림: 원인",
                fix: "검증된 수정안",
                verified: "단위 테스트 통과",
                next_step: "체크리스트 준수",
                signatures: &svec(&[sig]),
                files: &svec(files),
                symbols: &[],
                coupled: &svec(coupled),
                tags: &[],
                force: true,
            },
        )
        .unwrap()
    };

    let p1 = mk(
        "AuthTokenSig1",
        "토큰 발급 페이로드 변경",
        &["src/auth/token.py"],
        &["src/middleware/session.py"],
    );
    let p2 = mk(
        "SessionMidSig2",
        "세션 미들웨어 파서 동기화",
        &["src/middleware/session.py"],
        &["src/workers/audit.py"],
    );
    let p3 = mk(
        "AuditWorkerSig3",
        "감사 워커 스키마 검증",
        &["src/workers/audit.py"],
        &[],
    );
    let p4 = mk(
        "WeakNoiseSig4",
        "전혀 무관한 배치 로깅 변경 작업",
        &["src/batch/cleanup.py"],
        &[],
    );
    let id1 = p1.file_stem().unwrap().to_str().unwrap();
    let id2 = p2.file_stem().unwrap().to_str().unwrap();
    let id3 = p3.file_stem().unwrap().to_str().unwrap();
    let id4 = p4.file_stem().unwrap().to_str().unwrap();

    // `--explain` conflicts with `-b` and `--full`
    assert!(mistology::run_cli(t.path(), ["mistology", "search", "--explain", "-b", "src/auth/token.py"]).is_err());
    assert!(mistology::run_cli(t.path(), ["mistology", "search", "--explain", "--full", "src/auth/token.py"]).is_err());

    let explain_out = mistology::run_cli(
        t.path(),
        ["mistology", "search", "--explain", "src/auth/token.py 동기화 알파 베타 배치"],
    )
    .unwrap();

    assert!(explain_out.contains("src/auth/token.py"), "must show extracted query entity:\n{explain_out}");
    assert!(explain_out.contains(id1), "must include direct hit id1:\n{explain_out}");
    assert!(explain_out.contains(id2), "must include 1-hop coupled hit id2:\n{explain_out}");
    let (selected_sec, dropped_sec) = explain_out.split_once("## [탈락/하위]").expect("must have dropped section");
    assert!(selected_sec.contains(id2), "id2 must be in selected section:\n{explain_out}");
    assert!(
        !dropped_sec.contains(id2),
        "1-hop rescued note id2 must NOT remain in dropped section:\n{explain_out}"
    );
    assert!(
        explain_out.contains("src/middleware/session.py → src/workers/audit.py") && explain_out.contains(id3),
        "must show 2-hop chain propagation for id3:\n{explain_out}"
    );
    assert!(
        dropped_sec.contains(id4),
        "must explain dropped/cutoff candidate id4:\n{explain_out}"
    );
}

#[test]
fn doctor_reports_health_broken_notes_skill_drift_and_redacts_paths() {
    let repo = TempDir::new();
    let home = TempDir::new();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    let sub = repo.path().join("services/billing");
    fs::create_dir_all(&sub).unwrap();

    let p_active = note::new_note(
        repo.path(),
        NewFields {
            problem: "정산 반올림 오차",
            tried: "float 나눗셈",
            evidence: "AssertionError: 100 != 99",
            why: "처음부터 틀림: 부동소수점 오차",
            fix: "Decimal ROUND_HALF_EVEN 사용",
            verified: "정산 테스트 통과",
            next_step: "통화 연산은 Decimal 사용",
            signatures: &svec(&["AssertionError: 100 != 99"]),
            files: &svec(&["services/billing/calc.py"]),
            symbols: &[],
            coupled: &[],
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    let active_id = p_active.file_stem().unwrap().to_str().unwrap().to_string();

    // Add 1 retired note, 1 malformed note, and 1 duplicate-ID collision note
    let notes_d = repo.path().join(".mistology/notes");
    let retired_raw = fs::read_to_string(&p_active)
        .unwrap()
        .replacen(&format!("id: {active_id}"), "id: 2026-01-01-001", 1)
        .replacen("status: active", "status: retired", 1);
    fs::write(notes_d.join("2026-01-01-001.md"), retired_raw).unwrap();
    fs::write(notes_d.join("broken.md"), "깨진 노트 (frontmatter 없음)\n").unwrap();
    let dup_raw = fs::read_to_string(&p_active).unwrap();
    fs::write(notes_d.join("2026-01-01-099.md"), dup_raw).unwrap();

    // Build index.db + record 1 ok search and 1 error event
    let _ = mistology::run_cli(&sub, ["mistology", "search", "SECRET_INTERNAL_TOKEN_XYZ AssertionError"]).unwrap();
    let _ = mistology::run_cli(&sub, ["mistology", "supersede", "invalid-id", "--evidence", "e", "--why", "바뀌어서 틀려짐: w", "--fix", "f", "--verified", "v", "--next", "n"]);

    // Install skills to `home`, then tamper `.claude/skills/mistology/SKILL.md` to simulate drift
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    mistology::run_cli(
        repo.path(),
        ["mistology", "setup", "--home", home.path().to_str().unwrap()],
    )
    .unwrap();
    fs::write(
        home.path().join(".claude/skills/mistology/SKILL.md"),
        "# outdated skill\n",
    )
    .unwrap();

    let home_trailing = format!("{}/", home.path().display());
    let report = mistology::run_cli(
        &sub,
        ["mistology", "doctor", "--home", &home_trailing],
    )
    .unwrap();

    // Privacy check: neither host repo path, host home path (logical or canonical), nor secret search query may appear!
    let home_canon = fs::canonicalize(home.path()).unwrap();
    assert!(
        !report.contains(repo.path().to_str().unwrap()),
        "doctor must redact repo absolute path:\n{report}"
    );
    assert!(
        !report.contains(home.path().to_str().unwrap()) && !report.contains(home_canon.to_str().unwrap()),
        "doctor must redact home logical & canonical paths:\n{report}"
    );
    assert!(
        !report.contains("~.claude") && !report.contains("~.agents"),
        "trailing slash on --home must not eat path separator:\n{report}"
    );
    assert!(
        !report.contains("SECRET_INTERNAL_TOKEN_XYZ"),
        "doctor must never dump raw user search queries:\n{report}"
    );

    // Health checks
    assert!(report.contains("services/billing"), "must show relative cwd:\n{report}");
    assert!(report.contains("broken.md") && report.contains("frontmatter 없음"), "must list skipped broken note:\n{report}");
    assert!(report.contains(&active_id) && report.contains("중복"), "must detect duplicate note id:\n{report}");
    assert!(report.contains("index.db") && report.contains("ok"), "must verify sqlite index integrity:\n{report}");
    assert!(report.contains("~/.agents/skills/mistology/SKILL.md") && report.contains("최신"), "must report synced skill:\n{report}");
    assert!(report.contains("~/.claude/skills/mistology/SKILL.md") && report.contains("갱신 필요"), "must detect drifted skill:\n{report}");
    assert!(report.contains("events.jsonl"), "must summarize flight recorder stats:\n{report}");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mist = repo.path().join(".mistology");
        for e in fs::read_dir(&mist).unwrap().flatten() {
            if e.path().is_file() {
                let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(0o444));
            }
        }
        fs::set_permissions(&mist, fs::Permissions::from_mode(0o555)).unwrap();
        let ro_report = mistology::run_cli(
            &sub,
            ["mistology", "doctor", "--home", &home_trailing],
        )
        .unwrap();
        let _ = fs::set_permissions(&mist, fs::Permissions::from_mode(0o755));
        for e in fs::read_dir(&mist).unwrap().flatten() {
            if e.path().is_file() {
                let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(0o644));
            }
        }
        assert!(
            ro_report.contains("- index.db: ok | schema=v4"),
            "doctor must inspect persistent WAL index.db via immutable=1 even inside 0555 RO dir:\n{ro_report}"
        );
    }
}

#[test]
fn canon_ssot_lifecycle_forbid_polarity_and_mixed_corpus_ranking() {
    let repo = TempDir::new();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join("src/paths")).unwrap();
    fs::create_dir_all(repo.path().join("src/cli")).unwrap();
    fs::write(
        repo.path().join("src/paths/rebase.rs"),
        "pub fn rebase_cwd_paths() {}\npub fn repo_root_prefixes() {}\n",
    )
    .unwrap();
    fs::write(repo.path().join("src/cli/entry.rs"), "pub fn run() {}\n").unwrap();

    // 1. Validation gates: missing --file, missing --symbol, nonexistent file without --force, `..` traversal
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "상대경로 정규화",
                "--contract",
                "rebase_cwd_paths 재사용",
                "--verified",
                "단위 테스트 통과",
                "--signature",
                "rebase_cwd_paths path canon",
                "--symbol",
                "rebase_cwd_paths",
            ]
        )
        .is_err(),
        "canon without --file must fail"
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "상대경로 정규화",
                "--contract",
                "rebase_cwd_paths 재사용",
                "--verified",
                "단위 테스트 통과",
                "--signature",
                "rebase_cwd_paths path canon",
                "--file",
                "src/paths/rebase.rs",
            ]
        )
        .is_err(),
        "canon without --symbol must fail"
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "상대경로 정규화",
                "--contract",
                "rebase_cwd_paths 재사용",
                "--verified",
                "단위 테스트 통과",
                "--signature",
                "rebase_cwd_paths path canon",
                "--file",
                "src/paths/nonexistent.rs",
                "--symbol",
                "rebase_cwd_paths",
            ]
        )
        .is_err(),
        "canon on nonexistent file without --force must fail"
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--force",
                "--intent",
                "상위 탈출 시도",
                "--contract",
                "규약",
                "--verified",
                "근거",
                "--signature",
                "escape-sig",
                "--file",
                "../outside.rs",
                "--symbol",
                "escape_fn",
            ]
        )
        .is_err(),
        "canon must reject parent traversal even with --force"
    );

    // 2. Valid `canon` note (`v1`, `wrong = 0`) with >45-char `--forbid` containing backticks
    let forbid_v1 = "개별 모듈에서 `strip_prefix`나 대소문자 구분 치환으로 경로 정규화 함수를 중복 구현하지 말 것";
    let canon_rel = mistology::run_cli(
        repo.path(),
        [
            "mistology",
            "canon",
            "--intent",
            "저장소 상대경로 정규화 및 호스트 루트 접두사 제거",
            "--contract",
            "경로 정규화는 반드시 src/paths/rebase.rs의 rebase_cwd_paths를 재사용할 것",
            "--verified",
            "tests/paths_test.rs 통과",
            "--forbid",
            forbid_v1,
            "--signature",
            "rebase_cwd_paths repo_root_prefixes",
            "--file",
            "src/paths/rebase.rs",
            "--symbol",
            "rebase_cwd_paths",
            "--coupled",
            "src/cli/entry.rs",
            "--tag",
            "path",
        ],
    )
    .unwrap();
    let canon_id = Path::new(canon_rel.trim())
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Duplicate signature guard applies to active `canon` too
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "중복 시그니처",
                "--contract",
                "규약",
                "--verified",
                "근거",
                "--signature",
                "rebase_cwd_paths repo_root_prefixes",
                "--file",
                "src/paths/rebase.rs",
                "--symbol",
                "rebase_cwd_paths",
            ]
        )
        .is_err(),
        "duplicate active canon signature must be rejected without --force"
    );

    // Add a repeated `mistake` note (`wrong = 2` via supersede) in the same corpus
    let mistake_p = note::new_note(
        repo.path(),
        NewFields {
            problem: "정산 수수료 누락 에러",
            tried: "통화만 추가",
            evidence: "KeyError: 'VND' in FEE_BPS",
            why: "처음부터 틀림: 수수료 테이블 누락",
            fix: "FEE_BPS 동시 등록",
            verified: "정산 테스트 통과",
            next_step: "통화 추가 시 FEE_BPS 확인",
            signatures: &svec(&["KeyError: 'VND' in FEE_BPS"]),
            files: &svec(&["src/billing/fee.rs"]),
            symbols: &svec(&["FEE_BPS"]),
            coupled: &[],
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    let mistake_id = mistake_p.file_stem().unwrap().to_str().unwrap().to_string();
    note::supersede(
        repo.path(),
        &mistake_id,
        note::SupersedeFields {
            evidence: "KeyError: 'KWD' in FEE_BPS",
            why: "바뀌어서 틀려짐: 3소수점 통화 분기 누락",
            fix: "CURRENCY_SCALE과 FEE_BPS 동시 검증",
            verified: "KWD 테스트 통과",
            next_step: "통화 상수 일괄 검증",
            signatures: &svec(&["KeyError: 'KWD' in FEE_BPS"]),
            files: &[],
            symbols: &[],
            coupled: &[],
            tags: &[],
        },
    )
    .unwrap();

    // 3. Brand-new file + functional intent words recalls `canon` with `★표준` and intact `- 금지:` line
    let brief = mistology::run_cli(
        repo.path(),
        [
            "mistology",
            "search",
            "-b",
            "src/audit/exporter.rs 상대경로 정규화",
        ],
    )
    .unwrap();
    assert!(
        brief.contains(&canon_id) && brief.contains("v1 ★표준"),
        "new-file functional intent query must recall canon v1:\n{brief}"
    );
    assert!(
        brief.contains(&format!("- 금지: {forbid_v1}")),
        "canon forbid must remain intact as its own line without compact_pitfalls inversion:\n{brief}"
    );
    let card = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "src/audit/exporter.rs 상대경로 정규화"],
    )
    .unwrap();
    assert!(
        card.contains(&format!("- 금지: {forbid_v1}")) && card.contains("표준 규약"),
        "standard card must also display canon header and forbid line:\n{card}"
    );

    // 4. Unrelated new file + single overlapping common body word ("정규화") must ABSTAIN!
    let abstain = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "-b", "src/notify/mailer.rs 정규화"],
    )
    .unwrap();
    assert_eq!(
        abstain, "관련 오답노트 없음\n",
        "single body-word overlap on canon must not false-positive:\n{abstain}"
    );

    // 5. Exact mistake error signature still ranks `#1` cleanly
    let err_top = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "-b", "KeyError: 'KWD' in FEE_BPS"],
    )
    .unwrap();
    assert!(
        err_top.lines().next().unwrap().contains(&mistake_id),
        "exact mistake signature must remain top-1:\n{err_top}"
    );

    // 6. Superseding `canon` without `--forbid` moves v1 to pitfalls (`wrong=1`) and drops stale v1 forbid!
    mistology::run_cli(
        repo.path(),
        [
            "mistology",
            "supersede",
            &canon_id,
            "--evidence",
            "Windows UNC 경로에서 접두사 누락 확인",
            "--why",
            "바뀌어서 틀려짐: Windows \\\\?\\ 접두사 지원 필요",
            "--fix",
            "clean_dir_prefix 통합 유틸리티 사용",
            "--verified",
            "windows_unc 테스트 통과",
            "--next",
            "경로 비교 전 clean_dir_prefix 호출",
        ],
    )
    .unwrap();

    let v2_brief = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "-b", "src/paths/rebase.rs"],
    )
    .unwrap();
    assert!(
        v2_brief.contains("v2 ★표준") && v2_brief.contains("clean_dir_prefix"),
        "superseded canon must advance to v2:\n{v2_brief}"
    );
    assert!(
        !v2_brief.contains("- 금지:"),
        "old v1 forbid must NOT auto-inherit onto v2 when --forbid was omitted:\n{v2_brief}"
    );

    // 7. Real 16-column Schema v3 -> v4 auto-rebuild + backup survival check
    let backup_path = repo.path().join(".mistology/index.db.backup");
    fs::write(&backup_path, b"user- precious-backup").unwrap();
    {
        let db_file = repo.path().join(".mistology/index.db");
        let _ = fs::remove_file(&db_file);
        let _ = fs::remove_file(repo.path().join(".mistology/index.db-wal"));
        let _ = fs::remove_file(repo.path().join(".mistology/index.db-shm"));
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 3;
             CREATE TABLE cards (
                 id TEXT PRIMARY KEY, mtime INTEGER NOT NULL, size INTEGER NOT NULL,
                 current TEXT NOT NULL, wrong INTEGER NOT NULL,
                 problem TEXT NOT NULL, fix TEXT NOT NULL, next_step TEXT NOT NULL,
                 pitfalls TEXT NOT NULL, signatures TEXT NOT NULL,
                 files TEXT NOT NULL, symbols TEXT NOT NULL, coupled TEXT NOT NULL,
                 tags TEXT NOT NULL, evidence TEXT NOT NULL, body TEXT NOT NULL
             );
             CREATE TABLE edges (note_id TEXT NOT NULL, kind TEXT NOT NULL, entity TEXT NOT NULL);",
        )
        .unwrap();
    }
    let after_mig = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "-b", "src/paths/rebase.rs"],
    )
    .unwrap();
    assert!(
        after_mig.contains(&canon_id) && after_mig.contains("v2 ★표준"),
        "real 16-col v3 schema must rebuild cleanly to v4:\n{after_mig}"
    );
    assert_eq!(
        fs::read(&backup_path).unwrap(),
        b"user- precious-backup",
        "index.db.backup must NEVER be deleted during schema rebuild"
    );

    // Corrupt index.db with non-SQLite bytes and verify backup still survives recovery!
    fs::write(repo.path().join(".mistology/index.db"), b"CORRUPT_BYTES_NOT_SQLITE").unwrap();
    let after_corrupt = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "-b", "src/paths/rebase.rs"],
    )
    .unwrap();
    assert!(after_corrupt.contains(&canon_id));
    assert_eq!(
        fs::read(&backup_path).unwrap(),
        b"user- precious-backup",
        "index.db.backup must NEVER be deleted during corrupt index.db recovery"
    );
}

#[test]
fn impact_bidirectional_one_hop_and_git_porcelain_collector() {
    use std::process::Command;

    // 1. Non-Git directory without explicit paths returns a clear error, while explicit paths work!
    let non_git = TempDir::new();
    fs::create_dir_all(non_git.path().join("src")).unwrap();
    fs::write(non_git.path().join("src/a.py"), "x = 1\n").unwrap();
    fs::write(non_git.path().join("src/b.py"), "y = 2\n").unwrap();
    fs::write(non_git.path().join("src/c.py"), "z = 3\n").unwrap();

    // Single note A -> B (forward) and single note C -> A (reverse), plus A -> pure symbol `FEE_CAP`
    note::new_note(
        non_git.path(),
        NewFields {
            problem: "A 변경 시 B 스키마 파손",
            tried: "A만 변경",
            evidence: "SchemaError in b.py",
            why: "처음부터 틀림: B 파서 미동기화",
            fix: "A와 B의 페이로드 키를 함께 맞출 것",
            verified: "통합 테스트 통과",
            next_step: "b.py 호환성 확인",
            signatures: &svec(&["SchemaError in b.py"]),
            files: &svec(&["src/a.py"]),
            symbols: &[],
            coupled: &svec(&["src/b.py", "FEE_CAP"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    note::new_note(
        non_git.path(),
        NewFields {
            problem: "C가 A의 반환 포맷에 의존",
            tried: "C 단독 가정",
            evidence: "FormatError in c.py",
            why: "처음부터 틀림: A 포맷 변경 시 C 깨짐",
            fix: "A 포맷 수정 시 c.py 테스트 실행",
            verified: "c 테스트 통과",
            next_step: "c.py 계약 확인",
            signatures: &svec(&["FormatError in c.py"]),
            files: &svec(&["src/c.py"]),
            symbols: &[],
            coupled: &svec(&["src/a.py"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();

    let err_no_git = mistology::run_cli(non_git.path(), ["mistology", "impact"]).unwrap_err();
    assert!(
        err_no_git.contains("Git"),
        "non-git auto mode must explain error:\n{err_no_git}"
    );

    // Delete `src/a.py` on disk and run explicit `impact a.py` from `src/` subdir (tests lexical rebase on deleted path!)
    fs::remove_file(non_git.path().join("src/a.py")).unwrap();
    let imp_explicit = mistology::run_cli(
        &non_git.path().join("src"),
        ["mistology", "impact", "a.py"],
    )
    .unwrap();
    assert!(
        imp_explicit.contains("src/b.py") && imp_explicit.contains("src/c.py"),
        "impact on a.py must surface BOTH forward (src/b.py) and reverse (src/c.py) 1-hop targets:\n{imp_explicit}"
    );
    assert!(
        imp_explicit.contains("FEE_CAP"),
        "impact must mention coupled symbol FEE_CAP without listing it as a file path:\n{imp_explicit}"
    );

    // When both `a.py` and `b.py` are passed, `b.py` moves to co-modified (`함께 수정됨`) while `c.py` stays unchecked (`미수정`)!
    let imp_both = mistology::run_cli(
        non_git.path(),
        ["mistology", "impact", "src/a.py", "src/b.py"],
    )
    .unwrap();
    let (unchecked_part, comod_part) = imp_both
        .split_once("함께 수정됨")
        .expect("must have co-modified section");
    assert!(unchecked_part.contains("src/c.py") && !unchecked_part.contains("src/b.py"));
    assert!(comod_part.contains("src/b.py"));

    // 2. Real Git repo: unborn HEAD, untracked file, rename with space, deletion, `--base`, invalid `--base`, and linked worktree
    let git_repo = TempDir::new();
    let git = |dir: &Path, args: &[&str]| {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(git_repo.path(), &["init", "-b", "main"]);
    git(git_repo.path(), &["config", "user.name", "Test"]);
    git(git_repo.path(), &["config", "user.email", "test@example.com"]);

    fs::create_dir_all(git_repo.path().join("pkg/sub src")).unwrap();
    fs::write(git_repo.path().join("pkg/sub src/alpha file.py"), "v = 1\n").unwrap();
    fs::write(git_repo.path().join("pkg/peer.py"), "p = 1\n").unwrap();
    fs::write(git_repo.path().join("pkg/untracked_entry.py"), "u = 1\n").unwrap();

    note::new_note(
        git_repo.path(),
        NewFields {
            problem: "공백 파일명 결합",
            tried: "단독 변경",
            evidence: "AlphaPeerErr",
            why: "처음부터 틀림: peer 동기화 누락",
            fix: "peer.py 함께 검증",
            verified: "테스트 통과",
            next_step: "peer 확인",
            signatures: &svec(&["AlphaPeerErr"]),
            files: &svec(&["pkg/sub src/alpha file.py", "pkg/untracked_entry.py"]),
            symbols: &[],
            coupled: &svec(&["pkg/peer.py"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();

    // Unborn HEAD + untracked files must work via `status --porcelain=v1 -z`!
    let unborn_imp = mistology::run_cli(git_repo.path(), ["mistology", "impact"]).unwrap();
    assert!(
        unborn_imp.contains("pkg/peer.py"),
        "unborn HEAD with untracked files must still detect impact:\n{unborn_imp}"
    );

    // Commit baseline (`c1`), removing `untracked_entry.py` first so we can test untracked after commit too
    fs::remove_file(git_repo.path().join("pkg/untracked_entry.py")).unwrap();
    git(git_repo.path(), &["add", "."]);
    git(git_repo.path(), &["commit", "-m", "c1"]);

    let clean_imp = mistology::run_cli(git_repo.path(), ["mistology", "impact"]).unwrap();
    assert!(
        clean_imp.contains("변경된 파일 없음"),
        "clean working tree should report no changed files:\n{clean_imp}"
    );

    // Rename `pkg/sub src/alpha file.py` -> `pkg/sub src/renamed file.py` in commit `c2`
    git(
        git_repo.path(),
        &["mv", "pkg/sub src/alpha file.py", "pkg/sub src/renamed file.py"],
    );
    git(git_repo.path(), &["commit", "-m", "c2"]);

    // `--base HEAD~1` must catch the old side of the rename (`pkg/sub src/alpha file.py`) and surface `pkg/peer.py`!
    let base_imp = mistology::run_cli(
        git_repo.path(),
        ["mistology", "impact", "--base", "HEAD~1"],
    )
    .unwrap();
    assert!(
        base_imp.contains("pkg/peer.py"),
        "--base HEAD~1 across a space-containing git mv must surface coupled pkg/peer.py:\n{base_imp}"
    );

    // Invalid `--base` must return Err
    assert!(mistology::run_cli(git_repo.path(), ["mistology", "impact", "--base", "no-such-ref-999"]).is_err());

    // Untracked new file (`pkg/untracked_entry.py`) on clean HEAD must be picked up by default `mistology impact`
    fs::write(git_repo.path().join("pkg/untracked_entry.py"), "u = 2\n").unwrap();
    let untracked_imp = mistology::run_cli(git_repo.path(), ["mistology", "impact"]).unwrap();
    assert!(
        untracked_imp.contains("pkg/peer.py"),
        "untracked file matching note must surface coupled pkg/peer.py:\n{untracked_imp}"
    );
    fs::remove_file(git_repo.path().join("pkg/untracked_entry.py")).unwrap();

    // Linked git worktree check
    let wt_parent = TempDir::new();
    let wt_dir = wt_parent.path().join("linked-wt");
    git(
        git_repo.path(),
        &["worktree", "add", "-b", "wt-branch", wt_dir.to_str().unwrap()],
    );
    fs::write(wt_dir.join("pkg/peer.py"), "p = 2\n").unwrap();
    let wt_imp = mistology::run_cli(&wt_dir, ["mistology", "impact"]).unwrap();
    assert!(
        wt_imp.contains("pkg/sub src/alpha file.py"),
        "reverse coupling inside a linked git worktree (.git file) must work:\n{wt_imp}"
    );
}

#[test]
fn doctor_detects_repo_internal_file_drift_safely() {
    let repo = TempDir::new();
    let ext = TempDir::new();
    let home = TempDir::new();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join("src/live")).unwrap();
    fs::write(repo.path().join("src/live/Present.rs"), "pub fn ok() {}\n").unwrap();
    fs::write(ext.path().join("secret.rs"), "pub fn secret() {}\n").unwrap();

    // Active note with: 1 present file, 1 missing mixed-case file, 1 missing coupled file,
    // plus TS aliases (`@/...`, `~/...`), extensionless import, and pure coupled symbol (must NOT false-positive!)
    note::new_note(
        repo.path(),
        NewFields {
            problem: "드리프트 정밀도 검증",
            tried: "구 경로 참조",
            evidence: "DriftCheckSig1",
            why: "처음부터 틀림: 경로 이동",
            fix: "새 경로 참조",
            verified: "테스트 통과",
            next_step: "경로 갱신",
            signatures: &svec(&["DriftCheckSig1"]),
            files: &svec(&["src/live/Present.rs", "src/old/MissingMixedCase.rs"]),
            symbols: &svec(&["NONEXISTENT_ENV_KEY_OK"]),
            coupled: &svec(&[
                "src/coupled/GonePeer.rs",
                "@/components/Button",
                "~/lib/env",
                "packages/ui/Button",
                "PURE_COUPLED_SYMBOL",
            ]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();

    // Retired note referencing missing `src/retired/DeadFile.rs` must NOT be flagged as active drift
    let retired_p = note::new_note(
        repo.path(),
        NewFields {
            problem: "은퇴 노트",
            tried: "구 코드",
            evidence: "RetiredDriftSig2",
            why: "처음부터 틀림: 폐기됨",
            fix: "모듈 제거",
            verified: "통과",
            next_step: "무시",
            signatures: &svec(&["RetiredDriftSig2"]),
            files: &svec(&["src/retired/DeadFile.rs"]),
            symbols: &[],
            coupled: &[],
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    let raw_ret = fs::read_to_string(&retired_p)
        .unwrap()
        .replacen("status: active", "status: retired", 1);
    fs::write(&retired_p, raw_ret).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(ext.path(), repo.path().join("src/ext_link")).unwrap();
        note::new_note(
            repo.path(),
            NewFields {
                problem: "외부 심볼릭 링크 방어",
                tried: "외부 참조",
                evidence: "ExtLinkSig3",
                why: "처음부터 틀림: 외부 링크",
                fix: "내부 경로만 사용",
                verified: "통과",
                next_step: "경계 준수",
                signatures: &svec(&["ExtLinkSig3"]),
                files: &svec(&["src/ext_link/missing_secret.rs"]),
                symbols: &[],
                coupled: &[],
                tags: &[],
                force: false,
            },
        )
        .unwrap();
    }

    let report = mistology::run_cli(
        repo.path(),
        ["mistology", "doctor", "--home", home.path().to_str().unwrap()],
    )
    .unwrap();

    // Exact original-case missing repo-internal files are reported
    assert!(
        report.contains("src/old/MissingMixedCase.rs"),
        "must report missing file preserving original case:\n{report}"
    );
    assert!(
        report.contains("src/coupled/GonePeer.rs"),
        "must report missing coupled repo file:\n{report}"
    );
    // Present files, retired notes, aliases, extensionless imports, pure symbols, and external symlinks are NOT flagged!
    for clean_item in [
        "src/live/Present.rs",
        "src/retired/DeadFile.rs",
        "@/components/Button",
        "~/lib/env",
        "packages/ui/Button",
        "PURE_COUPLED_SYMBOL",
        "NONEXISTENT_ENV_KEY_OK",
        "missing_secret.rs",
    ] {
        assert!(
            !report.contains(clean_item),
            "doctor drift check must NOT false-positive or traverse external symlink on {clean_item}:\n{report}"
        );
    }
}

#[test]
fn coherence_engine_edge_cases_boundaries_precision_and_privacy() {
    use std::process::Command;

    let repo = TempDir::new();
    let ext = TempDir::new();
    let home = TempDir::new();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::create_dir_all(repo.path().join("pkg")).unwrap();
    fs::create_dir_all(repo.path().join("scripts")).unwrap();
    fs::write(repo.path().join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    fs::write(repo.path().join("scripts/Makefile"), "all:\n\t@true\n").unwrap();
    fs::write(repo.path().join("src/canon_ok.rs"), "pub fn ok() {}\n").unwrap();
    fs::write(repo.path().join("src/TurnedDir.rs"), "pub fn f() {}\n").unwrap();
    fs::write(repo.path().join("foo.py"), "root_foo = 1\n").unwrap();
    fs::write(ext.path().join("secret.rs"), "pub fn secret() {}\n").unwrap();

    let git = |dir: &Path, args: &[&str]| {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);

    // 1. `canon` boundary guards (`--force` on directory / external symlink, `--coupled` escape, `supersede` guards, long `--forbid` truncation)
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--force",
                "--intent",
                "디렉터리 경로 차단",
                "--contract",
                "규약",
                "--verified",
                "통과",
                "--signature",
                "dir-check-sig",
                "--file",
                "src",
                "--symbol",
                "dir_sym",
            ]
        )
        .is_err(),
        "canon --force on a directory must fail (NotRegularFile)"
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "외부 coupled 차단",
                "--contract",
                "규약",
                "--verified",
                "통과",
                "--signature",
                "ext-coupled-sig",
                "--file",
                "src/canon_ok.rs",
                "--symbol",
                "ok",
                "--coupled",
                "/opt/private/customer-secret/app.py",
            ]
        )
        .is_err(),
        "canon --coupled with external absolute path must fail"
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "상위 coupled 차단",
                "--contract",
                "규약",
                "--verified",
                "통과",
                "--signature",
                "parent-coupled-sig",
                "--file",
                "src/canon_ok.rs",
                "--symbol",
                "ok",
                "--coupled",
                "../escape.py",
            ]
        )
        .is_err(),
        "canon --coupled with parent traversal must fail"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(ext.path(), repo.path().join("src/ext_link")).unwrap();
        assert!(
            mistology::run_cli(
                repo.path(),
                [
                    "mistology",
                    "canon",
                    "--force",
                    "--intent",
                    "심볼릭 링크 우회 차단",
                    "--contract",
                    "규약",
                    "--verified",
                    "통과",
                    "--signature",
                    "symlink-force-sig",
                    "--file",
                    "src/ext_link/planned.rs",
                    "--symbol",
                    "planned_fn",
                ]
            )
            .is_err(),
            "canon --force through external symlink must still fail"
        );
        assert!(
            mistology::run_cli(
                repo.path(),
                [
                    "mistology",
                    "canon",
                    "--intent",
                    "심볼릭 링크 coupled 차단",
                    "--contract",
                    "규약",
                    "--verified",
                    "통과",
                    "--signature",
                    "symlink-coupled-sig",
                    "--file",
                    "src/canon_ok.rs",
                    "--symbol",
                    "ok",
                    "--coupled",
                    "src/ext_link/secret.rs",
                ]
            )
            .is_err(),
            "canon --coupled through external symlink must fail"
        );
    }

    let long_forbid = format!("절대금지패턴_{}", "X".repeat(220));
    let root_canon_rel = mistology::run_cli(
        repo.path(),
        [
            "mistology",
            "canon",
            "--intent",
            "빌드 매니페스트 표준",
            "--contract",
            "Cargo.toml과 scripts/Makefile 동기화",
            "--verified",
            "CI 빌드 통과",
            "--forbid",
            &long_forbid,
            "--signature",
            "cargo_manifest_makefile_sync",
            "--file",
            "Cargo.toml",
            "--file",
            "scripts/Makefile",
            "--symbol",
            "workspace_manifest",
        ],
    )
    .unwrap();
    let root_canon_id = Path::new(root_canon_rel.trim())
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Verify `supersede` on a `canon` note ALSO enforces `validate_canon_file_path` and `validate_canon_coupled_entry`!
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "supersede",
                &root_canon_id,
                "--evidence",
                "ev",
                "--why",
                "바뀌어서 틀려짐: 이유",
                "--fix",
                "새 규약",
                "--verified",
                "통과",
                "--next",
                "점검",
                "--file",
                "src/missing_on_supersede.rs",
            ]
        )
        .is_err(),
        "supersede on canon with nonexistent --file must fail"
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "supersede",
                &root_canon_id,
                "--evidence",
                "ev",
                "--why",
                "바뀌어서 틀려짐: 이유",
                "--fix",
                "새 규약",
                "--verified",
                "통과",
                "--next",
                "점검",
                "--coupled",
                "/opt/private/customer-secret/coupled.py",
            ]
        )
        .is_err(),
        "supersede on canon with external --coupled must fail"
    );

    // Verify `- 금지:` in `search -b` is bounded to <= 120 chars + ellipsis!
    let brief_forbid = mistology::run_cli(
        repo.path(),
        ["mistology", "search", "-b", "Cargo.toml"],
    )
    .unwrap();
    let forbid_line = brief_forbid
        .lines()
        .find(|l| l.trim_start().starts_with("- 금지:"))
        .expect("must have forbid line");
    assert!(
        forbid_line.chars().count() <= 135 && forbid_line.ends_with('…'),
        "long forbid line in brief must be bounded with ellipsis:\n{forbid_line}"
    );

    // 2. `impact` precision: comma-in-filename, exact case, root config.ts vs src/config.ts, self-coupling, deleted `./foo.py` rebase, and privacy
    note::new_note(
        repo.path(),
        NewFields {
            problem: "쉼표 포함 파일명 결합",
            tried: "단독 수정",
            evidence: "CommaFileSig",
            why: "처음부터 틀림: 쉼표 파일 누락",
            fix: "src/target, peer.py 함께 수정",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["CommaFileSig"]),
            files: &svec(&["src/a, b.py"]),
            symbols: &[],
            coupled: &svec(&["src/target, peer.py"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    note::new_note(
        repo.path(),
        NewFields {
            problem: "src/config.ts 전용 결합",
            tried: "단독 수정",
            evidence: "SrcConfigOnlySig",
            why: "처음부터 틀림: 하위 config",
            fix: "src/consumer.ts 확인",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["SrcConfigOnlySig"]),
            files: &svec(&["src/config.ts"]),
            symbols: &[],
            coupled: &svec(&["src/consumer.ts"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    note::new_note(
        repo.path(),
        NewFields {
            problem: "대소문자 구분 파일",
            tried: "단독 수정",
            evidence: "CaseExactSig",
            why: "처음부터 틀림: 소문자 전용",
            fix: "src/lower_peer.py 확인",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["CaseExactSig"]),
            files: &svec(&["src/case_only.py"]),
            symbols: &[],
            coupled: &svec(&["src/lower_peer.py"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    note::new_note(
        repo.path(),
        NewFields {
            problem: "자기 참조 coupled 방어",
            tried: "단독 수정",
            evidence: "SelfCoupledSig",
            why: "처음부터 틀림: 중복 기재",
            fix: "규약 준수",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["SelfCoupledSig"]),
            files: &svec(&["src/self_ref.py"]),
            symbols: &svec(&["SELF_SYM"]),
            coupled: &svec(&["src/self_ref.py"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    note::new_note(
        repo.path(),
        NewFields {
            problem: "삭제된 하위 ./foo.py 리베이스",
            tried: "루트 오인",
            evidence: "PkgFooSig",
            why: "처음부터 틀림: pkg/foo.py 결합",
            fix: "pkg/bar.py 확인",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["PkgFooSig"]),
            files: &svec(&["pkg/foo.py"]),
            symbols: &[],
            coupled: &svec(&["pkg/bar.py"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();
    note::new_note(
        repo.path(),
        NewFields {
            problem: "일반 파일이 디렉터리로 변형됨",
            tried: "방치",
            evidence: "TurnedDirSig",
            why: "처음부터 틀림: 디렉터리 전환",
            fix: "경로 정리",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["TurnedDirSig"]),
            files: &svec(&["src/TurnedDir.rs"]),
            symbols: &[],
            coupled: &[],
            tags: &[],
            force: false,
        },
    )
    .unwrap();

    // Comma-containing filenames work without CSV splitting corruption!
    let comma_imp = mistology::run_cli(repo.path(), ["mistology", "impact", "src/a, b.py"]).unwrap();
    assert!(
        comma_imp.contains("src/target, peer.py"),
        "comma-containing filename must match and surface coupled comma filename:\n{comma_imp}"
    );

    // Root `config.ts` must NOT falsely match `src/config.ts`!
    let root_cfg_imp = mistology::run_cli(repo.path(), ["mistology", "impact", "config.ts"]).unwrap();
    assert!(
        root_cfg_imp.contains("이상 없음") && !root_cfg_imp.contains("src/consumer.ts"),
        "root config.ts must NOT alias-collide onto src/config.ts:\n{root_cfg_imp}"
    );

    // `src/Case_Only.py` must NOT falsely match `src/case_only.py`!
    let case_imp = mistology::run_cli(repo.path(), ["mistology", "impact", "src/Case_Only.py"]).unwrap();
    assert!(
        case_imp.contains("이상 없음") && !case_imp.contains("src/lower_peer.py"),
        "different case path must NOT trigger lowercase note:\n{case_imp}"
    );

    // Self-coupling `files=[src/self_ref.py], coupled=[src/self_ref.py]` must NOT emit `↔` co-modified!
    let self_imp = mistology::run_cli(repo.path(), ["mistology", "impact", "src/self_ref.py"]).unwrap();
    assert!(
        !self_imp.contains("↔"),
        "self-coupled file must NOT emit bogus A ↔ A co-modified row:\n{self_imp}"
    );
    assert!(
        self_imp.contains("SELF_SYM"),
        "self-coupled file with symbol should still surface symbol check:\n{self_imp}"
    );

    // Deleted `pkg/foo.py` while root `foo.py` exists: `mistology impact ./foo.py` inside `pkg/` MUST resolve to `pkg/foo.py`!
    let pkg_dot_imp = mistology::run_cli(
        &repo.path().join("pkg"),
        ["mistology", "impact", "./foo.py"],
    )
    .unwrap();
    assert!(
        pkg_dot_imp.contains("pkg/bar.py"),
        "explicit ./foo.py in pkg/ must lexically rebase to pkg/foo.py even when deleted:\n{pkg_dot_imp}"
    );

    // External path passed to `impact` fails AND never leaks into `.mistology/events.jsonl`!
    assert!(
        mistology::run_cli(
            repo.path(),
            ["mistology", "impact", "/opt/private/customer-secret/app.py"]
        )
        .is_err()
    );
    let events_raw = fs::read_to_string(repo.path().join(".mistology/events.jsonl")).unwrap();
    assert!(
        events_raw.contains("\"outside_repo\""),
        "events.jsonl must record fixed error code outside_repo:\n{events_raw}"
    );
    assert!(
        !events_raw.contains("customer-secret") && !events_raw.contains("/opt/private"),
        "events.jsonl must NEVER leak external host paths from canon/supersede/impact errors:\n{events_raw}"
    );

    // 3. Git `--base HEAD` staged+unstaged cancellation, nested `.mistology` sub-root, and newline filename
    #[cfg(unix)]
    let _ = fs::remove_file(repo.path().join("src/ext_link"));
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "baseline"]);

    // Edit Cargo.toml -> `git add Cargo.toml` -> revert working-tree back to HEAD content -> `--base HEAD` must see 0 changes!
    let orig_cargo = fs::read_to_string(repo.path().join("Cargo.toml")).unwrap();
    fs::write(repo.path().join("Cargo.toml"), "[package]\nname = \"modified\"\n").unwrap();
    git(repo.path(), &["add", "Cargo.toml"]);
    fs::write(repo.path().join("Cargo.toml"), &orig_cargo).unwrap();
    let cancelled_base = mistology::run_cli(
        repo.path(),
        ["mistology", "impact", "--base", "HEAD"],
    )
    .unwrap();
    assert!(
        cancelled_base.contains("변경된 파일 없음"),
        "staged edit reverted in working tree back to HEAD must cancel out under --base HEAD:\n{cancelled_base}"
    );
    git(repo.path(), &["reset", "-q", "HEAD", "--", "Cargo.toml"]);

    // Nested `.mistology` sub-root inside outer Git repo ignores outer changes!
    let sub_root = repo.path().join("subproj");
    fs::create_dir_all(sub_root.join(".mistology/notes")).unwrap();
    fs::create_dir_all(sub_root.join("src")).unwrap();
    fs::write(sub_root.join("src/inner.py"), "i = 1\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "add-subproj"]);
    // Modify outer `foo.py` only -> `mistology impact` inside `subproj/` must see 0 changed files!
    fs::write(repo.path().join("foo.py"), "root_foo = 2\n").unwrap();
    let sub_imp = mistology::run_cli(&sub_root, ["mistology", "impact"]).unwrap();
    assert!(
        sub_imp.contains("변경된 파일 없음"),
        "nested .mistology root must ignore Git changes outside its subtree:\n{sub_imp}"
    );
    git(repo.path(), &["checkout", "--", "foo.py"]);

    #[cfg(unix)]
    {
        let nl_name = "src/line1\nline2.rs";
        git(repo.path(), &["mv", "src/canon_ok.rs", nl_name]);
        let nl_imp = mistology::run_cli(repo.path(), ["mistology", "impact"]).unwrap();
        assert!(
            nl_imp.contains("변경 파일 2개"),
            "NUL-delimited git status -z must cleanly parse rename into newline filename (both old & new paths):\n{nl_imp}"
        );
        git(repo.path(), &["mv", nl_name, "src/canon_ok.rs"]);
    }

    // 4. `doctor` drift on root `Cargo.toml`, extensionless `scripts/Makefile`, and file-turned-directory `src/TurnedDir.rs`
    fs::remove_file(repo.path().join("Cargo.toml")).unwrap();
    fs::remove_file(repo.path().join("scripts/Makefile")).unwrap();
    fs::remove_file(repo.path().join("src/TurnedDir.rs")).unwrap();
    fs::create_dir_all(repo.path().join("src/TurnedDir.rs")).unwrap();

    let doc = mistology::run_cli(
        repo.path(),
        ["mistology", "doctor", "--home", home.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(
        doc.contains("Cargo.toml") && doc.contains("scripts/Makefile"),
        "doctor must detect missing root-level and extensionless canon files:\n{doc}"
    );
    assert!(
        doc.contains("src/TurnedDir.rs (일반 파일 아님)"),
        "doctor must detect file replaced by a directory (NotRegularFile):\n{doc}"
    );
}

#[test]
fn round3_counterexamples_root_symlinks_colon_logs_root_vs_sub_aliases_and_io_preservation() {
    let repo = TempDir::new();
    let ext = TempDir::new();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join("src/lib")).unwrap();
    fs::create_dir_all(repo.path().join("scripts")).unwrap();
    fs::write(repo.path().join("config.ts"), "export const ROOT = 1;\n").unwrap();
    fs::write(repo.path().join("src/config.ts"), "export const SUB = 2;\n").unwrap();
    fs::write(repo.path().join("scripts/root_consumer.ts"), "// root\n").unwrap();
    fs::write(repo.path().join("src/sub_consumer.ts"), "// sub\n").unwrap();
    fs::write(repo.path().join("src/app_entry.ts"), "// app\n").unwrap();
    fs::write(repo.path().join("src/lib/env.ts"), "export const ENV = 1;\n").unwrap();
    fs::write(ext.path().join("secret.rs"), "pub fn secret() {}\n").unwrap();

    // ① Root-level external symlink (`secret.rs`, no slash) + non-normalized `src//...`, `src/./...`, `@/../...` in `--coupled` MUST fail!
    for bad_coupled in ["src//sub_consumer.ts", "src/./sub_consumer.ts", "@/../escape", "."] {
        assert!(
            mistology::run_cli(
                repo.path(),
                [
                    "mistology",
                    "canon",
                    "--intent",
                    "비정규 coupled 차단",
                    "--contract",
                    "규약",
                    "--verified",
                    "통과",
                    "--signature",
                    "nonnorm-coupled-sig",
                    "--file",
                    "config.ts",
                    "--symbol",
                    "ROOT",
                    "--coupled",
                    bad_coupled,
                ]
            )
            .is_err(),
            "non-normalized coupled path {bad_coupled:?} must be rejected"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(ext.path().join("secret.rs"), repo.path().join("secret.rs")).unwrap();
        symlink(ext.path(), repo.path().join("src/ext_dir")).unwrap();
        assert!(
            mistology::run_cli(
                repo.path(),
                [
                    "mistology",
                    "canon",
                    "--intent",
                    "루트 외부 심볼릭 링크 차단",
                    "--contract",
                    "규약",
                    "--verified",
                    "통과",
                    "--signature",
                    "root-symlink-coupled-sig",
                    "--file",
                    "config.ts",
                    "--symbol",
                    "ROOT",
                    "--coupled",
                    "secret.rs",
                ]
            )
            .is_err(),
            "root-level external symlink in --coupled must be rejected"
        );
        assert!(
            mistology::run_cli(
                repo.path(),
                [
                    "mistology",
                    "canon",
                    "--intent",
                    "이중 슬래시 외부 심볼릭 링크 차단",
                    "--contract",
                    "규약",
                    "--verified",
                    "통과",
                    "--signature",
                    "double-slash-symlink-sig",
                    "--file",
                    "config.ts",
                    "--symbol",
                    "ROOT",
                    "--coupled",
                    "src//ext_dir/secret.rs",
                ]
            )
            .is_err(),
            "double-slash relative external symlink in --coupled must be rejected"
        );
    }

    // ④ Coexisting root `config.ts` canon AND `src/config.ts` note in the SAME corpus!
    mistology::run_cli(
        repo.path(),
        [
            "mistology",
            "canon",
            "--intent",
            "루트 설정 규약",
            "--contract",
            "scripts/root_consumer.ts와 동기화",
            "--verified",
            "통과",
            "--signature",
            "root_config_canon_sig",
            "--file",
            "config.ts",
            "--symbol",
            "ROOT",
            "--coupled",
            "scripts/root_consumer.ts",
        ],
    )
    .unwrap();
    note::new_note(
        repo.path(),
        NewFields {
            problem: "하위 src/config.ts 전용 결합",
            tried: "단독 수정",
            evidence: "SubConfigOnlySig",
            why: "처음부터 틀림: 하위 전용",
            fix: "src/sub_consumer.ts 확인",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["SubConfigOnlySig"]),
            files: &svec(&["src/config.ts"]),
            symbols: &[],
            coupled: &svec(&["src/sub_consumer.ts"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();

    let root_hit = mistology::run_cli(repo.path(), ["mistology", "impact", "config.ts"]).unwrap();
    assert!(
        root_hit.contains("scripts/root_consumer.ts") && !root_hit.contains("src/sub_consumer.ts"),
        "changing root config.ts must trigger ONLY root config.ts canon (not be hijacked by src/config.ts):\n{root_hit}"
    );
    let sub_hit = mistology::run_cli(repo.path(), ["mistology", "impact", "src/config.ts"]).unwrap();
    assert!(
        sub_hit.contains("src/sub_consumer.ts") && !sub_hit.contains("scripts/root_consumer.ts"),
        "changing src/config.ts must trigger ONLY src/config.ts note:\n{sub_hit}"
    );

    // ⑤ `coupled = ["@/lib/env"]` reverse & forward resolution even when NO note lists `src/lib/env.ts` in `files`!
    note::new_note(
        repo.path(),
        NewFields {
            problem: "별칭 coupled 역방향 점검",
            tried: "env 단독 변경",
            evidence: "AliasEnvSig",
            why: "처음부터 틀림: app_entry 연동 누락",
            fix: "src/app_entry.ts 함께 확인",
            verified: "통과",
            next_step: "확인",
            signatures: &svec(&["AliasEnvSig"]),
            files: &svec(&["src/app_entry.ts"]),
            symbols: &[],
            coupled: &svec(&["@/lib/env"]),
            tags: &[],
            force: false,
        },
    )
    .unwrap();

    let rev_alias = mistology::run_cli(repo.path(), ["mistology", "impact", "src/lib/env.ts"]).unwrap();
    assert!(
        rev_alias.contains("src/app_entry.ts"),
        "changing src/lib/env.ts must reverse-match coupled @/lib/env onto src/app_entry.ts:\n{rev_alias}"
    );
    // Even if `src/lib/env.ts` was deleted from disk, `changed_rel_paths` alone resolves `@/lib/env` -> `src/lib/env.ts`!
    fs::remove_file(repo.path().join("src/lib/env.ts")).unwrap();
    let rev_deleted_alias = mistology::run_cli(repo.path(), ["mistology", "impact", "src/lib/env.ts"]).unwrap();
    assert!(
        rev_deleted_alias.contains("src/app_entry.ts"),
        "deleted src/lib/env.ts in changed_rel_paths must still reverse-match coupled @/lib/env:\n{rev_deleted_alias}"
    );

    // ② External paths containing `: ` (`/opt/private/client: report.rs`) on rejected `canon` / `supersede` / `impact` MUST NEVER leak into `events.jsonl`!
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "canon",
                "--intent",
                "콜론 외부 경로 차단",
                "--contract",
                "규약",
                "--verified",
                "통과",
                "--signature",
                "colon-ext-sig",
                "--file",
                "config.ts",
                "--symbol",
                "ROOT",
                "--coupled",
                "/opt/private/client: report.rs",
            ]
        )
        .is_err()
    );
    assert!(
        mistology::run_cli(
            repo.path(),
            [
                "mistology",
                "supersede",
                "/opt/private/client: secret-note.md",
                "--evidence",
                "e",
                "--why",
                "바뀌어서 틀려짐: w",
                "--fix",
                "f",
                "--verified",
                "v",
                "--next",
                "n",
            ]
        )
        .is_err()
    );
    let ev = fs::read_to_string(repo.path().join(".mistology/events.jsonl")).unwrap();
    for secret_frag in ["/opt/private", "client:", "report.rs", "secret-note.md"] {
        assert!(
            !ev.contains(secret_frag),
            "events.jsonl must NEVER leak {secret_frag:?}:\n{ev}"
        );
    }

    // ③ & ⑥ Unix permission-error checks:
    // - `impact` when `.mistology` is 0o000 MUST return Err (NOT false "이상 없음")!
    // - `search` when `.mistology/notes` is 0o000 MUST NOT delete a healthy `index.db`!
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Warm index.db and plant sentinel in `cards.problem`
        let _ = mistology::run_cli(repo.path(), ["mistology", "search", "SubConfigOnlySig"]).unwrap();
        let db_path = repo.path().join(".mistology/index.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("UPDATE cards SET problem = 'sentinel_intact'", []).unwrap();
        }

        // ⑥ Unreadable `.mistology/notes` (0o000) during `search` must NOT delete `index.db`!
        let notes_d = repo.path().join(".mistology/notes");
        fs::set_permissions(&notes_d, fs::Permissions::from_mode(0o000)).unwrap();
        let _ = mistology::index::search(repo.path(), "SubConfigOnlySig", 3);
        fs::set_permissions(&notes_d, fs::Permissions::from_mode(0o755)).unwrap();
        let prob_after: String = rusqlite::Connection::open(&db_path)
            .unwrap()
            .query_row("SELECT problem FROM cards LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            prob_after, "sentinel_intact",
            "notes directory read_dir error must NEVER trigger corrupt-DB deletion of index.db"
        );

        // ③ Unreadable `.mistology` (0o000) during `impact` must return Err instead of `exists() == false`!
        let mist_d = repo.path().join(".mistology");
        fs::set_permissions(&mist_d, fs::Permissions::from_mode(0o000)).unwrap();
        let imp_perm_res = mistology::run_cli(repo.path(), ["mistology", "impact", "config.ts"]);
        fs::set_permissions(&mist_d, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            imp_perm_res.is_err(),
            "unreadable .mistology during impact must propagate notes_io_error instead of reporting all clear"
        );

        // ⑦ Subdirectory external symlinks (`pkg/secret.rs` AND extensionless `pkg/Makefile`, when root `secret.rs` / `Makefile` do NOT exist!):
        // Running `canon` from `pkg/` with `--coupled ./secret.rs`, `--coupled secret.rs`, `--coupled Makefile`, `--force --file ./secret.rs`, or `--force --file Makefile` MUST all fail!
        let _ = fs::remove_file(repo.path().join("secret.rs"));
        let pkg_dir = repo.path().join("pkg");
        fs::create_dir_all(&pkg_dir).unwrap();
        std::os::unix::fs::symlink(ext.path().join("secret.rs"), pkg_dir.join("secret.rs")).unwrap();
        std::os::unix::fs::symlink(ext.path().join("secret.rs"), pkg_dir.join("Makefile")).unwrap();
        for sub_coupled in ["./secret.rs", "secret.rs", "Makefile"] {
            assert!(
                mistology::run_cli(
                    &pkg_dir,
                    [
                        "mistology",
                        "canon",
                        "--intent",
                        "하위 폴더 외부 symlink 차단",
                        "--contract",
                        "규약",
                        "--verified",
                        "통과",
                        "--signature",
                        "pkg-sub-symlink-coupled-sig",
                        "--file",
                        "../config.ts",
                        "--symbol",
                        "ROOT",
                        "--coupled",
                        sub_coupled,
                    ]
                )
                .is_err(),
                "canon from pkg/ with external symlink {sub_coupled:?} must NOT fall back to missing root path"
            );
        }
        for sub_file in ["./secret.rs", "Makefile"] {
            assert!(
                mistology::run_cli(
                    &pkg_dir,
                    [
                        "mistology",
                        "canon",
                        "--force",
                        "--intent",
                        "하위 폴더 외부 symlink --force 차단",
                        "--contract",
                        "규약",
                        "--verified",
                        "통과",
                        "--signature",
                        "pkg-sub-symlink-file-sig",
                        "--file",
                        sub_file,
                        "--symbol",
                        "ROOT",
                    ]
                )
                .is_err(),
                "canon --force --file {sub_file:?} from pkg/ must reject external symlink in pkg/"
            );
        }
    }

    // ⑧ Oversized (>55 KiB) `LIKE`-branch entity queries (`src/<no-ext>`, `@/<alias>`, `<bare>.ts`)
    // genuinely hit `unique_matching_entity` -> SQLite `ErrorCode::Unknown` (`LIKE or GLOB pattern too complex`)
    // on a healthy DB — MUST NOT delete `index.db`!
    let _ = mistology::run_cli(repo.path(), ["mistology", "search", "SubConfigOnlySig"]).unwrap();
    let db_path = repo.path().join(".mistology/index.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        // Sanity-check that SQLite itself raises `ErrorCode::Unknown` on a 60 KiB LIKE pattern against `edges`:
        let like_err = conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE entity LIKE ?1",
                [format!("%src/{}%", "a".repeat(60_000))],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_err();
        assert!(
            matches!(
                like_err,
                rusqlite::Error::SqliteFailure(err, _) if err.code == rusqlite::ErrorCode::Unknown
            ),
            "expected SQLite LIKE limit to surface as ErrorCode::Unknown, got: {like_err:?}"
        );
        conn.execute("UPDATE cards SET problem = 'sentinel_after_like_limit'", []).unwrap();
    }
    for huge_like_query in [
        format!("@/{}", "a".repeat(60_000)),
        format!("src/{}", "a".repeat(60_000)),
        format!("{}.ts", "a".repeat(60_000)),
    ] {
        let _ = mistology::index::search(repo.path(), &huge_like_query, 3);
    }
    let prob_after_like: String = rusqlite::Connection::open(&db_path)
        .unwrap()
        .query_row("SELECT problem FROM cards LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        prob_after_like, "sentinel_after_like_limit",
        "SQLITE_ERROR (ErrorCode::Unknown, e.g. oversized LIKE pattern) must NEVER delete healthy index.db"
    );
}
