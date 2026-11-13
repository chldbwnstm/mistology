use crate::{index, note};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_EVENTS_BYTES: u64 = 256 * 1024;
const MAX_QUERY_CHARS: usize = 512;

pub enum EventDetail<'a> {
    Search {
        mode: &'static str,
        k: usize,
        query: &'a str,
        hits: &'a [index::ScoredHit],
    },
    Write {
        note_id: Option<&'a str>,
    },
}

fn rel_cwd_display(root: &Path, cwd: &Path) -> String {
    let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let cwd_canon = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    match cwd_canon.strip_prefix(&root_canon) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => ".".to_string(),
    }
}

fn clean_dir_prefix(raw: &str) -> Option<String> {
    let s = raw
        .trim_start_matches(r"\\?\")
        .trim_start_matches("//?/")
        .trim_end_matches(['/', '\\']);
    if s.is_empty() || s == "/" || (s.len() == 2 && s.as_bytes()[1] == b':') {
        return None;
    }
    Some(s.to_string())
}

fn push_path_variants(out: &mut Vec<(String, &'static str)>, p: &Path, label: &'static str) {
    let parent_canon = p
        .parent()
        .and_then(|par| fs::canonicalize(par).ok())
        .zip(p.file_name())
        .map(|(par, name)| par.join(name));
    for cand in [fs::canonicalize(p).ok(), Some(p.to_path_buf()), parent_canon]
        .into_iter()
        .flatten()
    {
        if let Some(clean) = clean_dir_prefix(&cand.to_string_lossy()) {
            for variant in [clean.replace('\\', "/"), clean.replace('/', "\\")] {
                if !out
                    .iter()
                    .any(|(ex, _)| ex.eq_ignore_ascii_case(&variant))
                {
                    out.push((variant, label));
                }
            }
        }
    }
}

fn replace_path_token_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || haystack.len() < needle.len() {
        return haystack.to_string();
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0usize;
    let mut last = 0usize;
    while i + needle.len() <= haystack.len() {
        if haystack
            .get(i..i + needle.len())
            .is_some_and(|sub| sub.eq_ignore_ascii_case(needle))
        {
            let boundary_ok = haystack[i + needle.len()..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_alphanumeric() && ch != '_' && ch != '-');
            if boundary_ok {
                out.push_str(&haystack[last..i]);
                out.push_str(replacement);
                i += needle.len();
                last = i;
                continue;
            }
        }
        let step = haystack[i..].chars().next().map_or(1, char::len_utf8);
        i += step;
    }
    out.push_str(&haystack[last..]);
    out
}

fn redact_paths(s: &str, root: &Path, cwd: &Path, home: Option<&Path>) -> String {
    let mut patterns: Vec<(String, &'static str)> = Vec::new();
    push_path_variants(&mut patterns, root, "<repo>");
    let logical_root = note::find_logical_root(cwd);
    push_path_variants(&mut patterns, &logical_root, "<repo>");
    if let Some(h) = home {
        push_path_variants(&mut patterns, h, "~");
    }
    patterns.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut out = s.to_string();
    for (pat, label) in &patterns {
        out = replace_path_token_ci(&out, pat, label);
    }
    out
}

fn ensure_events_gitignore(mist_dir: &Path) {
    let gi = mist_dir.join(".gitignore");
    match fs::read_to_string(&gi) {
        Ok(existing) => {
            if !existing.lines().any(|l| l.trim() == "events.jsonl*") {
                let prefix = if existing.is_empty() || existing.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                let _ = fs::write(&gi, format!("{existing}{prefix}events.jsonl*\n"));
            }
        }
        Err(_) => {
            let _ = fs::write(&gi, "index.db*\nevents.jsonl*\n*.tmp\n");
        }
    }
}

pub fn record_event(
    root: &Path,
    cwd: &Path,
    home: Option<&Path>,
    cmd: &'static str,
    elapsed_ms: f64,
    detail: EventDetail<'_>,
    err: Option<&str>,
) {
    let mist_dir = root.join(".mistology");
    if !mist_dir.is_dir() {
        return;
    }
    ensure_events_gitignore(&mist_dir);

    let events_path = mist_dir.join("events.jsonl");
    if fs::metadata(&events_path).map(|m| m.len()).unwrap_or(0) >= MAX_EVENTS_BYTES {
        let _ = fs::rename(&events_path, mist_dir.join("events.jsonl.1"));
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ms = (elapsed_ms * 100.0).round() / 100.0;
    let cwd_rel = rel_cwd_display(root, cwd);
    let err_clean = err.map(|e| redact_paths(e.trim(), root, cwd, home));

    let entry = match detail {
        EventDetail::Search { mode, k, query, hits } => {
            let q_redacted = redact_paths(query, root, cwd, home);
            let q_trunc: String = q_redacted.chars().take(MAX_QUERY_CHARS).collect();
            let hit_items: Vec<Value> = hits
                .iter()
                .map(|sh| {
                    json!({
                        "id": sh.hit.id,
                        "score": sh.score,
                        "via": sh.hit.via,
                    })
                })
                .collect();
            json!({
                "ts": ts,
                "v": env!("CARGO_PKG_VERSION"),
                "cmd": cmd,
                "ms": ms,
                "cwd": cwd_rel,
                "mode": mode,
                "k": k,
                "q": q_trunc,
                "hits": hit_items,
                "err": err_clean,
            })
        }
        EventDetail::Write { note_id } => {
            let note_clean = note_id.map(|n| redact_paths(n, root, cwd, home));
            json!({
                "ts": ts,
                "v": env!("CARGO_PKG_VERSION"),
                "cmd": cmd,
                "ms": ms,
                "cwd": cwd_rel,
                "note": note_clean,
                "err": err_clean,
            })
        }
    };

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
        let line = format!("{entry}\n");
        let _ = f.write_all(line.as_bytes());
    }
}

fn inspect_skill_target(base: &Path, rel: &str, prefix_label: &str, embedded_skill_md: &str) -> String {
    let skill_dir = base.join(rel);
    let skill_file = skill_dir.join("SKILL.md");
    let disp = format!("{prefix_label}/{rel}/SKILL.md");

    let dir_symlink = fs::symlink_metadata(&skill_dir)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    let file_symlink = fs::symlink_metadata(&skill_file)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    if (dir_symlink && !skill_dir.is_dir()) || (file_symlink && !skill_file.is_file()) {
        return format!("  - `{disp}`: 깨진 심볼릭 링크 (`mistology setup` 권장)");
    }
    let Ok(raw) = fs::read_to_string(&skill_file) else {
        return format!("  - `{disp}`: 미설치 (`mistology setup` 권장)");
    };
    let link_note = if dir_symlink || file_symlink {
        " (심볼릭 링크)"
    } else {
        ""
    };
    if raw == embedded_skill_md {
        format!("  - `{disp}`: 최신{link_note}")
    } else {
        format!("  - `{disp}`: 갱신 필요{link_note} (`mistology setup --force` 권장)")
    }
}

fn open_index_readonly(db_path: &Path) -> rusqlite::Result<Connection> {
    if let Ok(conn) = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        if conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)).is_ok() {
            return Ok(conn);
        }
    }
    let escaped = db_path
        .to_string_lossy()
        .replace('%', "%25")
        .replace('?', "%3f")
        .replace('#', "%23");
    let uri = format!("file:{escaped}?immutable=1");
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
}

