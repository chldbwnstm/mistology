use crate::{impact, index, note};
use clap::ValueEnum;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HOOK_TIMEOUT_SECS: u64 = 5;
const STATE_TTL_SECS: u64 = 86_400;
const MAX_PENDING_NOTE_IDS: usize = 4;
const MAX_BASELINE_ENTRIES: usize = 512;

const SESSION_CONTRACT_MSG: &str = "[MISTOLOGY 상시 규약] 이 저장소에서 코드 수정·디버깅 시:\n\
1) 수정/헬퍼 작성 전 `mistology search -b \"<파일·심볼·기능>\"`으로 ★표준·결합 모듈 확인\n\
2) 에러 발생 시 추측 수정 전 에러 원문으로 `mistology search -b` 실행\n\
3) 완료 전 `mistology impact`로 미수정 결합 파일 점검\n\
4) 검증된 실패·해결은 `mistology new`, 재사용 표준은 `mistology canon`, 규약 변경은 `mistology supersede <id>`로 기록";

const CURSOR_RULE_CONTENT: &str = "---\n\
description: MISTOLOGY always-on coherence and mistake ontology rules\n\
alwaysApply: true\n\
---\n\
<!-- mistology:managed -->\n\
# MISTOLOGY (상시 적용)\n\
\n\
- **작업 시작·헬퍼 작성 전**: 코드 수정 전 `mistology search -b \"<파일·심볼·기능>\"`으로 `★표준` 규약과 결합 모듈(`← ...`)을 먼저 확인한다.\n\
- **에러 발생 시**: 빌드·테스트·런타임 에러를 추측으로 고치기 전 에러 원문으로 `mistology search -b`를 실행한다.\n\
- **완료 전 파급 점검**: 코드 수정 후 종료·커밋 전 `mistology impact`로 미수정 결합 파일의 계약과 테스트를 확인한다.\n\
- **검증 후 기록**: 검증된 실패·해결은 `mistology new`, 공유 표준 진입점은 `mistology canon`, 기존 규약 변경은 `mistology supersede <id>`로 `--file`, `--symbol`, `--coupled`와 함께 남긴다.\n";

static TMP_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum HookAgent {
    Gemini,
    Claude,
    Codex,
}

impl HookAgent {
    pub fn as_str(self) -> &'static str {
        match self {
            HookAgent::Gemini => "gemini",
            HookAgent::Claude => "claude",
            HookAgent::Codex => "codex",
        }
    }
}

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum HookEvent {
    #[value(name = "session-start")]
    SessionStart,
    #[value(name = "subagent-start")]
    SubagentStart,
    #[value(name = "pre-invocation")]
    PreInvocation,
    #[value(name = "user-prompt-submit")]
    UserPromptSubmit,
    #[value(name = "post-tool-use")]
    PostToolUse,
    #[value(name = "post-tool-use-failure")]
    PostToolUseFailure,
    #[value(name = "stop")]
    Stop,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "session-start",
            HookEvent::SubagentStart => "subagent-start",
            HookEvent::PreInvocation => "pre-invocation",
            HookEvent::UserPromptSubmit => "user-prompt-submit",
            HookEvent::PostToolUse => "post-tool-use",
            HookEvent::PostToolUseFailure => "post-tool-use-failure",
            HookEvent::Stop => "stop",
        }
    }

    fn from_payload_name(name: &str) -> Option<Self> {
        match name {
            "SessionStart" => Some(HookEvent::SessionStart),
            "SubagentStart" => Some(HookEvent::SubagentStart),
            "PreInvocation" => Some(HookEvent::PreInvocation),
            "UserPromptSubmit" => Some(HookEvent::UserPromptSubmit),
            "PostToolUse" => Some(HookEvent::PostToolUse),
            "PostToolUseFailure" => Some(HookEvent::PostToolUseFailure),
            "Stop" => Some(HookEvent::Stop),
            _ => None,
        }
    }

    fn claude_event_name(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SubagentStart => "SubagentStart",
            HookEvent::PreInvocation => "PreInvocation",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::PostToolUseFailure => "PostToolUseFailure",
            HookEvent::Stop => "Stop",
        }
    }
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Kernel-owned cross-process advisory lock (`flock` / `LockFileEx`), automatically released on drop or crash.
struct FileLock {
    _file: fs::File,
}

impl FileLock {
    fn acquire(target: &Path) -> Result<Self, String> {
        let fname = target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "state".to_string());
        let parent = target.parent().ok_or_else(|| "잠금 상위 디렉터리 없음".to_string())?;
        let lock_path = parent.join(format!(".{fname}.mistology.lock"));
        if let Ok(meta) = fs::symlink_metadata(&lock_path) {
            if meta.file_type().is_symlink() {
                return Err("심볼릭 링크 잠금 파일 거부".to_string());
            }
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts
            .open(&lock_path)
            .map_err(|e| format!("잠금 파일 열기 실패: {e}"))?;

        for attempt in 0..25 {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(fs::TryLockError::WouldBlock) => {
                    if attempt == 24 {
                        return Err("동시 잠금 대기 시간 초과".to_string());
                    }
                    std::thread::sleep(Duration::from_millis(8));
                }
                Err(fs::TryLockError::Error(e)) => {
                    return Err(format!("잠금 획득 실패: {e}"));
                }
            }
        }
        Err("잠금 획득 실패".to_string())
    }
}

pub fn format_hook_command_for_platform(
    exe: &str,
    agent: HookAgent,
    event: HookEvent,
    windows: bool,
) -> Option<String> {
    if exe.is_empty() || exe.contains('\0') || exe.contains('\n') || exe.contains('\r') {
        return None;
    }
    let suffix = format!("hook {} {}", agent.as_str(), event.as_str());
    if exe == "mistology" {
        return Some(format!("mistology {suffix}"));
    }
    if windows {
        // Reject shell/PowerShell/cmd metacharacters (including % and ! expansion)
        if exe.bytes().any(|b| {
            matches!(
                b,
                b'"' | b'\''
                    | b'%'
                    | b'!'
                    | b'^'
                    | b'&'
                    | b'|'
                    | b'<'
                    | b'>'
                    | b';'
                    | b'$'
                    | b'`'
                    | b'('
                    | b')'
                    | b'{'
                    | b'}'
            )
        }) {
            return None;
        }
        if !exe.contains(' ') {
            if agent == HookAgent::Gemini {
                // Antigravity uses `cmd /c` on Windows
                Some(format!("{exe} {suffix}"))
            } else {
                // Claude/Codex on Windows may run under Git Bash or PowerShell: forward slashes avoid backslash escaping
                let fwd = exe.replace('\\', "/");
                Some(format!("{fwd} {suffix}"))
            }
        } else if agent == HookAgent::Gemini {
            // Antigravity invokes hooks via `cmd /c` on Windows
            Some(format!("\"{exe}\" {suffix}"))
        } else {
            // Claude/Codex on Windows may use PowerShell where bare `"path" args` is a string literal
            None
        }
    } else {
        let escaped = exe.replace('\'', "'\\''");
        Some(format!("'{escaped}' {suffix}"))
    }
}

fn resolve_setup_exe(custom_home: bool) -> Option<String> {
    if custom_home {
        return Some("mistology".to_string());
    }
    let exe = std::env::current_exe().ok()?;
    let s = exe.to_string_lossy().to_string();
    if s.contains("/deps/") || s.contains("\\deps\\") {
        return Some("mistology".to_string());
    }
    Some(s)
}

