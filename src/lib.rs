pub mod diag;
pub mod hook;
pub mod impact;
pub mod index;
pub mod note;

use clap::{Args, Parser, Subcommand};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

const EMBEDDED_SKILL_MD: &str = include_str!("../skills/mistology/SKILL.md");

#[derive(Parser)]
#[command(name = "mistology", about = "프로젝트 오답 온톨로지")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args)]
struct CommonWrite {
    #[arg(long, allow_hyphen_values = true, help = "틀린 증거 (에러 메시지 그대로)")]
    evidence: String,
    #[arg(long, allow_hyphen_values = true, help = "'처음부터 틀림: ...' 또는 '바뀌어서 틀려짐: ...'")]
    why: String,
    #[arg(long, allow_hyphen_values = true, help = "지금 통하는 방법")]
    fix: String,
    #[arg(long, allow_hyphen_values = true, help = "확인 근거 (통과한 테스트, 사용자 확인 등)")]
    verified: String,
    #[arg(long = "next", allow_hyphen_values = true, help = "다음에 처음부터 할 행동 한 줄")]
    next_step: String,
    #[arg(long = "signature", allow_hyphen_values = true)]
    signatures: Vec<String>,
    #[arg(long = "file", allow_hyphen_values = true)]
    files: Vec<String>,
    #[arg(long = "symbol", allow_hyphen_values = true, help = "관련 함수·타입·설정 키")]
    symbols: Vec<String>,
    #[arg(long = "coupled", allow_hyphen_values = true, help = "함께 확인·수정해야 하는 결합 파일이나 심볼")]
    coupled: Vec<String>,
    #[arg(long = "tag", allow_hyphen_values = true)]
    tags: Vec<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 관련 오답 요약 카드(또는 -b 초압축 브리핑 / --full 전문 / --explain 진단) top-k 출력
    Search {
        #[arg(required = true, allow_hyphen_values = true)]
        query: Vec<String>,
        #[arg(short = 'k')]
        k: Option<usize>,
        #[arg(short = 'b', long, conflicts_with_all = ["full", "explain"], help = "토큰 절약형 초압축 브리핑 출력 (기본 k=6)")]
        brief: bool,
        #[arg(long, conflicts_with = "explain", help = "요약 카드 대신 마크다운 노트 전문 출력")]
        full: bool,
        #[arg(long, help = "후보별 점수 계산·1/2-hop 전파·절삭 사유 진단 출력")]
        explain: bool,
    },
    /// 새 문제: v1(오답) + v2(현재 정답)
    New {
        #[arg(long, allow_hyphen_values = true, help = "무엇을 하려고 했나")]
        problem: String,
        #[arg(long, allow_hyphen_values = true, help = "처음 시도한 (틀린) 방법")]
        tried: String,
        #[arg(long, help = "같은 signature 노트가 있어도 새로 만든다")]
        force: bool,
        #[command(flatten)]
        common: CommonWrite,
    },
    /// 표준 진입점(SSOT) · 모듈 간 불변 계약 등록 (중복 구현 및 첫 충돌 사전 차단)
    Canon {
        #[arg(long, allow_hyphen_values = true, help = "어떤 기능·역할을 수행할 때 적용되는가")]
        intent: String,
        #[arg(long, allow_hyphen_values = true, help = "반드시 재사용할 표준 구현 또는 지켜야 할 불변 계약")]
        contract: String,
        #[arg(long, allow_hyphen_values = true, help = "현재 코드·테스트 확인 근거")]
        verified: String,
        #[arg(long, default_value = "", allow_hyphen_values = true, help = "금지할 중복 구현이나 위반 패턴 (선택)")]
        forbid: String,
        #[arg(long = "signature", required = true, allow_hyphen_values = true, help = "핵심 심볼·도메인 식별 구문")]
        signatures: Vec<String>,
        #[arg(long = "file", required = true, allow_hyphen_values = true, help = "표준 구현이 위치한 저장소 파일 경로")]
        files: Vec<String>,
        #[arg(long = "symbol", required = true, allow_hyphen_values = true, help = "재사용할 대표 함수·타입·상수")]
        symbols: Vec<String>,
        #[arg(long = "coupled", allow_hyphen_values = true, help = "이 규약과 결합된 파일이나 심볼")]
        coupled: Vec<String>,
        #[arg(long = "tag", allow_hyphen_values = true)]
        tags: Vec<String>,
        #[arg(long, help = "예정된 파일이거나 같은 signature가 있어도 강제로 기록")]
        force: bool,
    },
    /// 현재 정답이나 표준 규약이 바뀌었을 때: 구버전을 격리하고 새 버전을 붙인다
    Supersede {
        note_id: String,
        #[arg(long, default_value = "", allow_hyphen_values = true, help = "새 버전에서 적용할 금지 패턴 (생략 시 이전 금지 미상속)")]
        forbid: String,
        #[command(flatten)]
        common: CommonWrite,
    },
    /// 변경된 파일(기본 Git 작업트리 또는 지정 경로)과 1-hop 양방향 결합된 대상을 점검
    Impact {
        #[arg(long, conflicts_with = "paths", help = "비교 기준 Git ref (예: HEAD~1, main)")]
        base: Option<String>,
        #[arg(help = "점검할 파일 경로 (생략 시 Git 변경 파일 자동 수집)")]
        paths: Vec<String>,
    },
    /// 내장된 mistology 에이전트 스킬(SKILL.md)과 전역 자동 훅/상시 규칙을 코딩 에이전트 폴더에 자동 설치
    Setup {
        #[arg(long, help = "현재 저장소(.agents/skills/mistology)에 프로젝트 스킬로 설치")]
        project: bool,
        #[arg(long, help = "미설치 에이전트 폴더(.claude, .gemini, .codex, .cursor)까지 모두 생성")]
        all: bool,
        #[arg(long, help = "기존 심볼릭 링크도 내장 스킬 파일로 교체")]
        force: bool,
        #[arg(long, help = "전역 에이전트 훅·상시 규칙 등록은 건너뛰고 스킬만 설치")]
        no_hooks: bool,
        #[arg(
            long,
            conflicts_with_all = ["no_hooks", "project", "all"],
            help = "등록된 MISTOLOGY 전역 훅·상시 규칙만 안전하게 제거"
        )]
        uninstall_hooks: bool,
        #[arg(long, hide = true)]
        home: Option<PathBuf>,
    },
    /// 코딩 에이전트 라이프사이클 훅 핸들러 (stdin JSON -> stdout JSON, 항상 fail-open)
    Hook {
        #[arg(value_enum)]
        agent: hook::HookAgent,
        #[arg(value_enum)]
        event: Option<hook::HookEvent>,
        #[arg(long, hide = true)]
        home: Option<PathBuf>,
    },
    /// 설치·인덱스·노트 파싱·이벤트 로그 상태를 점검해 이슈 제보용 요약을 출력
    Doctor {
        #[arg(long, hide = true)]
        home: Option<PathBuf>,
    },
}

