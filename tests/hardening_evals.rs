use mistology::note::{self, NewFields};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Instant;
use unicode_normalization::UnicodeNormalization;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mistology-hardening-{tag}-{}-{}",
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

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("evals").join(name)
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        if name.starts_with("index.db")
            || name.starts_with("events.jsonl")
            || name.ends_with(".tmp")
            || name == "__pycache__"
            || name == "target"
            || name == "node_modules"
        {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name_os);
        if src_path.is_dir() {
            copy_tree(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}

fn clone_fixture(name: &str, tag: &str) -> TempDir {
    let tmp = TempDir::new(&format!("{name}-{tag}"));
    copy_tree(&fixture_root(name), tmp.path());
    fs::create_dir_all(tmp.path().join(".git")).unwrap();

    let notes_dir = tmp.path().join(".mistology/notes");
    let mut md_files: Vec<PathBuf> = fs::read_dir(&notes_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    md_files.sort();
    let expected_count = match name {
        "shopcore" => 12,
        "webmono" | "rustkv" => 6,
        _ => panic!("unknown fixture {name}"),
    };
    assert_eq!(md_files.len(), expected_count, "unexpected note count in {name}");
    for md in md_files {
        let raw = fs::read_to_string(&md).unwrap();
        note::parse(&raw)
            .unwrap_or_else(|e| panic!("fixture {name} note {:?} failed note::parse: {e}", md));
    }
    tmp
}

#[derive(Debug, Clone)]
struct ParsedCard {
    file_stem: String,
    header: String,
    via: Option<String>,
}

fn parse_cards(cli_out: &str) -> Vec<ParsedCard> {
    cli_out
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("# ")?;
            let (path_part, bracket_part) = rest.split_once(" [")?;
            let fname = path_part.rsplit(['/', '\\']).next()?;
            let stem = fname.strip_suffix(".md")?.to_string();
            let via = bracket_part
                .split_once("| 연결: ")
                .and_then(|(_, after)| after.strip_suffix(" 경유]"))
                .map(str::to_string);
            Some(ParsedCard {
                file_stem: stem,
                header: line.to_string(),
                via,
            })
        })
        .collect()
}

fn short_id(id: &str) -> &str {
    id.rsplit('-').next().unwrap_or(id)
}