fn parse_single_shell_word(head: &str) -> Option<(String, bool)> {
    let bytes = head.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if bytes[0] == b'\'' {
        // Strict single-quoted Unix word: internal `'` ONLY permitted as `'\''`
        let mut out = String::new();
        let mut i = 1usize;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                if i + 1 == bytes.len() {
                    return Some((out, false));
                }
                if bytes.get(i..i + 4) == Some(b"'\\''") {
                    out.push('\'');
                    i += 4;
                    continue;
                }
                // Closing quote followed by extra token (e.g. `'env' 'mistology'`) -> NOT a single word!
                return None;
            }
            let ch = head[i..].chars().next()?;
            out.push(ch);
            i += ch.len_utf8();
        }
        None
    } else if bytes[0] == b'"' {
        // Strict double-quoted Windows word: no internal quotes, must close at end
        if bytes.len() < 2 || bytes[bytes.len() - 1] != b'"' {
            return None;
        }
        let inner = &head[1..head.len() - 1];
        if inner.contains('"') {
            return None;
        }
        Some((inner.to_string(), true))
    } else {
        // Unquoted single word (bare `mistology` or space-free Windows path / UNC)
        if head.chars().any(char::is_whitespace) {
            return None;
        }
        let is_win = head.contains('\\')
            || head.contains(':')
            || head.starts_with("//")
            || head.to_ascii_lowercase().ends_with(".exe");
        Some((head.to_string(), is_win))
    }
}

fn is_installable_exe_identity(exe: &str) -> bool {
    if exe.eq_ignore_ascii_case("mistology") || exe.eq_ignore_ascii_case("mistology.exe") {
        return true;
    }
    let has_drive = exe.len() >= 3
        && exe.as_bytes()[0].is_ascii_alphabetic()
        && exe.as_bytes()[1] == b':'
        && matches!(exe.as_bytes()[2], b'/' | b'\\');
    exe.starts_with('/') || exe.starts_with('\\') || has_drive
}

fn is_managed_command(cmd: &str, agent: HookAgent, event: HookEvent) -> bool {
    let trimmed = cmd.trim();
    let expected_suffix = format!(" hook {} {}", agent.as_str(), event.as_str());
    let Some(head_raw) = trimmed.strip_suffix(&expected_suffix) else {
        return false;
    };
    let Some((exe, is_win)) = parse_single_shell_word(head_raw.trim()) else {
        return false;
    };
    if !is_installable_exe_identity(&exe) {
        return false;
    }
    let slash = exe.replace('\\', "/");
    let base = slash.rsplit('/').next().unwrap_or(&slash);
    if !(base.eq_ignore_ascii_case("mistology") || base.eq_ignore_ascii_case("mistology.exe")) {
        return false;
    }
    // Round-trip verification guarantees exact generated command ownership
    format_hook_command_for_platform(&exe, agent, event, is_win).as_deref() == Some(trimmed)
}

/// Checks whether a leaf handler object is an EXACT generated MISTOLOGY leaf for `(agent, event)`
/// (exact 3 keys: `type == "command"`, `timeout == HOOK_TIMEOUT_SECS`, `command` matching `(agent, event)`).
fn is_exact_managed_leaf(item: &Value, agent: HookAgent, event: HookEvent) -> bool {
    let Some(obj) = item.as_object() else {
        return false;
    };
    if obj.len() != 3 {
        return false;
    }
    obj.get("type").and_then(Value::as_str) == Some("command")
        && obj.get("timeout").and_then(Value::as_u64) == Some(HOOK_TIMEOUT_SECS)
        && obj
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| is_managed_command(c, agent, event))
}

#[derive(Default)]
struct SessionState {
    turn_id: u64,
    turn_key: String,
    last_prompt_hash: u64,
    turn_started_at: u64,
    baseline_dirty_map: BTreeMap<String, u64>,
    baseline_complete: bool,
    baseline_notes_digest: u64,
    saw_tool_error: bool,
    stop_nudged_key: Option<String>,
    pending_note_ids: Vec<String>,
    updated_at: u64,
}

impl SessionState {
    fn from_value(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        let updated_at = obj.get("updated_at").and_then(Value::as_u64).unwrap_or(0);
        if updated_at > 0 && now_unix_secs().saturating_sub(updated_at) > STATE_TTL_SECS {
            return None;
        }
        let pending_note_ids = obj
            .get("pending_note_ids")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .filter(|id| note::is_valid_id(id))
                    .take(MAX_PENDING_NOTE_IDS)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let baseline_dirty_map = obj
            .get("baseline_dirty_map")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .take(MAX_BASELINE_ENTRIES)
                    .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            turn_id: obj.get("turn_id").and_then(Value::as_u64).unwrap_or(0),
            turn_key: obj
                .get("turn_key")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            last_prompt_hash: obj.get("last_prompt_hash").and_then(Value::as_u64).unwrap_or(0),
            turn_started_at: obj.get("turn_started_at").and_then(Value::as_u64).unwrap_or(0),
            baseline_dirty_map,
            baseline_complete: obj
                .get("baseline_complete")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            baseline_notes_digest: obj
                .get("baseline_notes_digest")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            saw_tool_error: obj
                .get("saw_tool_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            stop_nudged_key: obj
                .get("stop_nudged_key")
                .and_then(Value::as_str)
                .map(String::from),
            pending_note_ids,
            updated_at,
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "turn_id": self.turn_id,
            "turn_key": self.turn_key,
            "last_prompt_hash": self.last_prompt_hash,
            "turn_started_at": self.turn_started_at,
            "baseline_dirty_map": self.baseline_dirty_map,
            "baseline_complete": self.baseline_complete,
            "baseline_notes_digest": self.baseline_notes_digest,
            "saw_tool_error": self.saw_tool_error,
            "stop_nudged_key": self.stop_nudged_key,
            "pending_note_ids": self.pending_note_ids,
            "updated_at": self.updated_at,
        })
    }
}

