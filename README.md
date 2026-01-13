# zellij-toggler

A [Zellij](https://zellij.dev/) plugin that can open/close/toggle command panes via the [pipe](https://zellij.dev/documentation/zellij-pipe.html) interface.

## Why?

Zellij CLI has limitations for pane management:
- `zellij action new-pane` doesn't return pane ID
- `zellij action close-pane` only closes focused pane (no `--id` flag)

See [this comment](https://github.com/zellij-org/zellij/issues/2835#issuecomment-2090386835) for more context.

> [!WARNING]
> This plugin is alpha. It does not account for [multiplayer](https://zellij.dev/news/multiplayer-sessions/) or tabs. If you find this useful but missing something, open an issue.

## Build

Requires `wasm32-wasip1` target:

```bash
rustup target add wasm32-wasip1
cargo build --release
# Output: target/wasm32-wasip1/release/zellij-toggler.wasm
```

## Usage

The `pane_id` field is a client-defined identifier (not a native Zellij pane ID). The client generates this ID and uses it to manage the pane lifecycle

```bash
PLUGIN="file:$(pwd)/target/wasm32-wasip1/release/zellij-toggler.wasm"

# Open pane with htop
echo '{"pane_id":"my_htop","cmd":"htop"}' | zellij pipe --name open --plugin "$PLUGIN"

# Open pane with custom args and cwd
echo '{"pane_id":"my_pane","cmd":"python","args":["-m","http.server"],"cwd":"/tmp"}' | zellij pipe --name open --plugin "$PLUGIN"

# Close pane
echo '{"pane_id":"my_htop"}' | zellij pipe --name close --plugin "$PLUGIN"

# Toggle pane (requires cmd for re-open, ignored on close)
echo '{"pane_id":"my_htop", "cmd":"htop"}' | zellij pipe --name toggle --plugin "$PLUGIN"
```

### Responses

**Success:**
```json
{"ok": true}
{"ok": true, "action": "opened"}
{"ok": true, "action": "closed"}
```

**Warning**:
```json
{"ok": true, "warning": "pane is already opened"}
{"ok": true, "warning": "pane not found"}
```

**Error**:
```json
{"ok": false, "error": "pane is closing"}
{"ok": false, "error": "unknown command: ..."}
```
