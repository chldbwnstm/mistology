use crate::note;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use unicode_normalization::UnicodeNormalization;

pub struct ImpactError {
    pub code: &'static str,
    pub message: String,
}

pub struct ImpactSummary {
    pub changed: usize,
    pub changed_paths: Vec<String>,
    pub hit_notes: usize,
    pub unchecked_count: usize,
    pub report: String,
}

impl ImpactError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

fn is_internal_meta_path(rel_slash: &str) -> bool {
    rel_slash
        .split('/')
        .any(|seg| seg == ".mistology" || seg == ".git")
}

fn map_git_rel_to_root(
    git_rel: &str,
    root_in_git: Option<&str>,
    git_in_root: Option<&str>,
) -> Option<String> {
    let norm: String = git_rel
        .replace('\\', "/")
        .trim_start_matches("./")
        .nfc()
        .collect();
    if norm.is_empty() {
        return None;
    }
    let mapped = if let Some(sub) = root_in_git {
        let pref = format!("{sub}/");
        norm.strip_prefix(&pref)?.to_string()
    } else if let Some(nested) = git_in_root {
        format!("{nested}/{norm}")
    } else {
        norm
    };
    if mapped.is_empty() || is_internal_meta_path(&mapped) {
        None
    } else {
        Some(mapped)
    }
}

fn resolve_git_toplevel(root: &Path, cwd: &Path) -> Result<PathBuf, ImpactError> {
    for probe in [cwd, root] {
        if let Ok(out) = Command::new("git")
            .arg("-C")
            .arg(probe)
            .args(["rev-parse", "--show-toplevel"])
            .output()
        {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !raw.is_empty() {
                    return Ok(PathBuf::from(raw));
                }
            }
        }
    }
    Err(ImpactError::new(
        "not_git_repo",
        "Git 저장소가 아닙니다. 점검할 파일 경로를 직접 지정하세요 (예: mistology impact src/app.rs)",
    ))
}