pub fn run_cli<I, T>(cwd: &Path, args: I) -> Result<String, String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) if !e.use_stderr() => return Ok(e.to_string()),
        Err(e) => return Err(e.to_string()),
    };
    let root = note::find_root(cwd);
    let default_home = resolve_default_home();
    let started = Instant::now();
    match cli.cmd {
        Cmd::Search { query, k, brief, full, explain } => {
            let limit = k.unwrap_or(if brief { 6 } else { 3 });
            let q_str = query.join(" ");
            if explain {
                let (scored, report) = index::explain_in(&root, cwd, &q_str, limit);
                diag::record_event(
                    &root,
                    cwd,
                    default_home.as_deref(),
                    "search",
                    started.elapsed().as_secs_f64() * 1000.0,
                    diag::EventDetail::Search {
                        mode: "explain",
                        k: limit,
                        query: &q_str,
                        hits: &scored,
                    },
                    None,
                );
                return Ok(report);
            }
            let scored = index::search_scored_in(&root, cwd, &q_str, limit);
            let mode = if brief {
                "brief"
            } else if full {
                "full"
            } else {
                "card"
            };
            diag::record_event(
                &root,
                cwd,
                default_home.as_deref(),
                "search",
                started.elapsed().as_secs_f64() * 1000.0,
                diag::EventDetail::Search {
                    mode,
                    k: limit,
                    query: &q_str,
                    hits: &scored,
                },
                None,
            );
            if scored.is_empty() {
                return Ok("관련 오답노트 없음\n".to_string());
            }
            let sep = if brief { "\n" } else { "\n\n" };
            let out = scored
                .iter()
                .map(|sh| {
                    let h = &sh.hit;
                    if brief {
                        index::format_brief(h)
                    } else if let Some(raw) = full.then(|| fs::read_to_string(&h.path).ok()).flatten() {
                        let rel = h
                            .path
                            .strip_prefix(&root)
                            .unwrap_or(&h.path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        format!("# {rel}\n{}", raw.trim_end_matches('\n'))
                    } else {
                        index::format_card(&root, h)
                    }
                })
                .collect::<Vec<_>>()
                .join(sep);
            Ok(format!("{out}\n"))
        }
        Cmd::New { problem, tried, force, common } => {
            let files = note::rebase_cwd_paths(&root, cwd, &common.files);
            let coupled = note::rebase_cwd_paths(&root, cwd, &common.coupled);
            let res = note::new_note(
                &root,
                note::NewFields {
                    problem: &problem,
                    tried: &tried,
                    evidence: &common.evidence,
                    why: &common.why,
                    fix: &common.fix,
                    verified: &common.verified,
                    next_step: &common.next_step,
                    signatures: &common.signatures,
                    files: &files,
                    symbols: &common.symbols,
                    coupled: &coupled,
                    tags: &common.tags,
                    force,
                },
            );
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match res {
                Ok(p) => {
                    let nid = p.file_stem().and_then(|s| s.to_str());
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "new",
                        elapsed_ms,
                        diag::EventDetail::Write { note_id: nid },
                        None,
                    );
                    let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    Ok(format!("{rel}\n"))
                }
                Err(e) => {
                    let err_code = classify_write_error(&e);
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "new",
                        elapsed_ms,
                        diag::EventDetail::Write { note_id: None },
                        Some(err_code),
                    );
                    Err(e)
                }
            }
        }
        Cmd::Canon {
            intent,
            contract,
            verified,
            forbid,
            signatures,
            files: raw_files,
            symbols,
            coupled: raw_coupled,
            tags,
            force,
        } => {
            let files = note::rebase_cwd_paths(&root, cwd, &raw_files);
            let coupled = note::rebase_cwd_paths(&root, cwd, &raw_coupled);
            let res = note::canon_note(
                &root,
                note::CanonFields {
                    intent: &intent,
                    contract: &contract,
                    verified: &verified,
                    forbid: &forbid,
                    signatures: &signatures,
                    files: &files,
                    symbols: &symbols,
                    coupled: &coupled,
                    tags: &tags,
                    force,
                },
            );
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match res {
                Ok(p) => {
                    let nid = p.file_stem().and_then(|s| s.to_str());
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "canon",
                        elapsed_ms,
                        diag::EventDetail::Write { note_id: nid },
                        None,
                    );
                    let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    Ok(format!("{rel}\n"))
                }
                Err(e) => {
                    let err_code = classify_write_error(&e);
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "canon",
                        elapsed_ms,
                        diag::EventDetail::Write { note_id: None },
                        Some(err_code),
                    );
                    Err(e)
                }
            }
        }
        Cmd::Supersede { note_id, forbid, common } => {
            let norm_id = note::rebase_cwd_paths(&root, cwd, std::slice::from_ref(&note_id))
                .into_iter()
                .next()
                .unwrap_or(note_id);
            let files = note::rebase_cwd_paths(&root, cwd, &common.files);
            let coupled = note::rebase_cwd_paths(&root, cwd, &common.coupled);
            let res = note::supersede_with_forbid(
                &root,
                &norm_id,
                note::SupersedeFields {
                    evidence: &common.evidence,
                    why: &common.why,
                    fix: &common.fix,
                    verified: &common.verified,
                    next_step: &common.next_step,
                    signatures: &common.signatures,
                    files: &files,
                    symbols: &common.symbols,
                    coupled: &coupled,
                    tags: &common.tags,
                },
                &forbid,
            );
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match res {
                Ok(p) => {
                    let nid = p.file_stem().and_then(|s| s.to_str());
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "supersede",
                        elapsed_ms,
                        diag::EventDetail::Write { note_id: nid },
                        None,
                    );
                    let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    Ok(format!("{rel}\n"))
                }
                Err(e) => {
                    let err_code = classify_write_error(&e);
                    let safe_nid = note::is_valid_id(&norm_id).then_some(norm_id.as_str());
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "supersede",
                        elapsed_ms,
                        diag::EventDetail::Write { note_id: safe_nid },
                        Some(err_code),
                    );
                    Err(e)
                }
            }
        }
        Cmd::Impact { base, paths } => {
            let res = impact::run_impact(&root, cwd, base.as_deref(), &paths);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match res {
                Ok((changed, hit_notes, report)) => {
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "impact",
                        elapsed_ms,
                        diag::EventDetail::Impact { changed, hit_notes },
                        None,
                    );
                    Ok(report)
                }
                Err(err) => {
                    diag::record_event(
                        &root,
                        cwd,
                        default_home.as_deref(),
                        "impact",
                        elapsed_ms,
                        diag::EventDetail::Impact { changed: 0, hit_notes: 0 },
                        Some(err.code),
                    );
                    Err(err.message)
                }
            }
        }
        Cmd::Setup {
            project,
            all,
            force,
            no_hooks,
            uninstall_hooks,
            home,
        } => run_setup(&root, project, all, force, no_hooks, uninstall_hooks, home),
        Cmd::Hook { agent, event, home } => {
            let mut raw = String::new();
            let _ = std::io::stdin()
                .take(1024 * 1024)
                .read_to_string(&mut raw);
            Ok(hook::run_hook(cwd, agent, event, &raw, home.as_deref()))
        }
        Cmd::Doctor { home } => {
            let eff_home = home.or(default_home);
            let bin_in_path = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(dir_on_path))
                .unwrap_or(false);
            Ok(diag::run_doctor(
                &root,
                cwd,
                eff_home.as_deref(),
                EMBEDDED_SKILL_MD,
                bin_in_path,
            ))
        }
    }
}

