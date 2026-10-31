# Nova

[日本語版](docs/README.ja.md)

An async zsh prompt that never makes you wait. Written in Rust.

![Nova prompt demo](docs/assets/vhs/readme-prompt.gif)

Nova draws the prompt from cheap, synchronous data and lets a per-shell worker
fill in everything slow — Git status, runtime versions, your own commands —
as it arrives. You can type the moment the prompt appears.

## Highlights

- **Never blocks.** `git status`, version probes, and custom commands run in a
  worker with per-segment timeouts; the prompt redraws when results land.
  Typical overhead is under 1 ms per prompt (`command_lag_ms` ≈ 0.75 ms).
- **Fast startup.** Besides `nova init` and the worker itself, starting a shell
  forks nothing, and the worker begins collecting for your directory while the
  rest of `.zshrc` runs, so the first prompt usually arrives complete.
- **Optional transient prompt.** Accepted lines can collapse to a few segments,
  such as the directory and `❯`, keeping scrollback dense. Two-line layout with
  a right-aligned block that survives pane splits and window resizes.
- **Custom segments.** Turn any command into an async segment with a few lines
  of TOML.
- **Terminal integration.** OSC 133 prompt marks (jump between prompts in
  Ghostty, WezTerm, kitty, iTerm2, and VS Code) and OSC 7 directory reports.
- **Configurable without recompiling.** Layout, icons, colors, separators,
  timeouts, and cache lifetimes live in one TOML file, checked by
  `nova check`.

## Requirements

- zsh 5.8 or later
- A Nerd Font for the default icons
- Git, when Git segments are enabled

## Installation

Release artifacts are published from Git tags by cargo-dist. The commands below
work after the first GitHub Release is published.

### Shell Installer

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ixxq-labs/nova/releases/latest/download/nova-installer.sh | sh
```

### Homebrew

```sh
brew install ixxq-labs/tap/nova
```

### Nix

Run without installing:

```sh
nix run github:ixxq-labs/nova -- --help
```

Install into your profile:

```sh
nix profile install github:ixxq-labs/nova
```

### mise

Use the GitHub backend:

```sh
mise use -g github:ixxq-labs/nova@latest
```

Nova does not document the `ubi` backend because mise warns that it is
deprecated.

## Shell Setup

Add this to your `.zshrc` after `nova` is installed:

```zsh
eval "$(nova init zsh)"
```

For a temporary smoke test without touching your normal zsh configuration:

```sh
zsh -f
eval "$(nova init zsh)"
```

## Configuration

Most users should put their config here:

```text
~/.config/nova/config.toml
```

If `XDG_CONFIG_HOME` is set, Nova uses that base directory instead:

```text
$XDG_CONFIG_HOME/nova/config.toml
```

Config lookup order:

1. `--config PATH`
2. `$NOVA_CONFIG`
3. `$XDG_CONFIG_HOME/nova/config.toml`
4. `$HOME/.config/nova/config.toml`

Use `NOVA_CONFIG` when you want to load an explicit config file:

```sh
NOVA_CONFIG=/path/to/config.toml
```

Start from the complete example:

```sh
nova_config="${XDG_CONFIG_HOME:-$HOME/.config}/nova/config.toml"
mkdir -p "$(dirname "$nova_config")"
curl --proto '=https' --tlsv1.2 -fsSL \
  https://raw.githubusercontent.com/ixxq-labs/nova/main/examples/config.toml \
  -o "$nova_config"
nova check --config "$nova_config"
```

Preview the prompt without starting an interactive shell:

```sh
nova prompt --format preview --cwd "$PWD" --cols "${COLUMNS:-80}" --duration-ms 2000 --time 11:16:42
```

## Customize

Every key is documented in [examples/config.toml](examples/config.toml). A few
common recipes:

Collapse accepted lines to the directory and prompt character:

```toml
[layout]
transient = ["dir", "prompt_char"]
```

Show the Kubernetes context, but only in directories that have manifests:

```toml
[layout.line1]
left = ["dir", "git_branch", "git_status", "kube", "duration"]

[segments.kube]
command = "kubectl config current-context"
detect = ["k8s", "Chart.yaml"]
icon = "☸"
style = { fg = "cyan", bold = true }
ttl_ms = 5000
```

The command runs with `sh -c` in the prompt's directory, never blocks the
prompt, and shows its first output line. Empty output or a failing exit status
hides the segment.

Draw each segment on a rounded pill (uses each segment's `style.bg`, or a
neutral gray):

```toml
[layout]
style = "capsule"
```

Turn off terminal integration sequences:

```toml
[terminal]
semantic_prompts = false
report_cwd = false
```

## Performance

Nova is measured with [zsh-bench](https://github.com/romkatv/zsh-bench) on
every pull request; see [docs/performance.md](docs/performance.md) for the
method and history. Async work is bounded by per-segment `timeout_ms`, and a
timed-out probe is stopped together with any processes it started.