fn fmt_cards(cards: &[ParsedCard]) -> String {
    if cards.is_empty() {
        return "[]".to_string();
    }
    let items: Vec<String> = cards
        .iter()
        .map(|c| match &c.via {
            Some(v) => format!("-{}(via:{v})", short_id(&c.file_stem)),
            None => format!("-{}", short_id(&c.file_stem)),
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn has_card(cards: &[ParsedCard], suffix: &str) -> bool {
    cards.iter().any(|c| c.file_stem.ends_with(suffix))
}

fn top1_is(cards: &[ParsedCard], suffix: &str) -> bool {
    cards.first().is_some_and(|c| c.file_stem.ends_with(suffix))
}

fn copy_notes_retagged(src_notes: &Path, dst_notes: &Path, date_prefix: &str) {
    let mut entries: Vec<PathBuf> = fs::read_dir(src_notes)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    entries.sort();
    for (i, src) in entries.into_iter().enumerate() {
        let new_id = format!("{date_prefix}-{:03}", i + 1);
        let raw = fs::read_to_string(&src).unwrap();
        let (mut meta, body) = note::parse(&raw).unwrap();
        meta.id = new_id.clone();
        fs::write(dst_notes.join(format!("{new_id}.md")), note::render(&meta, &body)).unwrap();
    }
}

fn populate_polyglot_scale_notes(root: &Path, count: usize) {
    let domains = [
        ("ts-router", "apps/web/src/routes/guard.ts", "enforceRouteGuard", "packages/auth/src/claims.ts", "RouteGuardClaimError"),
        ("ts-cache", "apps/web/src/cache/tag.ts", "revalidateOrderTag", "packages/db/src/queries.ts", "StaleCacheTagError"),
        ("ts-form", "apps/admin/src/forms/refund.tsx", "RefundForm", "packages/ui/src/Modal.tsx", "RefundFormSchemaError"),
        ("rs-wal", "crates/wal/src/reader.rs", "SegmentReader::next_frame", "crates/wal/src/header.rs", "WalFrameTruncatedError"),
        ("rs-sstable", "crates/engine/src/sstable.rs", "BlockBuilder::finish", "crates/engine/src/bloom.rs", "BloomFilterFalseNegError"),
        ("rs-manifest", "crates/engine/src/manifest.rs", "Manifest::apply_edit", "crates/cli/src/admin.rs", "ManifestEpochRollbackError"),
        ("py-ledger", "src/ledger/journal.py", "postDoubleEntry", "src/ledger/trial_balance.py", "UnbalancedJournalError"),
        ("py-queue", "src/workers/outbox.py", "drainOutboxBatch", "src/orders/events.py", "OutboxLeaseExpiredError"),
    ];
    for idx in 0..count {
        let (tag, file_base, sym_base, coupled_base, err_cls) = domains[idx % domains.len()];
        let shard = idx / domains.len();
        let (stem, ext) = file_base.rsplit_once('.').unwrap();
        let (c_stem, c_ext) = coupled_base.rsplit_once('.').unwrap();
        let file = format!("{stem}_{shard}.{ext}");
        let coupled = format!("{c_stem}_{shard}.{c_ext}");
        let symbol = format!("{sym_base}_{shard}");
        let sig = format!("{err_cls}: invariant violated shard={shard} idx={idx}");
        let problem = format!("{tag} 샤드 {shard}(케이스 {idx}) 경계 조건 동기화 불일치");
        let tried = format!("{file}의 {symbol}만 단독 수정하고 결합된 {coupled} 규약은 생략함");
        let why = format!("처음부터 틀림: {file} 출력과 {coupled} 검증기가 버전 불변식을 공유함");
        let fix = format!("{file}의 {symbol}와 {coupled} 검증기를 함께 갱신");
        let verified = format!("polyglot suite {tag}_{shard}_{idx} 통과");
        let next_step = format!("{file} 변경 시 결합된 {coupled} 테스트를 함께 실행");
        note::new_note(
            root,
            NewFields {
                problem: &problem,
                tried: &tried,
                evidence: &sig,
                why: &why,
                fix: &fix,
                verified: &verified,
                next_step: &next_step,
                signatures: std::slice::from_ref(&sig),
                files: std::slice::from_ref(&file),
                symbols: std::slice::from_ref(&symbol),
                coupled: std::slice::from_ref(&coupled),
                tags: &[tag.to_string()],
                force: true,
            },
        )
        .unwrap();
    }
}

struct CaseOutcome {
    category: usize,
    name: &'static str,
    passed: bool,
    detail: String,
}

#[test]
fn mistology_product_hardening_eval_suite() {
    let webmono = clone_fixture("webmono", "ro");
    let rustkv = clone_fixture("rustkv", "ro");
    let shopcore = clone_fixture("shopcore", "ro");

    let mut outcomes: Vec<CaseOutcome> = Vec::with_capacity(24);

    // =========================================================================
    // Category 1: Compiler Stacktraces & Dirty Path Extraction (4 cases)
    // =========================================================================
    {
        // 1.1 rustc_line_col_path
        let q1 = "파일 점검 --> crates/engine/src/memtable.rs:73:18";
        let out1 = mistology::run_cli(rustkv.path(), ["mistology", "search", q1]).unwrap_or_default();
        let cards1 = parse_cards(&out1);
        let passed1 = top1_is(&cards1, "-001");
        outcomes.push(CaseOutcome {
            category: 1,
            name: "1.1 rustc_line_col_path",
            passed: passed1,
            detail: format!("query={q1:?} expected_top1=-001 got={}", fmt_cards(&cards1)),
        });

        // 1.2 tsc_paren_line_col
        let q2 = "파일 수정 apps/web/src/app/checkout/page.tsx(42,15): error";
        let out2 = mistology::run_cli(webmono.path(), ["mistology", "search", q2]).unwrap_or_default();
        let cards2 = parse_cards(&out2);
        let passed2 = top1_is(&cards2, "-004") && !has_card(&cards2, "-001");
        outcomes.push(CaseOutcome {
            category: 1,
            name: "1.2 tsc_paren_line_col",
            passed: passed2,
            detail: format!("query={q2:?} expected=[-004 top1, no -001 clutter] got={}", fmt_cards(&cards2)),
        });

        // 1.3 python_abs_traceback
        let abs = shopcore.path().join("src/orders/state.py");
        let q3 = format!("File \"{}\", line 41", abs.display());
        let out3 = mistology::run_cli(shopcore.path(), ["mistology", "search", &q3]).unwrap_or_default();
        let cards3 = parse_cards(&out3);
        let clutter3 = cards3
            .iter()
            .filter(|c| c.file_stem.ends_with("-007") || c.file_stem.ends_with("-008"))
            .count();
        let passed3 = top1_is(&cards3, "-005") && has_card(&cards3, "-006") && clutter3 == 0;
        outcomes.push(CaseOutcome {
            category: 1,
            name: "1.3 python_abs_traceback",
            passed: passed3,
            detail: format!("query={q3:?} expected=[-005 & -006, clutter=0] got={} clutter={clutter3}", fmt_cards(&cards3)),
        });

        // 1.4 git_diff_ab_prefix
        let q4 = "파일 변경 a/packages/db/src/schema.ts b/packages/db/src/schema.ts";
        let out4 = mistology::run_cli(webmono.path(), ["mistology", "search", q4]).unwrap_or_default();
        let cards4 = parse_cards(&out4);
        let clutter4 = cards4
            .iter()
            .filter(|c| c.file_stem.ends_with("-001") || c.file_stem.ends_with("-002"))
            .count();
        let passed4 = top1_is(&cards4, "-006") && clutter4 == 0;
        outcomes.push(CaseOutcome {
            category: 1,
            name: "1.4 git_diff_ab_prefix",
            passed: passed4,
            detail: format!("query={q4:?} expected=[-006, clutter=0] got={} clutter={clutter4}", fmt_cards(&cards4)),
        });
    }

    // =========================================================================
    // Category 2: Monorepo Subdirectory Isolation & TS Aliases (5 cases)
    // =========================================================================
    {
        // 2.1 monorepo_cwd_web_search
        let q_mw = "파일 점검 src/middleware.ts";
        let out_web = mistology::run_cli(&webmono.path().join("apps/web"), ["mistology", "search", q_mw])
            .unwrap_or_default();
        let cards_web = parse_cards(&out_web);
        let passed_web = top1_is(&cards_web, "-001") && !top1_is(&cards_web, "-003");
        outcomes.push(CaseOutcome {
            category: 2,
            name: "2.1 monorepo_cwd_web_search",
            passed: passed_web,
            detail: format!("cwd=apps/web query={q_mw:?} expected_top1=-001 got={}", fmt_cards(&cards_web)),
        });

        // 2.2 monorepo_cwd_admin_search
        let out_adm = mistology::run_cli(&webmono.path().join("apps/admin"), ["mistology", "search", q_mw])
            .unwrap_or_default();
        let cards_adm = parse_cards(&out_adm);
        let passed_adm = top1_is(&cards_adm, "-003");
        outcomes.push(CaseOutcome {
            category: 2,
            name: "2.2 monorepo_cwd_admin_search",
            passed: passed_adm,
            detail: format!("cwd=apps/admin query={q_mw:?} expected_top1=-003 got={}", fmt_cards(&cards_adm)),
        });

        // 2.3 monorepo_cwd_new_rebases_file
        let wm_new = clone_fixture("webmono", "cwd-new");
        let res_new = mistology::run_cli(
            &wm_new.path().join("apps/web"),
            [
                "mistology",
                "new",
                "--problem",
                "Edge 미들웨어 공개 경로 리다이렉트 루프",
                "--tried",
                "모든 요청에 인증 리다이렉트 일괄 적용",
                "--evidence",
                "RedirectLoopError: /login redirected to /login",
                "--why",
                "처음부터 틀림: 로그인 경로 예외 조건을 빠뜨림",
                "--fix",
                "공개 경로(/login, /_next) 예외 분기 추가",
                "--verified",
                "pnpm --filter web test:edge 통과",
                "--next",
                "미들웨어 리다이렉트 수정 시 공개 경로 예외 목록 확인",
                "--signature",
                "RedirectLoopError: /login redirected to /login",
                "--file",
                "src/middleware.ts",
                "--symbol",
                "middleware",
            ],
        );
        let (passed_new, detail_new) = match res_new {
            Ok(rel_out) => {
                let note_path = wm_new.path().join(rel_out.trim());
                match fs::read_to_string(&note_path).ok().and_then(|r| note::parse(&r).ok()) {
                    Some((meta, _)) => {
                        let ok = meta.files == vec!["apps/web/src/middleware.ts".to_string()];
                        (ok, format!("meta.files={:?} expected=[\"apps/web/src/middleware.ts\"]", meta.files))
                    }
                    None => (false, format!("failed to parse created note at {rel_out:?}")),
                }
            }
            Err(e) => (false, format!("run_cli new error: {e}")),
        };
        outcomes.push(CaseOutcome {
            category: 2,
            name: "2.3 monorepo_cwd_new_rebases_file",
            passed: passed_new,
            detail: detail_new,
        });

        // 2.4 ts_at_alias_1hop_coupling
        let q_alias = "파일 점검 apps/web/src/app/checkout/page.tsx";
        let out_alias = mistology::run_cli(webmono.path(), ["mistology", "search", q_alias]).unwrap_or_default();
        let cards_alias = parse_cards(&out_alias);
        let passed_alias = top1_is(&cards_alias, "-004") && has_card(&cards_alias, "-005");
        outcomes.push(CaseOutcome {
            category: 2,
            name: "2.4 ts_at_alias_1hop_coupling",
            passed: passed_alias,
            detail: format!("query={q_alias:?} expected=[-004 & -005] got={}", fmt_cards(&cards_alias)),
        });

        // 2.5 extensionless_ts_import_1hop_and_clean_ui_abstention
        let q_ext = "파일 점검 apps/web/src/middleware.ts";
        let out_ext = mistology::run_cli(webmono.path(), ["mistology", "search", q_ext]).unwrap_or_default();
        let cards_ext = parse_cards(&out_ext);
        let q_ui = "버튼 스타일 수정 packages/ui/src/Button.tsx";
        let out_ui = mistology::run_cli(webmono.path(), ["mistology", "search", q_ui]).unwrap_or_default();
        let cards_ui = parse_cards(&out_ui);
        let passed_2_5 = top1_is(&cards_ext, "-001")
            && has_card(&cards_ext, "-002")
            && out_ui == "관련 오답노트 없음\n";
        outcomes.push(CaseOutcome {
            category: 2,
            name: "2.5 extensionless_ts_import_1hop_and_clean_ui_abstention",
            passed: passed_2_5,
            detail: format!(
                "middleware_got={} (expected [-001 & -002]) button_ui_got={}",
                fmt_cards(&cards_ext),
                fmt_cards(&cards_ui)
            ),
        });
    }

    // =========================================================================
    // Category 3: Cross-Platform Windows Paths & Unicode Normalization (4 cases)
    // =========================================================================
    {
        // 3.1 windows_backslash_query_with_line
        let q_win = r"파일 점검 .\crates\wal\src\segment.rs:19";
        let out_win = mistology::run_cli(rustkv.path(), ["mistology", "search", q_win]).unwrap_or_default();
        let cards_win = parse_cards(&out_win);
        let no_backslash_hdr = cards_win.iter().all(|c| !c.header.contains('\\'));
        // When .\crates\wal\src\segment.rs:19 normalizes to crates/wal/src/segment.rs, -002 gets the -18 file edge bonus (outranking -001 and cutting off sibling -005)
        let passed_3_1 = top1_is(&cards_win, "-002") && !has_card(&cards_win, "-005") && no_backslash_hdr;
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.1 windows_backslash_query_with_line",
            passed: passed_3_1,
            detail: format!("query={q_win:?} expected=[-002 top1, no -005] got={}", fmt_cards(&cards_win)),
        });

        // 3.2 windows_backslash_new_normalizes_slashes
        let sc_win = clone_fixture("shopcore", "win-new");
        let res_win_new = mistology::run_cli(
            sc_win.path(),
            [
                "mistology",
                "new",
                "--problem",
                "Windows 개발 환경에서 웹훅 재시도 백오프 누락",
                "--tried",
                "실패 즉시 재호출",
                "--evidence",
                "WebhookBackoffError: retry storm detected",
                "--why",
                "처음부터 틀림: 지수 백오프 없이 즉시 재전송함",
                "--fix",
                "재시도 간격에 지수 백오프와 지터 추가",
                "--verified",
                "python3 -m unittest 통과",
                "--next",
                "웹훅 워커 수정 시 주문 상태 전이 멱등성도 확인",
                "--signature",
                "WebhookBackoffError: retry storm detected",
                "--file",
                r".\src\workers\webhook.py",
                "--coupled",
                r"src\orders\state.py",
                "--symbol",
                "verifyWebhookSig",
            ],
        );
        let (passed_3_2, detail_3_2) = match res_win_new {
            Ok(ret_path) => {
                let clean_ret = !ret_path.contains('\\') && ret_path.trim().starts_with(".mistology/notes/");
                let note_path = sc_win.path().join(ret_path.trim());
                match fs::read_to_string(&note_path).ok().and_then(|r| note::parse(&r).ok()) {
                    Some((meta, _)) => {
                        let ok = clean_ret
                            && meta.files == vec!["src/workers/webhook.py".to_string()]
                            && meta.coupled == vec!["src/orders/state.py".to_string()];
                        (
                            ok,
                            format!(
                                "ret={:?} files={:?} coupled={:?}",
                                ret_path.trim(),
                                meta.files,
                                meta.coupled
                            ),
                        )
                    }
                    None => (false, format!("failed to parse created note {ret_path:?}")),
                }
            }
            Err(e) => (false, format!("run_cli new error: {e}")),
        };
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.2 windows_backslash_new_normalizes_slashes",
            passed: passed_3_2,
            detail: detail_3_2,
        });

        // 3.3 windows_backslash_coupled_legacy_note
        let rk_leg = clone_fixture("rustkv", "win-legacy");
        let legacy_note = concat!(
            "---\n",
            "id: 2026-11-11-007\n",
            "current: v2\n",
            "status: active\n",
            "tags:\n",
            "  - \"snapshot\"\n",
            "files:\n",
            "  - \"crates/cli/src/snapshot_import.rs\"\n",
            "symbols:\n",
            "  - \"import_snapshot\"\n",
            "coupled:\n",
            "  - \"crates\\\\wal\\\\src\\\\segment.rs\"\n",
            "signatures:\n",
            "  - \"SnapshotImportAlignError: frame boundary unaligned\"\n",
            "---\n",
            "## 문제 상황\n",
            "외부 스냅샷 반입 시 세그먼트 정렬 경계 불일치 오류 발생\n\n",
            "## v1 (오답) 2026-11-11\n",
            "- 한 것: 원본 바이트 스트림을 패딩 없이 그대로 복사\n",
            "- 틀린 증거: SnapshotImportAlignError: frame boundary unaligned\n",
            "- 왜 틀렸나: 처음부터 틀림: 블록 경계 정렬 조건을 맞추지 않음\n\n",
            "## v2 (현재 정답) 2026-11-11\n",
            "- 한 것: 반입 시 4096 바이트 경계로 정렬 후 기록\n",
            "- 확인 근거: snapshot_import 통합 테스트 통과\n",
            "- 다음에는: 스냅샷 반입 수정 시 세그먼트 포맷 규약 확인\n",
        );
        fs::write(
            rk_leg.path().join(".mistology/notes/2026-11-11-007.md"),
            legacy_note,
        )
        .unwrap();
        let out_leg = mistology::run_cli(
            rk_leg.path(),
            ["mistology", "search", "파일 점검 crates/cli/src/snapshot_import.rs"],
        )
        .unwrap_or_default();
        let cards_leg = parse_cards(&out_leg);
        let passed_3_3 = top1_is(&cards_leg, "-007") && has_card(&cards_leg, "-002");
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.3 windows_backslash_coupled_legacy_note",
            passed: passed_3_3,
            detail: format!("expected=[-007 & -002] got={}", fmt_cards(&cards_leg)),
        });

        // 3.4 crlf_and_nfd_korean_roundtrip
        let nfd_dir = TempDir::new("crlf-nfd");
        fs::create_dir_all(nfd_dir.path().join(".git")).unwrap();
        let notes_d = note::notes_dir(nfd_dir.path()).unwrap();
        let lf_nfc = concat!(
            "---\n",
            "id: 2026-11-11-001\n",
            "current: v2\n",
            "status: active\n",
            "tags:\n",
            "  - \"webhook\"\n",
            "files:\n",
            "  - \"src/webhook/handler.ts\"\n",
            "symbols:\n",
            "  - \"handleWebhook\"\n",
            "coupled:\n",
            "signatures:\n",
            "  - \"WebhookMacError: invalid digest\"\n",
            "---\n",
            "## 문제 상황\n",
            "웹훅 서명 검증 시 본문 개행 정규화로 해시 불일치 발생\n\n",
            "## v1 (오답) 2026-11-11\n",
            "- 한 것: 파싱된 문자열로 웹훅 HMAC 계산\n",
            "- 틀린 증거: WebhookMacError: invalid digest\n",
            "- 왜 틀렸나: 처음부터 틀림: 원본 바이트 배열로 계산해야 함\n\n",
            "## v2 (현재 정답) 2026-11-11\n",
            "- 한 것: 원본 raw body 바이트로 웹훅 서명 검증\n",
            "- 확인 근거: 웹훅 단위 테스트 통과\n",
            "- 다음에는: 웹훅 핸들러는 항상 raw body 버퍼를 유지한다\n",
        );
        let crlf_nfd: String = lf_nfc.nfd().collect::<String>().replace('\n', "\r\n");
        fs::write(notes_d.join("2026-11-11-001.md"), crlf_nfd.as_bytes()).unwrap();
        let parse_ok = note::parse(&crlf_nfd).is_ok();
        let nfc_query: String = "웹훅".nfc().collect();
        let out_nfd = mistology::run_cli(nfd_dir.path(), ["mistology", "search", &nfc_query]).unwrap_or_default();
        let cards_nfd = parse_cards(&out_nfd);
        let passed_3_4 = parse_ok && top1_is(&cards_nfd, "-001");
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.4 crlf_and_nfd_korean_roundtrip",
            passed: passed_3_4,
            detail: format!("parse_ok={parse_ok} got={}", fmt_cards(&cards_nfd)),
        });
    }

    // =========================================================================
    // Category 4: Rust `::` Scoped Symbols & `snake_case` <-> `camelCase` (4 cases)
    // =========================================================================
    {
        // 4.1 rust_scoped_symbol_1hop
        let q_4_1 = "파일 점검 crates/engine/src/memtable.rs";
        let out_4_1 = mistology::run_cli(rustkv.path(), ["mistology", "search", q_4_1]).unwrap_or_default();
        let cards_4_1 = parse_cards(&out_4_1);
        let passed_4_1 = top1_is(&cards_4_1, "-001") && has_card(&cards_4_1, "-002");
        outcomes.push(CaseOutcome {
            category: 4,
            name: "4.1 rust_scoped_symbol_1hop",
            passed: passed_4_1,
            detail: format!("query={q_4_1:?} expected=[-001 & -002] got={}", fmt_cards(&cards_4_1)),
        });

        // 4.2 rust_qualified_call_query
        let q_4_2 = "WalSegment::sync_data(&mut seg) 호출 점검";
        let out_4_2 = mistology::run_cli(rustkv.path(), ["mistology", "search", q_4_2]).unwrap_or_default();
        let cards_4_2 = parse_cards(&out_4_2);
        let passed_4_2 = has_card(&cards_4_2, "-002") && has_card(&cards_4_2, "-001");
        outcomes.push(CaseOutcome {
            category: 4,
            name: "4.2 rust_qualified_call_query",
            passed: passed_4_2,
            detail: format!("query={q_4_2:?} expected=[-002 & -001] got={}", fmt_cards(&cards_4_2)),
        });

        // 4.3 camel_query_hits_snake_symbol
        let q_4_3 = "maxSegmentBytes zero check";
        let out_4_3 = mistology::run_cli(rustkv.path(), ["mistology", "search", q_4_3]).unwrap_or_default();
        let cards_4_3 = parse_cards(&out_4_3);
        let passed_4_3 = top1_is(&cards_4_3, "-004");
        outcomes.push(CaseOutcome {
            category: 4,
            name: "4.3 camel_query_hits_snake_symbol",
            passed: passed_4_3,
            detail: format!("query={q_4_3:?} expected_top1=-004 got={}", fmt_cards(&cards_4_3)),
        });

        // 4.4 snake_query_hits_camel_symbol
        let q_4_4 = "verify_frame_checksum trailer";
        let out_4_4 = mistology::run_cli(rustkv.path(), ["mistology", "search", q_4_4]).unwrap_or_default();
        let cards_4_4 = parse_cards(&out_4_4);
        let passed_4_4 = top1_is(&cards_4_4, "-005");
        outcomes.push(CaseOutcome {
            category: 4,
            name: "4.4 snake_query_hits_camel_symbol",
            passed: passed_4_4,
            detail: format!("query={q_4_4:?} expected_top1=-005 got={}", fmt_cards(&cards_4_4)),
        });
    }

    // =========================================================================
    // Category 5: Git Merge Conflicts, Duplicate-ID Files & Same-Tick Edits (4 cases)
    // =========================================================================
    {
        // 5.1 git_conflict_markers_rejected
        let cf_dir = TempDir::new("conflict");
        fs::create_dir_all(cf_dir.path().join(".git")).unwrap();
        let cf_notes = note::notes_dir(cf_dir.path()).unwrap();
        let conflicted_raw = concat!(
            "---\n",
            "id: 2026-11-11-008\n",
            "current: v2\n",
            "status: active\n",
            "tags:\n",
            "  - \"merge\"\n",
            "files:\n",
            "  - \"src/merge/target.rs\"\n",
            "symbols:\n",
            "  - \"resolve_state\"\n",
            "coupled:\n",
            "signatures:\n",
            "  - \"ConflictMarkerLeakError999: unresolved merge\"\n",
            "---\n",
            "## 문제 상황\n",
            "병합 충돌 마커가 본문에 남아 있는 깨진 노트\n\n",
            "## v1 (오답) 2026-11-11\n",
            "- 한 것: 충돌 상태로 커밋\n",
            "- 틀린 증거: ConflictMarkerLeakError999: unresolved merge\n",
            "- 왜 틀렸나: 처음부터 틀림: 병합 미완료\n\n",
            "## v2 (현재 정답) 2026-11-11\n",
            "<<<<<<< HEAD\n",
            "- 한 것: HEAD 브랜치 해법\n",
            "=======\n",
            "- 한 것: feature 브랜치 해법\n",
            ">>>>>>> feature/branch\n",
            "- 확인 근거: cargo test\n",
            "- 다음에는: 병합 마커 확인\n",
        );
        let parse_rejected = note::parse(conflicted_raw).is_err();
        fs::write(cf_notes.join("2026-11-11-008.md"), conflicted_raw).unwrap();
        let supersede_rejected = mistology::run_cli(
            cf_dir.path(),
            [
                "mistology",
                "supersede",
                "2026-11-11-008",
                "--evidence",
                "e",
                "--why",
                "바뀌어서 틀려짐: x",
                "--fix",
                "f",
                "--verified",
                "v",
                "--next",
                "n",
            ],
        )
        .is_err();
        let search_cf = mistology::run_cli(
            cf_dir.path(),
            ["mistology", "search", "ConflictMarkerLeakError999: unresolved merge"],
        )
        .unwrap_or_default();
        let search_skipped = search_cf == "관련 오답노트 없음\n"
            && !search_cf.contains(">>>>>>>")
            && !search_cf.contains("<<<<<<<");
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.1 git_conflict_markers_rejected",
            passed: parse_rejected && supersede_rejected && search_skipped,
            detail: format!(
                "parse_err={parse_rejected} supersede_err={supersede_rejected} search_skipped={search_skipped}"
            ),
        });

        // 5.2 duplicate_frontmatter_id_no_clobber_or_thrash
        let dup_dir = clone_fixture("rustkv", "dup-id");
        let dup_note_099 = concat!(
            "---\n",
            "id: 2026-11-11-001\n",
            "current: v2\n",
            "status: active\n",
            "tags:\n",
            "  - \"merge\"\n",
            "files:\n",
            "  - \"crates/wal/src/recovery.rs\"\n",
            "symbols:\n",
            "  - \"replay_tail\"\n",
            "coupled:\n",
            "signatures:\n",
            "  - \"DuplicateMergeIdError: wal tail recovery drift\"\n",
            "---\n",
            "## 문제 상황\n",
            "수동 브랜치 병합 중 복제된 frontmatter id를 가진 두 번째 노트\n\n",
            "## v1 (오답) 2026-11-11\n",
            "- 한 것: 손상된 마지막 레코드를 무시하지 않고 패닉\n",
            "- 틀린 증거: DuplicateMergeIdError: wal tail recovery drift\n",
            "- 왜 틀렸나: 처음부터 틀림: 비정상 종료 시 마지막 미완성 프레임은 잘라내야 함\n\n",
            "## v2 (현재 정답) 2026-11-11\n",
            "- 한 것: replay_tail에서 미완성 꼬리 프레임을 안전하게 절단\n",
            "- 확인 근거: recovery 테스트 통과\n",
            "- 다음에는: 복구 루프 수정 시 꼬리 절단 조건 확인\n",
        );
        fs::write(
            dup_dir.path().join(".mistology/notes/2026-11-11-099.md"),
            dup_note_099,
        )
        .unwrap();
        let s1_a = mistology::run_cli(
            dup_dir.path(),
            ["mistology", "search", "error[E0597]: 'batch' does not live long enough"],
        )
        .unwrap_or_default();
        let s1_b = mistology::run_cli(
            dup_dir.path(),
            ["mistology", "search", "DuplicateMergeIdError: wal tail recovery drift"],
        )
        .unwrap_or_default();
        let s2_a = mistology::run_cli(
            dup_dir.path(),
            ["mistology", "search", "error[E0597]: 'batch' does not live long enough"],
        )
        .unwrap_or_default();
        let s2_b = mistology::run_cli(
            dup_dir.path(),
            ["mistology", "search", "DuplicateMergeIdError: wal tail recovery drift"],
        )
        .unwrap_or_default();
        let c1_a = parse_cards(&s1_a);
        let c1_b = parse_cards(&s1_b);
        let c2_a = parse_cards(&s2_a);
        let c2_b = parse_cards(&s2_b);
        let passed_5_2 = has_card(&c1_a, "-001")
            && has_card(&c1_b, "-099")
            && has_card(&c2_a, "-001")
            && has_card(&c2_b, "-099");
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.2 duplicate_frontmatter_id_no_clobber_or_thrash",
            passed: passed_5_2,
            detail: format!(
                "cold_001={} warm_099={} warm2_001={} warm2_099={}",
                fmt_cards(&c1_a),
                fmt_cards(&c1_b),
                fmt_cards(&c2_a),
                fmt_cards(&c2_b)
            ),
        });

        // 5.3 rapid_same_mtime_same_size_edit_detected
        let tick_dir = TempDir::new("same-tick");
        fs::create_dir_all(tick_dir.path().join(".git")).unwrap();
        let created_rel = mistology::run_cli(
            tick_dir.path(),
            [
                "mistology",
                "new",
                "--problem",
                "동일 타임스탬프 및 동일 바이트 길이 빠른 수정 감지",
                "--tried",
                "구형 설정 유지",
                "--evidence",
                "SameTickEditError: stale fix cached",
                "--why",
                "처음부터 틀림: 파일시스템 mtime 해상도 한계",
                "--fix",
                "AAAA_OLD_FIX",
                "--verified",
                "cargo test 통과",
                "--next",
                "빠른 재편집 후에도 최신 정답이 반영되어야 함",
                "--signature",
                "SameTickEditError: stale fix cached",
            ],
        )
        .unwrap();
        let note_file = tick_dir.path().join(created_rel.trim());
        let old_mtime = fs::metadata(&note_file).unwrap().modified().unwrap();
        let old_raw = fs::read_to_string(&note_file).unwrap();
        let before_edit = mistology::run_cli(
            tick_dir.path(),
            ["mistology", "search", "SameTickEditError: stale fix cached"],
        )
        .unwrap_or_default();
        let new_raw = old_raw.replace("AAAA_OLD_FIX", "BBBB_NEW_FIX");
        assert_eq!(old_raw.len(), new_raw.len(), "edit must keep exact byte size");
        fs::write(&note_file, &new_raw).unwrap();
        let f_handle = fs::OpenOptions::new().write(true).open(&note_file).unwrap();
        f_handle.set_modified(old_mtime).unwrap();
        drop(f_handle);
        let after_edit = mistology::run_cli(
            tick_dir.path(),
            ["mistology", "search", "SameTickEditError: stale fix cached"],
        )
        .unwrap_or_default();
        let passed_5_3 = before_edit.contains("AAAA_OLD_FIX")
            && after_edit.contains("BBBB_NEW_FIX")
            && !after_edit.contains("AAAA_OLD_FIX");
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.3 rapid_same_mtime_same_size_edit_detected",
            passed: passed_5_3,
            detail: format!(
                "before_has_old={} after_has_new={} (same_size={}, same_mtime_restored=true)",
                before_edit.contains("AAAA_OLD_FIX"),
                after_edit.contains("BBBB_NEW_FIX"),
                old_raw.len()
            ),
        });

        // 5.4 clean_abstention_on_rustkv_health
        let q_health = "헬스체크 상태 엔드포인트 수정 crates/cli/src/health.rs";
        let out_health = mistology::run_cli(rustkv.path(), ["mistology", "search", q_health]).unwrap_or_default();
        let q_ret = "LegacyMmapSigbusError: error invalid";
        let out_ret = mistology::run_cli(rustkv.path(), ["mistology", "search", q_ret]).unwrap_or_default();
        let passed_5_4 = out_health == "관련 오답노트 없음\n" && out_ret == "관련 오답노트 없음\n";
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.4 clean_abstention_on_rustkv_health",
            passed: passed_5_4,
            detail: format!(
                "health_got={} retired_got={}",
                fmt_cards(&parse_cards(&out_health)),
                fmt_cards(&parse_cards(&out_ret))
            ),
        });
    }

    // =========================================================================
    // Category 6: Parallel Multi-Agent (Swarm) Concurrency Stress (3 cases)
    // =========================================================================
    {
        // 6.1 parallel_8_threads_new_note_no_collision
        let swarm_new = TempDir::new("swarm-new");
        fs::create_dir_all(swarm_new.path().join(".git")).unwrap();
        let _ = note::notes_dir(swarm_new.path()).unwrap();
        let barrier_new = Arc::new(Barrier::new(8));
        let mut handles_new = Vec::with_capacity(8);
        for worker in 0..8usize {
            let root_buf = swarm_new.path().to_path_buf();
            let bar = Arc::clone(&barrier_new);
            handles_new.push(std::thread::spawn(move || {
                bar.wait();
                let mut results = Vec::with_capacity(2);
                for seq in 0..2usize {
                    let sig = format!("SwarmParallelNewError: worker={worker} item={seq}");
                    let res = mistology::run_cli(
                        &root_buf,
                        [
                            "mistology",
                            "new",
                            "--problem",
                            "병렬 에이전트 노트 동시 생성 경합 테스트",
                            "--tried",
                            "동시 파일 번호 채번",
                            "--evidence",
                            &sig,
                            "--why",
                            "처음부터 틀림: 동시 채번 시 충돌 재시도 필요",
                            "--fix",
                            "create_new 원자적 채번 루프 확인",
                            "--verified",
                            "8 스레드 동시 생성 테스트",
                            "--next",
                            "병렬 에이전트 환경에서도 고유 ID 보장",
                            "--signature",
                            &sig,
                        ],
                    );
                    results.push(res);
                }
                results
            }));
        }
        let mut ok_outputs = Vec::new();
        for h in handles_new {
            for s in h.join().unwrap().into_iter().flatten() {
                ok_outputs.push(s.trim().to_string());
            }
        }
        let unique_outputs: HashSet<&String> = ok_outputs.iter().collect();
        let mut parsed_ids = HashSet::new();
        if let Ok(entries) = fs::read_dir(swarm_new.path().join(".mistology/notes")) {
            for e in entries.flatten() {
                if e.path().extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Ok(raw) = fs::read_to_string(e.path()) {
                        if let Ok((m, _)) = note::parse(&raw) {
                            parsed_ids.insert(m.id);
                        }
                    }
                }
            }
        }
        let passed_6_1 = ok_outputs.len() == 16 && unique_outputs.len() == 16 && parsed_ids.len() == 16;
        outcomes.push(CaseOutcome {
            category: 6,
            name: "6.1 parallel_8_threads_new_note_no_collision",
            passed: passed_6_1,
            detail: format!(
                "ok_calls={}/16 unique_paths={}/16 valid_parsed_notes={}/16",
                ok_outputs.len(),
                unique_outputs.len(),
                parsed_ids.len()
            ),
        });

        // 6.2 parallel_writers_and_searchers_zero_busy_drops
        let swarm_rw = clone_fixture("shopcore", "swarm-rw");
        mistology::run_cli(
            swarm_rw.path(),
            [
                "mistology",
                "new",
                "--problem",
                "다중 에이전트 동시 쓰기 중 검색 누락 방지 앵커",
                "--tried",
                "기본 롤백 저널과 DEFERRED 트랜잭션 사용",
                "--evidence",
                "TargetAnchorError999: concurrent reader busy drop",
                "--why",
                "처음부터 틀림: 동시 sync 트랜잭션 업그레이드 시 SQLITE_BUSY 발생",
                "--fix",
                "TargetAnchorFix999: WAL 모드와 IMMEDIATE 트랜잭션으로 무중단 조회 보장",
                "--verified",
                "병렬 스트레스 통과",
                "--next",
                "동시 에이전트 환경에서도 빈 결과 드롭 없이 반환",
                "--signature",
                "TargetAnchorError999: concurrent reader busy drop",
            ],
        )
        .unwrap();
        // Warm initial index before launching concurrent writers + searchers
        let seed_check = mistology::run_cli(
            swarm_rw.path(),
            ["mistology", "search", "TargetAnchorError999"],
        )
        .unwrap_or_default();
        assert!(
            seed_check.contains("TargetAnchorFix999"),
            "seed target note must be searchable before concurrency stress"
        );

        let barrier_rw = Arc::new(Barrier::new(8));
        let mut writer_handles = Vec::with_capacity(4);
        for w in 0..4usize {
            let root_buf = swarm_rw.path().to_path_buf();
            let bar = Arc::clone(&barrier_rw);
            writer_handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut wrote = 0usize;
                for step in 0..5usize {
                    let sig = format!("SwarmWriterBusySig_{w}_{step}");
                    if mistology::run_cli(
                        &root_buf,
                        [
                            "mistology",
                            "new",
                            "--problem",
                            "병렬 쓰기 트랜잭션 부하 생성",
                            "--tried",
                            "동시 인덱싱 유발",
                            "--evidence",
                            &sig,
                            "--why",
                            "처음부터 틀림: 부하 시나리오",
                            "--fix",
                            "정상 기록",
                            "--verified",
                            "ok",
                            "--next",
                            "ok",
                            "--signature",
                            &sig,
                        ],
                    )
                    .is_ok()
                    {
                        wrote += 1;
                    }
                    // Immediately trigger sync() write transaction from writer side as well
                    let _ = mistology::run_cli(&root_buf, ["mistology", "search", &sig]);
                }
                wrote
            }));
        }

        let mut searcher_handles = Vec::with_capacity(4);
        for _ in 0..4usize {
            let root_buf = swarm_rw.path().to_path_buf();
            let bar = Arc::clone(&barrier_rw);
            searcher_handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut hits_ok = 0usize;
                for _ in 0..15usize {
                    if let Ok(out) = mistology::run_cli(&root_buf, ["mistology", "search", "TargetAnchorError999"]) {
                        if out.contains("TargetAnchorFix999") {
                            hits_ok += 1;
                        }
                    }
                }
                hits_ok
            }));
        }

        let total_wrote: usize = writer_handles.into_iter().map(|h| h.join().unwrap()).sum();
        let total_search_hits: usize = searcher_handles.into_iter().map(|h| h.join().unwrap()).sum();
        let busy_drops = 60usize.saturating_sub(total_search_hits);
        let passed_6_2 = total_wrote == 20 && total_search_hits == 60;
        outcomes.push(CaseOutcome {
            category: 6,
            name: "6.2 parallel_writers_and_searchers_zero_busy_drops",
            passed: passed_6_2,
            detail: format!(
                "writer_notes={total_wrote}/20 search_hits={total_search_hits}/60 busy_drops={busy_drops}"
            ),
        });

        // 6.3 polyglot_combined_warm_latency_sla
        let poly = TempDir::new("polyglot-112");
        fs::create_dir_all(poly.path().join(".git")).unwrap();
        let poly_notes = note::notes_dir(poly.path()).unwrap();
        copy_notes_retagged(&fixture_root("shopcore").join(".mistology/notes"), &poly_notes, "2026-11-08");
        copy_notes_retagged(&fixture_root("webmono").join(".mistology/notes"), &poly_notes, "2026-11-09");
        copy_notes_retagged(&fixture_root("rustkv").join(".mistology/notes"), &poly_notes, "2026-11-10");
        populate_polyglot_scale_notes(poly.path(), 88);

        let total_poly_notes = fs::read_dir(&poly_notes)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .count();
        assert_eq!(total_poly_notes, 112, "combined polyglot workspace should have 112 notes");

        let t_cold = Instant::now();
        let _ = mistology::run_cli(
            poly.path(),
            ["mistology", "search", "SignatureMismatchError: invalid token delimiter or MAC"],
        )
        .unwrap();
        let cold_ms = t_cold.elapsed().as_secs_f64() * 1000.0;

        let bench_queries = [
            "SignatureMismatchError: invalid token delimiter or MAC",
            "주문 상태 부분환불 수정 src/orders/state.py",
            "EdgeRuntimeError: The edge runtime does not support Node.js 'crypto' module",
            "파일 점검 apps/web/src/app/checkout/page.tsx",
            "NumericPrecisionLossError: Postgres numeric parsed as IEEE754 float",
            "error[E0597]: 'batch' does not live long enough",
            "WalFsyncDurabilityError: fdatasync skipped before header commit",
            "CompactionDeadlockError: lock order manifest -> memtable inverted",
            "maxSegmentBytes zero check",
            "verify_frame_checksum trailer",
        ];
        let mut warm_sum_ms = 0.0f64;
        let mut warm_max_ms = 0.0f64;
        for q in bench_queries {
            let t = Instant::now();
            let _ = mistology::run_cli(poly.path(), ["mistology", "search", q]).unwrap();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            warm_sum_ms += ms;
            if ms > warm_max_ms {
                warm_max_ms = ms;
            }
        }
        let warm_avg_ms = warm_sum_ms / bench_queries.len() as f64;
        let passed_6_3 = warm_avg_ms < 15.0;
        outcomes.push(CaseOutcome {
            category: 6,
            name: "6.3 polyglot_combined_warm_latency_sla",
            passed: passed_6_3,
            detail: format!(
                "notes={total_poly_notes} cold={cold_ms:.2}ms warm_avg={warm_avg_ms:.2}ms (<15.0ms, max={warm_max_ms:.2}ms)"
            ),
        });
    }

    // =========================================================================
    // Category 7: Open-Source Ecosystem Pitfalls (claude-mem / beads / Sandboxes) (4 cases)
    // =========================================================================
    {
        // 7.1 common_domain_term_idf_no_collapse (claude-mem #4955)
        let idf_dir = TempDir::new("oss-idf");
        fs::create_dir_all(idf_dir.path().join(".git")).unwrap();
        let _ = mistology::run_cli(
            idf_dir.path(),
            [
                "mistology",
                "new",
                "--problem",
                "JWT 액세스 토큰 만료 검증 누락",
                "--tried",
                "exp 클레임 비교 생략",
                "--evidence",
                "JwtAuthError: token expired",
                "--why",
                "처음부터 틀림: 만료 시각 검증 누락",
                "--fix",
                "exp 클레임과 현재 UTC 시각 비교",
                "--verified",
                "auth test 통과",
                "--next",
                "JWT 토큰 검증 시 exp 확인",
                "--signature",
                "JwtAuthError: token expired",
                "--tag",
                "jwt",
            ],
        )
        .unwrap();
        let _ = mistology::run_cli(
            idf_dir.path(),
            [
                "mistology",
                "new",
                "--problem",
                "JWT 리프레시 토큰 서명 시크릿 혼용",
                "--tried",
                "액세스 시크릿 재사용",
                "--evidence",
                "JwtAuthError: token key mismatch",
                "--why",
                "처음부터 틀림: 토큰 종류별 키 분리 필요",
                "--fix",
                "REFRESH_SECRET 전용 키로 검증",
                "--verified",
                "auth test 통과",
                "--next",
                "JWT 토큰 시크릿 분리 확인",
                "--signature",
                "JwtAuthError: token key mismatch",
                "--tag",
                "jwt",
            ],
        )
        .unwrap();
        let _ = mistology::run_cli(
            idf_dir.path(),
            [
                "mistology",
                "new",
                "--problem",
                "프로필 썸네일 이미지 리사이즈 실패",
                "--tried",
                "원본 크기 그대로 반환",
                "--evidence",
                "ThumbnailResizeError: width missing",
                "--why",
                "처음부터 틀림: 기본 너비 미지정",
                "--fix",
                "기본 너비 256px 지정",
                "--verified",
                "ui test 통과",
                "--next",
                "이미지 리사이즈 기본값 확인",
                "--signature",
                "ThumbnailResizeError: width missing",
                "--file",
                "src/ui/thumb.ts",
                "--tag",
                "ui",
            ],
        )
        .unwrap();

        let out_idf = mistology::run_cli(
            idf_dir.path(),
            ["mistology", "search", "JwtAuthError: token claim missing"],
        )
        .unwrap_or_default();
        let cards_idf = parse_cards(&out_idf);
        let top2_are_jwt = cards_idf.len() >= 2
            && (cards_idf[0].file_stem.ends_with("-001") || cards_idf[0].file_stem.ends_with("-002"))
            && (cards_idf[1].file_stem.ends_with("-001") || cards_idf[1].file_stem.ends_with("-002"));

        let out_idf_edge = mistology::run_cli(
            idf_dir.path(),
            ["mistology", "search", "JwtAuthError: token claim missing src/ui/thumb.ts"],
        )
        .unwrap_or_default();
        let cards_idf_edge = parse_cards(&out_idf_edge);
        let jwt_survives_cutoff = has_card(&cards_idf_edge, "-001") && has_card(&cards_idf_edge, "-002");

        let passed_7_1 = top2_are_jwt && jwt_survives_cutoff;
        outcomes.push(CaseOutcome {
            category: 7,
            name: "7.1 common_domain_term_idf_no_collapse",
            passed: passed_7_1,
            detail: format!(
                "pure_query={} with_other_file={}",
                fmt_cards(&cards_idf),
                fmt_cards(&cards_idf_edge)
            ),
        });

        // 7.2 absolute_and_symlink_path_stripping
        let sym_target = clone_fixture("shopcore", "oss-sym-target");
        let sym_holder = TempDir::new("oss-sym-holder");
        let sym_repo = sym_holder.path().join("linked-shopcore");
        #[cfg(unix)]
        std::os::unix::fs::symlink(sym_target.path(), &sym_repo).unwrap();
        #[cfg(not(unix))]
        let sym_repo = sym_target.path().to_path_buf();

        let canon_target = fs::canonicalize(sym_target.path()).unwrap();
        let abs_sym_webhook = sym_repo.join("src/workers/webhook.py").to_string_lossy().to_string();
        let abs_canon_state = canon_target.join("src/orders/state.py").to_string_lossy().to_string();

        let created_sym = mistology::run_cli(
            &sym_repo,
            [
                "mistology",
                "new",
                "--problem",
                "심볼릭 링크 워크스페이스에서 절대 경로 전달 시 로컬 경로 유출 방지",
                "--tried",
                "절대 경로 그대로 기록",
                "--evidence",
                "WebhookSymlinkError: duplicate delivery",
                "--why",
                "처음부터 틀림: 에이전트 툴이 절대 경로를 넘김",
                "--fix",
                "저장소 루트 기준 상대 경로로 정규화",
                "--verified",
                "cargo test 통과",
                "--next",
                "절대 경로 입력도 상대 경로로 기록되어야 함",
                "--signature",
                "WebhookSymlinkError: duplicate delivery",
                "--file",
                &abs_sym_webhook,
                "--coupled",
                &abs_canon_state,
            ],
        );
        let q_sym_trace = format!(
            "Traceback:\n  File \"{}\", line 42, in transition\n    raise ValueError('bad')",
            sym_repo.join("src/orders/state.py").display()
        );
        let out_sym_search = mistology::run_cli(&sym_repo, ["mistology", "search", &q_sym_trace]).unwrap_or_default();
        let cards_sym_search = parse_cards(&out_sym_search);

        let (passed_7_2, detail_7_2) = match created_sym {
            Ok(rel_out) => {
                let note_p = sym_target.path().join(rel_out.trim());
                match fs::read_to_string(&note_p).ok().and_then(|r| note::parse(&r).ok()) {
                    Some((meta, _)) => {
                        let clean_files = meta.files == vec!["src/workers/webhook.py".to_string()]
                            && meta.coupled == vec!["src/orders/state.py".to_string()];
                        let trace_hit = top1_is(&cards_sym_search, "-005") && has_card(&cards_sym_search, "-006");
                        (
                            clean_files && trace_hit,
                            format!(
                                "files={:?} coupled={:?} trace_hits={}",
                                meta.files,
                                meta.coupled,
                                fmt_cards(&cards_sym_search)
                            ),
                        )
                    }
                    None => (false, format!("failed to parse created note {rel_out:?}")),
                }
            }
            Err(e) => (false, format!("new error: {e}")),
        };
        outcomes.push(CaseOutcome {
            category: 7,
            name: "7.2 absolute_and_symlink_path_stripping",
            passed: passed_7_2,
            detail: detail_7_2,
        });

        // 7.3 readonly_sandbox_in_memory_fallback
        let ro_fresh = clone_fixture("shopcore", "oss-ro-fresh");
        let ro_warm = clone_fixture("shopcore", "oss-ro-warm");
        let _ = mistology::run_cli(
            ro_warm.path(),
            ["mistology", "search", "SignatureMismatchError: invalid token delimiter or MAC"],
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for root_p in [ro_fresh.path(), ro_warm.path()] {
                let mist = root_p.join(".mistology");
                let notes_d = mist.join("notes");
                if let Ok(entries) = fs::read_dir(&mist) {
                    for e in entries.flatten() {
                        if e.path().is_file() {
                            let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(0o444));
                        }
                    }
                }
                fs::set_permissions(&notes_d, fs::Permissions::from_mode(0o555)).unwrap();
                fs::set_permissions(&mist, fs::Permissions::from_mode(0o555)).unwrap();
            }
        }
        let probe = ro_fresh.path().join(".mistology/.ro_probe");
        let ro_enforced = fs::write(&probe, b"x").is_err();
        let _ = fs::remove_file(&probe);
        let q_ro = "주문 상태 부분환불 수정 src/orders/state.py";
        let out_ro_fresh = mistology::run_cli(ro_fresh.path(), ["mistology", "search", q_ro]).unwrap_or_default();
        let out_ro_warm = mistology::run_cli(ro_warm.path(), ["mistology", "search", q_ro]).unwrap_or_default();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for root_p in [ro_fresh.path(), ro_warm.path()] {
                let mist = root_p.join(".mistology");
                let notes_d = mist.join("notes");
                let _ = fs::set_permissions(&mist, fs::Permissions::from_mode(0o755));
                let _ = fs::set_permissions(&notes_d, fs::Permissions::from_mode(0o755));
                if let Ok(entries) = fs::read_dir(&mist) {
                    for e in entries.flatten() {
                        if e.path().is_file() {
                            let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(0o644));
                        }
                    }
                }
            }
        }
        let cards_ro_fresh = parse_cards(&out_ro_fresh);
        let cards_ro_warm = parse_cards(&out_ro_warm);
        let no_db_written = !ro_enforced || !ro_fresh.path().join(".mistology/index.db").exists();
        let passed_7_3 = no_db_written
            && top1_is(&cards_ro_fresh, "-005")
            && has_card(&cards_ro_fresh, "-006")
            && top1_is(&cards_ro_warm, "-005")
            && has_card(&cards_ro_warm, "-006");
        outcomes.push(CaseOutcome {
            category: 7,
            name: "7.3 readonly_sandbox_in_memory_fallback",
            passed: passed_7_3,
            detail: format!(
                "no_db_written={no_db_written} fresh={} warm={}",
                fmt_cards(&cards_ro_fresh),
                fmt_cards(&cards_ro_warm)
            ),
        });

        // 7.4 git_worktree_and_supersede_path_id_ergonomics
        let wt = clone_fixture("shopcore", "oss-worktree");
        fs::remove_dir_all(wt.path().join(".git")).unwrap();
        fs::write(
            wt.path().join(".git"),
            "gitdir: /tmp/fake-main-repo/.git/worktrees/feature-a\n",
        )
        .unwrap();
        let nested_billing = wt.path().join("src/billing");
        let sup_by_rel_path = mistology::run_cli(
            &nested_billing,
            [
                "mistology",
                "supersede",
                ".mistology/notes/2026-11-11-004.md",
                "--evidence",
                "SettlementBatchTzError: worktree v3 regression",
                "--why",
                "바뀌어서 틀려짐: 배치 스케줄러가 KST 오프셋 문자열을 전달함",
                "--fix",
                "입력 시각을 즉시 UTC aware datetime으로 변환",
                "--verified",
                "settlement 테스트 통과",
                "--next",
                "정산 배치 진입점에서 UTC 변환 강제",
            ],
        );
        let sup_by_md_name = mistology::run_cli(
            &nested_billing,
            [
                "mistology",
                "supersede",
                "2026-11-11-004.md",
                "--evidence",
                "SettlementBatchTzError: naive timestamp slipped",
                "--why",
                "바뀌어서 틀려짐: tzinfo 없는 datetime 허용됨",
                "--fix",
                "tzinfo is None이면 즉시 예외 발생 후 UTC 정규화",
                "--verified",
                "settlement 테스트 통과",
                "--next",
                "naive datetime 원천 차단",
            ],
        );
        let bad_traversal = mistology::run_cli(
            &nested_billing,
            [
                "mistology",
                "supersede",
                "../../etc/passwd",
                "--evidence",
                "e",
                "--why",
                "w",
                "--fix",
                "f",
                "--verified",
                "v",
                "--next",
                "n",
            ],
        );
        let out_wt_search = mistology::run_cli(
            &nested_billing,
            ["mistology", "search", "파일 점검 currency.py"],
        )
        .unwrap_or_default();
        let passed_7_4 = sup_by_rel_path.is_ok()
            && sup_by_md_name.is_ok()
            && bad_traversal.is_err()
            && out_wt_search.contains("2026-11-11-004.md [v4 | 오답 3회");
        outcomes.push(CaseOutcome {
            category: 7,
            name: "7.4 git_worktree_and_supersede_path_id_ergonomics",
            passed: passed_7_4,
            detail: format!(
                "rel_ok={} md_ok={} traversal_rejected={} v4_found={}",
                sup_by_rel_path.is_ok(),
                sup_by_md_name.is_ok(),
                bad_traversal.is_err(),
                out_wt_search.contains("2026-11-11-004.md [v4 | 오답 3회")
            ),
        });
    }

    // Ensure source fixture directories were never polluted with index.db
    for fixture in ["shopcore", "webmono", "rustkv"] {
        assert!(
            !fixture_root(fixture).join(".mistology/index.db").exists(),
            "evals/{fixture}/.mistology/index.db must never be written into the source tree"
        );
    }

    let category_meta: [(usize, &str); 7] = [
        (1, "Compiler Stacktraces & Dirty Path Extraction"),
        (2, "Monorepo Subdirectory Isolation & TS Aliases"),
        (3, "Cross-Platform Windows Paths & Unicode Normalization"),
        (4, "Rust `::` Scoped Symbols & `snake_case` <-> `camelCase`"),
        (5, "Git Merge Conflicts, Duplicate IDs & Same-Tick Edits"),
        (6, "Parallel Multi-Agent (Swarm) Concurrency Stress"),
        (7, "Open-Source Ecosystem Pitfalls (IDF, Symlinks, RO Sandbox, Worktrees)"),
    ];

    assert_eq!(outcomes.len(), 28);
    let total_passed = outcomes.iter().filter(|o| o.passed).count();

    println!("\n## MISTOLOGY Product Hardening Scorecard\n");
    println!("| # | Category | Passed | Rate | Status |");
    println!("|---|---|---:|---:|---|");
    for (cat_num, title) in category_meta {
        let cat_cases: Vec<&CaseOutcome> = outcomes.iter().filter(|o| o.category == cat_num).collect();
        let passed = cat_cases.iter().filter(|o| o.passed).count();
        let total = cat_cases.len();
        let pct = (passed as f64 / total as f64) * 100.0;
        let status = if passed == total { "PASS" } else { "FAIL" };
        println!("| {cat_num} | {title} | {passed} / {total} | {pct:.1}% | {status} |");
    }
    let total_pct = (total_passed as f64 / 28.0) * 100.0;
    println!(
        "| **Total** | **All 7 Hardening Categories** | **{total_passed} / 28** | **{total_pct:.1}%** | **{}** |",
        if total_passed == 28 { "PASS" } else { "FAIL" }
    );

    let failures: Vec<&CaseOutcome> = outcomes.iter().filter(|o| !o.passed).collect();
    if !failures.is_empty() {
        println!("\n### Failing Cases ({})\n", failures.len());
        for f in failures {
            println!("- **[Cat {}] `{}`** — {}", f.category, f.name, f.detail);
        }
    }
    println!();

    assert_eq!(
        total_passed,
        28,
        "hardening eval suite failed {} cases",
        28 - total_passed
    );
}