fn resolve_state_dir(home_override: Option<&Path>) -> Option<(PathBuf, String)> {
    if let Some(h) = home_override {
        if !h.is_absolute() {
            return None;
        }
        return Some((h.to_path_buf(), ".mistology-hook-state".to_string()));
    }
    let abs_env = |k: &str| {
        std::env::var_os(k)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if cfg!(windows) {
        abs_env("LOCALAPPDATA")
            .map(|p| (p, "mistology/hook-state".to_string()))
            .or_else(|| abs_env("USERPROFILE").map(|p| (p, ".mistology-hook-state".to_string())))
    } else {
        abs_env("XDG_STATE_HOME")
            .map(|p| (p, "mistology/hook-state".to_string()))
            .or_else(|| abs_env("HOME").map(|p| (p, ".cache/mistology/hook-state".to_string())))
    }
}

fn verify_and_prepare_private_dir(base: &Path, rel: &str) -> Result<PathBuf, String> {
    if !base.is_absolute() {
        return Err("비절대 상태 경로 거부".to_string());
    }
    let base_meta = fs::symlink_metadata(base).map_err(|e| e.to_string())?;
    if !base_meta.is_dir() {
        return Err("상태 기본 경로가 디렉터리가 아님".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Reject shared group- or world-writable roots (e.g. /tmp)
        if base_meta.mode() & 0o022 != 0 {
            return Err("공유 쓰기 가능 디렉터리 거부".to_string());
        }
    }
    if path_has_symlink_under(base, rel) {
        return Err("심볼릭 링크 상태 경로 거부".to_string());
    }
    let mut cur = base.to_path_buf();
    for comp in Path::new(rel).iter() {
        cur.push(comp);
        if !cur.exists() {
            fs::create_dir(&cur).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&cur, fs::Permissions::from_mode(0o700))
                    .map_err(|e| e.to_string())?;
            }
        }
        let meta = fs::symlink_metadata(&cur).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("상태 디렉터리 타입 불일치".to_string());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != base_meta.uid() {
                return Err("상태 디렉터리 소유자 불일치".to_string());
            }
            if meta.mode() & 0o022 != 0 {
                return Err("상태 디렉터리 그룹/타인 쓰기 권한 허용됨".to_string());
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        // Obtain kernel-verified effective UID via an exclusive probe file inside `cur`
        let probe = cur.join(format!(
            ".mistology-euid-{}-{}",
            std::process::id(),
            TMP_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true).mode(0o600);
        let f = opts.open(&probe).map_err(|e| e.to_string())?;
        let euid = f.metadata().map_err(|e| e.to_string())?.uid();
        drop(f);
        let _ = fs::remove_file(&probe);
        if base_meta.uid() != euid {
            return Err("기본 상태 디렉터리 소유자 불일치".to_string());
        }
        let leaf_meta = fs::symlink_metadata(&cur).map_err(|e| e.to_string())?;
        if leaf_meta.uid() != euid {
            return Err("상태 디렉터리 소유자 불일치".to_string());
        }
    }
    Ok(cur)
}

fn verified_state_file_path(
    home_override: Option<&Path>,
    agent: HookAgent,
    session_id: &str,
    root: &Path,
) -> Option<PathBuf> {
    if session_id.trim().is_empty() {
        return None;
    }
    let (base, rel) = resolve_state_dir(home_override)?;
    let dir = verify_and_prepare_private_dir(&base, &rel).ok()?;
    let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let key = format!(
        "{}\0{}\0{}",
        agent.as_str(),
        session_id.trim(),
        root_canon.to_string_lossy()
    );
    let h = fnv1a_64(key.as_bytes());
    let p = dir.join(format!("{h:016x}.json"));
    if let Ok(meta) = fs::symlink_metadata(&p) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return None;
        }
    }
    Some(p)
}

fn load_session_state_checked(path: &Path) -> Option<SessionState> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    let val = serde_json::from_str::<Value>(&raw).ok()?;
    SessionState::from_value(&val)
}

fn save_session_state(path: &Path, state: &mut SessionState) -> Result<(), String> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err("심볼릭 링크 상태 파일 쓰기 거부".to_string());
        }
    }
    state.updated_at = now_unix_secs();
    let payload = serde_json::to_string(&state.to_value()).map_err(|e| e.to_string())?;
    atomic_write_private(path, payload.as_bytes(), false)
}

fn atomic_write_checked(
    target: &Path,
    bytes: &[u8],
    preserve_existing_perms: bool,
    expected_digest: Option<Option<u64>>,
) -> Result<(), String> {
    let parent = target.parent().ok_or_else(|| "대상 상위 경로 없음".to_string())?;
    let existing_perms = if preserve_existing_perms {
        fs::metadata(target).ok().map(|m| m.permissions())
    } else {
        None
    };

    let pid = std::process::id();
    let mut tmp_path = PathBuf::new();
    let mut file = None;
    for _ in 0..16 {
        let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
        let cand = parent.join(format!(".mistology-tmp-{pid}-{nonce}"));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&cand) {
            Ok(f) => {
                tmp_path = cand;
                file = Some(f);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("임시 파일 생성 실패: {e}")),
        }
    }
    let mut f = file.ok_or_else(|| "임시 파일 생성 재시도 초과".to_string())?;
    if let Err(e) = f.write_all(bytes).and_then(|()| f.flush()) {
        drop(f);
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("임시 파일 쓰기 실패: {e}"));
    }
    drop(f);
    if let Some(perms) = existing_perms {
        if let Err(e) = fs::set_permissions(&tmp_path, perms) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("권한 보존 실패: {e}"));
        }
    }
    if let Some(exp) = expected_digest {
        if let Err(reason) = verify_unchanged_before_commit(target, exp) {
            let _ = fs::remove_file(&tmp_path);
            return Err(reason.to_string());
        }
    }
    if let Err(e) = fs::rename(&tmp_path, target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("원자적 교체 실패: {e}"));
    }
    Ok(())
}

fn atomic_write_private(
    target: &Path,
    bytes: &[u8],
    preserve_existing_perms: bool,
) -> Result<(), String> {
    atomic_write_checked(target, bytes, preserve_existing_perms, None)
}

fn path_stat_hash(root: &Path, rel: &str) -> u64 {
    let p = root.join(rel);
    if let Ok(meta) = fs::symlink_metadata(&p) {
        let len = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        fnv1a_64(format!("{len}:{mtime}").as_bytes())
    } else {
        fnv1a_64(b"deleted")
    }
}

fn opaque_path_key(rel: &str) -> String {
    format!("{:016x}", fnv1a_64(rel.as_bytes()))
}

fn snapshot_current_dirty_map(root: &Path, cwd: &Path) -> (BTreeMap<String, u64>, bool) {
    let Ok(sum) = impact::run_impact_detailed(root, cwd, None, &[]) else {
        return (BTreeMap::new(), false);
    };
    if sum.changed_paths.len() > MAX_BASELINE_ENTRIES {
        return (BTreeMap::new(), false);
    }
    let map = sum
        .changed_paths
        .iter()
        .map(|rel| (opaque_path_key(rel), path_stat_hash(root, rel)))
        .collect();
    (map, true)
}

fn notes_dir_digest(root: &Path) -> u64 {
    let notes_d = root.join(".mistology").join("notes");
    let Ok(entries) = fs::read_dir(&notes_d) else {
        return 0;
    };
    let mut items: Vec<String> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("md") {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if let Ok(meta) = fs::symlink_metadata(&p) {
                let len = meta.len();
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                items.push(format!("{name}:{len}:{mtime}"));
            }
        }
    }
    items.sort();
    fnv1a_64(items.join("\n").as_bytes())
}

fn format_brief_for_note_ids(root: &Path, note_ids: &[String]) -> Option<String> {
    let notes_d = root.join(".mistology").join("notes");
    let mut lines = Vec::new();
    for nid in note_ids.iter().take(MAX_PENDING_NOTE_IDS) {
        if !note::is_valid_id(nid) {
            continue;
        }
        let p = notes_d.join(format!("{nid}.md"));
        let Ok(raw) = fs::read_to_string(&p) else {
            continue;
        };
        let Ok((meta, body)) = note::parse(&raw) else {
            continue;
        };
        if meta.status != "active" {
            continue;
        }
        let sum = note::extract_summary(&body);
        let badge = if meta.kind == "canon" { " ★표준" } else { "" };
        let rule_lbl = if meta.kind == "canon" { "표준 규약" } else { "정답" };
        let files = meta.files.join(", ");
        let mut card = format!(
            "[{nid} {}{badge}] {files}\n- {rule_lbl}: {}",
            meta.current, sum.fix
        );
        if !sum.forbid.is_empty() {
            card.push_str(&format!("\n- 금지: {}", sum.forbid));
        }
        lines.push(card);
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn brief_search_hits_readonly(root: &Path, cwd: &Path, query: &str) -> (Vec<String>, Option<String>) {
    let q = query.trim();
    if q.is_empty() || !root.join(".mistology").join("notes").is_dir() {
        return (Vec::new(), None);
    }
    let scored = index::search_scored_readonly(root, cwd, q, MAX_PENDING_NOTE_IDS);
    if scored.is_empty() {
        return (Vec::new(), None);
    }
    let ids: Vec<String> = scored
        .iter()
        .map(|s| s.hit.id.clone())
        .filter(|id| note::is_valid_id(id))
        .take(MAX_PENDING_NOTE_IDS)
        .collect();
    let brief = scored
        .iter()
        .map(|s| index::format_brief(&s.hit))
        .collect::<Vec<_>>()
        .join("\n");
    (ids, Some(brief))
}

fn check_turn_events_and_notes(
    root: &Path,
    turn_started_at: u64,
    baseline_notes_digest: u64,
) -> (bool, bool) {
    let mut ran_impact = false;
    let wrote_note = notes_dir_digest(root) != baseline_notes_digest;

    let events_path = root.join(".mistology").join("events.jsonl");
    if let Ok(raw) = fs::read_to_string(&events_path) {
        for line in raw.lines().rev().take(64) {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let ts = v.get("ts").and_then(Value::as_u64).unwrap_or(0);
            if ts < turn_started_at {
                continue;
            }
            if !v.get("err").is_none_or(Value::is_null) {
                continue;
            }
            if v.get("cmd").and_then(Value::as_str) == Some("impact") {
                ran_impact = true;
            }
        }
    }

    (ran_impact, wrote_note)
}

fn extract_payload_cwd(payload: &Value, fallback_cwd: &Path) -> PathBuf {
    if let Some(s) = payload
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        return PathBuf::from(s);
    }
    if let Some(s) = payload
        .get("workspacePaths")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        return PathBuf::from(s);
    }
    fallback_cwd.to_path_buf()
}

