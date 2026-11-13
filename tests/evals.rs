use mistology::index::{self, SearchHit};
use mistology::note::{self, NewFields};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mistology-evals-{tag}-{}-{}",
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

fn shopcore_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("evals/shopcore")
}

fn clone_shopcore_notes(tag: &str) -> TempDir {
    let tmp = TempDir::new(tag);
    fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let dst_notes = note::notes_dir(tmp.path()).unwrap();
    let src_notes = shopcore_fixture_root().join(".mistology/notes");
    let mut entries: Vec<PathBuf> = fs::read_dir(&src_notes)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    entries.sort();
    assert_eq!(entries.len(), 12, "expected 12 shopcore fixture notes");
    for src in entries {
        let dst = dst_notes.join(src.file_name().unwrap());
        fs::copy(&src, &dst).unwrap();
    }
    tmp
}

fn populate_88_scale_notes(root: &Path) {
    let domains = [
        ("catalog", "src/catalog/sku.py", "normalizeSku", "src/catalog/pricing.py", "SkuFormatError"),
        ("inventory", "src/inventory/stock.py", "reserveStock", "src/inventory/warehouse.py", "StockUnderflowError"),
        ("search", "src/search/indexer.py", "rebuildFacet", "src/search/synonyms.py", "FacetSyncError"),
        ("analytics", "src/analytics/funnel.py", "recordConversion", "src/analytics/attribution.py", "DuplicateEventKeyError"),
        ("fraud", "src/fraud/velocity.py", "checkVelocityWindow", "src/fraud/rules.py", "VelocityWindowError"),
        ("subscription", "src/subscription/renewal.py", "advanceBillingCycle", "src/subscription/proration.py", "BillingAnchorDriftError"),
        ("tax", "src/tax/jurisdiction.py", "computeVatMinor", "src/tax/rounding.py", "TaxRoundingDriftError"),
        ("audit", "src/audit/trail.py", "appendAuditEntry", "src/audit/redaction.py", "UnredactedSecretError"),
    ];
    for idx in 0..88usize {
        let (tag, file_base, sym_base, coupled_base, err_cls) = domains[idx % domains.len()];
        let shard = idx / domains.len();
        let file = format!("{}_{shard}.py", file_base.trim_end_matches(".py"));
        let coupled = format!("{}_{shard}.py", coupled_base.trim_end_matches(".py"));
        let symbol = format!("{sym_base}V{shard}");
        let sig = format!("{err_cls}: invariant failed on shard={shard} case={idx}");
        let problem = format!("{tag} 도메인 샤드 {shard}(케이스 {idx}) 상태 동기화 중 경계 조건 불일치 발생");
        let tried = format!("{file} 내부의 {symbol}만 단독 수정하고 결합된 {coupled} 검증 로직은 갱신하지 않음");
        let why = format!("처음부터 틀림: {file} 출력 포맷과 {coupled}의 입력 스키마가 암묵적으로 결합되어 있음");
        let fix = format!("{file}의 {symbol}와 {coupled}의 스키마 검증 함수를 동일한 규격 버전으로 동시 갱신");
        let verified = format!("python3 -m unittest test_{tag}_{shard}_{idx} 통합 시나리오 통과");
        let next_step = format!("{file} 수정 시 반드시 결합 모듈 {coupled}의 경계 조건 테스트를 함께 실행");
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
                force: false,
            },
        )
        .unwrap();
    }
}

fn est_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(3)
}

