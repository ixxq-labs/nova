//! zsh adapter script embedding.

use std::env;
use std::fs::{self, DirBuilder, File};
use std::io::Read;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use nix::sys::stat::Mode;
use nix::unistd::mkfifo;

use crate::worker::protocol;

const INIT_ZSH: &str = include_str!("shell/init.zsh");
const SESSION_TOKEN_BYTES: usize = 16;
const RUNTIME_DIR_SUFFIX_BYTES: usize = 6;

pub fn render_init_script(binary_path: &Path) -> String {
    // The init process is already running, so creating the FIFOs here saves zsh three forks.
    let runtime_dir = prepare_runtime_dir(&runtime_base_dir());
    render_init_script_with(
        binary_path,
        &session_token_initializer(),
        runtime_dir.as_deref(),
    )
}

fn render_init_script_with(
    binary_path: &Path,
    session_token_initializer: &str,
    runtime_dir: Option<&Path>,
) -> String {
    let runtime_dir = runtime_dir
        .map(|dir| zsh_single_quote(&dir.to_string_lossy()))
        .unwrap_or_else(|| "''".to_string());
    INIT_ZSH
        .replace(
            "@NOVA_BIN@",
            &zsh_single_quote(&binary_path.to_string_lossy()),
        )
        .replace("@NOVA_RUNTIME_DIR@", &runtime_dir)
        .replace("@NOVA_PROTOCOL_VERSION@", protocol::VERSION)
        .replace("@NOVA_SESSION_TOKEN@", session_token_initializer)
}