fn extract_session_id(payload: &Value) -> Option<&str> {
    payload
        .get("conversationId")
        .or_else(|| payload.get("session_id"))
        .or_else(|| payload.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn extract_payload_turn_id(payload: &Value) -> Option<&str> {
    payload
        .get("turn_id")
        .or_else(|| payload.get("turnId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn gemini_inject(msg: String) -> String {
    format!(
        "{}\n",
        json!({
            "injectSteps": [
                {
                    "systemMessage": {
                        "systemMessage": msg
                    }
                }
            ]
        })
    )
}

fn claude_inject(event_name: &str, ctx: String) -> String {
    format!(
        "{}\n",
        json!({
            "hookSpecificOutput": {
                "hookEventName": event_name,
                "additionalContext": ctx
            }
        })
    )
}

fn evaluate_stop_nudge(
    root: &Path,
    cwd: &Path,
    state_path: &Path,
    explicit_turn_id: Option<&str>,
) -> Option<String> {
    // Serialize Stop claim across simultaneous processes
    let _lock = FileLock::acquire(state_path).ok()?;
    // Fail-open on missing or corrupt session state — NEVER invent a turn!
    let mut state = load_session_state_checked(state_path)?;
    if state.turn_id == 0 || state.turn_key.is_empty() || !state.baseline_complete {
        return None;
    }
    // If host sent an explicit turn_id (e.g. Codex), it MUST match the active stored turn!
    if let Some(tid) = explicit_turn_id {
        if state.turn_key != tid {
            return None;
        }
    }
    let claim_key = state.turn_key.clone();
    if state.stop_nudged_key.as_deref() == Some(claim_key.as_str()) {
        return None;
    }

    let full_summary = impact::run_impact_detailed(root, cwd, None, &[]).ok()?;
    if full_summary.changed == 0 {
        return None;
    }
    // Compute exact per-path delta edited during THIS turn (ignoring pre-existing dirty files!)
    let edited_this_turn: Vec<String> = full_summary
        .changed_paths
        .iter()
        .filter(|rel| {
            state
                .baseline_dirty_map
                .get(&opaque_path_key(rel))
                .copied()
                != Some(path_stat_hash(root, rel))
        })
        .cloned()
        .collect();
    if edited_this_turn.is_empty() {
        return None;
    }

    let turn_summary = impact::run_impact_detailed(root, root, None, &edited_this_turn).ok()?;
    let (ran_impact, wrote_note) =
        check_turn_events_and_notes(root, state.turn_started_at, state.baseline_notes_digest);
    let need_impact = turn_summary.unchecked_count > 0 && !ran_impact;
    let need_note = state.saw_tool_error && !wrote_note;
    if !need_impact && !need_note {
        return None;
    }

    // Claim atomically BEFORE emitting: if persistence fails, fail-open so we can never loop.
    state.stop_nudged_key = Some(claim_key);
    if save_session_state(state_path, &mut state).is_err() {
        return None;
    }

    let mut parts = Vec::new();
    if need_impact {
        parts.push(format!(
            "[MISTOLOGY 파급 점검 필요] 이번 턴 수정 파일과 결합된 미수정 파일 {}건이 감지되었습니다. 계약·테스트를 확인하세요:\n{}",
            turn_summary.unchecked_count,
            turn_summary.report.trim_end()
        ));
    }
    if need_note {
        parts.push(format!(
            "[MISTOLOGY 기록 확인] 이번 턴에서 명령/도구 에러를 겪고 코드({}개 파일)를 수정했으나 아직 오답·규약 기록이 없습니다. 원인과 해결책이 검증됐다면 `mistology new`(또는 `supersede`/`canon`)로 남기고 마무리하세요. (단순 오타·미검증 수정이면 기록 없이 바로 종료해도 됩니다.)",
            edited_this_turn.len()
        ));
    }
    Some(parts.join("\n\n"))
}

fn extract_tool_failure_query(payload: &Value, agent: HookAgent) -> Option<String> {
    match agent {
        HookAgent::Gemini => {
            let err = payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if err.is_empty() {
                return None;
            }
            let cmd = payload
                .get("toolCall")
                .and_then(|t| t.get("args"))
                .and_then(|a| a.get("CommandLine"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if cmd.is_empty() || cmd.len() > 240 {
                Some(err.to_string())
            } else {
                Some(format!("{err} {cmd}"))
            }
        }
        HookAgent::Claude => {
            let err = payload
                .get("error")
                .or_else(|| payload.get("tool_error"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if err.is_empty() {
                None
            } else {
                Some(err.to_string())
            }
        }
        HookAgent::Codex => {
            if let Some(err) = payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(err.to_string());
            }
            let resp = payload.get("tool_response")?;
            let is_err = resp
                .get("isError")
                .or_else(|| resp.get("is_error"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || resp
                    .get("exit_code")
                    .or_else(|| resp.get("exitCode"))
                    .and_then(Value::as_i64)
                    .is_some_and(|code| code != 0);
            if !is_err {
                return None;
            }
            let mcp_text = resp
                .get("content")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.get("text").and_then(Value::as_str))
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|s| !s.is_empty());
            let text = mcp_text
                .as_deref()
                .or_else(|| {
                    resp.get("stderr")
                        .or_else(|| resp.get("output"))
                        .or_else(|| resp.get("error"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or("");
            if text.is_empty() {
                Some("command failed".to_string())
            } else {
                Some(text.chars().take(600).collect())
            }
        }
    }
}

pub fn run_hook(
    process_cwd: &Path,
    agent: HookAgent,
    explicit_event: Option<HookEvent>,
    stdin_raw: &str,
    home_override: Option<&Path>,
) -> String {
    let Ok(payload) = serde_json::from_str::<Value>(stdin_raw) else {
        return "{}\n".to_string();
    };
    let Some(event) = explicit_event.or_else(|| {
        payload
            .get("hookEventName")
            .or_else(|| payload.get("hook_event_name"))
            .and_then(Value::as_str)
            .and_then(HookEvent::from_payload_name)
    }) else {
        return "{}\n".to_string();
    };

    let cwd = extract_payload_cwd(&payload, process_cwd);
    let root = note::find_root(&cwd);
    let active_workspace = root.join(".mistology").is_dir() || root.join(".git").exists();
    if !active_workspace {
        return "{}\n".to_string();
    }

    let session_id = extract_session_id(&payload);
    let state_path =
        session_id.and_then(|sid| verified_state_file_path(home_override, agent, sid, &root));

    match (agent, event) {
        (HookAgent::Gemini, HookEvent::SessionStart) => {
            gemini_inject(SESSION_CONTRACT_MSG.to_string())
        }
        (HookAgent::Claude | HookAgent::Codex, HookEvent::SessionStart | HookEvent::SubagentStart) => {
            claude_inject(event.claude_event_name(), SESSION_CONTRACT_MSG.to_string())
        }
        (HookAgent::Gemini, HookEvent::PreInvocation) => {
            let mut injected = Vec::new();
            let prompt = payload
                .get("lastUserInput")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");

            if let Some(sp) = state_path.as_deref() {
                if let Ok(_lock) = FileLock::acquire(sp) {
                    let mut state = load_session_state_checked(sp).unwrap_or_default();
                    let mut dirty_state = false;

                    if !state.pending_note_ids.is_empty() {
                        let ids = std::mem::take(&mut state.pending_note_ids);
                        dirty_state = true;
                        if let Some(brief) = format_brief_for_note_ids(&root, &ids) {
                            injected.push(format!(
                                "[MISTOLOGY 직전 에러 관련 오답·표준 규약]\n{brief}"
                            ));
                        }
                    }

                    if !prompt.is_empty() {
                        let p_hash = fnv1a_64(prompt.as_bytes());
                        if state.last_prompt_hash != p_hash {
                            state.turn_id = state.turn_id.saturating_add(1);
                            state.turn_key = format!("t{}", state.turn_id);
                            state.last_prompt_hash = p_hash;
                            state.turn_started_at = now_unix_secs();
                            let (bmap, bcomplete) = snapshot_current_dirty_map(&root, &cwd);
                            state.baseline_dirty_map = bmap;
                            state.baseline_complete = bcomplete;
                            state.baseline_notes_digest = notes_dir_digest(&root);
                            state.saw_tool_error = false;
                            state.stop_nudged_key = None;
                            dirty_state = true;

                            let (_, brief_opt) = brief_search_hits_readonly(&root, &cwd, prompt);
                            if let Some(brief) = brief_opt {
                                injected.push(format!(
                                    "[MISTOLOGY 작업 전 관련 오답·★표준 규약]\n{brief}"
                                ));
                            }
                        }
                    }

                    if dirty_state {
                        let _ = save_session_state(sp, &mut state);
                    }
                }
            } else if !prompt.is_empty()
                && payload
                    .get("invocationNum")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| n <= 1)
            {
                let (_, brief_opt) = brief_search_hits_readonly(&root, &cwd, prompt);
                if let Some(brief) = brief_opt {
                    injected.push(format!(
                        "[MISTOLOGY 작업 전 관련 오답·★표준 규약]\n{brief}"
                    ));
                }
            }

            if injected.is_empty() {
                "{}\n".to_string()
            } else {
                gemini_inject(injected.join("\n\n"))
            }
        }
        (HookAgent::Claude | HookAgent::Codex, HookEvent::UserPromptSubmit) => {
            let prompt = payload
                .get("prompt")
                .or_else(|| payload.get("lastUserInput"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if let Some(sp) = state_path.as_deref() {
                if let Ok(_lock) = FileLock::acquire(sp) {
                    let mut state = load_session_state_checked(sp).unwrap_or_default();
                    let incoming_tid = extract_payload_turn_id(&payload);
                    let duplicate_turn = incoming_tid
                        .is_some_and(|tid| !state.turn_key.is_empty() && state.turn_key == tid);
                    if !duplicate_turn {
                        state.turn_id = state.turn_id.saturating_add(1);
                        state.turn_key = incoming_tid
                            .map(String::from)
                            .unwrap_or_else(|| format!("t{}", state.turn_id));
                        state.last_prompt_hash = fnv1a_64(prompt.as_bytes());
                        state.turn_started_at = now_unix_secs();
                        let (bmap, bcomplete) = snapshot_current_dirty_map(&root, &cwd);
                        state.baseline_dirty_map = bmap;
                        state.baseline_complete = bcomplete;
                        state.baseline_notes_digest = notes_dir_digest(&root);
                        state.saw_tool_error = false;
                        state.stop_nudged_key = None;
                        state.pending_note_ids.clear();
                        let _ = save_session_state(sp, &mut state);
                    }
                }
            }
            let (_, brief_opt) = brief_search_hits_readonly(&root, &cwd, prompt);
            match brief_opt {
                Some(brief) => claude_inject(
                    event.claude_event_name(),
                    format!("[MISTOLOGY 작업 전 관련 오답·★표준 규약]\n{brief}"),
                ),
                None => "{}\n".to_string(),
            }
        }
        (HookAgent::Gemini, HookEvent::PostToolUse) => {
            if let Some(err_q) = extract_tool_failure_query(&payload, HookAgent::Gemini) {
                if let Some(sp) = state_path.as_deref() {
                    if let Ok(_lock) = FileLock::acquire(sp) {
                        if let Some(mut state) = load_session_state_checked(sp) {
                            state.saw_tool_error = true;
                            let (ids, _) = brief_search_hits_readonly(&root, &cwd, &err_q);
                            if !ids.is_empty() {
                                state.pending_note_ids = ids;
                            }
                            let _ = save_session_state(sp, &mut state);
                        }
                    }
                }
            }
            "{}\n".to_string()
        }
        (HookAgent::Claude, HookEvent::PostToolUseFailure)
        | (HookAgent::Codex, HookEvent::PostToolUse | HookEvent::PostToolUseFailure) => {
            let Some(err_q) = extract_tool_failure_query(&payload, agent) else {
                return "{}\n".to_string();
            };
            if let Some(sp) = state_path.as_deref() {
                if let Ok(_lock) = FileLock::acquire(sp) {
                    if let Some(mut state) = load_session_state_checked(sp) {
                        let turn_matches = extract_payload_turn_id(&payload)
                            .is_none_or(|tid| state.turn_key == tid);
                        if turn_matches {
                            state.saw_tool_error = true;
                            let _ = save_session_state(sp, &mut state);
                        }
                    }
                }
            }
            let (_, brief_opt) = brief_search_hits_readonly(&root, &cwd, &err_q);
            match brief_opt {
                Some(brief) => claude_inject(
                    event.claude_event_name(),
                    format!("[MISTOLOGY 에러 관련 과거 오답·★표준 규약]\n{brief}"),
                ),
                None => "{}\n".to_string(),
            }
        }
        (HookAgent::Gemini, HookEvent::Stop) => {
            match payload.get("continuationActive") {
                Some(Value::Bool(false)) | None => {}
                _ => return "{}\n".to_string(),
            }
            match payload.get("fullyIdle") {
                Some(Value::Bool(true)) | None => {}
                _ => return "{}\n".to_string(),
            }
            let reason = payload
                .get("terminationReason")
                .and_then(Value::as_str)
                .unwrap_or("");
            if reason != "NO_TOOL_CALL" && reason != "model_stop" {
                return "{}\n".to_string();
            }
            let Some(sp) = state_path.as_deref() else {
                return "{}\n".to_string();
            };
            match evaluate_stop_nudge(&root, &cwd, sp, extract_payload_turn_id(&payload)) {
                Some(msg) => format!(
                    "{}\n",
                    json!({
                        "decision": "continue",
                        "reason": msg
                    })
                ),
                None => "{}\n".to_string(),
            }
        }
        (HookAgent::Claude | HookAgent::Codex, HookEvent::Stop) => {
            match payload.get("stop_hook_active") {
                Some(Value::Bool(false)) | None => {}
                _ => return "{}\n".to_string(),
            }
            let Some(sp) = state_path.as_deref() else {
                return "{}\n".to_string();
            };
            match evaluate_stop_nudge(&root, &cwd, sp, extract_payload_turn_id(&payload)) {
                Some(msg) => format!(
                    "{}\n",
                    json!({
                        "decision": "block",
                        "reason": msg
                    })
                ),
                None => "{}\n".to_string(),
            }
        }
        _ => "{}\n".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Strict JSON Duplicate-Key Scanner + Safe Config Merge / Uninstall / Doctor
// ---------------------------------------------------------------------------

struct JsonScanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonScanner<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    fn parse_string(&mut self) -> Result<String, &'static str> {
        self.skip_ws();
        let start = self.pos;
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err("JSON 파싱 실패");
        }
        self.pos += 1;
        let mut escaped = false;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                let slice = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| "JSON 파싱 실패")?;
                return serde_json::from_str::<String>(slice).map_err(|_| "JSON 파싱 실패");
            }
        }
        Err("JSON 파싱 실패")
    }

    fn check_value(&mut self, depth: usize) -> Result<bool, &'static str> {
        if depth > 64 {
            return Err("JSON 중첩 깊이 초과");
        }
        self.skip_ws();
        match self.bytes.get(self.pos) {
            Some(b'{') => self.check_object(depth + 1),
            Some(b'[') => self.check_array(depth + 1),
            Some(b'"') => {
                let _ = self.parse_string()?;
                Ok(false)
            }
            Some(_) => {
                while self.pos < self.bytes.len()
                    && !matches!(
                        self.bytes[self.pos],
                        b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'
                    )
                {
                    self.pos += 1;
                }
                Ok(false)
            }
            None => Err("JSON 파싱 실패"),
        }
    }

    fn check_object(&mut self, depth: usize) -> Result<bool, &'static str> {
        self.pos += 1; // '{'
        self.skip_ws();
        let mut keys = HashSet::new();
        let mut has_dup = false;
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(false);
        }
        loop {
            let key = self.parse_string()?;
            if !keys.insert(key) {
                has_dup = true;
            }
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b':') {
                return Err("JSON 파싱 실패");
            }
            self.pos += 1;
            if self.check_value(depth)? {
                has_dup = true;
            }
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(has_dup);
                }
                _ => return Err("JSON 파싱 실패"),
            }
        }
    }

    fn check_array(&mut self, depth: usize) -> Result<bool, &'static str> {
        self.pos += 1; // '['
        self.skip_ws();
        let mut has_dup = false;
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(false);
        }
        loop {
            if self.check_value(depth)? {
                has_dup = true;
            }
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(has_dup);
                }
                _ => return Err("JSON 파싱 실패"),
            }
        }
    }
}

