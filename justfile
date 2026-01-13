target := "wasm32-wasip1"
wasm_name := "zellij-toggler.wasm"
debug_wasm := "target" / target / "debug" / wasm_name
release_wasm := "target" / target / "release" / wasm_name

default:
    @just --list

dev:
    cargo build
    zellij action start-or-reload-plugin "file:{{justfile_directory()}}/{{debug_wasm}}"

release:
    cargo build --release
    zellij action start-or-reload-plugin "file:{{justfile_directory()}}/{{release_wasm}}"
