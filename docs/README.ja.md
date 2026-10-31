# Nova

[English](../README.md)

Nova は、入力を待たせない非同期の zsh prompt です。Rust で書かれています。

<p align="center">
  <img src="assets/vhs/readme-prompt.gif" alt="Nova prompt demo">
</p>

prompt はすぐに得られる同期的な情報だけで描画します。Git status、runtime
version、自分で定義した command のように時間のかかる情報は per-shell
worker が集め、届いた時点で prompt に反映します。prompt が表示された瞬間から
入力できます。

## 特徴

- **待たせない。** `git status`、version の取得、custom command は timeout
  付きで worker が実行し、結果が届くと prompt を再描画します。prompt 1 回あたり
  の overhead は通常 1 ms 未満です（`command_lag_ms` は約 0.75 ms）。
- **起動が速い。** shell 起動時に fork するのは `nova init` と worker だけです。
  worker は `.zshrc` の残りが実行されている間に現在の directory の情報を集め
  始めるので、最初の prompt も多くの場合は揃った状態で表示されます。
- **Transient prompt（任意）。** 実行済みの行を directory と `❯` などに畳んで、
  scrollback をすっきりさせられます。2 行 layout の右寄せ部分は、pane の分割や
  window の resize をしても崩れません。
- **Custom segment。** 任意の command を数行の TOML で非同期 segment にできます。
- **Terminal との連携。** OSC 133 の prompt mark（Ghostty、WezTerm、kitty、
  iTerm2、VS Code で prompt 間を移動できます）と、OSC 7 による directory の
  通知に対応しています。
- **再 compile 不要の設定。** layout、icon、色、区切り文字、timeout、cache の
  有効期間を 1 つの TOML で設定でき、`nova check` で検証できます。


## Requirements

- zsh 5.8 以上
- default icon 表示用の Nerd Font
- Git segment を使う場合は Git

## Installation

Release artifact は Git tag から cargo-dist で publish されます。以下の
command は初回 GitHub Release の publish 後に使えます。

### Shell Installer

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ixxq-labs/nova/releases/latest/download/nova-installer.sh | sh
```

### Homebrew

```sh
brew install ixxq-labs/tap/nova
```

### Nix

install せずに実行:

```sh
nix run github:ixxq-labs/nova -- --help
```

profile に install:

```sh
nix profile install github:ixxq-labs/nova
```

### mise

GitHub backend を使います:

```sh
mise use -g github:ixxq-labs/nova@latest
```

`ubi` backend は mise 側で deprecated warning が出るため、Nova では案内しま
せん。

## Shell Setup

`nova` を install したあと、`.zshrc` に追加します:

```zsh
eval "$(nova init zsh)"
```

普段の zsh config を読み込まずに一時確認する場合:

```sh
zsh -f
eval "$(nova init zsh)"
```

## Configuration

ほとんどのユーザーはここに config を置けば十分です:

```text
~/.config/nova/config.toml
```

`XDG_CONFIG_HOME` を設定している場合は、その directory が base になります:

```text
$XDG_CONFIG_HOME/nova/config.toml
```

config の探索順:

1. `--config PATH`
2. `$NOVA_CONFIG`
3. `$XDG_CONFIG_HOME/nova/config.toml`
4. `$HOME/.config/nova/config.toml`

別の config file を明示する場合は `NOVA_CONFIG` に path を指定します:

```sh
NOVA_CONFIG=/path/to/config.toml
```

完全な example から始められます:

```sh
nova_config="${XDG_CONFIG_HOME:-$HOME/.config}/nova/config.toml"
mkdir -p "$(dirname "$nova_config")"
curl --proto '=https' --tlsv1.2 -fsSL \
  https://raw.githubusercontent.com/ixxq-labs/nova/main/examples/config.toml \
  -o "$nova_config"
nova check --config "$nova_config"
```

interactive shell を起動せずに prompt を preview できます:

```sh
nova prompt --format preview --cwd "$PWD" --cols "${COLUMNS:-80}" --duration-ms 2000 --time 11:16:42
```

## カスタマイズ

すべての key は [examples/config.toml](../examples/config.toml) に説明があります。
よく使う設定例:

実行済みの行を directory、終了コード、prompt character に畳む:

```toml
[layout]
transient = ["dir", "exit_status", "prompt_char"]
```

通常の prompt と同じく、畳んだ行にはその前に実行した command の結果が出ます。
そのため `[127]` や赤い `❯` は、失敗した command の次の行に付きます。

manifest がある directory でだけ Kubernetes の context を表示する:

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

command は prompt の directory で `sh -c` によって実行され、prompt を待たせる
ことはありません。表示されるのは出力の最初の行です。出力が空のときや
exit status が 0 以外のときは、segment を表示しません。

各 segment を角の丸い pill の上に描画する（segment の `style.bg`、未設定なら
落ち着いた gray を使います）:

```toml
[layout]
style = "capsule"
```

terminal 連携の escape sequence を無効にする:

```toml
[terminal]
semantic_prompts = false
report_cwd = false
```

## Performance

Nova は pull request ごとに [zsh-bench](https://github.com/romkatv/zsh-bench) で
計測しています。計測方法と履歴は [docs/performance.md](performance.md) を参照
してください。非同期処理は segment ごとの `timeout_ms` で打ち切られ、timeout
した probe は、それが起動した process もまとめて停止します。
