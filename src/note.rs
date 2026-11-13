use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use unicode_normalization::UnicodeNormalization;

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub const CURRENT: &str = "현재 정답";
pub const LIST_KEYS: [&str; 5] = ["tags", "files", "symbols", "coupled", "signatures"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub id: String,
    pub current: String,
    pub status: String,
    pub tags: Vec<String>,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
    pub coupled: Vec<String>,
    pub signatures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub problem: String,
    pub fix: String,
    pub next_step: String,
    pub pitfalls: String,
}

pub struct NewFields<'a> {
    pub problem: &'a str,
    pub tried: &'a str,
    pub evidence: &'a str,
    pub why: &'a str,
    pub fix: &'a str,
    pub verified: &'a str,
    pub next_step: &'a str,
    pub signatures: &'a [String],
    pub files: &'a [String],
    pub symbols: &'a [String],
    pub coupled: &'a [String],
    pub tags: &'a [String],
    pub force: bool,
}

pub fn today_iso() -> String {
    rusqlite::Connection::open_in_memory()
        .expect("sqlite in-memory")
        .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))
        .expect("localtime date")
}

pub fn norm_text(s: &str) -> String {
    s.replace("\r\n", "\n").nfc().collect::<String>().trim().to_string()
}

fn norm_rel_path(s: &str) -> String {
    let mut p = norm_text(s).replace('\\', "/");
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    p
}

pub fn clean_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in values {
        let cleaned = norm_text(v);
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            out.push(cleaned);
        }
    }
    out
}

fn clean_files(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in values {
        let cleaned = norm_rel_path(v);
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            out.push(cleaned);
        }
    }
    out
}

fn clean_coupled(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in values {
        let trimmed = norm_text(v);
        let cleaned = if trimmed.contains('/') || trimmed.contains('\\') {
            norm_rel_path(&trimmed)
        } else {
            trimmed
        };
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            out.push(cleaned);
        }
    }
    out
}

pub fn find_logical_root(start: &Path) -> PathBuf {
    for d in start.ancestors() {
        if d.join(".mistology").is_dir() || d.join(".git").exists() {
            return d.to_path_buf();
        }
    }
    start.to_path_buf()
}

pub fn repo_root_prefixes(root: &Path, cwd: &Path) -> Vec<String> {
    let logical_root = find_logical_root(cwd);
    let parent_canon_logical = logical_root
        .parent()
        .and_then(|p| fs::canonicalize(p).ok())
        .zip(logical_root.file_name())
        .map(|(p, name)| p.join(name));
    let mut out = Vec::new();
    for cand in [
        fs::canonicalize(root).ok(),
        Some(root.to_path_buf()),
        Some(logical_root),
        parent_canon_logical,
    ]
    .into_iter()
    .flatten()
    {
        let slash = cand
            .to_string_lossy()
            .nfc()
            .collect::<String>()
            .replace('\\', "/")
            .trim_start_matches("//?/")
            .trim_end_matches('/')
            .to_string();
        if !slash.is_empty() {
            let pref = format!("{slash}/");
            if !out.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&pref)) {
                out.push(pref);
            }
        }
    }
    out
}

fn strip_repo_root_prefix<'a>(path_slash: &'a str, prefixes: &[String]) -> Option<&'a str> {
    let trimmed = path_slash.trim_start_matches("//?/");
    for pref in prefixes {
        if let Some(rest) = trimmed.strip_prefix(pref.as_str()) {
            return Some(rest);
        }
        if trimmed.len() > pref.len()
            && trimmed.get(..pref.len()).is_some_and(|h| h.eq_ignore_ascii_case(pref))
        {
            return Some(&trimmed[pref.len()..]);
        }
    }
    None
}

pub fn rebase_cwd_paths(root: &Path, cwd: &Path, items: &[String]) -> Vec<String> {
    let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let cwd_canon = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let prefixes = repo_root_prefixes(root, cwd);
    let rel_cwd = cwd_canon
        .strip_prefix(&root_canon)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().replace('\\', "/"));

    items
        .iter()
        .map(|item| {
            let clean = norm_rel_path(item);
            if clean.is_empty() {
                return item.clone();
            }
            if let Some(stripped) = strip_repo_root_prefix(&clean, &prefixes) {
                let rel = norm_rel_path(stripped);
                if !rel.is_empty() && !rel.split('/').any(|seg| seg == ".." || seg == ".") {
                    return rel;
                }
            }
            let has_drive = clean.len() >= 2 && clean.as_bytes()[1] == b':';
            if clean.starts_with('/') || has_drive {
                if let Some(rel) = fs::canonicalize(Path::new(&clean)).ok().and_then(|p| {
                    p.strip_prefix(&root_canon)
                        .ok()
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                }) {
                    if !rel.is_empty() {
                        return rel;
                    }
                }
                return item.clone();
            }
            if let Some(ref rel_prefix) = rel_cwd {
                let prefix_slash = format!("{rel_prefix}/");
                if !clean.starts_with("@/")
                    && !clean.starts_with("~/")
                    && !clean.starts_with(&prefix_slash)
                    && (clean.contains('/') || clean.contains('.'))
                {
                    if let Some(rel) = fs::canonicalize(cwd_canon.join(&clean)).ok().and_then(|p| {
                        p.strip_prefix(&root_canon)
                            .ok()
                            .map(|r| r.to_string_lossy().replace('\\', "/"))
                    }) {
                        return rel;
                    }
                }
            }
            item.clone()
        })
        .collect()
}