pub fn run_doctor(
    root: &Path,
    cwd: &Path,
    home: Option<&Path>,
    embedded_skill_md: &str,
    bin_in_path: bool,
) -> String {
    let mut lines = vec![
        "# MISTOLOGY Doctor".to_string(),
        format!(
            "- version: v{} ({}/{})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!(
            "- binary_in_path: {}",
            if bin_in_path { "ok" } else { "PATH 미등록" }
        ),
    ];

    let git_path = root.join(".git");
    let git_kind = if git_path.is_dir() {
        "git repo (.git/)"
    } else if git_path.is_file() {
        "git worktree (.git file)"
    } else {
        "none"
    };
    let mist_dir = root.join(".mistology");
    lines.push(format!(
        "- workspace: cwd=`{}` | vcs={git_kind} | .mistology={}",
        rel_cwd_display(root, cwd),
        if mist_dir.is_dir() { "present" } else { "uninitialized" }
    ));

    // 1. Notes health
    let notes_d = mist_dir.join("notes");
    if !notes_d.is_dir() {
        lines.push("- notes: .mistology/notes 없음 (아직 기록된 오답노트 없음)".to_string());
    } else {
        let mut md_paths: Vec<PathBuf> = fs::read_dir(&notes_d)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        md_paths.sort();

        let mut active = 0usize;
        let mut retired = 0usize;
        let mut skipped: Vec<(String, String)> = Vec::new();
        let mut id_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for p in md_paths {
            let fname = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match fs::read_to_string(&p) {
                Ok(raw) => match note::parse(&raw) {
                    Ok((meta, _)) => {
                        if meta.status == "active" {
                            active += 1;
                        } else {
                            retired += 1;
                        }
                        id_map.entry(meta.id).or_default().push(fname);
                    }
                    Err(reason) => skipped.push((fname, redact_paths(&reason, root, cwd, home))),
                },
                Err(e) => skipped.push((fname, redact_paths(&e.to_string(), root, cwd, home))),
            }
        }
        lines.push(format!(
            "- notes: active={active}, retired={retired}, skipped={}",
            skipped.len()
        ));
        for (fname, reason) in &skipped {
            lines.push(format!("  - 건너뜀 `{fname}`: {reason}"));
        }
        for (id, files) in id_map {
            if files.len() > 1 {
                lines.push(format!(
                    "  - 중복 id 감지 `{id}`: [{}]",
                    files.join(", ")
                ));
            }
        }
    }

    // 2. SQLite index health (read-only, with immutable=1 fallback for 0555 RO sandboxes)
    let db_path = mist_dir.join("index.db");
    if !db_path.is_file() {
        lines.push("- index.db: 미생성 (다음 검색 시 자동 생성)".to_string());
    } else {
        let size_kb = fs::metadata(&db_path).map(|m| m.len() as f64 / 1024.0).unwrap_or(0.0);
        match open_index_readonly(&db_path) {
            Ok(conn) => {
                let ver: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(-1);
                let qc: String = conn
                    .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
                    .unwrap_or_else(|e| {
                        format!("error: {}", redact_paths(&e.to_string(), root, cwd, home))
                    });
                let cards_cnt: i64 = conn
                    .query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0))
                    .unwrap_or(-1);
                let edges_cnt: i64 = conn
                    .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
                    .unwrap_or(-1);
                let schema_note = if ver == index::SCHEMA_VERSION {
                    format!("v{ver} (최신)")
                } else {
                    format!("v{ver} (재구축 예정 -> v{})", index::SCHEMA_VERSION)
                };
                lines.push(format!(
                    "- index.db: {qc} | schema={schema_note} | cards={cards_cnt}, edges={edges_cnt} ({size_kb:.1} KiB)"
                ));
            }
            Err(e) => {
                lines.push(format!(
                    "- index.db: 열기 실패 ({})",
                    redact_paths(&e.to_string(), root, cwd, home)
                ));
            }
        }
    }

    // 3. Flight recorder summary (redacted — never prints user queries)
    let events_path = mist_dir.join("events.jsonl");
    if !events_path.is_file() {
        lines.push("- events.jsonl: 기록 없음".to_string());
    } else {
        let size_kb = fs::metadata(&events_path).map(|m| m.len() as f64 / 1024.0).unwrap_or(0.0);
        let rotated = mist_dir.join("events.jsonl.1").is_file();
        let raw = fs::read_to_string(&events_path).unwrap_or_default();
        let mut total = 0usize;
        let mut search_cnt = 0usize;
        let mut empty_search_cnt = 0usize;
        let mut search_ms_sum = 0.0f64;
        let mut err_cnt = 0usize;
        for line in raw.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                total += 1;
                if !v["err"].is_null() {
                    err_cnt += 1;
                }
                if v["cmd"] == "search" {
                    search_cnt += 1;
                    search_ms_sum += v["ms"].as_f64().unwrap_or(0.0);
                    if v["hits"].as_array().is_some_and(|a| a.is_empty()) {
                        empty_search_cnt += 1;
                    }
                }
            }
        }
        let avg_ms = if search_cnt > 0 {
            search_ms_sum / search_cnt as f64
        } else {
            0.0
        };
        let rot_tag = if rotated { ", rotated=.1 있음" } else { "" };
        lines.push(format!(
            "- events.jsonl: 총 {total}건 (search={search_cnt}, 빈결과={empty_search_cnt}, 평균={avg_ms:.2}ms, 에러={err_cnt}) [{size_kb:.1} KiB{rot_tag}]"
        ));
    }

    // 4. Skill sync check
    lines.push("- skills:".to_string());
    let mut skill_lines = Vec::new();
    if let Some(h) = home {
        skill_lines.push(inspect_skill_target(
            h,
            ".agents/skills/mistology",
            "~",
            embedded_skill_md,
        ));
        for (probe, rel) in [
            (".claude", ".claude/skills/mistology"),
            (".gemini", ".gemini/config/skills/mistology"),
            (".gemini/skills", ".gemini/skills/mistology"),
            (".codex", ".codex/skills/mistology"),
            (".cursor", ".cursor/skills/mistology"),
        ] {
            if h.join(probe).exists() || fs::symlink_metadata(h.join(rel)).is_ok() {
                skill_lines.push(inspect_skill_target(h, rel, "~", embedded_skill_md));
            }
        }
    }
    for rel in [
        ".agents/skills/mistology",
        ".claude/skills/mistology",
        ".gemini/skills/mistology",
        ".codex/skills/mistology",
        ".cursor/skills/mistology",
    ] {
        if fs::symlink_metadata(root.join(rel)).is_ok() {
            skill_lines.push(inspect_skill_target(root, rel, "<repo>", embedded_skill_md));
        }
    }
    if skill_lines.is_empty() {
        lines.push("  - (탐지된 에이전트 스킬 경로 없음)".to_string());
    } else {
        lines.extend(skill_lines);
    }

    format!("{}\n", lines.join("\n"))
}
