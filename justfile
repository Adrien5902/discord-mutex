alias rd := run-daemon
run-daemon:
	cargo run --bin discord-mutexd

alias rc := run-cli
run-cli:
	cargo run --bin mutex

alias w := watch
watch:
	watchexec -r "just run-daemon"

alias b := build
build:
	cargo build -r --bin mutex & cargo build -r --bin discord-mutexd