pub fn find_root(start: &Path) -> PathBuf {
    let start = fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    for d in start.ancestors() {
        if d.join(".mistology").is_dir() || d.join(".git").exists() {
            return d.to_path_buf();
        }
    }
    start
}

pub fn notes_dir(root: &Path) -> Result<PathBuf, String> {
    let d = root.join(".mistology").join("notes");
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let ignore = root.join(".mistology").join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "index.db*\nevents.jsonl*\n*.tmp\n").map_err(|e| e.to_string())?;
    }
    Ok(d)
}

pub fn is_valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 14
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'-'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[11..].iter().all(|c| c.is_ascii_digit())
}

pub fn parse_version_num(current: &str) -> Option<u32> {
    let digits = current.strip_prefix('v')?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub fn render(meta: &Meta, body: &str) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("id: {}", meta.id),
        format!("current: {}", meta.current),
        format!("status: {}", meta.status),
    ];
    for (key, vals) in [
        ("tags", &meta.tags),
        ("files", &meta.files),
        ("symbols", &meta.symbols),
        ("coupled", &meta.coupled),
        ("signatures", &meta.signatures),
    ] {
        lines.push(format!("{key}:"));
        for v in vals {
            lines.push(format!("  - {}", serde_json::to_string(v).unwrap()));
        }
    }
    format!("{}\n---\n{}", lines.join("\n"), body)
}

pub fn parse(raw: &str) -> Result<(Meta, String), String> {
    let text: String = raw.replace("\r\n", "\n").nfc().collect();
    if text.lines().any(|l| {
        l == "<<<<<<<"
            || l.starts_with("<<<<<<< ")
            || l == "|||||||"
            || l.starts_with("||||||| ")
            || l == "======="
            || l == ">>>>>>>"
            || l.starts_with(">>>>>>> ")
    }) {
        return Err("해결되지 않은 Git 병합 충돌 마커가 남아 있음".to_string());
    }
    let rest = text.strip_prefix("---\n").ok_or("frontmatter 없음")?;
    let (head, body) = rest.split_once("\n---\n").ok_or("frontmatter 끝(---) 없음")?;

    let mut id = String::new();
    let mut current = String::new();
    let mut status = String::new();
    let mut lists: [Vec<String>; 5] = Default::default();
    let mut cur_list: Option<usize> = None;

    for line in head.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(json_part) = line.strip_prefix("  - ") {
            let idx = cur_list.ok_or_else(|| format!("목록 키 없이 항목이 나옴: {line}"))?;
            let val: serde_json::Value =
                serde_json::from_str(json_part).map_err(|e| format!("JSON 파싱 실패: {e}"))?;
            let s = val.as_str().ok_or_else(|| format!("{} 항목이 문자열이 아님", LIST_KEYS[idx]))?;
            lists[idx].push(s.to_string());
        } else {
            let (key, value) = line.split_once(':').ok_or_else(|| format!("잘못된 frontmatter 줄: {line}"))?;
            if !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') || key.is_empty() {
                return Err(format!("잘못된 frontmatter 키: {line}"));
            }
            if let Some(idx) = LIST_KEYS.iter().position(|&k| k == key) {
                if !value.trim().is_empty() {
                    return Err(format!("{key} 값은 줄바꿈 목록이어야 함: {line}"));
                }
                cur_list = Some(idx);
            } else {
                cur_list = None;
                let v = value.trim().to_string();
                match key {
                    "id" => id = v,
                    "current" => current = v,
                    "status" => status = v,
                    _ => return Err(format!("알 수 없는 frontmatter 키: {key}")),
                }
            }
        }
    }

    if id.is_empty() || current.is_empty() || status.is_empty() {
        return Err("필수 필드(id, current, status) 누락".to_string());
    }
    if status != "active" && status != "retired" {
        return Err(format!("status가 active|retired가 아님: {status}"));
    }
    if parse_version_num(&current).is_none() {
        return Err(format!("current 형식이 vN이 아님: {current}"));
    }
    let [tags, files, symbols, coupled, signatures] = lists;
    Ok((
        Meta { id, current, status, tags, files, symbols, coupled, signatures },
        body.to_string(),
    ))
}