/// Mirrors the zsh fallback: `${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}`.
fn runtime_base_dir() -> PathBuf {
    ["XDG_RUNTIME_DIR", "TMPDIR"]
        .into_iter()
        .filter_map(env::var_os)
        .find(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// Creates `nova-init-<random>` with both FIFOs for the shell to claim by
/// renaming it. `None` makes the zsh adapter create its own runtime directory.
fn prepare_runtime_dir(base: &Path) -> Option<PathBuf> {
    let suffix = random_hex(RUNTIME_DIR_SUFFIX_BYTES)?;
    let dir = base.join(format!("nova-init-{suffix}"));
    DirBuilder::new().mode(0o700).create(&dir).ok()?;

    let fifo_mode = Mode::S_IRUSR | Mode::S_IWUSR;
    let created = ["req", "resp"]
        .into_iter()
        .all(|name| mkfifo(&dir.join(name), fifo_mode).is_ok());
    if created {
        Some(dir)
    } else {
        let _ = fs::remove_dir_all(&dir);
        None
    }
}

fn session_token_initializer() -> String {
    match random_hex(SESSION_TOKEN_BYTES) {
        Some(token) => zsh_single_quote(&token),
        None => zsh_random_session_token_expression(),
    }
}

fn random_hex(len: usize) -> Option<String> {
    let mut bytes = vec![0_u8; len];
    let mut random = File::open("/dev/urandom").ok()?;
    random.read_exact(&mut bytes).ok()?;
    Some(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn zsh_random_session_token_expression() -> String {
    "\"${RANDOM}${RANDOM}${RANDOM}${RANDOM}${RANDOM}${RANDOM}${RANDOM}${RANDOM}-$$\"".to_string()
}

#[cfg(test)]
fn render_init_script_with_session_token(binary_path: &Path, session_token: &str) -> String {
    render_init_script_with(binary_path, &zsh_single_quote(session_token), None)
}

fn zsh_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_binary_path_as_a_single_quoted_zsh_string() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova bin"), "token");

        assert!(script.contains("typeset -g _nova_bin='/tmp/nova bin'"));
    }

    #[test]
    fn embeds_the_protocol_version() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("typeset -g _nova_protocol_version=8"));
        assert!(script.contains("\"${fields[2]}\" == \"$_nova_protocol_version\""));
        assert!(!script.contains("@NOVA_PROTOCOL_VERSION@"));
    }

    #[test]
    fn embeds_the_session_token_as_a_single_quoted_zsh_string() {
        let script =
            render_init_script_with_session_token(Path::new("/tmp/nova"), "token'with quote");

        assert!(script.contains("typeset -g _nova_session_token='token'\\''with quote'"));
        assert!(!script.contains("@NOVA_SESSION_TOKEN@"));
    }

    #[test]
    fn reads_initial_wait_budget_from_handshake() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("typeset -g _nova_wait_cs=0"));
        assert!(script.contains("_nova_wait_cs=$(( (${fields[4]:-0} + 9) / 10 ))"));
        assert!(script.contains("[[ \"$_nova_reply_status\" == partial ]]"));
    }

    #[test]
    fn escapes_single_quotes_in_binary_paths() {
        let script = render_init_script_with_session_token(Path::new("/tmp/no'va"), "token");

        assert!(script.contains("typeset -g _nova_bin='/tmp/no'\\''va'"));
    }

    #[test]
    fn registers_zle_line_init_for_pending_updates() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("autoload -Uz add-zsh-hook add-zle-hook-widget"));
        assert!(script.contains("add-zle-hook-widget line-init _nova_zle_line_init"));
    }

    #[test]
    fn registers_zle_keymap_select_for_vi_prompt_char_updates() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("typeset -g _nova_last_exit_status=0"));
        assert!(script.contains("typeset -g _nova_last_duration_ms="));
        assert!(script.contains("_nova_last_exit_status=$exit_status"));
        assert!(script.contains("_nova_last_duration_ms=$duration_ms"));
        assert!(script.contains(
            "_nova_send_request \"$_nova_last_exit_status\" \"$_nova_last_duration_ms\""
        ));
        assert!(script.contains("add-zle-hook-widget keymap-select _nova_zle_keymap_select"));
    }

    #[test]
    fn eagerly_spawns_worker_once_for_interactive_shells() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("_nova_worker_alive && return 0"));
        assert!(script.contains("if ! _nova_worker_alive; then"));
        assert!(script.contains(
            "if [[ -o interactive ]]; then\n  _nova_spawn_worker || true\nelse\n  _nova_remove_runtime\nfi\n\nadd-zsh-hook"
        ));
    }

    #[test]
    fn falls_back_to_a_pid_runtime_directory_when_init_could_not_prepare_one() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains(
            "typeset -g _nova_runtime_dir=\"${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/nova-$$\""
        ));
        assert!(script.contains("local prepared=''\n"));
        assert!(!script.contains("@NOVA_RUNTIME_DIR@"));
    }

    #[test]
    fn claims_the_prepared_runtime_directory_with_an_atomic_rename() {
        let script = render_init_script_with(
            Path::new("/tmp/nova"),
            "'token'",
            Some(Path::new("/tmp/it's/nova-init-abc")),
        );
        let (_guard, body) = script
            .split_once("typeset -g _NOVA_ZSH_LOADED=1")
            .expect("load guard");

        assert!(body.contains("local prepared='/tmp/it'\\''s/nova-init-abc'\n"));
        assert!(body.contains("local claimed=${prepared:h}/nova-$$-${${prepared:t}#nova-init-}"));
        assert!(body.contains("zf_mv -- \"$prepared\" \"$claimed\"\n"));
        assert!(body.contains("command mv -- \"$prepared\" \"$claimed\"\n"));
        assert!(body.contains("fi 2>/dev/null && _nova_runtime_dir=$claimed"));
    }

    #[test]
    fn reloading_removes_the_unclaimed_prepared_directory() {
        let script = render_init_script_with(
            Path::new("/tmp/nova"),
            "'token'",
            Some(Path::new("/tmp/nova-init-abc")),
        );
        let (guard, _rest) = script
            .split_once("typeset -g _NOVA_ZSH_LOADED=1")
            .expect("load guard");

        assert!(guard.contains("local prepared='/tmp/nova-init-abc'"));
        assert!(guard.contains("[[ ${prepared:t} == nova-init-* ]] || return 0"));
        assert!(guard.contains("zf_rm -rf -- \"$prepared\""));
    }

    #[test]
    fn prepares_a_private_runtime_directory_with_both_fifos() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};

        let base = tempfile::tempdir().expect("tempdir");
        let dir = prepare_runtime_dir(base.path()).expect("runtime dir should be prepared");
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name");

        assert!(name.starts_with("nova-init-"));

        assert_eq!(
            fs::metadata(&dir).expect("dir").permissions().mode() & 0o777,
            0o700
        );
        for fifo in ["req", "resp"] {
            let metadata = fs::metadata(dir.join(fifo)).expect("fifo");
            assert!(metadata.file_type().is_fifo());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
        assert_ne!(
            prepare_runtime_dir(base.path()),
            Some(dir),
            "each init gets a fresh directory"
        );
    }

    #[test]
    fn removes_runtime_with_builtins_and_keeps_command_fallbacks() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("zmodload -F zsh/files b:zf_mkdir b:zf_mv b:zf_rm"));
        assert!(script.contains("zf_rm -rf -- \"$_nova_runtime_dir\""));
        assert!(script.contains("zf_mkdir -m 700 -- \"$_nova_runtime_dir\""));
    }

    #[test]
    fn hardens_runtime_directory_creation() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("command mkdir -m 700 -- \"$_nova_runtime_dir\""));
        assert!(script.contains("[[ -d \"$_nova_runtime_dir\" && -O \"$_nova_runtime_dir\" ]]"));
        assert!(
            script.contains("command mkfifo -m 600 -- \"$_nova_req_fifo\" \"$_nova_resp_fifo\"")
        );
        assert!(!script.contains("command od -An -N16 -tx1 /dev/urandom"));
        assert!(!script.contains("command tr -d"));
        assert!(!script.contains("command chmod 700"));
        assert!(!script.contains("umask 077"));
    }

    #[test]
    fn passes_worker_session_details_through_environment() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("NOVA_SESSION_TOKEN=\"$_nova_session_token\" NOVA_PARENT_PID=$$"));
        assert!(script.contains("\"$_nova_bin\" worker --dir \"$_nova_runtime_dir\""));
        assert!(!script.contains("--session-token \"$_nova_session_token\""));
    }

    #[test]
    fn sends_virtual_env_in_render_requests() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains(
            "${KEYMAP:-main}${_nova_nul}${USER:-}${_nova_nul}${prompt_host}${_nova_nul}${prompt_time}${_nova_nul}${VIRTUAL_ENV:-}"
        ));
    }

    #[test]
    fn sends_nix_shell_env_in_render_requests() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(
            script
                .contains("${IN_NIX_SHELL:-}${_nova_nul}${name:-}${_nova_nul}${NIX_SHELL_LEVEL:-}")
        );
    }

    #[test]
    fn sends_aws_env_in_render_requests_without_secret_values() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(
            script.contains(
                "${HOME:-}${_nova_nul}${AWSU_PROFILE:-}${_nova_nul}${AWS_VAULT:-}${_nova_nul}${AWSUME_PROFILE:-}${_nova_nul}${AWS_PROFILE:-}${_nova_nul}${AWS_SSO_PROFILE:-}${_nova_nul}${AWS_REGION:-}${_nova_nul}${AWS_DEFAULT_REGION:-}${_nova_nul}${AWS_CONFIG_FILE:-}${_nova_nul}${AWS_SHARED_CREDENTIALS_FILE:-}${_nova_nul}${AWS_CREDENTIALS_FILE:-}${_nova_nul}${AWS_ACCESS_KEY_ID:+1}${_nova_nul}${AWS_SECRET_ACCESS_KEY:+1}${_nova_nul}${AWS_SESSION_TOKEN:+1}"
            )
        );
    }

    #[test]
    fn sends_path_in_render_requests() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("${AWS_SESSION_TOKEN:+1}${_nova_nul}${PATH:-}${_nova_rs}"));
    }

    #[test]
    fn checks_request_write_byte_count() {
        let script = render_init_script_with_session_token(Path::new("/tmp/nova"), "token");

        assert!(script.contains("local -i wrote=0 frame_len=0"));
        assert!(script.contains("setopt localoptions no_multibyte; frame_len=${#1}"));
        assert!(script.contains("syswrite -c wrote -o \"$_nova_req_fd\" -- \"$frame\""));
        assert!(script.contains("(( wrote != frame_len ))"));
    }
}
