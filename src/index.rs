use crate::note::{extract_summary, parse as parse_note, parse_version_num, repo_root_prefixes};
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_normalization::UnicodeNormalization;

const SCHEMA_VERSION: i32 = 3;

const SCHEMA: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
    id UNINDEXED, signatures, entities, tags, body
);
CREATE TABLE IF NOT EXISTS cards (
    id TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    mtime INTEGER NOT NULL,
    ctime INTEGER NOT NULL,
    size INTEGER NOT NULL,
    hash INTEGER NOT NULL,
    status TEXT NOT NULL,
    current TEXT NOT NULL,
    wrong INTEGER NOT NULL,
    problem TEXT NOT NULL,
    fix TEXT NOT NULL,
    next_step TEXT NOT NULL,
    pitfalls TEXT NOT NULL,
    files TEXT NOT NULL,
    symbols TEXT NOT NULL,
    coupled TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS edges (
    note_id TEXT NOT NULL,
    entity TEXT NOT NULL,
    role TEXT NOT NULL,
    raw_entity TEXT NOT NULL,
    PRIMARY KEY (note_id, raw_entity, role)
);
CREATE INDEX IF NOT EXISTS idx_edges_entity ON edges(entity);
CREATE INDEX IF NOT EXISTS idx_edges_raw ON edges(raw_entity);
";

const KO_PARTICLES: &[&str] = &[
    "으로부터", "에서는", "으로는", "에게서", "로부터", "까지도", "에서", "으로", "에게", "부터", "까지", "에는",
    "에도", "이나", "이라", "처럼", "보다", "마다", "은", "는", "이", "가", "을", "를", "의", "에",
    "로", "와", "과", "도", "만",
];

const NOISE_WORDS: &[&str] = &[
    "src", "lib", "app", "pkg", "crates", "tests", "test", "py", "rs", "ts", "tsx", "js", "jsx",
    "mjs", "cjs", "go", "md", "json", "toml", "yaml", "yml", "sql", "error", "invalid", "failed",
    "must", "be", "or", "in", "for", "to", "of", "on", "at", "by", "with", "from", "the", "is",
    "not", "and", "추가", "수정", "변경", "점검", "작성", "조정", "파일",
];

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

const PATH_PREFIXES: &[&str] = &[
    "./", "../", "@/", "~/", "/", "a/", "b/", "src/", "lib/", "app/", "apps/", "packages/",
    "crates/", "tests/", "evals/", "pkg/",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: String,
    pub path: PathBuf,
    pub current: String,
    pub wrong: i64,
    pub problem: String,
    pub fix: String,
    pub next_step: String,
    pub pitfalls: String,
    pub files: String,
    pub symbols: String,
    pub coupled: String,
    pub via: Option<String>,
}

#[derive(Clone, Copy)]
struct DiskStat {
    mtime: i64,
    ctime: i64,
    size: i64,
}

fn disk_stat(path: &Path) -> DiskStat {
    let Ok(m) = fs::metadata(path) else {
        return DiskStat { mtime: 0, ctime: 0, size: 0 };
    };
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    #[cfg(unix)]
    let ctime = {
        use std::os::unix::fs::MetadataExt;
        (m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128) as i64
    };
    #[cfg(not(unix))]
    let ctime = 0i64;
    DiskStat { mtime, ctime, size: m.len() as i64 }
}

fn fnv1a(bytes: &[u8]) -> i64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h as i64
}

fn strip_line_col_suffix(s: &str) -> &str {
    let mut cur = s;
    if let Some(open) = cur.rfind('(').filter(|&i| i > 0).filter(|&i| {
        cur[i + 1..].strip_suffix(')').is_some_and(|inside| {
            !inside.is_empty()
                && inside.bytes().any(|b| b.is_ascii_digit())
                && inside.bytes().all(|b| b.is_ascii_digit() || b == b',' || b == b' ')
        })
    }) {
        cur = &cur[..open];
    }
    while let Some((before, after)) = cur.rsplit_once(':') {
        if !before.is_empty()
            && !before.ends_with(':')
            && !after.is_empty()
            && after.bytes().all(|b| b.is_ascii_digit())
        {
            cur = before;
        } else {
            break;
        }
    }
    cur
}

fn norm_entity(s: &str) -> String {
    let mut cur = s
        .replace('\\', "/")
        .trim_matches(|c: char| {
            matches!(
                c,
                ' ' | '\t'
                    | '\n'
                    | '\r'
                    | '"'
                    | '\''
                    | '`'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ','
                    | ';'
                    | ':'
                    | '<'
                    | '>'
            )
        })
        .trim_end_matches('.')
        .to_string();
    while cur.starts_with('(') && cur.ends_with(')') && cur.len() >= 2 {
        cur = cur[1..cur.len() - 1].to_string();
    }
    let stripped = strip_line_col_suffix(&cur)
        .trim_matches(|c: char| matches!(c, '(' | ')' | ':'))
        .trim_end_matches('.');
    let mut slice = stripped.trim_start_matches("//?/");
    while let Some(rest) = slice.strip_prefix("./") {
        slice = rest;
    }
    slice.to_lowercase()
}

fn is_hangul(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

fn korean_stem(word: &str) -> &str {
    for &p in KO_PARTICLES {
        if let Some(stem) = word.strip_suffix(p) {
            if stem.chars().count() >= 2
                && stem.chars().last().is_some_and(|c| is_hangul(c) || c.is_ascii_alphanumeric())
            {
                return stem;
            }
        }
    }
    word
}

fn camel_subwords(tok: &str) -> Vec<String> {
    if !tok.is_ascii() || tok.len() < 3 {
        return Vec::new();
    }
    let bytes = tok.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..bytes.len() {
        let prev = bytes[i - 1] as char;
        let curr = bytes[i] as char;
        let next = bytes.get(i + 1).map(|&b| b as char);
        let split = (prev.is_ascii_lowercase() && curr.is_ascii_uppercase())
            || ((prev.is_ascii_uppercase() || prev.is_ascii_digit())
                && curr.is_ascii_uppercase()
                && next.is_some_and(|n| n.is_ascii_lowercase()));
        if split {
            parts.push(tok[start..i].to_ascii_lowercase());
            start = i;
        }
    }
    if parts.is_empty() {
        return Vec::new();
    }
    parts.push(tok[start..].to_ascii_lowercase());
    parts.retain(|p| p.len() >= 2);
    parts
}

fn expand_subwords(text: &str) -> String {
    let mut extra = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let tok = &text[start..i];
            extra.extend(camel_subwords(tok));
            if tok.len() >= 2 && text[i..].chars().next().is_some_and(is_hangul) {
                extra.push(tok.to_ascii_lowercase());
            }
        } else {
            i += 1;
        }
    }
    if extra.is_empty() {
        text.to_string()
    } else {
        format!("{text}\n{}", extra.join(" "))
    }
}