fn json_has_duplicate_keys(raw: &str) -> Result<bool, &'static str> {
    let mut sc = JsonScanner {
        bytes: raw.as_bytes(),
        pos: 0,
    };
    sc.check_value(0)
}

fn path_has_symlink_under(base: &Path, rel: &str) -> bool {
    let mut cur = base.to_path_buf();
    for comp in Path::new(rel).iter() {
        cur.push(comp);
        if let Ok(meta) = fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return true;
            }
        }
    }
    false
}

fn read_json_object_strict(path: &Path) -> Result<(Map<String, Value>, Option<u64>), &'static str> {
    if !path.exists() {
        return Ok((Map::new(), None));
    }
    let raw = fs::read_to_string(path).map_err(|_| "읽기 실패")?;
    let digest = fnv1a_64(raw.as_bytes());
    if raw.trim().is_empty() {
        return Ok((Map::new(), Some(digest)));
    }
    let val: Value = serde_json::from_str(&raw).map_err(|_| "JSON 파싱 실패")?;
    if json_has_duplicate_keys(&raw)? {
        return Err("중복 JSON 키 감지");
    }
    match val {
        Value::Object(m) => Ok((m, Some(digest))),
        _ => Err("JSON 루트가 객체가 아님"),
    }
}

fn verify_unchanged_before_commit(target: &Path, expected_digest: Option<u64>) -> Result<(), &'static str> {
    match (expected_digest, target.exists()) {
        (None, false) => Ok(()),
        (None, true) => Err("동시 수정 감지 (새 파일 생성됨)"),
        (Some(_), false) => Err("동시 수정 감지 (원본 삭제됨)"),
        (Some(exp), true) => {
            let cur_raw = fs::read_to_string(target).map_err(|_| "원본 재검증 읽기 실패")?;
            if fnv1a_64(cur_raw.as_bytes()) == exp {
                Ok(())
            } else {
                Err("동시 수정 감지 (원본 변경됨)")
            }
        }
    }
}

