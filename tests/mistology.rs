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

    // 5) Old schema version (user_version = 0) smoothly upgrades to v3 and rebuilds
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
    assert_eq!(ver, 3);
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
            ro_report.contains("- index.db: ok | schema=v3"),
            "doctor must inspect persistent WAL index.db via immutable=1 even inside 0555 RO dir:\n{ro_report}"
        );
    }
}