fn has_stem_and_ext(base: &str) -> bool {
    base.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
}

fn is_bare_filename(ent: &str) -> bool {
    !ent.contains('/') && has_stem_and_ext(ent) && !GENERIC_BASENAMES.contains(&ent)
}

fn is_canonical_file_path(p: &str) -> bool {
    p.contains('/')
        && !p.starts_with("@/")
        && !p.starts_with("~/")
        && p.rsplit_once('/').is_some_and(|(_, base)| has_stem_and_ext(base))
}

fn is_path_chunk(chunk: &str) -> bool {
    let trimmed = chunk
        .trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '<' | '>'
            )
        })
        .trim_end_matches('.')
        .replace('\\', "/");
    let s = strip_line_col_suffix(&trimmed).trim_start_matches("//?/");
    let Some((dir_part, last)) = s.rsplit_once('/') else {
        return false;
    };
    if last.is_empty() || last.chars().any(char::is_whitespace) {
        return false;
    }
    let has_drive = s.len() >= 3
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.as_bytes()[1] == b':'
        && s.as_bytes()[2] == b'/';
    if dir_part.chars().any(char::is_whitespace) {
        return (has_drive || s.starts_with('/') || s.starts_with("./") || s.starts_with("../"))
            && last.contains('.')
            && !last.starts_with('.');
    }
    has_drive
        || PATH_PREFIXES.iter().any(|p| s.starts_with(p))
        || (last.contains('.') && !last.starts_with('.'))
}

fn find_closing_quote(rest: &str, quote: char) -> Option<usize> {
    let line = rest.split('\n').next().unwrap_or(rest);
    if quote != '\'' {
        return line.find(quote);
    }
    let bytes = line.as_bytes();
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b'\'' && !bytes.get(idx + 1).is_some_and(u8::is_ascii_alphabetic) {
            return Some(idx);
        }
    }
    None
}

fn is_apostrophe_or_lifetime(prev_alnum: bool, rest: &str) -> bool {
    if prev_alnum {
        return true;
    }
    let ident_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    ident_len > 0
        && !matches!(
            rest.as_bytes().get(ident_len),
            Some(b'\'' | b'/' | b'\\' | b'.' | b':')
        )
}

fn scan_quoted_spans(s: &str) -> (Vec<String>, String, bool) {
    let mut spans = Vec::new();
    let mut cleaned = String::with_capacity(s.len());
    let mut saw_path = false;
    let mut prev_alnum = false;
    let mut chars = s.char_indices().peekable();
    while let Some((start_idx, ch)) = chars.next() {
        if matches!(ch, '"' | '\'' | '`') {
            let rest = &s[start_idx + ch.len_utf8()..];
            if !(ch == '\'' && is_apostrophe_or_lifetime(prev_alnum, rest)) {
                if let Some(end_rel) = find_closing_quote(rest, ch) {
                    let inner = &rest[..end_rel];
                    if !inner.is_empty() {
                        spans.push(inner.to_string());
                        let advance_to = start_idx + ch.len_utf8() + end_rel + ch.len_utf8();
                        while chars.peek().is_some_and(|&(idx, _)| idx < advance_to) {
                            chars.next();
                        }
                        if is_path_chunk(inner) {
                            saw_path = true;
                            cleaned.push(' ');
                        } else {
                            cleaned.push_str(&s[start_idx..advance_to]);
                        }
                        prev_alnum = false;
                        continue;
                    }
                }
            }
        }
        prev_alnum = ch.is_alphanumeric();
        cleaned.push(ch);
    }
    (spans, cleaned, saw_path)
}

fn push_entity(
    raw_cand: &str,
    root_prefixes: &[String],
    rel_cwd: Option<&str>,
    seen: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    let mut emit = |s: String| {
        if s.chars().count() >= 2 && seen.insert(s.clone()) {
            out.push(s);
        }
    };

    let mut ent = norm_entity(raw_cand);
    for rp in root_prefixes {
        if let Some(stripped) = ent.strip_prefix(rp.as_str()) {
            ent = stripped.to_string();
            break;
        }
    }

    let mut base_cands = vec![ent.clone()];
    let ascii_head = ent.trim_end_matches(is_hangul);
    if ascii_head.len() >= 2
        && ascii_head.len() < ent.len()
        && ascii_head.bytes().any(|b| b.is_ascii_alphanumeric())
    {
        base_cands.push(norm_entity(ascii_head));
    }

    let mut expanded = Vec::new();
    for c in base_cands {
        expanded.push(c.clone());
        if let Some(rest) = c.strip_prefix("a/").or_else(|| c.strip_prefix("b/")) {
            if rest.contains('/') || is_bare_filename(rest) {
                expanded.push(rest.to_string());
            }
        }
    }

    for c in expanded {
        if let Some(prefix) = rel_cwd {
            let prefix_slash = format!("{prefix}/");
            if !c.is_empty()
                && !c.starts_with('/')
                && !c.starts_with("@/")
                && !c.starts_with("~/")
                && !c.starts_with(&prefix_slash)
                && (c.contains('/') || is_bare_filename(&c))
            {
                emit(format!("{prefix}/{c}"));
            }
        }
        emit(c);
    }
}

fn extract_query_entities(
    norm_q: &str,
    quoted_spans: &[String],
    root: &Path,
    cwd: &Path,
    rel_cwd: Option<&str>,
) -> Vec<String> {
    let root_prefixes: Vec<String> = repo_root_prefixes(root, cwd)
        .into_iter()
        .map(|p| p.to_lowercase())
        .collect();

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for span in quoted_spans {
        push_entity(span, &root_prefixes, rel_cwd, &mut seen, &mut out);
    }
    for chunk in norm_q.split_whitespace() {
        push_entity(chunk, &root_prefixes, rel_cwd, &mut seen, &mut out);
        for part in chunk.split('(') {
            if !part.is_empty() && part != chunk {
                push_entity(part, &root_prefixes, rel_cwd, &mut seen, &mut out);
            }
        }
    }
    out
}

fn matches_src_alias(cand: &str, rest: &str) -> bool {
    let mut slices = Vec::with_capacity(2);
    if let Some(r) = cand.strip_prefix("src/") {
        slices.push(r);
    }
    if let Some((_, r)) = cand.rsplit_once("/src/") {
        slices.push(r);
    }
    slices.into_iter().any(|r| {
        r == rest
            || r.strip_prefix(rest)
                .and_then(|s| s.strip_prefix('.'))
                .is_some_and(|ext| !ext.is_empty() && !ext.contains('/'))
    })
}