pub fn read_note(path: &Path) -> Option<(Meta, String)> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mistology: {} 건너뜀 ({e})", path.file_name().unwrap_or_default().to_string_lossy());
            return None;
        }
    };
    match parse(&raw) {
        Ok(pair) => Some(pair),
        Err(e) => {
            eprintln!("mistology: {} 건너뜀 ({e})", path.file_name().unwrap_or_default().to_string_lossy());
            None
        }
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn extract_summary(body: &str) -> Summary {
    let mut problem = String::new();
    let mut fix = String::new();
    let mut next_step = String::new();
    let mut pitfalls = Vec::new();

    for block in body.split("\n## ") {
        let block = block.strip_prefix("## ").unwrap_or(block);
        let mut lines = block.lines();
        let Some(header) = lines.next() else { continue };
        if header == "문제 상황" {
            problem = one_line(&lines.collect::<Vec<_>>().join(" "));
        } else if let Some(ver) = header.split_whitespace().next().filter(|w| parse_version_num(w).is_some()) {
            let mut did = String::new();
            let mut next = String::new();
            let mut active = 0u8;
            for l in lines {
                if let Some(v) = l.strip_prefix("- 한 것: ") {
                    did = one_line(v);
                    active = 1;
                } else if let Some(v) = l.strip_prefix("- 다음에는: ") {
                    next = one_line(v);
                    active = 2;
                } else if let Some(cont) = l.strip_prefix("  ") {
                    let piece = one_line(cont);
                    if !piece.is_empty() {
                        if active == 1 {
                            did.push(' ');
                            did.push_str(&piece);
                        } else if active == 2 {
                            next.push(' ');
                            next.push_str(&piece);
                        }
                    }
                } else if l.starts_with("- ") {
                    active = 0;
                }
            }
            if header.contains(&format!("({CURRENT})")) {
                fix = did;
                next_step = next;
            } else if !did.is_empty() {
                pitfalls.push(format!("{ver}: {did}"));
            }
        }
    }
    Summary { problem, fix, next_step, pitfalls: pitfalls.join(" / ") }
}

pub fn bullets(fields: &[(&str, &str)]) -> String {
    // 여러 줄 값은 두 칸 들여 써서 값 안의 "## v.." 줄이 버전 헤더로 오인되지 않게 한다
    fields
        .iter()
        .map(|(name, val)| format!("- {name}: {}\n", norm_text(val).replace('\n', "\n  ")))
        .collect()
}

fn section_on(n: u32, label: &str, date: &str, fields: &[(&str, &str)]) -> String {
    format!("\n## v{n} ({label}) {date}\n{}", bullets(fields))
}

pub fn section(n: u32, label: &str, fields: &[(&str, &str)]) -> String {
    section_on(n, label, &today_iso(), fields)
}

pub fn new_note(root: &Path, f: NewFields) -> Result<PathBuf, String> {
    let signatures = clean_list(f.signatures);
    if signatures.is_empty() {
        return Err("signature가 최소 하나 필요함 (에러 메시지 그대로, 없으면 함수명·설정 키)".to_string());
    }
    if [f.problem, f.tried, f.evidence, f.why, f.fix, f.verified, f.next_step]
        .iter()
        .any(|v| norm_text(v).is_empty())
    {
        return Err("비어 있는 필수 항목이 있음".to_string());
    }
    let d = notes_dir(root)?;
    if !f.force {
        let sig_set: HashSet<&str> = signatures.iter().map(String::as_str).collect();
        let mut entries: Vec<PathBuf> = fs::read_dir(&d)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        entries.sort();
        for path in entries {
            if let Some((meta, _)) = read_note(&path) {
                if meta.status == "active" && meta.signatures.iter().any(|s| sig_set.contains(s.as_str())) {
                    let existing_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&meta.id);
                    return Err(format!(
                        "같은 signature의 노트가 이미 있음: {existing_id}\n같은 문제이고 현재 정답이 틀렸으면 supersede {existing_id}, 현재 정답대로 풀렸으면 기록할 것 없음.\n다른 문제면 --force"
                    ));
                }
            }
        }
    }
    let today = today_iso();
    let body = format!(
        "## 문제 상황\n{}\n{}{}",
        norm_text(f.problem).replace('\n', "\n  "),
        section_on(1, "오답", &today, &[("한 것", f.tried), ("틀린 증거", f.evidence), ("왜 틀렸나", f.why)]),
        section_on(2, CURRENT, &today, &[("한 것", f.fix), ("확인 근거", f.verified), ("다음에는", f.next_step)]),
    );
    let prefix = format!("{today}-");
    let max_seq = fs::read_dir(&d)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".md")?;
            let num = stem.strip_prefix(&prefix)?;
            num.bytes().all(|b| b.is_ascii_digit()).then(|| num.parse::<u32>().ok())?
        })
        .max()
        .unwrap_or(0);

    let mut meta = Meta {
        id: String::new(),
        current: "v2".to_string(),
        status: "active".to_string(),
        tags: clean_list(f.tags),
        files: clean_files(f.files),
        symbols: clean_list(f.symbols),
        coupled: clean_coupled(f.coupled),
        signatures,
    };
    for seq in (max_seq + 1).. {
        meta.id = format!("{today}-{seq:03}");
        let path = d.join(format!("{}.md", meta.id));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(render(&meta, &body).as_bytes()) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(e.to_string());
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    unreachable!()
}

