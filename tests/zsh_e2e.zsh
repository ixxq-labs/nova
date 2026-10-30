#!/usr/bin/env zsh

emulate -LR zsh
setopt err_exit no_unset pipe_fail extended_glob
zmodload zsh/datetime zsh/zpty zsh/zselect

typeset -gr script_dir=${0:A:h}
typeset -gr repo_root=${script_dir:h}
typeset -gr nova_bin=${NOVA_E2E_BIN:-$repo_root/target/debug/nova}
typeset -gr zsh_bin=${commands[zsh]}
typeset -gr main_marker='NOVA_E2E_MAIN>'
typeset -gr vi_marker='NOVA_E2E_VICMD>'
typeset -gr branch_marker='nova-e2e-branch'
typeset -gr e2e_root=$(mktemp -d "${TMPDIR:-/tmp}/nova-zsh-e2e.XXXXXX")
typeset -gr home_dir=$e2e_root/home
typeset -gr temp_dir=$e2e_root/tmp
typeset -gr state_dir=$e2e_root/state
typeset -gr git_repo=$e2e_root/repo
typeset -gr config_path=$e2e_root/config.toml

typeset -g transcript=
typeset -gi output_checkpoint=0
typeset -gi pty_fd=-1
typeset -gi session_active=0
typeset -g shell_pid=
typeset -g worker_pid=
typeset -g runtime_dir=

fail() {
  print -ru2 -- "zsh E2E: $*"
  if [[ -n $transcript ]]; then
    print -ru2 -- '--- terminal transcript ---'
    print -ru2 -- "${transcript//$'\r'/}"
    print -ru2 -- '--- end transcript ---'
  fi
  exit 1
}

cleanup() {
  local -i exit_status=$?
  setopt local_options no_err_exit

  if (( session_active )); then
    zpty -d nova_e2e >/dev/null 2>&1 || true
  fi
  if [[ $worker_pid == <-> ]] && kill -0 "$worker_pid" 2>/dev/null; then
    kill "$worker_pid" 2>/dev/null || true
  fi
  command rm -rf -- "$e2e_root"
  return $exit_status
}

drain_output() {
  local chunk
  while zpty -rt nova_e2e chunk; do
    transcript+=$chunk
  done
}