fn classify_write_error(err: &str) -> &'static str {
    if err.starts_with("노트 id 형식이 아님") {
        "노트 id 형식이 아님"
    } else if err.starts_with("노트 없음:") {
        "노트 없음"
    } else if err.contains("retired 노트는 supersede할 수 없음") {
        "retired 노트는 supersede할 수 없음"
    } else if err.starts_with("저장소 내부 상대 경로만 허용됨:")
        || err.starts_with("저장소 경계 밖이거나 읽을 수 없는 파일 경로:")
    {
        "invalid_or_outside_file_path"
    } else if err.starts_with("존재하지 않는 파일 경로:") {
        "missing_file_path"
    } else if err.starts_with("일반 파일이 아닌 경로:") {
        "not_regular_file"
    } else if err.contains("coupled") {
        "invalid_coupled_path"
    } else if err.contains("이미 활성 노트에 동일한 signature") {
        "duplicate_signature"
    } else if err.starts_with("signature가 최소 하나 필요함")
        || err.starts_with("비어 있는 필수 항목")
        || err.starts_with("canon 노트는")
    {
        "missing_required_field"
    } else {
        "write_error"
    }
}

fn remove_symlink_path(p: &Path) -> Result<(), String> {
    fs::remove_file(p)
        .or_else(|_| fs::remove_dir(p))
        .map_err(|e| format!("심볼릭 링크 제거 실패 ({}): {e}", p.display()))
}

