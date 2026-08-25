alias rd := run-daemon
alias rc := run-cli
alias b := build
alias w := watch
alias pkg := package
alias c := check

run-daemon *ARGS:
    cargo run --bin discord-mutexd -- {{ ARGS }}

run-cli *ARGS:
    cargo run --bin mutex -- {{ ARGS }}

watch:
    watchexec -r "just run-daemon"

check:
    cargo fmt & cargo c

build:
    cargo fmt & cargo build -r

package-aur:
    cargo aur

package:
    just package-aur