fn collect_git_paths(
    root: &Path,
    cwd: &Path,
    base: Option<&str>,
) -> Result<Vec<String>, ImpactError> {
    let git_top = resolve_git_toplevel(root, cwd)?;
    let git_canon = fs::canonicalize(&git_top).unwrap_or_else(|_| git_top.clone());
    let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let root_in_git = root_canon
        .strip_prefix(&git_canon)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let git_in_root = if root_in_git.is_none() {
        git_canon
            .strip_prefix(&root_canon)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    } else {
        None
    };

    let mut raw_git_paths: Vec<String> = Vec::new();

    if let Some(base_ref) = base {
        let trimmed = base_ref.trim();
        if trimmed.is_empty() || trimmed.starts_with('-') {
            return Err(ImpactError::new(
                "invalid_base_ref",
                format!("유효하지 않은 Git 기준(ref)입니다: {base_ref}"),
            ));
        }
        let verify = Command::new("git")
            .arg("-C")
            .arg(&git_top)
            .args(["rev-parse", "--verify", "--quiet", &format!("{trimmed}^{{commit}}")])
            .output()
            .map_err(|e| ImpactError::new("git_error", format!("git 실행 실패: {e}")))?;
        if !verify.status.success() {
            return Err(ImpactError::new(
                "invalid_base_ref",
                format!("유효하지 않은 Git 기준(ref)입니다: {trimmed}"),
            ));
        }

        let diff_out = Command::new("git")
            .arg("-C")
            .arg(&git_top)
            .args(["diff", "--name-status", "-z", "--find-renames", trimmed, "--"])
            .output()
            .map_err(|e| ImpactError::new("git_error", format!("git diff 실행 실패: {e}")))?;
        if !diff_out.status.success() {
            return Err(ImpactError::new(
                "git_error",
                format!("git diff 실패: {}", String::from_utf8_lossy(&diff_out.stderr).trim()),
            ));
        }

        let toks: Vec<&[u8]> = diff_out.stdout.split(|&b| b == 0).collect();
        let mut i = 0usize;
        while i < toks.len() {
            if toks[i].is_empty() {
                i += 1;
                continue;
            }
            let status = String::from_utf8_lossy(toks[i]);
            if status.starts_with('R') || status.starts_with('C') {
                if i + 2 < toks.len() {
                    raw_git_paths.push(String::from_utf8_lossy(toks[i + 1]).into_owned());
                    raw_git_paths.push(String::from_utf8_lossy(toks[i + 2]).into_owned());
                }
                i += 3;
            } else {
                if i + 1 < toks.len() {
                    raw_git_paths.push(String::from_utf8_lossy(toks[i + 1]).into_owned());
                }
                i += 2;
            }
        }

        let untracked_out = Command::new("git")
            .arg("-C")
            .arg(&git_top)
            .args(["ls-files", "--others", "--exclude-standard", "-z"])
            .output()
            .map_err(|e| ImpactError::new("git_error", format!("git ls-files 실행 실패: {e}")))?;
        if !untracked_out.status.success() {
            return Err(ImpactError::new(
                "git_error",
                format!(
                    "git ls-files 실패: {}",
                    String::from_utf8_lossy(&untracked_out.stderr).trim()
                ),
            ));
        }
        for tok in untracked_out.stdout.split(|&b| b == 0) {
            if !tok.is_empty() {
                raw_git_paths.push(String::from_utf8_lossy(tok).into_owned());
            }
        }
    } else {
        let st_out = Command::new("git")
            .arg("-C")
            .arg(&git_top)
            .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
            .output()
            .map_err(|e| ImpactError::new("git_error", format!("git status 실행 실패: {e}")))?;
        if !st_out.status.success() {
            return Err(ImpactError::new(
                "git_error",
                format!(
                    "git status 실패: {}",
                    String::from_utf8_lossy(&st_out.stderr).trim()
                ),
            ));
        }

        let toks: Vec<&[u8]> = st_out.stdout.split(|&b| b == 0).collect();
        let mut i = 0usize;
        while i < toks.len() {
            let entry = toks[i];
            if entry.len() < 4 {
                i += 1;
                continue;
            }
            let x = entry[0];
            let y = entry[1];
            if x == b'!' && y == b'!' {
                i += 1;
                continue;
            }
            raw_git_paths.push(String::from_utf8_lossy(&entry[3..]).into_owned());
            if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
                if i + 1 < toks.len() && !toks[i + 1].is_empty() {
                    raw_git_paths.push(String::from_utf8_lossy(toks[i + 1]).into_owned());
                }
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    let mut set = BTreeSet::new();
    for raw in raw_git_paths {
        if let Some(rel) =
            map_git_rel_to_root(&raw, root_in_git.as_deref(), git_in_root.as_deref())
        {
            set.insert(rel);
        }
    }
    Ok(set.into_iter().collect())
}

const GENERIC_BASENAMES: &[&str] = &[
    "mod.rs",
    "lib.rs",
    "main.rs",
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "__init__.py",
    "package.json",
    "cargo.toml",
    "pyproject.toml",
];

fn register_note_alias(map: &mut HashMap<String, Option<String>>, alias: String, canonical: &str) {
    if alias.is_empty() || alias == canonical {
        return;
    }
    map.entry(alias)
        .and_modify(|slot| {
            if slot.as_deref() != Some(canonical) {
                *slot = None;
            }
        })
        .or_insert_with(|| Some(canonical.to_string()));
}

fn is_canonical_path_shape(p: &str) -> bool {
    p.contains('/')
        && !p.starts_with("@/")
        && !p.starts_with("~/")
        && !p.contains("::")
        && p.rsplit_once('/')
            .is_some_and(|(_, base)| base.rsplit_once('.').is_some_and(|(st, ext)| !st.is_empty() && !ext.is_empty()))
}

fn is_coupled_file_entry(raw_c: &str, resolved_c: &str, canonical_paths: &HashSet<String>) -> bool {
    if canonical_paths.contains(resolved_c) || is_canonical_path_shape(resolved_c) {
        return true;
    }
    let slash = raw_c.replace('\\', "/");
    if slash.starts_with("@/") || slash.starts_with("~/") || slash.contains("::") {
        return false;
    }
    if let Some((_, base)) = slash.rsplit_once('/') {
        return base
            .rsplit_once('.')
            .is_some_and(|(st, ext)| !st.is_empty() && ext.bytes().any(|b| b.is_ascii_alphabetic()));
    }
    const KNOWN_EXTS: &[&str] = &[
        "py", "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "java", "kt", "swift", "c", "cc",
        "cpp", "h", "hpp", "rb", "php", "cs", "sql", "toml", "yaml", "yml", "json", "md", "sh",
        "css", "scss", "html", "vue", "svelte",
    ];
    slash
        .rsplit_once('.')
        .is_some_and(|(st, ext)| !st.is_empty() && KNOWN_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn probe_alias_in_repo(root: &Path, raw_alias: &str) -> Option<String> {
    let rest = raw_alias
        .strip_prefix("@/")
        .or_else(|| raw_alias.strip_prefix("~/"))?;
    if !note::is_safe_repo_rel_path(rest) {
        return None;
    }
    let mut hits = Vec::new();
    let direct = format!("src/{rest}");
    if note::inspect_repo_file(root, &direct) == note::RepoFileCheck::Present {
        hits.push(direct);
    }
    if !rest.rsplit('/').next().unwrap_or(rest).contains('.') {
        for ext in ["ts", "tsx", "js", "jsx", "mjs", "vue", "svelte"] {
            let cand = format!("src/{rest}.{ext}");
            if note::inspect_repo_file(root, &cand) == note::RepoFileCheck::Present {
                hits.push(cand);
            }
        }
    }
    (hits.len() == 1).then(|| hits.pop().unwrap())
}

struct LoadedNote {
    id: String,
    meta: note::Meta,
    summary: note::Summary,
}

struct ImpactRow {
    target_disp: String,
    note_id: String,
    kind: String,
    current: String,
    dir_label: String,
    coupled_syms: String,
    fix: String,
    forbid: String,
}

fn evaluate_impact_on_notes(
    root: &Path,
    changed_rel_paths: &[String],
) -> Result<(usize, usize, String), ImpactError> {
    if changed_rel_paths.is_empty() {
        return Ok((0, 0, "변경된 파일 없음\n".to_string()));
    }
    let d = root.join(".mistology").join("notes");
    let entries = match fs::read_dir(&d) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                0,
                0,
                format!("결합 점검 이상 없음 (변경 파일 {}개)\n", changed_rel_paths.len()),
            ));
        }
        Err(e) => {
            return Err(ImpactError::new(
                "notes_io_error",
                format!("노트 디렉터리 읽기 실패: {e}"),
            ));
        }
    };
    let mut md_paths: Vec<PathBuf> = Vec::new();
    for entry_res in entries {
        let entry = entry_res
            .map_err(|e| ImpactError::new("notes_io_error", format!("노트 목록 읽기 실패: {e}")))?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("md") {
            md_paths.push(p);
        }
    }
    md_paths.sort();

    let changed_set: HashSet<&str> = changed_rel_paths.iter().map(String::as_str).collect();
    let mut canonical_paths: HashSet<String> = changed_rel_paths.iter().cloned().collect();
    let mut active_notes: Vec<LoadedNote> = Vec::new();

    let is_real_root_file = |cand: &str, is_canon_primary: bool| -> bool {
        !cand.contains('/')
            && note::is_safe_repo_rel_path(cand)
            && !cand.contains("::")
            && (is_canon_primary
                || changed_set.contains(cand)
                || note::inspect_repo_file(root, cand) == note::RepoFileCheck::Present)
    };

    for p in md_paths {
        let fname = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let raw = fs::read_to_string(&p)
            .map_err(|e| ImpactError::new("notes_io_error", format!("{fname} 읽기 실패: {e}")))?;
        let Ok((meta, body)) = note::parse(&raw) else {
            continue;
        };
        if meta.status != "active" {
            continue;
        }
        let is_canon = meta.kind == "canon";
        for f in &meta.files {
            if f.contains('/') || is_real_root_file(f, is_canon) {
                canonical_paths.insert(f.clone());
            }
        }
        for c in &meta.coupled {
            if is_canonical_path_shape(c) || is_real_root_file(c, false) {
                canonical_paths.insert(c.clone());
            } else if let Some(probed) = probe_alias_in_repo(root, c) {
                canonical_paths.insert(probed);
            }
        }
        let id = fname.strip_suffix(".md").unwrap_or(&meta.id).to_string();
        let summary = note::extract_summary(&body);
        active_notes.push(LoadedNote { id, meta, summary });
    }

    // Build note-side alias table (resolves shorthand in notes -> canonical repo paths; NEVER mutates changed_set!)
    let mut alias_map: HashMap<String, Option<String>> = HashMap::new();
    // Block actual root-level files (e.g. `config.ts`) from being hijacked by nested `src/config.ts` basename aliases!
    for canon in &canonical_paths {
        if !canon.contains('/') {
            alias_map.insert(canon.clone(), None);
            if let Some((stem, _)) = canon.rsplit_once('.') {
                alias_map.insert(stem.to_string(), None);
            }
        }
    }
    for canon in &canonical_paths {
        if !is_canonical_path_shape(canon) {
            continue;
        }
        if let Some((_, base)) = canon.rsplit_once('/') {
            if !GENERIC_BASENAMES.contains(&base.to_ascii_lowercase().as_str()) {
                register_note_alias(&mut alias_map, base.to_string(), canon);
            }
        }
        if let Some((stem_path, _)) = canon.rsplit_once('.') {
            register_note_alias(&mut alias_map, stem_path.to_string(), canon);
        }
        let mut src_rels = Vec::with_capacity(2);
        if let Some(rest) = canon.strip_prefix("src/") {
            src_rels.push(rest);
        }
        if let Some((_, rest)) = canon.rsplit_once("/src/") {
            src_rels.push(rest);
        }
        for rest in src_rels {
            register_note_alias(&mut alias_map, format!("@/{rest}"), canon);
            register_note_alias(&mut alias_map, format!("~/{rest}"), canon);
            if let Some((rest_stem, _)) = rest.rsplit_once('.') {
                register_note_alias(&mut alias_map, format!("@/{rest_stem}"), canon);
                register_note_alias(&mut alias_map, format!("~/{rest_stem}"), canon);
            }
        }
    }

    let resolve_note_entry = |raw_entry: &str| -> String {
        if canonical_paths.contains(raw_entry) {
            return raw_entry.to_string();
        }
        alias_map
            .get(raw_entry)
            .and_then(|opt| opt.clone())
            .unwrap_or_else(|| raw_entry.to_string())
    };

    let mut matched_note_ids: HashSet<String> = HashSet::new();
    let mut seen_pairs: HashSet<(String, String)> = HashSet::new();
    let mut seen_comod_notes: HashSet<String> = HashSet::new();
    let mut unchecked: Vec<ImpactRow> = Vec::new();
    let mut comodified: Vec<ImpactRow> = Vec::new();
    let mut sym_only: Vec<ImpactRow> = Vec::new();

    for ln in active_notes {
        let note_id = ln.id;
        let kind = ln.meta.kind;
        let current = ln.meta.current;
        let fix = ln.summary.fix;
        let forbid = ln.summary.forbid;

        let note_files: Vec<(String, String)> = ln
            .meta
            .files
            .iter()
            .map(|f| (f.clone(), resolve_note_entry(f)))
            .collect();

        let mut coupled_files: Vec<(String, String)> = Vec::new();
        let mut coupled_syms: Vec<String> = Vec::new();
        for c in &ln.meta.coupled {
            let res = resolve_note_entry(c);
            if is_coupled_file_entry(c, &res, &canonical_paths) {
                coupled_files.push((c.clone(), res));
            } else {
                coupled_syms.push(c.clone());
            }
        }
        for s in &ln.meta.symbols {
            if !coupled_syms.iter().any(|ex| ex == s) {
                coupled_syms.push(s.clone());
            }
        }
        let sym_disp = coupled_syms.join(", ");

        let matched_src: Vec<(&str, &str)> = note_files
            .iter()
            .filter(|(_, res)| changed_set.contains(res.as_str()))
            .map(|(orig, res)| (orig.as_str(), res.as_str()))
            .collect();
        let matched_cpl: Vec<(&str, &str)> = coupled_files
            .iter()
            .filter(|(_, res)| changed_set.contains(res.as_str()))
            .map(|(orig, res)| (orig.as_str(), res.as_str()))
            .collect();

        let has_distinct_comod_pair = matched_src
            .iter()
            .any(|(_, sr)| matched_cpl.iter().any(|(_, cr)| sr != cr));

        if has_distinct_comod_pair && seen_comod_notes.insert(note_id.clone()) {
            matched_note_ids.insert(note_id.clone());
            let src_names: Vec<&str> = matched_src.iter().map(|(o, _)| *o).collect();
            let cpl_names: Vec<&str> = matched_cpl
                .iter()
                .filter(|(_, cr)| !(matched_src.len() == 1 && matched_src[0].1 == *cr))
                .map(|(o, _)| *o)
                .collect();
            comodified.push(ImpactRow {
                target_disp: format!("{} ↔ {}", src_names.join(", "), cpl_names.join(", ")),
                note_id: note_id.clone(),
                kind: kind.clone(),
                current: current.clone(),
                dir_label: "양측 변경됨".to_string(),
                coupled_syms: sym_disp.clone(),
                fix: fix.clone(),
                forbid: forbid.clone(),
            });
        }

        if !matched_src.is_empty() {
            let src_label = matched_src.iter().map(|(o, _)| *o).collect::<Vec<_>>().join(", ");
            let mut emitted_non_self_file = false;
            for (orig_c, res_c) in &coupled_files {
                if matched_src.len() == 1 && matched_src[0].1 == res_c.as_str() {
                    continue;
                }
                emitted_non_self_file = true;
                if changed_set.contains(res_c.as_str()) {
                    continue;
                }
                if !seen_pairs.insert((note_id.clone(), res_c.clone())) {
                    continue;
                }
                matched_note_ids.insert(note_id.clone());
                unchecked.push(ImpactRow {
                    target_disp: orig_c.clone(),
                    note_id: note_id.clone(),
                    kind: kind.clone(),
                    current: current.clone(),
                    dir_label: format!("{src_label} → {orig_c}"),
                    coupled_syms: sym_disp.clone(),
                    fix: fix.clone(),
                    forbid: forbid.clone(),
                });
            }
            if !emitted_non_self_file && !coupled_syms.is_empty() && !has_distinct_comod_pair {
                matched_note_ids.insert(note_id.clone());
                sym_only.push(ImpactRow {
                    target_disp: src_label,
                    note_id: note_id.clone(),
                    kind: kind.clone(),
                    current: current.clone(),
                    dir_label: "심볼/규약 확인".to_string(),
                    coupled_syms: sym_disp.clone(),
                    fix: fix.clone(),
                    forbid: forbid.clone(),
                });
            }
        }

        if !matched_cpl.is_empty() {
            let cpl_label = matched_cpl.iter().map(|(o, _)| *o).collect::<Vec<_>>().join(", ");
            for (orig_f, res_f) in &note_files {
                if matched_cpl.len() == 1 && matched_cpl[0].1 == res_f.as_str() {
                    continue;
                }
                if changed_set.contains(res_f.as_str()) {
                    continue;
                }
                if !seen_pairs.insert((note_id.clone(), res_f.clone())) {
                    continue;
                }
                matched_note_ids.insert(note_id.clone());
                unchecked.push(ImpactRow {
                    target_disp: orig_f.clone(),
                    note_id: note_id.clone(),
                    kind: kind.clone(),
                    current: current.clone(),
                    dir_label: format!("{cpl_label} 변경 ← {orig_f}"),
                    coupled_syms: sym_disp.clone(),
                    fix: fix.clone(),
                    forbid: forbid.clone(),
                });
            }
        }
    }

    if matched_note_ids.is_empty() {
        return Ok((
            0,
            0,
            format!("결합 점검 이상 없음 (변경 파일 {}개)\n", changed_rel_paths.len()),
        ));
    }

    let fmt_row = |r: &ImpactRow| -> String {
        let badge = if r.kind == "canon" { " ★표준" } else { "" };
        let rule_lbl = if r.kind == "canon" { "표준 규약" } else { "정답" };
        let sym_part = if r.coupled_syms.is_empty() {
            String::new()
        } else {
            format!(" | 결합 심볼: {}", r.coupled_syms)
        };
        let mut out = format!(
            "- {} [{} {}{badge} | {}{sym_part}]\n  - {rule_lbl}: {}",
            r.target_disp, r.note_id, r.current, r.dir_label, r.fix
        );
        if !r.forbid.is_empty() {
            out.push_str(&format!("\n  - 금지: {}", r.forbid));
        }
        out
    };

    let mut lines = vec![format!(
        "# MISTOLOGY Impact (변경 파일 {}개, 관련 노트 {}개)",
        changed_rel_paths.len(),
        matched_note_ids.len()
    )];

    if !unchecked.is_empty() {
        lines.push(String::new());
        lines.push(format!("## [미수정 결합 파일 · 점검 필요] ({}건)", unchecked.len()));
        for r in &unchecked {
            lines.push(fmt_row(r));
        }
    }
    if !comodified.is_empty() {
        lines.push(String::new());
        lines.push(format!("## [함께 수정됨 · 규약 확인] ({}건)", comodified.len()));
        for r in &comodified {
            lines.push(fmt_row(r));
        }
    }
    if !sym_only.is_empty() {
        lines.push(String::new());
        lines.push(format!("## [결합 심볼 · 규약 확인] ({}건)", sym_only.len()));
        for r in &sym_only {
            lines.push(fmt_row(r));
        }
    }

    Ok((matched_note_ids.len(), unchecked.len(), format!("{}\n", lines.join("\n"))))
}