fn monolithic_active_markdown(root: &Path) -> (usize, String) {
    let notes_dir = root.join(".mistology/notes");
    let mut paths: Vec<PathBuf> = fs::read_dir(&notes_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    paths.sort();
    let mut active_docs = Vec::new();
    for p in paths {
        let raw = fs::read_to_string(&p).unwrap();
        if let Ok((meta, _)) = note::parse(&raw) {
            if meta.status == "active" {
                active_docs.push(raw);
            }
        }
    }
    (active_docs.len(), active_docs.join("\n"))
}

fn short_id(id: &str) -> &str {
    id.rsplit('-').next().unwrap_or(id)
}

fn fmt_hits(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "[]".to_string();
    }
    let items: Vec<String> = hits
        .iter()
        .map(|h| match &h.via {
            Some(v) => format!("-{}(via:{v})", short_id(&h.id)),
            None => format!("-{}", short_id(&h.id)),
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn has_id(hits: &[SearchHit], suffix: &str) -> bool {
    hits.iter().any(|h| h.id.ends_with(suffix))
}

struct CaseOutcome {
    category: usize,
    name: &'static str,
    passed: bool,
    detail: String,
}

#[test]
fn mistology_shopcore_eval_suite() {
    let shopcore = clone_shopcore_notes("shopcore");
    let root = shopcore.path();

    let mut outcomes: Vec<CaseOutcome> = Vec::with_capacity(24);
    let mut positive_queries: Vec<&'static str> = Vec::new();

    // Category 1: Direct Error Signature Recall@1 (4 cases)
    let cat1_cases: [(&str, &str, &str); 4] = [
        (
            "1.1 exact_sig_token_delimiter",
            "SignatureMismatchError: invalid token delimiter or MAC",
            "-001",
        ),
        (
            "1.2 exact_sig_timezone_settlement",
            "TimezoneNaiveSettlementError: settled_at must be UTC",
            "-004",
        ),
        (
            "1.3 dynamic_sig_double_refund_ids",
            "DoubleRefundError: duplicate refund event evt_999 for ord_7777",
            "-005",
        ),
        (
            "1.4 dynamic_sig_session_ttl_nums",
            "SessionTtlError: ttl 7200000 exceeds max 86400s",
            "-002",
        ),
    ];
    for (name, q, expected) in cat1_cases {
        positive_queries.push(q);
        let hits = index::search(root, q, 3);
        let passed = hits.first().is_some_and(|h| h.id.ends_with(expected));
        let clutter = hits.iter().filter(|h| h.id.ends_with("-007") || h.id.ends_with("-008")).count();
        outcomes.push(CaseOutcome {
            category: 1,
            name,
            passed,
            detail: format!("query={q:?} expected_top1={expected} got={} sibling_clutter={clutter}", fmt_hits(&hits)),
        });
    }

    // Category 2: Natural Korean Particles & camelCase/PascalCase Subwords Recall@3 (5 cases)
    let cat2_cases: [(&str, &str, &str); 5] = [
        ("2.1 ko_josa_webhook_timestamp", "웹훅에서 타임스탬프가 어긋남", "-006"),
        ("2.2 ko_josa_env_boolean", "환경변수에서 불리언이 잘못됨", "-010"),
        ("2.3 ko_josa_mixed_ascii_error", "MinorUnitScaleError가 발생함", "-003"),
        ("2.4 subword_replay_attack", "replay attack", "-006"),
        ("2.5 subword_units_conversion", "units conversion", "-003"),
    ];
    for (name, q, expected) in cat2_cases {
        positive_queries.push(q);
        let hits = index::search(root, q, 3);
        let passed = has_id(&hits, expected);
        outcomes.push(CaseOutcome {
            category: 2,
            name,
            passed,
            detail: format!("query={q:?} expected_in_top3={expected} got={}", fmt_hits(&hits)),
        });
    }

    // Category 3: 1-Hop Ontology Coupling Recall@3 (5 cases)
    {
        let q1 = "토큰 클레임 구분자 수정 src/auth/token.py";
        positive_queries.push(q1);
        let hits = index::search(root, q1, 3);
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.1 coupling_token_to_session",
            passed: has_id(&hits, "-001") && has_id(&hits, "-002"),
            detail: format!("query={q1:?} expected=[-001 & -002] got={}", fmt_hits(&hits)),
        });

        let q2 = "세션 만료 설정 점검 src/middleware/session.py";
        positive_queries.push(q2);
        let hits = index::search(root, q2, 3);
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.2 coupling_session_to_env",
            passed: has_id(&hits, "-002") && has_id(&hits, "-010"),
            detail: format!("query={q2:?} expected=[-002 & -010] got={}", fmt_hits(&hits)),
        });

        let q3 = "주문 상태 부분환불 수정 src/orders/state.py";
        positive_queries.push(q3);
        let hits = index::search(root, q3, 3);
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.3 coupling_bare_filename_webhook",
            passed: has_id(&hits, "-005") && has_id(&hits, "-006"),
            detail: format!("query={q3:?} expected=[-005 & -006] got={}", fmt_hits(&hits)),
        });

        let q4 = "신규 통화 코드 등록 src/billing/currency.py";
        positive_queries.push(q4);
        let hits = index::search(root, q4, 3);
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.4 coupling_currency_to_settlement_money",
            passed: has_id(&hits, "-011") && (has_id(&hits, "-003") || has_id(&hits, "-004")),
            detail: format!("query={q4:?} expected=[-011 & (-003|-004)] got={}", fmt_hits(&hits)),
        });

        let q5 = "금액 합산 스케일 점검 src/billing/money.py";
        positive_queries.push(q5);
        let hits = index::search(root, q5, 3);
        outcomes.push(CaseOutcome {
            category: 3,
            name: "3.5 coupling_money_to_currency",
            passed: has_id(&hits, "-003") && has_id(&hits, "-011"),
            detail: format!("query={q5:?} expected=[-003 & -011] got={}", fmt_hits(&hits)),
        });
    }

    // Category 4: True-Negative Abstention Precision (4 cases)
    let cat4_cases: [(&str, &str); 4] = [
        ("4.1 abstention_health_ping", "헬스체크 응답 헤더 추가 src/health/ping.py"),
        (
            "4.2 abstention_metrics_prometheus",
            "프로메테우스 메트릭 수집기 추가 src/metrics/prometheus.py",
        ),
        (
            "4.3 abstention_web_static_assets",
            "정적 파일 캐시 제어 추가 src/web/static_assets.py",
        ),
        ("4.4 abstention_db_pool_size", "데이터베이스 커넥션 풀 크기 조정"),
    ];
    for (name, q) in cat4_cases {
        let hits = index::search(root, q, 3);
        outcomes.push(CaseOutcome {
            category: 4,
            name,
            passed: hits.is_empty(),
            detail: format!("query={q:?} expected=[] got={}", fmt_hits(&hits)),
        });
    }

    // Category 5: Version Evolution (supersede) & retired Lifecycle Fidelity (3 cases)
    {
        let q_evo1 = "MinorUnitScaleError: KRW 1000 became 100000";
        positive_queries.push(q_evo1);
        let hits = index::search(root, q_evo1, 3);
        let passed_evo1 = hits.first().is_some_and(|h| {
            h.id.ends_with("-003")
                && h.current == "v4"
                && h.fix.contains("toMinorUnits/fromMinorUnits")
                && !h.fix.contains("* 100")
                && h.pitfalls.contains("v2:")
                && h.pitfalls.contains("* 100")
        });
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.1 evolution_superseded_v4_only",
            passed: passed_evo1,
            detail: format!("query={q_evo1:?} got={}", fmt_hits(&hits)),
        });

        let q_evo2 = "금액 통화 src/billing/money.py";
        let q_evo2_shared = "billing 테스트 통과";
        positive_queries.push(q_evo2);
        let hits_money = index::search(root, q_evo2, 3);
        let hits_shared = index::search(root, q_evo2_shared, 3);
        let passed_evo2 = hits_money
            .first()
            .is_some_and(|h| h.id.ends_with("-003") && h.wrong == 3)
            && hits_shared
                .first()
                .is_some_and(|h| h.id.ends_with("-003") && h.wrong == 3);
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.2 evolution_wrong3_outranks_wrong1",
            passed: passed_evo2,
            detail: format!(
                "money_top={} shared_tag_top={}",
                fmt_hits(&hits_money),
                fmt_hits(&hits_shared)
            ),
        });

        let q_evo3 = "XmlEncodingError: invalid CP949 byte";
        let hits_ret = index::search(root, q_evo3, 3);
        outcomes.push(CaseOutcome {
            category: 5,
            name: "5.3 lifecycle_retired_suppressed",
            passed: hits_ret.is_empty(),
            detail: format!("query={q_evo3:?} expected=[] got={}", fmt_hits(&hits_ret)),
        });
    }

    // Category 6: Token & Latency Efficiency vs Monolithic MISTAKES.md & --full (3 checks)
    let (active_count, mono_raw) = monolithic_active_markdown(root);
    assert_eq!(active_count, 11, "shopcore should have 11 active notes");
    let mono_bytes = mono_raw.len();
    let mono_est_tokens = est_tokens(&mono_raw);

    let mut total_card_bytes = 0usize;
    let mut total_card_tokens = 0usize;
    let mut total_full_bytes = 0usize;
    let mut total_full_tokens = 0usize;
    for &q in &positive_queries {
        let card_out = mistology::run_cli(root, ["mistology", "search", q]).unwrap();
        let full_out = mistology::run_cli(root, ["mistology", "search", "--full", q]).unwrap();
        total_card_bytes += card_out.len();
        total_card_tokens += est_tokens(&card_out);
        total_full_bytes += full_out.len();
        total_full_tokens += est_tokens(&full_out);
    }
    let pos_n = positive_queries.len() as f64;
    let card_avg_bytes = total_card_bytes as f64 / pos_n;
    let card_avg_tokens = total_card_tokens as f64 / pos_n;
    let full_avg_bytes = total_full_bytes as f64 / pos_n;
    let full_avg_tokens = total_full_tokens as f64 / pos_n;
    let savings_vs_mono_pct = (1.0 - card_avg_tokens / mono_est_tokens as f64) * 100.0;
    let savings_vs_full_pct = (1.0 - card_avg_tokens / full_avg_tokens) * 100.0;

    outcomes.push(CaseOutcome {
        category: 6,
        name: "6.1 shopcore_11_note_token_savings",
        passed: savings_vs_mono_pct >= 70.0 && savings_vs_full_pct >= 35.0,
        detail: format!(
            "vs_monolithic={savings_vs_mono_pct:.1}% (>=70%) vs_full={savings_vs_full_pct:.1}% (>=35%)"
        ),
    });

    // Scale 100-note project (12 shopcore notes + 88 realistic notes = 100 total, 99 active)
    let scale = clone_shopcore_notes("scale100");
    populate_88_scale_notes(scale.path());
    let total_scale_files = fs::read_dir(scale.path().join(".mistology/notes"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
        .count();
    assert_eq!(total_scale_files, 100, "scaled project should contain 100 .md notes");

    let (scale_active_count, scale_mono_raw) = monolithic_active_markdown(scale.path());
    assert_eq!(scale_active_count, 99);
    let scale_mono_bytes = scale_mono_raw.len();
    let scale_mono_tokens = est_tokens(&scale_mono_raw);

    let t_cold = Instant::now();
    let _ = index::search(
        scale.path(),
        "SignatureMismatchError: invalid token delimiter or MAC",
        3,
    );
    let scale_cold_ms = t_cold.elapsed().as_secs_f64() * 1000.0;

    let mut warm_total_ms = 0.0f64;
    let mut warm_max_ms = 0.0f64;
    for &q in &positive_queries {
        let t_warm = Instant::now();
        let _ = index::search(scale.path(), q, 3);
        let elapsed_ms = t_warm.elapsed().as_secs_f64() * 1000.0;
        warm_total_ms += elapsed_ms;
        if elapsed_ms > warm_max_ms {
            warm_max_ms = elapsed_ms;
        }
    }
    let scale_warm_avg_ms = warm_total_ms / pos_n;

    let mut scale_card_bytes_sum = 0usize;
    let mut scale_card_tokens_sum = 0usize;
    for &q in &positive_queries {
        let card_out = mistology::run_cli(scale.path(), ["mistology", "search", q]).unwrap();
        scale_card_bytes_sum += card_out.len();
        scale_card_tokens_sum += est_tokens(&card_out);
    }
    let scale_card_avg_bytes = scale_card_bytes_sum as f64 / pos_n;
    let scale_card_avg_tokens = scale_card_tokens_sum as f64 / pos_n;
    let scale_savings_vs_mono_pct = (1.0 - scale_card_avg_tokens / scale_mono_tokens as f64) * 100.0;

    outcomes.push(CaseOutcome {
        category: 6,
        name: "6.2 scale_100_note_savings_and_warm_speed",
        passed: scale_savings_vs_mono_pct >= 95.0 && scale_warm_avg_ms < 15.0,
        detail: format!(
            "vs_monolithic_100={scale_savings_vs_mono_pct:.1}% (>=95%) warm_avg={scale_warm_avg_ms:.2}ms (<15ms, max={warm_max_ms:.2}ms)"
        ),
    });

    let retention_card = mistology::run_cli(
        root,
        ["mistology", "search", "신규 통화 코드 등록 src/billing/currency.py"],
    )
    .unwrap();
    let passed_retention = retention_card.contains("CURRENCY_EXPONENTS")
        && retention_card.contains("SETTLEMENT_FEE_BPS")
        && retention_card.contains("src/ledger/settlement.py")
        && !retention_card.contains("## 문제 상황");
    outcomes.push(CaseOutcome {
        category: 6,
        name: "6.3 actionable_card_retention",
        passed: passed_retention,
        detail: format!(
            "has_CURRENCY_EXPONENTS={} has_SETTLEMENT_FEE_BPS={} has_settlement_py={}",
            retention_card.contains("CURRENCY_EXPONENTS"),
            retention_card.contains("SETTLEMENT_FEE_BPS"),
            retention_card.contains("src/ledger/settlement.py"),
        ),
    });

    // Ensure source fixture directory was never polluted with index.db
    assert!(
        !shopcore_fixture_root().join(".mistology/index.db").exists(),
        "evals/shopcore/.mistology/index.db must never be written into the source tree"
    );

    let category_meta: [(usize, &str); 6] = [
        (1, "Direct Error Signature Recall@1"),
        (2, "Natural Korean Particles & Subwords Recall@3"),
        (3, "1-Hop Ontology Coupling Recall@3"),
        (4, "True-Negative Abstention Precision"),
        (5, "Version Evolution & Lifecycle Fidelity"),
        (6, "Token & Latency Efficiency"),
    ];

    assert_eq!(outcomes.len(), 24);
    let total_passed = outcomes.iter().filter(|o| o.passed).count();

    println!("\n## MISTOLOGY Evaluation Scorecard\n");
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
    let total_pct = (total_passed as f64 / 24.0) * 100.0;
    println!(
        "| **Total** | **All 6 Categories** | **{total_passed} / 24** | **{total_pct:.1}%** | **{}** |",
        if total_passed == 24 { "PASS" } else { "FAIL" }
    );

    println!("\n### Token & Latency Metrics ({} positive retrieval scenarios)\n", positive_queries.len());
    println!("| Corpus / Mode | Avg Bytes | Avg Est. Tokens | Savings vs Monolithic | Savings vs `--full` | Latency |");
    println!("|---|---:|---:|---:|---:|---:|");
    println!("| Shopcore Monolithic `MISTAKES.md` (11 active) | {mono_bytes} | {mono_est_tokens} | — | — | — |");
    println!("| Shopcore `mistology search --full` (top-3) | {full_avg_bytes:.0} | {full_avg_tokens:.1} | {:.1}% | — | — |", (1.0 - full_avg_tokens / mono_est_tokens as f64) * 100.0);
    println!("| Shopcore `mistology search` compact card (top-3) | {card_avg_bytes:.0} | {card_avg_tokens:.1} | **{savings_vs_mono_pct:.1}%** | **{savings_vs_full_pct:.1}%** | — |");
    println!("| Scale-100 Monolithic `MISTAKES.md` (99 active) | {scale_mono_bytes} | {scale_mono_tokens} | — | — | — |");
    println!(
        "| Scale-100 `mistology search` compact card (top-3) | {scale_card_avg_bytes:.0} | {scale_card_avg_tokens:.1} | **{scale_savings_vs_mono_pct:.1}%** | — | cold {scale_cold_ms:.2} ms / warm avg **{scale_warm_avg_ms:.2} ms** (max {warm_max_ms:.2} ms) |"
    );

    let failures: Vec<&CaseOutcome> = outcomes.iter().filter(|o| !o.passed).collect();
    if !failures.is_empty() {
        println!("\n### Failing Cases ({})\n", failures.len());
        for f in failures {
            println!("- **[Cat {}] `{}`** — {}", f.category, f.name, f.detail);
        }
    }
    println!();

    assert_eq!(total_passed, 24, "eval suite failed {} cases", 24 - total_passed);
}
