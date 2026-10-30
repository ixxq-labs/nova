# Changelog

All notable user-facing changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Nova uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.4.0] - 2026-10-31

### Added

- Custom command segments: any `[segments.<id>]` with `command` becomes an
  async segment showing the command's first output line, with optional
  `detect` markers, `icon`, `style`, `ttl_ms`, and `timeout_ms`.
- Transient prompt: accepted and interrupted (`Ctrl-C`) lines collapse to
  `layout.transient`, which defaults to `["prompt_char"]`. Set
  `transient = []` under `[layout]` to keep the full prompt in scrollback.
- Terminal resizes re-render the prompt, so path and branch truncation follow
  the new width.
- Terminal integration under `[terminal]`, on by default: OSC 133 prompt and
  command marks (`semantic_prompts`) for jumping between prompts and selecting
  command output, and OSC 7 (`report_cwd`) so new tabs and splits open in the
  current directory. Terminals without support ignore both.
- The worker starts collecting async segments for the directory it was
  started in, so the first prompt after `.zshrc` usually arrives complete
  instead of filling in.

### Changed

- **Default behavior change:** the transient prompt is enabled by default.
- The first-line right block of a two-line layout is aligned with a cursor
  move instead of space padding. It stays right-aligned after a pane split or
  window shrink and, like `RPROMPT`, leaves the last column empty.
- Startup no longer forks `rm`, `mkdir`, and `mkfifo`: `nova init zsh`
  prepares the runtime directory (`nova-<pid>-<random>`) itself, and exit
  cleanup uses the `zsh/files` builtins. This removes about 15 ms from the
  first prompt on macOS.
- The worker checks the shell's liveness with `kill(2)` instead of forking
  `kill -0` every second.
- Async collectors run with one slot per segment kind, so a slow `git status`
  no longer delays runtime versions behind it (#41).
- Worker protocol version 8. A shell that is still running an older worker
  falls back to the plain prompt until it is restarted.

### Fixed

- Re-sourcing `.zshrc` no longer skips everything after `eval "$(nova init zsh)"`:
  the load guard returned from the `eval`, which ended the sourced file.
- Timed-out runtime and command probes also stop the processes they started,
  instead of holding a collector until those exit.
- Worker records with an empty `RPROMPT` no longer shift the fields that
  follow it.

## [0.3.1] - 2026-07-10

### Fixed

- Narrow terminals keep the directory, branch, and status segments before
  lower-priority segments.
- Switching vi keymaps re-renders the prompt character.
- The worker sends a final status update after async results settle.
- Hardened runtime directory creation and worker lifecycle.

## [0.3.0] - 2026-07-08

### Added

- `git_status` display controls: an inner `separator` and `show_counts`.
- `[async].min_loading_ms` and per-segment `min_loading_ms` loading floors.

### Changed

- Git branch and status are collected independently, so the branch can appear
  before a slow status.

## [0.2.0] - 2026-07-08

### Added

- `aws.format` templating with `$symbol`, `$profile`, `$region`, and optional
  groups.
- Renderer customization: `layout.separator` and async `loading`
  placeholders.
- `[async].initial_wait_ms` budget for waiting on async results before the
  first draw.

### Fixed

- Runtime version probes use the shell's `PATH`.

## [0.1.1] - 2026-07-07

### Fixed

- The default config path is `config.toml`.

## [0.1.0] - 2026-07-07

### Added

- Initial release: a per-shell async worker, one- and two-line layouts, and
  the `dir`, `git_branch`, `git_status`, `exit_status`, `duration`, `time`,
  `ssh`, `user_host`, `prompt_char`, `nix_shell`, `aws`, and Rust, Node, Bun,
  Deno, and Python version segments.

[Unreleased]: https://github.com/ixxq-labs/nova/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/ixxq-labs/nova/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/ixxq-labs/nova/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ixxq-labs/nova/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ixxq-labs/nova/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/ixxq-labs/nova/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ixxq-labs/nova/releases/tag/v0.1.0