pub fn run_impact_detailed(
    root: &Path,
    cwd: &Path,
    base: Option<&str>,
    explicit_paths: &[String],
) -> Result<ImpactSummary, ImpactError> {
    let changed: Vec<String> = if !explicit_paths.is_empty() {
        let mut set = BTreeSet::new();
        for raw in explicit_paths {
            let Some(rel) = note::lexical_rebase_path(root, cwd, raw) else {
                return Err(ImpactError::new(
                    "outside_repo",
                    format!("저장소 내부 경로로 해석할 수 없음: {raw}"),
                ));
            };
            if !is_internal_meta_path(&rel) {
                set.insert(rel);
            }
        }
        set.into_iter().collect()
    } else {
        collect_git_paths(root, cwd, base)?
    };

    let changed_cnt = changed.len();
    let (hit_notes, unchecked_count, report) = evaluate_impact_on_notes(root, &changed)?;
    Ok(ImpactSummary {
        changed: changed_cnt,
        changed_paths: changed,
        hit_notes,
        unchecked_count,
        report,
    })
}

pub fn run_impact(
    root: &Path,
    cwd: &Path,
    base: Option<&str>,
    explicit_paths: &[String],
) -> Result<(usize, usize, String), ImpactError> {
    let s = run_impact_detailed(root, cwd, base, explicit_paths)?;
    Ok((s.changed, s.hit_notes, s.report))
}
