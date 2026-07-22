//! Opt-in local turn traces under `~/.grok/traces/` (metadata + messages only).
//!
//! Disabled by default. Enable with `[local_traces] enabled = true` in
//! `~/.grok/config.toml` or `GROK_LOCAL_TRACES=1`. Never writes repo snapshots.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Default per-session size cap (100 MiB).
pub const DEFAULT_MAX_BYTES_PER_SESSION: u64 = 104_857_600;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalTracesConfig {
    pub enabled: bool,
    pub max_bytes_per_session: u64,
}

impl Default for LocalTracesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_bytes_per_session: DEFAULT_MAX_BYTES_PER_SESSION,
        }
    }
}

/// Resolve whether local traces are enabled (env wins over config).
pub fn resolve_enabled(config: &LocalTracesConfig) -> bool {
    if let Ok(raw) = std::env::var("GROK_LOCAL_TRACES") {
        let v = raw.trim().to_ascii_lowercase();
        return matches!(v.as_str(), "1" | "true" | "yes" | "on");
    }
    config.enabled
}

/// Load `[local_traces]` from the effective config layers (env still wins via
/// [`resolve_enabled`]).
pub fn load_config() -> LocalTracesConfig {
    crate::config::load_effective_config()
        .ok()
        .and_then(|raw| {
            raw.get("local_traces")
                .cloned()
                .and_then(|v| v.try_into().ok())
        })
        .unwrap_or_default()
}

/// Serialize conversation items as JSONL (one JSON object per line).
pub fn messages_to_jsonl(items: &[xai_grok_sampling_types::ConversationItem]) -> String {
    let mut out = String::new();
    for item in items {
        match serde_json::to_string(item) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            Err(e) => {
                tracing::warn!(error = %e, "local_traces: skipping unserializable message");
            }
        }
    }
    out
}

fn traces_root(grok_home: &Path) -> PathBuf {
    grok_home.join("traces")
}

fn session_dir(grok_home: &Path, session_id: &str) -> PathBuf {
    traces_root(grok_home).join(sanitize_segment(session_id))
}

fn turn_dir(grok_home: &Path, session_id: &str, turn_number: u64) -> PathBuf {
    session_dir(grok_home, session_id).join(format!("turn_{turn_number}"))
}

fn sanitize_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn dir_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            total = total.saturating_add(dir_size_bytes(&p));
        } else if let Ok(meta) = entry.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

#[derive(Debug, Serialize)]
struct TurnMetadata<'a> {
    session_id: &'a str,
    turn_number: u64,
    written_at_unix_ms: i64,
}

/// Write opt-in local turn files. No-op when disabled. Stops with a warning when
/// the session size cap is exceeded.
pub fn write_turn_trace(
    grok_home: &Path,
    config: &LocalTracesConfig,
    session_id: &str,
    turn_number: u64,
    messages_jsonl: Option<&str>,
) -> std::io::Result<bool> {
    if !resolve_enabled(config) {
        return Ok(false);
    }
    let sess = session_dir(grok_home, session_id);
    let used = dir_size_bytes(&sess);
    if used >= config.max_bytes_per_session {
        tracing::warn!(
            session_id,
            used,
            max = config.max_bytes_per_session,
            "local_traces: session size cap reached; skipping write"
        );
        return Ok(false);
    }
    let dir = turn_dir(grok_home, session_id, turn_number);
    std::fs::create_dir_all(&dir)?;
    let meta = TurnMetadata {
        session_id,
        turn_number,
        written_at_unix_ms: chrono::Utc::now().timestamp_millis(),
    };
    let meta_bytes = serde_json::to_vec_pretty(&meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let projected = used
        .saturating_add(meta_bytes.len() as u64)
        .saturating_add(messages_jsonl.map(|s| s.len() as u64).unwrap_or(0));
    if projected > config.max_bytes_per_session {
        tracing::warn!(
            session_id,
            projected,
            max = config.max_bytes_per_session,
            "local_traces: write would exceed session size cap; skipping"
        );
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(false);
    }
    std::fs::write(dir.join("metadata.json"), meta_bytes)?;
    if let Some(body) = messages_jsonl {
        std::fs::write(dir.join("messages.jsonl"), body)?;
    }
    static WRITES: AtomicU64 = AtomicU64::new(0);
    WRITES.fetch_add(1, Ordering::Relaxed);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn disabled_by_default() {
        let dir = tempdir().unwrap();
        let cfg = LocalTracesConfig::default();
        assert!(!resolve_enabled(&cfg));
        assert!(!write_turn_trace(dir.path(), &cfg, "s1", 1, Some("{}")).unwrap());
        assert!(!dir.path().join("traces").exists());
    }

    #[test]
    fn writes_when_enabled() {
        let dir = tempdir().unwrap();
        let cfg = LocalTracesConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(write_turn_trace(dir.path(), &cfg, "sess-1", 3, Some("{\"a\":1}\n")).unwrap());
        let turn = dir.path().join("traces/sess-1/turn_3");
        assert!(turn.join("metadata.json").is_file());
        assert!(turn.join("messages.jsonl").is_file());
    }

    #[test]
    fn respects_size_cap() {
        let dir = tempdir().unwrap();
        let cfg = LocalTracesConfig {
            enabled: true,
            max_bytes_per_session: 32,
        };
        let _ = write_turn_trace(dir.path(), &cfg, "s", 1, Some(&"x".repeat(64)));
        // Either skipped entirely or cleaned up — no oversized session dir payload.
        let used = dir_size_bytes(&dir.path().join("traces/s"));
        assert!(used <= 32, "used={used}");
    }
}