checkpoint_output() {
  drain_output
  output_checkpoint=${#transcript}
}

output_since_checkpoint() {
  local -i start=$(( output_checkpoint + 1 ))
  REPLY=${transcript[$start,-1]}
}

wait_for_output() {
  local expected=$1
  local description=$2
  local -F timeout_seconds=${3:-10}
  local -F deadline=$(( EPOCHREALTIME + timeout_seconds ))
  local recent

  while (( EPOCHREALTIME < deadline )); do
    drain_output
    output_since_checkpoint
    recent=$REPLY
    [[ $recent == *"$expected"* ]] && return 0
    zselect -t 5 -r $pty_fd >/dev/null 2>&1 || true
  done

  fail "timed out waiting for $description (${(qqq)expected})"
}

# Keeps reading the pty while waiting so a full output buffer never blocks the shell.
drain_for() {
  local -F deadline=$(( EPOCHREALTIME + $1 ))

  while (( EPOCHREALTIME < deadline )); do
    drain_output
    zselect -t 2 -r $pty_fd >/dev/null 2>&1 || true
  done
}

wait_for_match() {
  local pattern=$1
  local description=$2
  local -F deadline=$(( EPOCHREALTIME + 10 ))

  while (( EPOCHREALTIME < deadline )); do
    drain_output
    output_since_checkpoint
    [[ $REPLY =~ $pattern ]] && return 0
    zselect -t 5 -r $pty_fd >/dev/null 2>&1 || true
  done

  fail "timed out waiting for $description (${(qqq)pattern})"
}

wait_for_file() {
  local path=$1
  local -F deadline=$(( EPOCHREALTIME + 10 ))

  while (( EPOCHREALTIME < deadline )); do
    drain_output
    [[ -s $path ]] && return 0
    zselect -t 5 -r $pty_fd >/dev/null 2>&1 || true
  done

  fail "timed out waiting for state file ${(qqq)path}"
}

wait_for_shell_exit() {
  local -F deadline=$(( EPOCHREALTIME + 10 ))

  while (( EPOCHREALTIME < deadline )); do
    # zpty -t keeps reporting a running command while unread output is pending.
    drain_output
    zpty -t nova_e2e || return 0
    zselect -t 5 >/dev/null 2>&1 || true
  done

  fail "shell $shell_pid survived SIGKILL"
}

wait_for_worker_cleanup() {
  local -F deadline=$(( EPOCHREALTIME + 10 ))

  while (( EPOCHREALTIME < deadline )); do
    if ! kill -0 "$worker_pid" 2>/dev/null && [[ ! -e $runtime_dir ]]; then
      return 0
    fi
    zselect -t 5 >/dev/null 2>&1 || true
  done

  fail "worker $worker_pid or runtime directory ${(qqq)runtime_dir} survived shell termination"
}

trap cleanup EXIT
trap 'exit 130' HUP INT TERM

[[ -x $nova_bin ]] || fail "nova binary not found: $nova_bin (run cargo build first)"
command mkdir -p -- "$home_dir" "$temp_dir" "$state_dir" "$git_repo"

{
  print -r -- '[async]'
  print -r -- 'initial_wait_ms = 0'
  print -r -- 'min_loading_ms = 0'
  print -r -- ''
  print -r -- '[layout]'
  print -r -- 'lines = 1'
  print -r -- ''
  print -r -- '[layout.line1]'
  print -r -- 'left = ["dir", "git_branch", "prompt_char"]'
  print -r -- 'right = []'
  print -r -- ''
  print -r -- '[segments.git_branch]'
  print -r -- 'icon = ""'
  # Keep the result beyond precmd's read window so the ZLE redraw is observable.
  print -r -- 'min_loading_ms = 750'
  print -r -- ''
  print -r -- '[segments.prompt_char]'
  print -r -- 'character = "NOVA_E2E_MAIN>"'
  print -r -- ''
  print -r -- '[segments.prompt_char.characters]'
  print -r -- 'main = "NOVA_E2E_MAIN>"'
  print -r -- 'vi_command = "NOVA_E2E_VICMD>"'
} >| "$config_path"

command git init --quiet "$git_repo"
command git -C "$git_repo" checkout --quiet -b "$branch_marker"
print -r -- 'fixture' >| "$git_repo/fixture.txt"
command git -C "$git_repo" add fixture.txt
command git -C "$git_repo" -c user.name=Nova -c user.email=nova@example.invalid \
  commit --quiet -m initial

export HOME=$home_dir
export TMPDIR=$temp_dir
export NOVA_CONFIG=$config_path
export NOVA_E2E_BIN=$nova_bin
export NOVA_E2E_STATE=$state_dir
export PATH="${nova_bin:h}:$PATH"
export TERM=xterm-256color
unset XDG_RUNTIME_DIR SSH_CLIENT SSH_CONNECTION SSH_TTY

zpty -b nova_e2e "$zsh_bin" -f
pty_fd=$REPLY
session_active=1

checkpoint_output
# Handlers set with the trap builtin before init must keep running after Nova's.
zpty -w nova_e2e 'trap "print -r -- USER_\${:-INT}" INT; trap "print -r -- USER_\${:-WINCH}" WINCH; eval "$("$NOVA_E2E_BIN" init zsh)"'
wait_for_output "$main_marker" 'the first Nova prompt'
print -r -- 'ok 1 - first prompt renders without fallback'

checkpoint_output
# Commands after the eval must still run, as the rest of a re-sourced .zshrc does.
zpty -w nova_e2e 'eval "$("$NOVA_E2E_BIN" init zsh)"; print -r -- "RELOADED_${:-OK}"'
wait_for_output 'RELOADED_OK' 'the re-sourced init script'
# The next prompt may already be in the transcript, so submit a fresh line to
# wait on instead of racing for the one after RELOADED_OK.
checkpoint_output
zpty -w nova_e2e ':'
wait_for_output "$main_marker" 'the prompt after re-sourcing init'
typeset -a runtime_dirs=("$temp_dir"/nova-*(N/))
(( ${#runtime_dirs} == 1 )) || \
  fail "re-sourcing init left extra runtime directories: ${(qqq)runtime_dirs}"
print -r -- 'ok 2 - re-sourcing init leaves no orphaned runtime directory'


checkpoint_output
zpty -w nova_e2e 'print -r -- "$$" >| "$NOVA_E2E_STATE/shell.pid"; print -r -- "$_nova_worker_pid" >| "$NOVA_E2E_STATE/worker.pid"; print -r -- "$_nova_runtime_dir" >| "$NOVA_E2E_STATE/runtime-dir"'
wait_for_output "$main_marker" 'the prompt after capturing process state'
wait_for_file "$state_dir/shell.pid"
wait_for_file "$state_dir/worker.pid"
wait_for_file "$state_dir/runtime-dir"

shell_pid=$(<"$state_dir/shell.pid")
worker_pid=$(<"$state_dir/worker.pid")
runtime_dir=$(<"$state_dir/runtime-dir")
[[ $shell_pid == <-> ]] || fail "invalid shell pid: ${(qqq)shell_pid}"
[[ $worker_pid == <-> ]] || fail "invalid worker pid: ${(qqq)worker_pid}"
kill -0 "$worker_pid" 2>/dev/null || fail "worker $worker_pid is not running"
[[ $runtime_dir == "$temp_dir/nova-$shell_pid-"[[:xdigit:]](#c12) ]] || \
  fail "unexpected runtime directory: ${(qqq)runtime_dir}"
[[ -d $runtime_dir ]] || fail "runtime directory does not exist: ${(qqq)runtime_dir}"

checkpoint_output
zpty -w nova_e2e "cd -- ${(q)git_repo}"
wait_for_output "$branch_marker" 'the asynchronous git branch redraw'
output_since_checkpoint
# The branch precedes the prompt character, so this order proves there were two renders.
[[ $REPLY == *"$main_marker"*"$branch_marker"* ]] || \
  fail 'git branch appeared before the initial prompt instead of through a later redraw'
print -r -- 'ok 3 - git branch redraws asynchronously after cd'

checkpoint_output
zpty -w nova_e2e 'bindkey -v'
wait_for_output "$main_marker" 'the vi insert-mode prompt'
checkpoint_output
zpty -w -n nova_e2e $'\e'
wait_for_output "$vi_marker" 'the vi command-mode prompt redraw'
print -r -- 'ok 4 - keymap switch redraws the prompt character'

checkpoint_output
zpty -w -n nova_e2e 'i'
zpty -w nova_e2e 'print -r -- "TRANSIENT=<$PROMPT|$RPROMPT>"'
# While the command runs, PROMPT still holds the collapsed prompt swapped in at line-finish.
wait_for_output $'TRANSIENT=<%{\e]133;A\a%}NOVA_E2E_MAIN> %{\e]133;B\a%}|>' 'the transient prompt during command execution'

wait_for_output "$main_marker" 'the full prompt after the command'
print -r -- 'ok 5 - accepted lines collapse to the transient prompt'

checkpoint_output
zpty -w nova_e2e 'print -r -- "OSC_${:-DONE}"; false'
wait_for_output 'OSC_DONE' 'the command used to check terminal marks'
wait_for_match $'\e\\]133;C\a.*OSC_DONE.*\e\\]133;D;1\a.*\e\\]7;file://[^\a]*/repo\a.*\e\\]133;A\a' \
  'OSC 133 command marks and the OSC 7 cwd report'
print -r -- 'ok 6 - terminal integration marks commands and reports the cwd'

# With the shell's stdout redirected, marks must still reach the terminal and
# never land in the captured output.
zpty -w nova_e2e 'exec 3>&1 >"$NOVA_E2E_STATE/captured.log"'
drain_for 0.3
checkpoint_output
zpty -w nova_e2e 'print -r -- "CAPTURED_${:-LINE}"'
wait_for_match $'\e\\]133;D;0\a' 'the command marks while stdout is redirected'
zpty -w nova_e2e 'exec >&3 3>&-'
drain_for 0.3
[[ $(<"$state_dir/captured.log") == CAPTURED_LINE ]] || \
  fail "redirected output captured integration bytes: ${(qqq)$(<"$state_dir/captured.log")}"
print -r -- 'ok 7 - command marks bypass redirected stdout'




drain_for 0.3
zpty -w -n nova_e2e 'echo cancelled-line'
wait_for_output 'cancelled-line' 'the typed but unsent line'
checkpoint_output
zpty -w -n nova_e2e $'\x03'
# zsh redraws without precmd after Ctrl-C: the collapsed prompt comes first
# (no branch before the marker), then line-init restores the full prompt.
wait_for_match "${main_marker}.*${branch_marker}.*${main_marker}" 'the collapsed then restored prompt after Ctrl-C'
wait_for_output 'USER_INT' 'the user INT trap chained after Nova'
print -r -- 'ok 8 - interrupted lines collapse to the transient prompt'



checkpoint_output
typeset -g gen_a_pattern='GEN_A=<([0-9]+)>' gen_b_pattern='GEN_B=<([0-9]+)[|]40>'
zpty -w nova_e2e 'print -r -- "$TTY" >| "$NOVA_E2E_STATE/tty"; print -r -- "GEN_A=<$_nova_last_applied_gen>"'
wait_for_match "$gen_a_pattern" 'the generation before resizing'
typeset -gi gen_before=$match[1]
wait_for_file "$state_dir/tty"
# Resize from outside the session: the kernel delivers SIGWINCH to the shell
# without job-control stops, and zle's own redraw cannot bump Nova's generation.
command stty cols 40 <"$(<"$state_dir/tty")"
drain_for 0.5
wait_for_output 'USER_WINCH' 'the user WINCH trap chained after Nova'
checkpoint_output
zpty -w nova_e2e 'print -r -- "GEN_B=<$_nova_last_applied_gen|$COLUMNS>"'
wait_for_match "$gen_b_pattern" 'the generation after resizing'
# One for the command's precmd, at least one more for the SIGWINCH re-render.
(( match[1] >= gen_before + 2 )) || \
  fail "resize did not re-render the prompt (generation $gen_before -> $match[1])"
print -r -- 'ok 9 - terminal resize re-renders the prompt'



kill -KILL "$shell_pid"
wait_for_shell_exit
session_active=0
wait_for_worker_cleanup
print -r -- 'ok 10 - killed shell leaves no worker or runtime directory'
