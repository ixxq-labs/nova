//! User-defined command segments: any `[segments.<id>]` with `command`.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::cache::CacheKey;
use crate::config::{Config, SegmentConfig};
use crate::segments::{AsyncJobSegment, AsyncSegmentFailure, SegmentContent, Style};
use crate::state::PromptState;

use super::{RuntimeCollectError, collect_command_output, remaining_time, runtime_cache_source};

/// Refresh on every prompt; the cached value is shown while the command runs.
pub const COMMAND_SEGMENT_TTL: Duration = Duration::ZERO;
pub const COMMAND_SEGMENT_TIMEOUT: Duration = Duration::from_secs(1);

/// Ids of layout segments backed by a user command, excluding built-in ids.
pub fn command_segment_ids<'a>(
    config: &'a Config,
    builtin_ids: &'a [&str],
) -> impl Iterator<Item = &'a str> {
    config
        .segments
        .iter()
        .filter(|(id, segment)| segment.command.is_some() && !builtin_ids.contains(&id.as_str()))
        .map(|(id, _segment)| id.as_str())
        .filter(|id| config.uses_segment(id))
}

pub fn command_cache_key(
    id: &str,
    config: &SegmentConfig,
    state: &PromptState,
    config_generation: u64,
) -> Option<CacheKey> {
    if !detected(&config.detect, &state.cwd) {
        return None;
    }

    Some(CacheKey::new(
        id,
        runtime_cache_source(state.cwd.to_string_lossy(), state.env.path.as_deref()),
        config_generation,
    ))
}

pub fn collect_command_segment(
    key: CacheKey,
    config: &SegmentConfig,
    state: &PromptState,
    deadline: Instant,
) -> Vec<AsyncJobSegment> {
    let content = run_command(config, state, deadline)
        .map(|output| render_command_output(&key.segment_id, &output, config))
        .map_err(|_error| AsyncSegmentFailure::Failed);
    vec![AsyncJobSegment { key, content }]
}

fn detected(detect: &[String], cwd: &Path) -> bool {
    detect.is_empty() || detect.iter().any(|name| cwd.join(name).exists())
}

fn run_command(
    config: &SegmentConfig,
    state: &PromptState,
    deadline: Instant,
) -> Result<Vec<u8>, RuntimeCollectError> {
    let command = config.command.as_deref().unwrap_or_default();
    collect_command_output(
        Path::new("/bin/sh"),
        &["-c", command],
        &state.cwd,
        remaining_time(deadline)?,
        state.env.path.as_deref(),
    )
}

/// Shows the first non-empty output line; empty output or a failing exit
/// status hides the segment.
fn render_command_output(
    id: &str,
    output: &[u8],
    config: &SegmentConfig,
) -> Option<SegmentContent> {
    let output = String::from_utf8_lossy(output);
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let text = match config.icon.as_deref() {
        Some(icon) if !icon.is_empty() => format!("{icon} {line}"),
        _ => line.to_string(),
    };
    Some(SegmentContent::new(id, text, Style::from(&config.style)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::state::{Keymap, PromptEnv};

    fn state(cwd: PathBuf) -> PromptState {
        PromptState {
            cwd,
            exit_status: 0,
            duration_ms: None,
            time: None,
            columns: 80,
            keymap: Keymap::Main,
            env: PromptEnv::default(),
        }
    }

    fn segment(command: &str) -> SegmentConfig {
        SegmentConfig {
            command: Some(command.to_string()),
            ..SegmentConfig::default()
        }
    }

    fn collect(
        config: &SegmentConfig,
        cwd: PathBuf,
    ) -> Result<Option<SegmentContent>, AsyncSegmentFailure> {
        let state = state(cwd);
        let key = command_cache_key("kube", config, &state, 1).expect("detected");
        collect_command_segment(key, config, &state, Instant::now() + Duration::from_secs(5))
            .remove(0)
            .content
    }

    #[test]
    fn renders_the_first_non_empty_line_in_the_prompt_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SegmentConfig {
            icon: Some("☸".to_string()),
            ..segment("printf '\\n  %s  \\nsecond\\n' \"$(basename \"$PWD\")\"")
        };

        let content = collect(&config, dir.path().to_path_buf())
            .expect("ok")
            .expect("shown");
        let name = dir
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name");

        assert_eq!(content.id, "kube");
        assert_eq!(content.text, format!("☸ {name}"));
    }

    #[test]
    fn hides_empty_output_and_failing_commands() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            collect(&segment("true"), dir.path().to_path_buf()),
            Ok(None)
        );
        assert_eq!(
            collect(&segment("echo no; exit 3"), dir.path().to_path_buf()),
            Ok(None)
        );
    }

    #[test]
    fn times_out_without_blocking_past_the_deadline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = segment("sleep 5");
        let state = state(dir.path().to_path_buf());
        let key = command_cache_key("slow", &config, &state, 1).expect("key");
        let started = Instant::now();

        let segments = collect_command_segment(
            key,
            &config,
            &state,
            Instant::now() + Duration::from_millis(100),
        );

        assert_eq!(segments[0].content, Err(AsyncSegmentFailure::Failed));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn detect_gates_the_segment_on_cwd_markers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SegmentConfig {
            detect: vec!["Chart.yaml".to_string(), "k8s".to_string()],
            ..segment("echo ctx")
        };
        let state = state(dir.path().to_path_buf());

        assert_eq!(command_cache_key("kube", &config, &state, 1), None);
        std::fs::create_dir(dir.path().join("k8s")).expect("marker");
        assert!(command_cache_key("kube", &config, &state, 1).is_some());
    }

    #[test]
    fn lists_only_used_non_builtin_command_segments() {
        let config = Config::from_toml(
            r#"
            [layout.line1]
            left = ["dir", "kube", "git_branch"]

            [segments.kube]
            command = "echo ctx"

            [segments.unused]
            command = "echo unused"

            [segments.git_branch]
            command = "echo ignored"
            "#,
        )
        .expect("config");

        assert_eq!(
            command_segment_ids(&config, &["dir", "git_branch"]).collect::<Vec<_>>(),
            ["kube"]
        );
    }
}