fn is_managed_gemini_group(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    if obj.len() != 4 {
        return false;
    }
    let flat_ok = |key: &str, ev: HookEvent| -> bool {
        obj.get(key)
            .and_then(Value::as_array)
            .is_some_and(|arr| arr.len() == 1 && is_exact_managed_leaf(&arr[0], HookAgent::Gemini, ev))
    };
    if !(flat_ok("SessionStart", HookEvent::SessionStart)
        && flat_ok("PreInvocation", HookEvent::PreInvocation)
        && flat_ok("Stop", HookEvent::Stop))
    {
        return false;
    }
    obj.get("PostToolUse")
        .and_then(Value::as_array)
        .is_some_and(|arr| {
            arr.len() == 1
                && arr[0].as_object().is_some_and(|g| {
                    g.len() == 2
                        && g.get("matcher").and_then(Value::as_str) == Some("run_command")
                        && g.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
                            hs.len() == 1
                                && is_exact_managed_leaf(&hs[0], HookAgent::Gemini, HookEvent::PostToolUse)
                        })
                })
        })
}

fn build_gemini_mistology_group(exe: &str) -> Option<Value> {
    let ss = format_hook_command_for_platform(exe, HookAgent::Gemini, HookEvent::SessionStart, cfg!(windows))?;
    let pre = format_hook_command_for_platform(exe, HookAgent::Gemini, HookEvent::PreInvocation, cfg!(windows))?;
    let post = format_hook_command_for_platform(exe, HookAgent::Gemini, HookEvent::PostToolUse, cfg!(windows))?;
    let stop = format_hook_command_for_platform(exe, HookAgent::Gemini, HookEvent::Stop, cfg!(windows))?;
    Some(json!({
        "SessionStart": [{ "type": "command", "command": ss, "timeout": HOOK_TIMEOUT_SECS }],
        "PreInvocation": [{ "type": "command", "command": pre, "timeout": HOOK_TIMEOUT_SECS }],
        "PostToolUse": [{
            "matcher": "run_command",
            "hooks": [{ "type": "command", "command": post, "timeout": HOOK_TIMEOUT_SECS }]
        }],
        "Stop": [{ "type": "command", "command": stop, "timeout": HOOK_TIMEOUT_SECS }]
    }))
}

