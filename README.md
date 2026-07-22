<div align="center">

<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://media.x.ai/v1/website/spacexai-symbol-white-transparent-0c31957f.png">
    <source media="(prefers-color-scheme: light)" srcset="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png">
    <img alt="SpaceXAI logo" src="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png" width="96">
  </picture>
  <br>
  Grok Build (<code>grok</code>)
</h1>

**Grok Build** is SpaceXAI's terminal-based AI coding agent. It runs as a
full-screen TUI that understands your codebase, edits files, executes shell
commands, searches the web, and manages long-running tasks — interactively,
headlessly for scripting/CI, or embedded in editors via the Agent Client
Protocol (ACP).

[Installing the released binary](#installing-the-released-binary) ·
[Building from source](#building-from-source) ·
[Local-only fork](#local-only-fork) ·
[Documentation](#documentation) ·
[Repository layout](#repository-layout) ·
[Development](#development) ·
[Contributing](#contributing) ·
[License](#license)

![Grok Build TUI](https://media.x.ai/v1/website/universe-tui-screenshot-6f7a0837.png)

**Learn more about Grok Build at [x.ai/cli](https://x.ai/cli)**

This repository contains the Rust source for the `grok` CLI/TUI and its agent
runtime. It is synced periodically from the SpaceXAI monorepo.

</div>

---

## Installing the released binary

Prebuilt binaries are published for macOS, Linux, and Windows:

```sh
curl -fsSL https://x.ai/cli/install.sh | bash   # macOS / Linux / Git Bash
irm https://x.ai/cli/install.ps1 | iex          # Windows PowerShell
grok --version
```

See the [changelog](https://x.ai/build/changelog) for the latest fixes,
features, and improvements in each release.

## Building from source

Requirements:

- **Rust** — the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` installs it automatically on first build.
- **protoc** — proto codegen resolves [`bin/protoc`](bin/protoc) (a
  [dotslash](https://dotslash-cli.com) launcher) or falls back to a `protoc` on
  `PATH` / `$PROTOC`.
- macOS and Linux are supported build hosts; Windows builds are best-effort
  and not currently tested from this tree.

```sh
cargo run -p xai-grok-pager-bin              # build + launch the TUI
cargo build -p xai-grok-pager-bin --release  # release binary: target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin            # fast validation
```

The binary artifact is named `xai-grok-pager`; official installs ship it as
`grok`.

> [!NOTE]
> **This fork defaults to a local-only build** (`local-only` Cargo feature on).
> It does **not** open a browser for xAI OAuth. Configure a local model
> `base_url` instead — see [Local-only fork](#local-only-fork). Upstream cloud
> auth docs remain at
> [02-authentication.md](crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md)
> for reference only.

## Local-only fork

This tree ships as a **hybrid local-only** agent: cloud storage uploads,
telemetry bake-in, auto-update, remote settings fetch, and default xAI OAuth
are compile-time disabled. Inference stays OpenAI-compatible HTTP so you can
point at Ollama, llama.cpp, or similar — with a hard deny-list for `*.x.ai` /
`*.grok.com`.

**Unsupported in this build:** xAI cloud inference, `grok login` / `auth.x.ai`,
cli-chat-proxy, GCS / `POST /v1/storage` uploads, and Mixpanel/Sentry bake-in.

### Build (defaults are enough)

```sh
cargo run -p xai-grok-pager-bin
# or
cargo build -p xai-grok-pager-bin --release
```

The `local-only` feature is on by default for `xai-grok-pager-bin` and
`xai-grok-shell`. Upstream-shaped cloud builds are out of scope for this fork.

### Minimal `~/.grok/config.toml` (Ollama)

Create this **before** first launch. No `auth.json` or `XAI_API_KEY` required.

```toml
[cli]
auto_update = false

[features]
telemetry = false
remote_fetch = false

[local_traces]
enabled = false

[models]
default = "local-qwen"

[model.local-qwen]
model = "qwen2.5-coder:32b"
base_url = "http://127.0.0.1:11434/v1"
api_backend = "chat_completions"
context_window = 32768
stream_tool_calls = false
```

Pull a model and confirm the OpenAI-compatible endpoint:

```sh
ollama pull qwen2.5-coder:32b
curl -s http://127.0.0.1:11434/v1/models | head
```

Then start the TUI (`cargo run -p xai-grok-pager-bin`). Expect traffic only to
your local `base_url` — not `cli-chat-proxy.grok.com` or `api.x.ai`.

### Opt-in local turn traces

Session resume files under `~/.grok/sessions/` are unchanged. Separately, you
can enable **local turn traces** (metadata + messages only — never full-repo
snapshots):

```toml
[local_traces]
enabled = true
# max_bytes_per_session = 104857600   # optional; default 100 MiB
```

Or set `GROK_LOCAL_TRACES=1`. When enabled, turns land under:

```text
~/.grok/traces/<session_id>/turn_<n>/
  metadata.json
  messages.jsonl
```

Default remains **off**.

### Related notes

- Security / threat-model research that motivated this fork:
  [`.cursor/plans/grok_build_security_review_5410264b.plan.md`](.cursor/plans/grok_build_security_review_5410264b.plan.md)
  — that plan describes **upstream cloud** behavior; for day-to-day use of
  **this** tree, prefer this README section.
- Fail-closed regression coverage:
  `cargo test -p xai-grok-shell --test test_local_only_regression`

## Documentation

Full online documentation is available at
[docs.x.ai/build/overview](https://docs.x.ai/build/overview).

The user guide ships with the pager crate:
[`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)
— getting started, keyboard shortcuts, slash commands, configuration, theming,
MCP servers, skills, plugins, hooks, headless mode, sandboxing, and more.

## Repository layout

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-pager-bin` | Composition-root package; builds the `xai-grok-pager` binary |
| `crates/codegen/xai-grok-pager` | The TUI: scrollback, prompt, modals, rendering |
| `crates/codegen/xai-grok-shell` | Agent runtime + leader/stdio/headless entry points |
| `crates/codegen/xai-grok-tools` | Tool implementations (terminal, file edit, search, ...) |
| `crates/codegen/xai-grok-workspace` | Host filesystem, VCS, execution, checkpoints |
| `crates/codegen/...` | The rest of the CLI crate closure (config, MCP, markdown, sandbox, ...) |
| `crates/common/`, `crates/build/`, `prod/mc/` | Small shared leaf crates pulled in by the closure |
| `third_party/` | Vendored upstream source (Mermaid diagram stack) — see below |

> [!IMPORTANT]
> The root `Cargo.toml` (workspace members, dependency versions, lints,
> profiles) is **generated** — treat it as read-only. Prefer editing per-crate
> `Cargo.toml` files.

## Development

```sh
cargo check -p <crate>        # always target specific crates; full-workspace builds are slow
cargo test -p xai-grok-config # per-crate tests
cargo clippy -p <crate>       # lint config: clippy.toml at the repo root
cargo fmt --all               # rustfmt.toml at the repo root
```

## Contributing

> [!NOTE]
> External contributions are not accepted. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

First-party code in this repository is licensed under the **Apache License,
Version 2.0** — see [`LICENSE`](LICENSE).

Third-party and vendored code remains under its original licenses. See:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git dependencies,
  bundled UI themes, and **in-tree source ports** (including openai/codex and
  sst/opencode tool implementations)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
  — crate-local notice for the codex and opencode ports (license texts +
  Apache §4(b) change notice)
- [`third_party/NOTICE`](third_party/NOTICE) — vendored Mermaid-stack index