pub const SUPERSEDED: &str = "오답, 한때 정답";

pub struct SupersedeFields<'a> {
    pub evidence: &'a str,
    pub why: &'a str,
    pub fix: &'a str,
    pub verified: &'a str,
    pub next_step: &'a str,
    pub signatures: &'a [String],
    pub files: &'a [String],
    pub symbols: &'a [String],
    pub coupled: &'a [String],
    pub tags: &'a [String],
}

fn normalize_note_id(root: &Path, raw_id: &str) -> String {
    let mut clean = norm_rel_path(raw_id);
    let prefixes = repo_root_prefixes(root, root);
    if let Some(stripped) = strip_repo_root_prefix(&clean, &prefixes) {
        clean = norm_rel_path(stripped);
    }
    if let Some(rest) = clean.strip_prefix(".mistology/notes/") {
        clean = rest.to_string();
    }
    if let Some(stem) = clean.strip_suffix(".md") {
        if !stem.contains('/') && !stem.contains("..") {
            clean = stem.to_string();
        }
    }
    clean
}

pub fn supersede(root: &Path, note_id: &str, f: SupersedeFields) -> Result<PathBuf, String> {
    let clean_id = normalize_note_id(root, note_id);
    if !is_valid_id(&clean_id) {
        return Err(format!("노트 id 형식이 아님 (예: 2026-11-11-001): {note_id}"));
    }
    let note_id = clean_id.as_str();
    if [f.evidence, f.why, f.fix, f.verified, f.next_step].iter().any(|v| norm_text(v).is_empty()) {
        return Err("비어 있는 필수 항목이 있음".to_string());
    }
    let path = root.join(".mistology").join("notes").join(format!("{note_id}.md"));
    if !path.is_file() {
        return Err(format!("노트 없음: {note_id}"));
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let (mut meta, body) = parse(&raw).map_err(|e| format!("{note_id}.md: {e}"))?;
    if meta.status != "active" {
        return Err(format!("{note_id}.md: retired 노트는 supersede할 수 없음"));
    }
    let n = parse_version_num(&meta.current).unwrap();
    let last = body.rfind("\n## v").map(|i| i + 1).unwrap_or(0);
    let expected = format!("## v{n} ({CURRENT}) ");
    if !body[last..].starts_with(&expected) {
        return Err(format!("{note_id}.md: 마지막 버전이 v{n} ({CURRENT})가 아님. 손으로 고친 곳을 먼저 확인할 것"));
    }
    let updated_tail = body[last..].replacen(&format!("({CURRENT})"), &format!("({SUPERSEDED})"), 1);
    let new_body = format!(
        "{}{}\n{}{}",
        &body[..last],
        updated_tail.trim_end_matches('\n'),
        bullets(&[("틀린 증거", f.evidence), ("왜 틀렸나", f.why)]),
        section(n + 1, CURRENT, &[("한 것", f.fix), ("확인 근거", f.verified), ("다음에는", f.next_step)]),
    );
    meta.current = format!("v{}", n + 1);
    meta.signatures = clean_list(&[meta.signatures, f.signatures.to_vec()].concat());
    meta.files = clean_files(&[meta.files, f.files.to_vec()].concat());
    meta.symbols = clean_list(&[meta.symbols, f.symbols.to_vec()].concat());
    meta.coupled = clean_coupled(&[meta.coupled, f.coupled.to_vec()].concat());
    meta.tags = clean_list(&[meta.tags, f.tags.to_vec()].concat());

    // shortcut: 같은 노트를 두 에이전트가 동시에 supersede하면 한쪽이 덮인다, 실제로 겹치면 파일 락 추가
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!("{note_id}.{}.{seq}.tmp", std::process::id()));
    if let Err(e) = fs::write(&tmp, render(&meta, &new_body)).and_then(|()| fs::rename(&tmp, &path)) {
        let _ = fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(path)
}