fn claude_specs(agent: HookAgent) -> &'static [(&'static str, HookEvent, Option<&'static str>)] {
    match agent {
        HookAgent::Claude => &[
            ("SessionStart", HookEvent::SessionStart, None),
            ("SubagentStart", HookEvent::SubagentStart, None),
            ("UserPromptSubmit", HookEvent::UserPromptSubmit, None),
            ("PostToolUseFailure", HookEvent::PostToolUseFailure, Some("*")),
            ("Stop", HookEvent::Stop, None),
        ],
        HookAgent::Codex => &[
            ("SessionStart", HookEvent::SessionStart, None),
            ("SubagentStart", HookEvent::SubagentStart, None),
            ("UserPromptSubmit", HookEvent::UserPromptSubmit, None),
            ("PostToolUse", HookEvent::PostToolUse, None),
            ("Stop", HookEvent::Stop, None),
        ],
        HookAgent::Gemini => &[],
    }
}

fn is_exact_managed_group(
    group: &Value,
    agent: HookAgent,
    event: HookEvent,
    expected_matcher: Option<&str>,
) -> bool {
    let Some(obj) = group.as_object() else {
        return false;
    };
    let expected_len = if expected_matcher.is_some() { 2 } else { 1 };
    if obj.len() != expected_len {
        return false;
    }
    if let Some(m) = expected_matcher {
        if obj.get("matcher").and_then(Value::as_str) != Some(m) {
            return false;
        }
    }
    obj.get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hs| hs.len() == 1 && is_exact_managed_leaf(&hs[0], agent, event))
}

fn strip_exact_managed_group_for_event(
    arr: &mut Vec<Value>,
    agent: HookAgent,
    event: HookEvent,
    expected_matcher: Option<&str>,
) -> bool {
    let before = arr.len();
    arr.retain(|group| !is_exact_managed_group(group, agent, event, expected_matcher));
    arr.len() != before
}

fn upsert_claude_style_hooks(
    root_obj: &mut Map<String, Value>,
    agent: HookAgent,
    exe: &str,
) -> Result<(), &'static str> {
    let specs = claude_specs(agent);
    // Pre-build all commands before touching `root_obj`
    let mut built_cmds = Vec::with_capacity(specs.len());
    for (ev_key, ev_enum, matcher) in specs {
        let Some(cmd) =
            format_hook_command_for_platform(exe, agent, *ev_enum, cfg!(windows))
        else {
            return Err("안전하지 않은 실행 경로");
        };
        built_cmds.push((*ev_key, *ev_enum, *matcher, cmd));
    }

    if let Some(existing_hooks) = root_obj.get("hooks") {
        let Some(hooks_obj) = existing_hooks.as_object() else {
            return Err("hooks 필드가 객체가 아님");
        };
        for (ev_key, _, _, _) in &built_cmds {
            if let Some(existing) = hooks_obj.get(*ev_key) {
                if !existing.is_array() {
                    return Err("hooks 이벤트 필드가 배열이 아님");
                }
            }
        }
    }

    let hooks_obj = root_obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .unwrap();

    for (ev_key, ev_enum, matcher, cmd) in built_cmds {
        let arr = hooks_obj
            .entry(ev_key.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap();
        // If user customized a mistology group (e.g. narrowed matcher to "Bash"), preserve it without broadening
        let has_user_customized_mistology = arr.iter().any(|group| {
            !is_exact_managed_group(group, agent, ev_enum, matcher)
                && group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|hs| hs.iter().any(|h| is_exact_managed_leaf(h, agent, ev_enum)))
        });
        if has_user_customized_mistology {
            continue;
        }
        strip_exact_managed_group_for_event(arr, agent, ev_enum, matcher);
        let entry = match matcher {
            Some(m) => json!({
                "matcher": m,
                "hooks": [{ "type": "command", "command": cmd, "timeout": HOOK_TIMEOUT_SECS }]
            }),
            None => json!({
                "hooks": [{ "type": "command", "command": cmd, "timeout": HOOK_TIMEOUT_SECS }]
            }),
        };
        arr.push(entry);
    }
    Ok(())
}

fn remove_claude_style_hooks(
    root_obj: &mut Map<String, Value>,
    agent: HookAgent,
) -> Result<bool, &'static str> {
    let Some(hooks_val) = root_obj.get_mut("hooks") else {
        return Ok(false);
    };
    let Some(hooks_obj) = hooks_val.as_object_mut() else {
        return Err("hooks 필드가 객체가 아님");
    };
    let mut changed = false;
    let mut empty_keys = Vec::new();
    for (ev_key, ev_enum, matcher) in claude_specs(agent) {
        if let Some(arr) = hooks_obj.get_mut(*ev_key).and_then(Value::as_array_mut) {
            if strip_exact_managed_group_for_event(arr, agent, *ev_enum, *matcher) {
                changed = true;
                if arr.is_empty() {
                    empty_keys.push((*ev_key).to_string());
                }
            }
        }
    }
    for k in empty_keys {
        hooks_obj.remove(&k);
    }
    Ok(changed)
}

fn is_exact_managed_cursor_rule(raw: &str) -> bool {
    raw.replace("\r\n", "\n") == CURSOR_RULE_CONTENT
}

pub fn install_agent_hooks(
    home: &Path,
    all: bool,
    custom_home: bool,
) -> Result<Vec<String>, String> {
    let Some(exe) = resolve_setup_exe(custom_home) else {
        return Ok(vec!["[건너뜀 (실행 경로 확인 불가)] 전역 훅 자동 등록".to_string()]);
    };
    let mut out = Vec::new();

    // 1. Antigravity / Gemini (~/.gemini/config/hooks.json)
    if all || home.join(".gemini").exists() {
        let rel = ".gemini/config/hooks.json";
        let target = home.join(rel);
        if path_has_symlink_under(home, rel) {
            out.push(format!("[유지 (심볼릭 링크 보호)] {}", target.display()));
        } else {
            if let Some(par) = target.parent() {
                fs::create_dir_all(par).map_err(|e| e.to_string())?;
            }
            let _lock = FileLock::acquire(&target)?;
            match read_json_object_strict(&target) {
                Ok((mut obj, digest)) => {
                    let conflict = obj
                        .get("mistology")
                        .is_some_and(|existing| !is_managed_gemini_group(existing));
                    if conflict {
                        out.push(format!(
                            "[건너뜀 (사용자 정의 mistology 훅 보존)] {}",
                            target.display()
                        ));
                    } else if let Some(group) = build_gemini_mistology_group(&exe) {
                        obj.insert("mistology".to_string(), group);
                        let pretty = serde_json::to_string_pretty(&Value::Object(obj))
                            .map_err(|e| e.to_string())?;
                        match atomic_write_checked(
                            &target,
                            format!("{pretty}\n").as_bytes(),
                            true,
                            Some(digest),
                        ) {
                            Ok(()) => out.push(format!("[훅 등록 완료] {}", target.display())),
                            Err(reason) => out.push(format!("[건너뜀 ({reason})] {}", target.display())),
                        }
                    } else {
                        out.push(format!(
                            "[건너뜀 (안전하지 않은 실행 경로)] {}",
                            target.display()
                        ));
                    }
                }
                Err(reason) => out.push(format!("[건너뜀 ({reason})] {}", target.display())),
            }
        }
    }

    // 2. Claude Code (~/.claude/settings.json) & Codex (~/.codex/hooks.json)
    for (probe, rel, agent) in [
        (".claude", ".claude/settings.json", HookAgent::Claude),
        (".codex", ".codex/hooks.json", HookAgent::Codex),
    ] {
        if !(all || home.join(probe).exists()) {
            continue;
        }
        let target = home.join(rel);
        if path_has_symlink_under(home, rel) {
            out.push(format!("[유지 (심볼릭 링크 보호)] {}", target.display()));
            continue;
        }
        if let Some(par) = target.parent() {
            fs::create_dir_all(par).map_err(|e| e.to_string())?;
        }
        let _lock = FileLock::acquire(&target)?;
        match read_json_object_strict(&target) {
            Ok((mut obj, digest)) => match upsert_claude_style_hooks(&mut obj, agent, &exe) {
                Ok(()) => {
                    let pretty = serde_json::to_string_pretty(&Value::Object(obj))
                        .map_err(|e| e.to_string())?;
                    match atomic_write_checked(
                        &target,
                        format!("{pretty}\n").as_bytes(),
                        true,
                        Some(digest),
                    ) {
                        Ok(()) => out.push(format!("[훅 등록 완료] {}", target.display())),
                        Err(reason) => out.push(format!("[건너뜀 ({reason})] {}", target.display())),
                    }
                }
                Err(reason) => out.push(format!("[건너뜀 ({reason})] {}", target.display())),
            },
            Err(reason) => out.push(format!("[건너뜀 ({reason})] {}", target.display())),
        }
    }

    // 3. Cursor (~/.cursor/rules/mistology.mdc — locked exact generated content ownership)
    if all || home.join(".cursor").exists() {
        let rel = ".cursor/rules/mistology.mdc";
        let target = home.join(rel);
        if path_has_symlink_under(home, rel) {
            out.push(format!("[유지 (심볼릭 링크 보호)] {}", target.display()));
        } else {
            if let Some(par) = target.parent() {
                fs::create_dir_all(par).map_err(|e| e.to_string())?;
            }
            let _lock = FileLock::acquire(&target)?;
            let existing_digest = if target.exists() {
                let raw = fs::read_to_string(&target).map_err(|e| e.to_string())?;
                if !is_exact_managed_cursor_rule(&raw) {
                    out.push(format!(
                        "[건너뜀 (사용자 편집 규칙 보존)] {}",
                        target.display()
                    ));
                    return Ok(out);
                }
                Some(fnv1a_64(raw.as_bytes()))
            } else {
                None
            };
            match atomic_write_checked(
                &target,
                CURSOR_RULE_CONTENT.as_bytes(),
                true,
                Some(existing_digest),
            ) {
                Ok(()) => out.push(format!("[상시 규칙 등록 완료] {}", target.display())),
                Err(reason) => out.push(format!("[건너뜀 ({reason})] {}", target.display())),
            }
        }
    }

    Ok(out)
}