fn unique_matching_entity<F>(
    conn: &Connection,
    like_pattern: &str,
    pred: F,
    fallback: &str,
) -> rusqlite::Result<String>
where
    F: Fn(&str) -> bool,
{
    let mut stmt = conn.prepare(
        "SELECT DISTINCT e.entity FROM edges e
         JOIN cards c ON c.id = e.note_id
         WHERE c.status = 'active' AND e.entity LIKE ?1",
    )?;
    let rows = stmt.query_map(params![like_pattern], |r| r.get::<_, String>(0))?;
    let mut found: Option<String> = None;
    for row in rows {
        let cand = row?;
        if pred(&cand) {
            match &found {
                None => found = Some(cand),
                Some(prev) if prev == &cand => {}
                Some(_) => return Ok(fallback.to_string()),
            }
        }
    }
    Ok(found.unwrap_or_else(|| fallback.to_string()))
}

fn resolve_entity(conn: &Connection, raw_ent: &str) -> rusqlite::Result<String> {
    if !raw_ent.contains('/') && !raw_ent.contains("::") && !is_bare_filename(raw_ent) {
        return Ok(raw_ent.to_string());
    }
    let direct: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM edges e
            JOIN cards c ON c.id = e.note_id
            WHERE c.status = 'active' AND e.entity = ?1
        )",
        params![raw_ent],
        |r| r.get(0),
    )?;
    if direct {
        return Ok(raw_ent.to_string());
    }

    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT e.entity FROM edges e
             JOIN cards c ON c.id = e.note_id
             WHERE c.status = 'active' AND e.raw_entity = ?1",
        )?;
        let cands: Vec<String> = stmt
            .query_map(params![raw_ent], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if cands.len() == 1 {
            return Ok(cands.into_iter().next().unwrap());
        }
        if cands.len() > 1 {
            return Ok(raw_ent.to_string());
        }
    }

    if is_bare_filename(raw_ent) {
        let pattern = format!("%/{raw_ent}");
        let suffix = format!("/{raw_ent}");
        return unique_matching_entity(conn, &pattern, |cand| cand.ends_with(&suffix), raw_ent);
    }

    if let Some(rest) = raw_ent.strip_prefix("@/").or_else(|| raw_ent.strip_prefix("~/")) {
        if !rest.is_empty() {
            let pattern = format!("%src/{rest}%");
            return unique_matching_entity(
                conn,
                &pattern,
                |cand| matches_src_alias(cand, rest),
                raw_ent,
            );
        }
    }

    if raw_ent.contains("::") && !raw_ent.contains('/') {
        let pattern = format!("%::{raw_ent}");
        let suffix = format!("::{raw_ent}");
        return unique_matching_entity(conn, &pattern, |cand| cand.ends_with(&suffix), raw_ent);
    }

    if raw_ent.contains('/') && raw_ent.rsplit_once('/').is_some_and(|(_, b)| !b.contains('.')) {
        let pattern = format!("{raw_ent}.%");
        let prefix = format!("{raw_ent}.");
        return unique_matching_entity(
            conn,
            &pattern,
            |cand| cand.strip_prefix(&prefix).is_some_and(|ext| !ext.is_empty() && !ext.contains('/')),
            raw_ent,
        );
    }

    Ok(raw_ent.to_string())
}

fn strip_chrome(body: &str) -> String {
    let mut out = Vec::new();
    for line in body.lines() {
        if line == "## 문제 상황" || (line.starts_with("## v") && line.contains(") ")) {
            continue;
        }
        let mut content = line;
        for prefix in ["- 한 것: ", "- 틀린 증거: ", "- 왜 틀렸나: ", "- 확인 근거: ", "- 다음에는: "] {
            if let Some(rest) = content.strip_prefix(prefix) {
                content = rest
                    .strip_prefix("처음부터 틀림:")
                    .or_else(|| rest.strip_prefix("바뀌어서 틀려짐:"))
                    .map(str::trim_start)
                    .unwrap_or(rest);
                break;
            }
        }
        out.push(content);
    }
    out.join("\n")
}

fn fts_entity_token(item: &str) -> String {
    if item.contains('/') || item.contains('\\') {
        let slash = item.replace('\\', "/");
        slash.rsplit_once('/').map(|(_, b)| b.to_string()).unwrap_or(slash)
    } else {
        item.to_string()
    }
}

fn register_alias(map: &mut HashMap<String, Option<String>>, alias: String, target: &str) {
    if alias == target || alias.is_empty() {
        return;
    }
    map.entry(alias)
        .and_modify(|slot| {
            if slot.as_deref() != Some(target) {
                *slot = None;
            }
        })
        .or_insert_with(|| Some(target.to_string()));
}

#[derive(Clone)]
struct IndexedRow {
    id: String,
    mtime: i64,
    ctime: i64,
    size: i64,
    hash: i64,
}