fn install_skill_dir(base: &Path, rel: &str, force: bool) -> Result<(&'static str, PathBuf), String> {
    let skill_dir = base.join(rel);
    let skill_file = skill_dir.join("SKILL.md");

    // Inspect intermediate ancestor components strictly below `base` so we never write through
    // a shared ancestor symlink (e.g. `.codex/skills -> ~/dotfiles/skills`) or crash with EEXIST
    // on a dangling ancestor symlink (e.g. `.cursor/skills -> /tmp/wiped`).
    let rel_parts: Vec<&std::ffi::OsStr> = Path::new(rel).iter().collect();
    if rel_parts.len() > 1 {
        let mut cur = base.to_path_buf();
        for comp in &rel_parts[..rel_parts.len() - 1] {
            cur.push(comp);
            if let Ok(meta) = fs::symlink_metadata(&cur) {
                if meta.file_type().is_symlink() {
                    if cur.is_dir() {
                        return Ok(("유지 (상위 심볼릭 링크)", skill_file));
                    }
                    return Ok(("건너뜀 (깨진 상위 심볼릭 링크)", skill_file));
                }
            }
        }
    }

    if let Ok(meta) = fs::symlink_metadata(&skill_dir) {
        if meta.file_type().is_symlink() {
            if skill_dir.is_dir() && !force {
                return Ok(("유지 (기존 심볼릭 링크)", skill_file));
            }
            remove_symlink_path(&skill_dir)?;
        }
    }
    fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("스킬 디렉터리 생성 실패 ({}): {e}", skill_dir.display()))?;

    if let Ok(meta) = fs::symlink_metadata(&skill_file) {
        if meta.file_type().is_symlink() {
            if skill_file.is_file() && !force {
                return Ok(("유지 (기존 심볼릭 링크)", skill_file));
            }
            remove_symlink_path(&skill_file)?;
        }
    }
    fs::write(&skill_file, EMBEDDED_SKILL_MD)
        .map_err(|e| format!("스킬 파일 기록 실패 ({}): {e}", skill_file.display()))?;
    Ok(("설치 완료", skill_file))
}