pub fn uninstall_agent_hooks(home: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();

    // 1. Antigravity / Gemini
    let gem_rel = ".gemini/config/hooks.json";
    let gem_path = home.join(gem_rel);
    if gem_path.exists() {
        if path_has_symlink_under(home, gem_rel) {
            out.push(format!("[유지 (심볼릭 링크 보호)] {}", gem_path.display()));
        } else {
            let _lock = FileLock::acquire(&gem_path)?;
            if let Ok((mut obj, digest)) = read_json_object_strict(&gem_path) {
                if obj.get("mistology").is_some_and(is_managed_gemini_group) {
                    obj.remove("mistology");
                    let pretty = serde_json::to_string_pretty(&Value::Object(obj))
                        .map_err(|e| e.to_string())?;
                    if atomic_write_checked(
                        &gem_path,
                        format!("{pretty}\n").as_bytes(),
                        true,
                        Some(digest),
                    )
                    .is_ok()
                    {
                        out.push(format!("[훅 제거 완료] {}", gem_path.display()));
                    }
                }
            }
        }
    }

    // 2. Claude & Codex
    for (rel, agent) in [
        (".claude/settings.json", HookAgent::Claude),
        (".codex/hooks.json", HookAgent::Codex),
    ] {
        let target = home.join(rel);
        if !target.exists() {
            continue;
        }
        if path_has_symlink_under(home, rel) {
            out.push(format!("[유지 (심볼릭 링크 보호)] {}", target.display()));
            continue;
        }
        let _lock = FileLock::acquire(&target)?;
        if let Ok((mut obj, digest)) = read_json_object_strict(&target) {
            if remove_claude_style_hooks(&mut obj, agent).unwrap_or(false) {
                let pretty = serde_json::to_string_pretty(&Value::Object(obj))
                    .map_err(|e| e.to_string())?;
                if atomic_write_checked(
                    &target,
                    format!("{pretty}\n").as_bytes(),
                    true,
                    Some(digest),
                )
                .is_ok()
                {
                    out.push(format!("[훅 제거 완료] {}", target.display()));
                }
            }
        }
    }

    // 3. Cursor Always-On rule (locked exact generated content ownership!)
    let cur_rel = ".cursor/rules/mistology.mdc";
    let cur_path = home.join(cur_rel);
    if cur_path.exists() && !path_has_symlink_under(home, cur_rel) {
        let _lock = FileLock::acquire(&cur_path)?;
        if fs::read_to_string(&cur_path)
            .map(|raw| is_exact_managed_cursor_rule(&raw))
            .unwrap_or(false)
        {
            fs::remove_file(&cur_path).map_err(|e| e.to_string())?;
            out.push(format!("[상시 규칙 제거 완료] {}", cur_path.display()));
        }
    }

    if out.is_empty() {
        out.push("[제거할 MISTOLOGY 훅 없음]".to_string());
    }
    Ok(out)
}

pub fn inspect_agent_hooks(home: &Path) -> Vec<String> {
    let mut lines = Vec::new();

    if home.join(".gemini").exists() {
        let p = home.join(".gemini/config/hooks.json");
        let status = match read_json_object_strict(&p) {
            Ok((obj, _)) if obj.get("mistology").is_some_and(is_managed_gemini_group) => "활성",
            Ok(_) => "미설치 (`mistology setup` 권장)",
            Err(e) => e,
        };
        lines.push(format!("  - `~/.gemini/config/hooks.json`: {status}"));
    }

    for (probe, rel, agent) in [
        (".claude", ".claude/settings.json", HookAgent::Claude),
        (".codex", ".codex/hooks.json", HookAgent::Codex),
    ] {
        if !home.join(probe).exists() {
            continue;
        }
        let p = home.join(rel);
        let status = match read_json_object_strict(&p) {
            Ok((obj, _)) => {
                let specs = claude_specs(agent);
                let matched_cnt = obj
                    .get("hooks")
                    .and_then(Value::as_object)
                    .map(|h| {
                        specs
                            .iter()
                            .filter(|(ev_key, ev_enum, matcher)| {
                                h.get(*ev_key)
                                    .and_then(Value::as_array)
                                    .is_some_and(|arr| {
                                        arr.iter().any(|grp| {
                                            is_exact_managed_group(grp, agent, *ev_enum, *matcher)
                                        })
                                    })
                            })
                            .count()
                    })
                    .unwrap_or(0);
                if matched_cnt == specs.len() {
                    if agent == HookAgent::Codex {
                        "등록됨 (Codex 훅 신뢰 상태 미확인)"
                    } else {
                        "활성"
                    }
                } else if matched_cnt > 0 {
                    "일부만 설치됨 (`mistology setup` 권장)"
                } else {
                    "미설치 (`mistology setup` 권장)"
                }
            }
            Err(e) => e,
        };
        lines.push(format!("  - `~/{rel}`: {status}"));
    }

    if home.join(".cursor").exists() {
        let p = home.join(".cursor/rules/mistology.mdc");
        let status = match fs::read_to_string(&p) {
            Ok(raw) if is_exact_managed_cursor_rule(&raw) => "활성 (alwaysApply)",
            Ok(_) => "사용자 정의 파일",
            Err(_) => "미설치 (`mistology setup` 권장)",
        };
        lines.push(format!("  - `~/.cursor/rules/mistology.mdc`: {status}"));
    }

    lines
}
