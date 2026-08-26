alias rd := run-daemon
alias rc := run-cli
alias b := build
alias w := watch
alias pkg := package
alias cpkg := clean-packages
alias de := systemd-enable
alias dd := systemd-disable
alias i := aur-install
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
    just clean-packages & just package-aur

clean-aur:
	rm -rf target/cargo-aur

clean-packages:
	just clean-aur

systemd-enable:
	systemctl --user enable --now discord-mutexd

systemd-disable:
	systemctl --user disable --now discord-mutexd

aur-install:
	just clean-aur; just package-aur; cd ./target/cargo-aur; makepkg -si; just systemd-enable