fn sync(conn: &mut Connection, d: &Path) -> rusqlite::Result<()> {
    let indexed: HashMap<String, IndexedRow> = {
        let mut stmt = conn.prepare("SELECT filename, id, mtime, ctime, size, hash FROM cards")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                IndexedRow {
                    id: r.get(1)?,
                    mtime: r.get(2)?,
                    ctime: r.get(3)?,
                    size: r.get(4)?,
                    hash: r.get(5)?,
                },
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_nanos() as i64)
        .unwrap_or(0);

    let mut on_disk: HashMap<String, DiskStat> = HashMap::new();
    if let Ok(entries) = fs::read_dir(d) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    on_disk.insert(name.to_string(), disk_stat(&p));
                }
            }
        }
    }

    let stale: Vec<String> = indexed.keys().filter(|k| !on_disk.contains_key(*k)).cloned().collect();
    let mut changed: Vec<String> = on_disk
        .iter()
        .filter(|(name, st)| {
            let Some(idx) = indexed.get(*name) else {
                return true;
            };
            if idx.mtime != st.mtime || idx.ctime != st.ctime || idx.size != st.size {
                return true;
            }
            let suspect_same_tick = if cfg!(unix) {
                now_ns.saturating_sub(st.ctime) <= 25_000_000
            } else if st.mtime % 100_000_000 == 0 {
                now_ns.saturating_sub(st.mtime) <= 2_000_000_000
            } else {
                now_ns.saturating_sub(st.mtime) <= 50_000_000
            };
            if suspect_same_tick {
                return fs::read(d.join(name)).map(|b| fnv1a(&b) != idx.hash).unwrap_or(true);
            }
            false
        })
        .map(|(name, _)| name.clone())
        .collect();
    changed.sort();

    if stale.is_empty() && changed.is_empty() {
        return Ok(());
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for name in &stale {
        if let Some(old) = indexed.get(name) {
            tx.execute("DELETE FROM notes_fts WHERE id = ?1", params![old.id])?;
            tx.execute("DELETE FROM cards WHERE id = ?1", params![old.id])?;
            tx.execute("DELETE FROM edges WHERE note_id = ?1", params![old.id])?;
        }
    }
    for name in &changed {
        if let Some(old) = indexed.get(name) {
            tx.execute("DELETE FROM notes_fts WHERE id = ?1", params![old.id])?;
            tx.execute("DELETE FROM cards WHERE id = ?1", params![old.id])?;
            tx.execute("DELETE FROM edges WHERE note_id = ?1", params![old.id])?;
        }
        let note_path = d.join(name);
        let raw_bytes = match fs::read(&note_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("mistology: {name} 건너뜀 ({e})");
                continue;
            }
        };
        let (meta, body) = match std::str::from_utf8(&raw_bytes)
            .map_err(|e| e.to_string())
            .and_then(parse_note)
        {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("mistology: {name} 건너뜀 ({e})");
                continue;
            }
        };
        let note_id = name.strip_suffix(".md").unwrap_or(&meta.id).to_string();
        let st = on_disk[name];
        let hash = fnv1a(&raw_bytes);
        let wrong = parse_version_num(&meta.current).unwrap_or(2) as i64 - 1;
        let summary = extract_summary(&body);
        let fts_entities: Vec<String> = meta
            .files
            .iter()
            .map(|f| fts_entity_token(f))
            .chain(meta.symbols.iter().cloned())
            .chain(meta.coupled.iter().map(|c| fts_entity_token(c)))
            .collect();
        let clean_body = strip_chrome(&body);

        tx.prepare_cached("DELETE FROM notes_fts WHERE id = ?1")?.execute(params![note_id])?;
        tx.prepare_cached("DELETE FROM edges WHERE note_id = ?1")?.execute(params![note_id])?;
        tx.prepare_cached(
            "INSERT INTO notes_fts (id, signatures, entities, tags, body) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![
            note_id,
            expand_subwords(&meta.signatures.join("\n")),
            expand_subwords(&fts_entities.join("\n")),
            meta.tags.join(" "),
            expand_subwords(&clean_body),
        ])?;
        tx.prepare_cached(
            "INSERT OR REPLACE INTO cards VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )?
        .execute(params![
            note_id,
            name,
            st.mtime,
            st.ctime,
            st.size,
            hash,
            meta.status,
            meta.current,
            wrong,
            summary.problem,
            summary.fix,
            summary.next_step,
            summary.pitfalls,
            meta.files.join(", "),
            meta.symbols.join(", "),
            meta.coupled.join(", "),
        ])?;
        let mut ins_edge = tx.prepare_cached(
            "INSERT OR IGNORE INTO edges (note_id, entity, role, raw_entity) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (role, items) in [("file", &meta.files), ("symbol", &meta.symbols), ("coupled", &meta.coupled)] {
            for item in items {
                let ent = norm_entity(item);
                if !ent.is_empty() {
                    ins_edge.execute(params![note_id, ent, role, ent])?;
                }
            }
        }
    }

    tx.execute("UPDATE edges SET entity = raw_entity WHERE entity != raw_entity", [])?;
    let all_raws: Vec<String> = {
        let mut stmt = tx.prepare("SELECT DISTINCT raw_entity FROM edges")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    if !all_raws.is_empty() {
        let active_raws: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT e.raw_entity
                 FROM edges e
                 JOIN cards c ON c.id = e.note_id
                 WHERE c.status = 'active'",
            )?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };

        let mut unique_alias: HashMap<String, Option<String>> = HashMap::new();
        for p in active_raws.iter().filter(|r| is_canonical_file_path(r)) {
            if let Some((_, base)) = p.rsplit_once('/') {
                if is_bare_filename(base) {
                    register_alias(&mut unique_alias, base.to_string(), p);
                }
            }
            if let Some((stem_path, _)) = p.rsplit_once('.') {
                register_alias(&mut unique_alias, stem_path.to_string(), p);
            }
            let mut src_rels = Vec::with_capacity(2);
            if let Some(rest) = p.strip_prefix("src/") {
                src_rels.push(rest);
            }
            if let Some((_, rest)) = p.rsplit_once("/src/") {
                src_rels.push(rest);
            }
            for rest in src_rels {
                register_alias(&mut unique_alias, format!("@/{rest}"), p);
                register_alias(&mut unique_alias, format!("~/{rest}"), p);
                if let Some((rest_stem, _)) = rest.rsplit_once('.') {
                    register_alias(&mut unique_alias, format!("@/{rest_stem}"), p);
                    register_alias(&mut unique_alias, format!("~/{rest_stem}"), p);
                }
            }
        }

        let sym_set: Vec<&String> = active_raws
            .iter()
            .filter(|r| r.contains("::") && !r.contains('/'))
            .collect();
        let mut non_maximal: HashSet<&str> = HashSet::new();
        for &full_sym in &sym_set {
            for (idx, _) in full_sym.match_indices("::") {
                non_maximal.insert(&full_sym[idx + 2..]);
            }
        }
        for &full_sym in &sym_set {
            if !non_maximal.contains(full_sym.as_str()) {
                for (idx, _) in full_sym.match_indices("::") {
                    let suffix = &full_sym[idx + 2..];
                    if suffix.contains("::") {
                        register_alias(&mut unique_alias, suffix.to_string(), full_sym);
                    }
                }
            }
        }

        let mut upd = tx.prepare("UPDATE edges SET entity = ?1 WHERE raw_entity = ?2")?;
        for raw in all_raws {
            if !is_canonical_file_path(&raw) {
                if let Some(Some(target)) = unique_alias.get(&raw) {
                    upd.execute(params![target, raw])?;
                }
            }
        }
    }

    tx.commit()
}

fn tokenize_fts(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

struct Candidate {
    score: f64,
    via: Option<String>,
    has_structured: bool,
    matched_body_words: usize,
}

struct QueryPlan<'a> {
    words: &'a [String],
    require_multi_body: bool,
    fts_query: &'a str,
    entities: &'a [String],
    k: usize,
}

