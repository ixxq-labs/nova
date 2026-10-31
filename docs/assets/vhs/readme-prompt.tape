# Nova README demo. Run from the repository root: vhs docs/assets/vhs/readme-prompt.tape
# Everything happens in a throwaway HOME and git repository; the checkout is
# only built, never modified.
Source docs/assets/vhs/config.tape
Output docs/assets/vhs/readme-prompt.gif
Require nix

Hide
Type "exec env -i HOME=$(mktemp -d) TERM=xterm-256color COLORTERM=truecolor PATH=$PATH NOVA_VHS_PROJECT=$PWD zsh -f"
Enter
Type "nix develop -c cargo build -q"
Enter
Sleep 5s
Type "export XDG_RUNTIME_DIR=$HOME PATH=$NOVA_VHS_PROJECT/target/debug:$PATH NOVA_CONFIG=$NOVA_VHS_PROJECT/docs/assets/vhs/async-demo.toml"
Enter
# Never touch a real home directory.
Type "[[ $HOME == /*/tmp.* ]] || exit 1"
Enter
Type "mkdir -p ~/dev/acme/api/k8s && cd ~/dev/acme/api && git init -q -b main && print 'fn main() {}' > main.rs && git add main.rs && git -c user.name=demo -c user.email=demo@example.invalid commit -qm init && git switch -qc feat/checkout-flow && print '// todo' >> main.rs && print notes > notes.md && print plan > plan.md && git add plan.md && cd ~"
Enter
Type `eval "$(nova init zsh)"`
Enter
Sleep 1s
Type "clear"
Enter
Sleep 500ms
Show
Sleep 1s

Type "cd dev/acme/api"
Sleep 0.25s
Enter
Sleep 2.2s
Type "git status --short"
Sleep 0.25s
Enter
Sleep 1.5s
Type "false"
Sleep 0.25s
Enter
Sleep 1.5s
Type "sleep 2"
Sleep 0.25s
Enter
Sleep 4s