fn resolve_default_home() -> Option<PathBuf> {
    let env_non_empty = |k: &str| std::env::var_os(k).filter(|s| !s.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env_non_empty("USERPROFILE").or_else(|| env_non_empty("HOME"))
    } else {
        env_non_empty("HOME").or_else(|| env_non_empty("USERPROFILE"))
    }
}

fn dir_on_path(bin_dir: &Path) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let bin_canon = fs::canonicalize(bin_dir).ok();
    std::env::split_paths(&paths).any(|p| {
        if p == bin_dir {
            return true;
        }
        if cfg!(windows) && p.to_string_lossy().eq_ignore_ascii_case(&bin_dir.to_string_lossy()) {
            return true;
        }
        matches!((&bin_canon, fs::canonicalize(&p).ok()), (Some(a), Some(b)) if a == &b)
    })
}

fn run_setup(
    root: &Path,
    project: bool,
    all: bool,
    force: bool,
    no_hooks: bool,
    uninstall_hooks: bool,
    home_override: Option<PathBuf>,
) -> Result<String, String> {
    let custom_home = home_override.is_some();
    if uninstall_hooks {
        let home = home_override
            .or_else(resolve_default_home)
            .ok_or_else(|| "홈 디렉터리(HOME/USERPROFILE)를 찾을 수 없습니다.".to_string())?;
        let lines = hook::uninstall_agent_hooks(&home)?;
        return Ok(format!("{}\n", lines.join("\n")));
    }

    let (base, rel_targets): (PathBuf, Vec<&'static str>) = if project {
        let mut rels = vec![".agents/skills/mistology"];
        for (probe, rel) in [
            (".claude", ".claude/skills/mistology"),
            (".gemini", ".gemini/skills/mistology"),
            (".codex", ".codex/skills/mistology"),
            (".cursor", ".cursor/skills/mistology"),
        ] {
            let dest = root.join(rel);
            if all || root.join(probe).exists() || fs::symlink_metadata(&dest).is_ok() {
                rels.push(rel);
            }
        }
        (root.to_path_buf(), rels)
    } else {
        let home = home_override
            .or_else(resolve_default_home)
            .ok_or_else(|| "홈 디렉터리(HOME/USERPROFILE)를 찾을 수 없습니다.".to_string())?;

        let mut rels = vec![".agents/skills/mistology"];
        for (probe, rel) in [
            (".claude", ".claude/skills/mistology"),
            (".gemini", ".gemini/config/skills/mistology"),
            (".codex", ".codex/skills/mistology"),
            (".cursor", ".cursor/skills/mistology"),
        ] {
            let dest = home.join(rel);
            if all || home.join(probe).exists() || fs::symlink_metadata(&dest).is_ok() {
                rels.push(rel);
            }
        }
        if home.join(".gemini/skills").is_dir()
            || fs::symlink_metadata(home.join(".gemini/skills/mistology")).is_ok()
        {
            rels.push(".gemini/skills/mistology");
        }
        (home, rels)
    };

    let mut lines = Vec::new();
    for rel in rel_targets {
        let (status, file_path) = install_skill_dir(&base, rel, force)?;
        lines.push(format!("[{status}] {}", file_path.display()));
    }

    if !project && !no_hooks {
        lines.extend(hook::install_agent_hooks(&base, all, custom_home)?);
    }

    if !project && !custom_home {
        if let Some(bin_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
            if !dir_on_path(&bin_dir) {
                let hint = if cfg!(windows) {
                    format!("$env:PATH = \"{};$env:PATH\"", bin_dir.display())
                } else {
                    format!("export PATH=\"{}:$PATH\"", bin_dir.display())
                };
                lines.push(format!("[안내] PATH에 바이너리 경로를 추가하세요: {hint}"));
            }
        }
    }

    Ok(format!("{}\n", lines.join("\n")))
}