fn score_fts_candidates(
    conn: &Connection,
    plan: &QueryPlan<'_>,
    scores: &mut HashMap<String, Candidate>,
) -> rusqlite::Result<()> {
    const COL_WEIGHTS: [f64; 4] = [10.0, 6.0, 2.0, 1.0];
    const COL_AVG_DL: [f64; 4] = [8.0, 5.0, 2.0, 45.0];

    if plan.fts_query.is_empty() || plan.words.is_empty() {
        return Ok(());
    }
    let active_docs: i64 =
        conn.query_row("SELECT COUNT(*) FROM cards WHERE status = 'active'", [], |r| r.get(0))?;
    if active_docs <= 0 {
        return Ok(());
    }
    let n_docs = active_docs as f64;

    let mut stmt = conn.prepare(
        "SELECT c.id, c.wrong,
                notes_fts.signatures, notes_fts.entities, notes_fts.tags, notes_fts.body
         FROM notes_fts
         JOIN cards c ON c.id = notes_fts.id
         WHERE notes_fts MATCH ?1 AND c.status = 'active'",
    )?;
    let raw_rows: Vec<(String, i64, [Vec<String>; 4])> = stmt
        .query_map(params![plan.fts_query], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                [
                    tokenize_fts(&r.get::<_, String>(2)?),
                    tokenize_fts(&r.get::<_, String>(3)?),
                    tokenize_fts(&r.get::<_, String>(4)?),
                    tokenize_fts(&r.get::<_, String>(5)?),
                ],
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if raw_rows.is_empty() {
        return Ok(());
    }

    let allow_prefix: Vec<bool> =
        plan.words.iter().map(|w| w.chars().any(is_hangul) || w.len() >= 5).collect();
    let slot_len = plan.words.len() * 2;
    let mut df = vec![0usize; slot_len];
    let mut row_tfs: Vec<[Vec<usize>; 4]> = Vec::with_capacity(raw_rows.len());

    for (_, _, cols) in &raw_rows {
        let mut col_tf: [Vec<usize>; 4] = [
            vec![0; slot_len],
            vec![0; slot_len],
            vec![0; slot_len],
            vec![0; slot_len],
        ];
        for (wi, w) in plan.words.iter().enumerate() {
            let exact_slot = wi * 2;
            let pref_slot = wi * 2 + 1;
            let pref = allow_prefix[wi];
            let mut has_exact = false;
            let mut has_pref = false;
            for ci in 0..4 {
                let mut exact_cnt = 0usize;
                let mut pref_cnt = 0usize;
                for t in &cols[ci] {
                    if t == w {
                        exact_cnt += 1;
                    } else if pref && t.starts_with(w.as_str()) {
                        pref_cnt += 1;
                    }
                }
                col_tf[ci][exact_slot] = exact_cnt;
                col_tf[ci][pref_slot] = pref_cnt;
                if exact_cnt > 0 {
                    has_exact = true;
                }
                if pref_cnt > 0 {
                    has_pref = true;
                }
            }
            if has_exact {
                df[exact_slot] += 1;
            }
            if has_exact || has_pref {
                df[pref_slot] += 1;
            }
        }
        row_tfs.push(col_tf);
    }

    let idf: Vec<f64> = df
        .iter()
        .enumerate()
        .map(|(si, &d)| {
            if d == 0 {
                0.0
            } else {
                let df_f = (d as f64).min(n_docs);
                let discount = if si % 2 == 1 { 0.65 } else { 1.0 };
                discount * (1.0 + (n_docs - df_f + 0.5) / (df_f + 0.5)).ln()
            }
        })
        .collect();

    for ((id, wrong, cols), col_tf) in raw_rows.into_iter().zip(row_tfs) {
        let mut raw_score = 0.0f64;
        let mut struct_score = 0.0f64;
        for ci in 0..4 {
            let dl = cols[ci].len().max(1) as f64;
            let norm_denom_add = 1.2 * (0.25 + 0.75 * (dl / COL_AVG_DL[ci]));
            for si in 0..slot_len {
                let tf = col_tf[ci][si];
                if tf > 0 {
                    let tf_f = tf as f64;
                    let tf_norm = (tf_f * 2.2) / (tf_f + norm_denom_add);
                    let contrib = -COL_WEIGHTS[ci] * idf[si] * tf_norm;
                    raw_score += contrib;
                    if ci < 3 {
                        struct_score += contrib;
                    }
                }
            }
        }
        if raw_score < 0.0 {
            let score = raw_score * (1.0 + 0.2 * wrong as f64);
            let has_structured = struct_score < -1e-9;
            let matched_body_words = (0..plan.words.len())
                .filter(|&wi| col_tf[3][wi * 2] > 0 || col_tf[3][wi * 2 + 1] > 0)
                .count();
            scores.insert(id, Candidate { score, via: None, has_structured, matched_body_words });
        }
    }
    Ok(())
}

fn query_conn(
    conn: &mut Connection,
    d: &Path,
    plan: &QueryPlan<'_>,
) -> rusqlite::Result<Vec<SearchHit>> {
    sync(conn, d)?;

    let mut scores: HashMap<String, Candidate> = HashMap::new();
    score_fts_candidates(conn, plan, &mut scores)?;
    let fts_scores: HashMap<String, f64> =
        scores.iter().map(|(id, c)| (id.clone(), c.score)).collect();

    let mut resolved_seen = HashSet::new();
    let mut queried_files = HashSet::new();
    let mut note_entity_bonuses: HashMap<String, Vec<f64>> = HashMap::new();
    let mut note_primary_bonuses: HashMap<String, Vec<f64>> = HashMap::new();
    let mut note_primary_ents: HashMap<String, Vec<String>> = HashMap::new();
    let mut note_coupled_ents: HashMap<String, HashSet<String>> = HashMap::new();
    {
        let mut edge_stmt = conn.prepare(
            "SELECT e.note_id,
                    MIN(CASE
                        WHEN e.role = 'file' THEN -18.0 / MAX(1, (SELECT COUNT(DISTINCT ef.entity) FROM edges ef WHERE ef.note_id = e.note_id AND ef.role = 'file'))
                        WHEN e.role = 'symbol' THEN -12.0
                        ELSE -7.5
                    END),
                    c.wrong,
                    MAX(CASE WHEN e.role = 'file' THEN 1 ELSE 0 END),
                    MAX(CASE WHEN e.role != 'coupled' THEN 1 ELSE 0 END)
             FROM edges e
             JOIN cards c ON c.id = e.note_id
             WHERE e.entity = ?1 AND c.status = 'active'
             GROUP BY e.note_id",
        )?;
        for raw_ent in plan.entities {
            let ent = resolve_entity(conn, raw_ent)?;
            if !resolved_seen.insert(ent.clone()) {
                continue;
            }
            if is_canonical_file_path(&ent)
                && ent
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| ext.bytes().any(|b| b.is_ascii_alphabetic()))
            {
                queried_files.insert(ent.clone());
            }
            let rows = edge_stmt.query_map(params![ent], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (note_id, base_bonus, wrong, is_file, has_primary) = row?;
                if is_file != 0 {
                    queried_files.insert(ent.clone());
                }
                let bonus = base_bonus * (1.0 + 0.2 * wrong as f64);
                note_entity_bonuses.entry(note_id.clone()).or_default().push(bonus);
                if has_primary != 0 {
                    note_primary_bonuses.entry(note_id.clone()).or_default().push(bonus);
                    note_primary_ents.entry(note_id).or_default().push(ent.clone());
                } else {
                    note_coupled_ents.entry(note_id).or_default().insert(ent.clone());
                }
            }
        }
    }
    for (note_id, bonuses) in note_entity_bonuses.iter_mut() {
        bonuses.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let total_bonus = bonuses[0] + bonuses[1..].iter().sum::<f64>() * 0.25;
        let entry = scores.entry(note_id.clone()).or_insert(Candidate {
            score: 0.0,
            via: None,
            has_structured: true,
            matched_body_words: 0,
        });
        entry.score += total_bonus;
        entry.has_structured = true;
    }

    let mut primary_ent_totals: HashMap<String, f64> = HashMap::new();
    let mut best_primary_ent_bonus = 0.0f64;
    for (note_id, bonuses) in note_primary_bonuses.iter_mut() {
        bonuses.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let tot = bonuses[0] + bonuses[1..].iter().sum::<f64>() * 0.25;
        if tot < best_primary_ent_bonus {
            best_primary_ent_bonus = tot;
        }
        primary_ent_totals.insert(note_id.clone(), tot);
    }

    if plan.require_multi_body {
        scores.retain(|_, c| c.has_structured || c.matched_body_words >= 2);
    }

    let mut direct_ranked: Vec<(String, f64)> = scores
        .iter()
        .map(|(id, c)| (id.clone(), c.score))
        .collect();
    direct_ranked.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.0.cmp(&a.0))
    });

    // 1-hop 온톨로지 확장: 최상위 적중 시드 및 미커버 직접 엔티티 시드의 coupled/symbol 이웃을 끌어올린다
    if let Some(best_score) = direct_ranked.first().map(|(_, s)| *s) {
        let ent_cutoff = (best_primary_ent_bonus * 0.40).min(-9.0);
        let mut seeds: Vec<(String, f64)> = Vec::new();
        let mut covered_ents: HashSet<String> = HashSet::new();
        for (idx, (id, sc)) in direct_ranked.iter().enumerate() {
            if idx == 0 {
                if let Some(ents) = note_primary_ents.get(id) {
                    covered_ents.extend(ents.iter().cloned());
                }
                seeds.push((id.clone(), *sc));
            } else if let Some(&prim_sc) = primary_ent_totals.get(id) {
                let covers_new = note_primary_ents
                    .get(id)
                    .is_some_and(|ents| ents.iter().any(|e| !covered_ents.contains(e)));
                if prim_sc <= ent_cutoff && covers_new {
                    if let Some(ents) = note_primary_ents.get(id) {
                        covered_ents.extend(ents.iter().cloned());
                    }
                    seeds.push((id.clone(), *sc));
                }
            }
            if seeds.len() >= plan.k {
                break;
            }
        }
        let seed_ids: HashSet<String> = seeds.iter().map(|(id, _)| id.clone()).collect();
        let weakest_seed_score = seeds.last().map(|(_, s)| *s).unwrap_or(best_score);

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum HopDir {
            Forward,  // src.file == dst.coupled
            Backward, // src.coupled == dst.file
            Other,
        }
        type Hop1Entry = (f64, String, HopDir, i64, i64);

        let mut stmt = conn.prepare(
            "SELECT e2.note_id, e1.entity, e1.role, e2.role, c.wrong,
                    (SELECT COUNT(DISTINCT ef.entity) FROM edges ef WHERE ef.note_id = e2.note_id AND ef.role = 'file'),
                    (SELECT COUNT(DISTINCT ec.entity) FROM edges ec WHERE ec.note_id = e2.note_id AND ec.role = 'coupled')
             FROM edges e1
             JOIN edges e2 ON e1.entity = e2.entity AND e1.note_id != e2.note_id
             JOIN cards c ON c.id = e2.note_id
             WHERE e1.note_id = ?1 AND c.status = 'active'
               AND (e1.role != 'file' OR e2.role != 'file')
               AND (e1.role != 'coupled' OR e2.role != 'coupled')
             ORDER BY e1.entity ASC",
        )?;
        let mut hop1_directed: HashMap<(String, u8), Hop1Entry> = HashMap::new();
        for (seed_id, seed_score) in &seeds {
            let rows = stmt.query_map(params![seed_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })?;
            for row in rows {
                let (nbr_id, via_entity, e1_role, e2_role, wrong, nbr_file_cnt, nbr_coupled_cnt) = row?;
                if seed_ids.contains(&nbr_id) {
                    continue;
                }
                if !queried_files.is_empty() && e1_role == "file" && !queried_files.contains(&via_entity) {
                    continue;
                }
                if e2_role == "file" && nbr_file_cnt > 2 {
                    continue;
                }
                let dir = match (e1_role.as_str(), e2_role.as_str()) {
                    ("file", "coupled") => HopDir::Forward,
                    ("coupled", "file") => HopDir::Backward,
                    _ => HopDir::Other,
                };
                let fanout_disc = if e2_role == "file" && nbr_file_cnt == 2 { 0.90 } else { 1.0 };
                let hop_factor = (0.55 * (1.0 + 0.2 * wrong as f64) * fanout_disc).min(0.85);
                let hop_score = (seed_score * hop_factor).max(weakest_seed_score * 0.92 * fanout_disc);
                let pure_fts = fts_scores.get(&nbr_id).copied().unwrap_or(0.0);
                let coupled_only_direct = !note_primary_ents.contains_key(&nbr_id)
                    && pure_fts > hop_score
                    && note_coupled_ents
                        .get(&nbr_id)
                        .is_some_and(|s| s.contains(&via_entity));
                let eff_score = match scores.get(&nbr_id) {
                    None => {
                        scores.insert(
                            nbr_id.clone(),
                            Candidate {
                                score: hop_score,
                                via: Some(via_entity.clone()),
                                has_structured: true,
                                matched_body_words: 0,
                            },
                        );
                        hop_score
                    }
                    Some(existing) if hop_score < existing.score => {
                        scores.insert(
                            nbr_id.clone(),
                            Candidate {
                                score: hop_score,
                                via: Some(via_entity.clone()),
                                has_structured: true,
                                matched_body_words: existing.matched_body_words,
                            },
                        );
                        hop_score
                    }
                    Some(existing) if coupled_only_direct && existing.via.is_none() => {
                        let kept = existing.score;
                        let mbw = existing.matched_body_words;
                        scores.insert(
                            nbr_id.clone(),
                            Candidate {
                                score: kept,
                                via: Some(via_entity.clone()),
                                has_structured: true,
                                matched_body_words: mbw,
                            },
                        );
                        kept.max(hop_score)
                    }
                    Some(existing) => existing.score.max(hop_score),
                };
                if dir != HopDir::Other {
                    let dir_key = if dir == HopDir::Forward { 1u8 } else { 2u8 };
                    let should_save = hop1_directed
                        .get(&(nbr_id.clone(), dir_key))
                        .is_none_or(|prev| eff_score < prev.0);
                    if should_save {
                        hop1_directed.insert(
                            (nbr_id, dir_key),
                            (eff_score, via_entity, dir, nbr_file_cnt, nbr_coupled_cnt),
                        );
                    }
                }
            }
        }

        // 단조 방향성 2-hop 확장: file->coupled->coupled 또는 coupled->file->file 사슬만 따라가 허브·형제 역류를 차단한다
        let mut hop1_list: Vec<(String, f64, String, HopDir, i64, i64)> = hop1_directed
            .into_iter()
            .map(|((id, _), (sc, v, d, fc, cc))| (id, sc, v, d, fc, cc))
            .collect();
        hop1_list.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        for (h1_id, h1_score, via1, dir1, h1_file_cnt, h1_coupled_cnt) in hop1_list {
            if h1_file_cnt > 2 || (dir1 == HopDir::Backward && h1_coupled_cnt > 2) {
                continue;
            }
            let rows = stmt.query_map(params![h1_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (h2_id, via2, e1_role, e2_role, wrong2, h2_file_cnt) = row?;
                if e2_role == "file" && h2_file_cnt > 2 {
                    continue;
                }
                let dir2 = match (e1_role.as_str(), e2_role.as_str()) {
                    ("file", "coupled") => HopDir::Forward,
                    ("coupled", "file") => HopDir::Backward,
                    _ => HopDir::Other,
                };
                if dir2 != dir1
                    || via2 == via1
                    || seed_ids.contains(&h2_id)
                    || note_primary_ents.contains_key(&h2_id)
                {
                    continue;
                }
                let fanout_disc = if e2_role == "file" && h2_file_cnt == 2 { 0.90 } else { 1.0 };
                let hop2_factor = (0.50 * (1.0 + 0.2 * wrong2 as f64) * fanout_disc).min(0.72);
                let hop2_score = h1_score * hop2_factor;
                if scores.get(&h2_id).is_none_or(|ex| hop2_score < ex.score) {
                    let prev_mbw = scores.get(&h2_id).map_or(0, |ex| ex.matched_body_words);
                    scores.insert(
                        h2_id,
                        Candidate {
                            score: hop2_score,
                            via: Some(format!("{via1} → {via2}")),
                            has_structured: true,
                            matched_body_words: prev_mbw,
                        },
                    );
                }
            }
        }

        if weakest_seed_score < -15.0 {
            let cutoff = weakest_seed_score * 0.12;
            scores.retain(|_, c| c.score <= cutoff);
        }
    }

    let mut ranked: Vec<(String, f64, Option<String>)> =
        scores.into_iter().map(|(id, c)| (id, c.score, c.via)).collect();
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| b.0.cmp(&a.0)));
    ranked.truncate(plan.k);

    let mut hits = Vec::with_capacity(ranked.len());
    let mut card_stmt = conn.prepare(
        "SELECT filename, current, wrong, problem, fix, next_step, pitfalls, files, symbols, coupled
         FROM cards WHERE id = ?1",
    )?;
    for (id, _, via) in ranked {
        let hit = card_stmt.query_row(params![id], |r| {
            let filename: String = r.get(0)?;
            Ok(SearchHit {
                id: id.clone(),
                path: d.join(filename),
                current: r.get(1)?,
                wrong: r.get(2)?,
                problem: r.get(3)?,
                fix: r.get(4)?,
                next_step: r.get(5)?,
                pitfalls: r.get(6)?,
                files: r.get(7)?,
                symbols: r.get(8)?,
                coupled: r.get(9)?,
                via: via.clone(),
            })
        })?;
        hits.push(hit);
    }
    Ok(hits)
}

fn query_index(
    index_path: &Path,
    d: &Path,
    plan: &QueryPlan<'_>,
) -> rusqlite::Result<Vec<SearchHit>> {
    let mut conn = Connection::open(index_path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let ver: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if ver != SCHEMA_VERSION {
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cur_ver: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if cur_ver != SCHEMA_VERSION {
            tx.execute_batch(&format!(
                "DROP TABLE IF EXISTS notes_fts;
                 DROP TABLE IF EXISTS cards;
                 DROP TABLE IF EXISTS edges;
                 {SCHEMA}
                 PRAGMA user_version = {SCHEMA_VERSION};"
            ))?;
        }
        tx.commit()?;
    }
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    query_conn(&mut conn, d, plan)
}

fn query_in_memory(d: &Path, plan: &QueryPlan<'_>) -> rusqlite::Result<Vec<SearchHit>> {
    let mut conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA)?;
    query_conn(&mut conn, d, plan)
}

pub fn search(root: &Path, query: &str, k: usize) -> Vec<SearchHit> {
    search_in(root, root, query, k)
}

pub fn search_in(root: &Path, cwd: &Path, query: &str, k: usize) -> Vec<SearchHit> {
    let d = root.join(".mistology").join("notes");
    if !d.is_dir() || k == 0 {
        return Vec::new();
    }
    let norm_q: String = query.nfc().collect();
    let (quoted_spans, cleaned_q, saw_quoted_path) = scan_quoted_spans(&norm_q);

    let mut had_path = saw_quoted_path;
    let mut non_path_chunks = Vec::new();
    for chunk in cleaned_q.split_whitespace() {
        if is_path_chunk(chunk) {
            had_path = true;
        } else {
            non_path_chunks.push(chunk);
        }
    }
    let words_source = non_path_chunks.join(" ");

    let mut seen = HashSet::new();
    let mut words = Vec::new();
    for raw in words_source.split(|c: char| !c.is_alphanumeric()) {
        if raw.chars().count() >= 2 {
            let stemmed_raw = korean_stem(raw);
            let stemmed = stemmed_raw.to_lowercase();
            if seen.insert(stemmed.clone()) {
                words.push(stemmed);
            }
            for sub in camel_subwords(stemmed_raw) {
                if seen.insert(sub.clone()) {
                    words.push(sub);
                }
            }
        }
    }
    words.retain(|w| !NOISE_WORDS.contains(&w.as_str()));

    let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let cwd_canon = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let rel_cwd = cwd_canon
        .strip_prefix(&root_canon)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().replace('\\', "/").to_lowercase());

    let query_entities = extract_query_entities(&norm_q, &quoted_spans, root, cwd, rel_cwd.as_deref());
    if words.is_empty() && query_entities.is_empty() {
        return Vec::new();
    }
    let require_multi_body = words.len() >= 4 || (had_path && words.len() >= 3);
    let fts_query = words
        .iter()
        .map(|w| {
            if w.chars().any(is_hangul) || w.len() >= 5 {
                format!("\"{w}\" OR \"{w}\"*")
            } else {
                format!("\"{w}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let index_path = root.join(".mistology").join("index.db");
    let plan = QueryPlan {
        words: &words,
        require_multi_body,
        fts_query: &fts_query,
        entities: &query_entities,
        k,
    };

    match query_index(&index_path, &d, &plan) {
        Ok(hits) => hits,
        Err(rusqlite::Error::SqliteFailure(err, _))
            if matches!(err.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) =>
        {
            query_in_memory(&d, &plan).unwrap_or_default()
        }
        Err(_) => {
            if let Ok(entries) = fs::read_dir(root.join(".mistology")) {
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().starts_with("index.db") {
                        let _ = fs::remove_file(e.path());
                    }
                }
            }
            query_index(&index_path, &d, &plan)
                .or_else(|_| query_in_memory(&d, &plan))
                .unwrap_or_default()
        }
    }
}

pub fn format_card(root: &Path, h: &SearchHit) -> String {
    let rel = h
        .path
        .strip_prefix(root)
        .unwrap_or(&h.path)
        .to_string_lossy()
        .replace('\\', "/");
    let via_tag = h.via.as_deref().map(|v| format!(" | 연결: {v} 경유")).unwrap_or_default();
    let mut lines = vec![
        format!("# {rel} [{} | 오답 {}회{via_tag}]", h.current, h.wrong),
        format!("- 문제: {}", h.problem),
        format!("- 현재 정답: {}", h.fix),
        format!("- 다음에는: {}", h.next_step),
    ];
    if !h.pitfalls.is_empty() {
        lines.push(format!("- 피할 오답: {}", h.pitfalls));
    }
    let mut links = Vec::new();
    if !h.files.is_empty() {
        links.push(format!("files=[{}]", h.files));
    }
    if !h.symbols.is_empty() {
        links.push(format!("symbols=[{}]", h.symbols));
    }
    if !h.coupled.is_empty() {
        links.push(format!("coupled=[{}]", h.coupled));
    }
    if !links.is_empty() {
        lines.push(format!("- 연결: {}", links.join(" ")));
    }
    lines.join("\n")
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => {
            let head = s[..idx].trim_end();
            let close_tick = if head.bytes().filter(|&b| b == b'`').count() % 2 == 1 {
                "`"
            } else {
                ""
            };
            format!("{head}{close_tick}…")
        }
        None => s.to_string(),
    }
}

fn extract_backtick_spans(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            let content_start = i + run;
            let mut j = content_start;
            let mut closed = None;
            while j < bytes.len() {
                if bytes[j] == b'`' {
                    let close_run = bytes[j..].iter().take_while(|&&b| b == b'`').count();
                    if close_run == run {
                        closed = Some(j);
                        break;
                    }
                    j += close_run;
                } else {
                    j += 1;
                }
            }
            if let Some(end_idx) = closed {
                let inner = s[content_start..end_idx].trim();
                if inner.chars().count() >= 2 {
                    out.push(inner.to_string());
                }
                i = end_idx + run;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn split_pitfall_versions(pitfalls: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut last = 0usize;
    for (idx, _) in pitfalls.match_indices(" / v") {
        let after_sep = &pitfalls[idx + 3..];
        if after_sep
            .split_once(": ")
            .is_some_and(|(ver, _)| parse_version_num(ver).is_some())
        {
            parts.push(&pitfalls[last..idx]);
            last = idx + 3;
        }
    }
    parts.push(&pitfalls[last..]);
    parts
}

fn compact_pitfalls(h: &SearchHit) -> String {
    split_pitfall_versions(&h.pitfalls)
        .into_iter()
        .filter_map(|seg| {
            let (prefix, rest) = match seg.split_once(": ") {
                Some((p, r)) if parse_version_num(p).is_some() => (Some(p), r),
                _ => (None, seg),
            };
            let has_negation = rest.to_ascii_lowercase().contains("without ") || rest.contains("없이");
            let distinct_spans: Vec<String> = if has_negation {
                Vec::new()
            } else {
                extract_backtick_spans(rest)
                    .into_iter()
                    .filter(|sp| {
                        !(h.fix.contains(sp.as_str())
                            || h.files.contains(sp.as_str())
                            || h.symbols.contains(sp.as_str())
                            || h.coupled.contains(sp.as_str()))
                    })
                    .collect()
            };
            let body = if distinct_spans.iter().any(|sp| sp.chars().count() >= 4) {
                distinct_spans
                    .into_iter()
                    .take(2)
                    .map(|sp| format!("`{}`", truncate_chars(&sp, 48)))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else if h.wrong >= 2 || has_negation || rest.chars().count() <= 40 {
                truncate_chars(rest, 56)
            } else {
                return None;
            };
            Some(match prefix {
                Some(p) => format!("{p}: {body}"),
                None => body,
            })
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

pub fn format_brief(h: &SearchHit) -> String {
    let via_part = h.via.as_deref().map(|v| format!(" ← {v}")).unwrap_or_default();
    let base_target = if !h.files.is_empty() {
        &h.files
    } else if !h.symbols.is_empty() {
        &h.symbols
    } else {
        &h.id
    };
    let sym_hint = h
        .symbols
        .split(", ")
        .next()
        .filter(|s| !s.is_empty() && !h.files.is_empty() && !h.fix.contains(*s))
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    let extra_coupled: Vec<&str> = h
        .coupled
        .split(", ")
        .filter(|c| !c.is_empty())
        .filter(|c| {
            let cn = norm_entity(c);
            !h.via.as_deref().is_some_and(|v| v.contains(&cn))
        })
        .collect();
    let coupled_part = if !extra_coupled.is_empty() {
        format!(" → [{}]", extra_coupled.join(", "))
    } else {
        String::new()
    };
    let fix_line = if h.fix.chars().count() < 48 && !h.next_step.is_empty() && h.next_step != h.fix {
        format!("- 정답: {} ({})", h.fix, truncate_chars(&h.next_step, 72))
    } else {
        format!("- 정답: {}", h.fix)
    };
    let mut lines = vec![
        format!("[{} {}{via_part}] {base_target}{sym_hint}{coupled_part}", h.id, h.current),
        fix_line,
    ];
    if !h.pitfalls.is_empty() {
        let cp = compact_pitfalls(h);
        if !cp.is_empty() {
            lines.push(format!("- 피할 오답: {cp}"));
        }
    }
    lines.join("\n")
}
