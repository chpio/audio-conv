# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`audio-conv` is a CLI that walks a `from` directory, transcodes matching audio files (opus, flac, mp3, or plain copy) via GStreamer into a mirrored directory structure under `to`, and shows progress in a terminal UI. It requires GStreamer >= 1.16 with the base plugins at runtime; which source formats work depends on the installed GStreamer plugins.

## Commands

```bash
cargo build
cargo clippy --all-targets
cargo test
UPDATE_SCHEMA=1 cargo test --test schema   # regenerate audio-conv.schema.json after changing the config types
cargo fmt                  # hard tabs (.rustfmt.toml)
cargo run -- init          # writes example.audio-conv.yaml as ./audio-conv.yaml
cargo run -- -f <from> -t <to> [-j N] [-c config.yaml]
cargo outdated -R          # find outdated direct deps
```

Tests live in `tests/` (integration tests against the library, no inline `#[cfg(test)]` modules) and cover config/match compilation, the directory scan and the encoder/muxer elements per codec (`tests/gstreamer.rs`, needs the GStreamer plugins of all codecs installed); the full transcode of files and the UI are untested.

## Architecture

- **Crate layout**: all modules live in the library (`lib.rs`) so tests under `tests/` can use them; the binary (`main.rs`) only has `main`, `init` and `main_loop`.
- **Context** (`context/mod.rs`): there is no separate config struct or module, `context::load` merges config file and CLI args directly into `Context`, which also holds runtime resources, currently the `Log` (`log.rs`, opens the file lazily, so tests can build a `Context` directly). It's shared as `Arc<Context>` (the UI state moves into `spawn_blocking` for rendering). The name clashes with `anyhow::Context`, which is then imported as `Context as _`.
- **Runtime model** (`main.rs`): single-threaded tokio runtime inside a `LocalSet`. Before the UI starts, `init` (in `spawn_blocking`) loads the config, initializes GStreamer and walks the directory, then `delete::run` handles `--delete` in its own `spawn_blocking`; the UI gets the context, the number of tasks and the delete result directly from `ui::init()`. Then two local tasks run concurrently: `main_loop` (the conversion work) and the UI future.
- **Job selection** (`scan.rs`, `get_conversion_args`): every file under `from` is matched against `ctx.matches` (first match wins, matching on the raw path bytes). A file is converted only if the target is missing or older than the source (mtime). Transcodes run with `try_for_each_concurrent`, limited by `jobs` (default: number of CPUs).
- **Transcoding** (`transcode/`): `mod.rs` writes output to the target path with the extension `.tmp` first and renames it on success (`rm_file_on_err` deletes the tmp file on failure), so partial files never pass the mtime check. `Transcode::Copy` is a plain `fs::copy`; everything else goes through `gstreamer.rs`, which builds a GStreamer pipeline `filesrc ! decodebin` and attaches `audioresample ! audioconvert ! <encoder> [! mux] ! filesink` (for `copy_audio`: `filesrc ! parsebin` and `<muxer for the configured extension> ! filesink`, without decoding) dynamically in decodebin's `pad-added` callback (first audio pad only, so the audio of video files works too; `autoplug-select` exposes video/image streams undecoded, and the run fails with "No audio stream found" if no audio pad got linked). Errors inside that callback are wrapped in `GBoxErrorWrapper` (a glib `Boxed` type, in `error.rs`) and sent through `element_error!` so the bus stream can recover the original `anyhow::Error`. Progress is polled from decodebin position/duration and sent to the UI.
- **Deleting** (`delete.rs`, CLI only, deliberately no config file setting): `--delete[=converted|all]` lists the files in `to` that aren't the target of a matching file in `from` (`converted`: only extensions of configured codecs, `.tmp` and paths a `copy` match would create) and logs them; only with `--force` they're deleted, including emptied dirs. `find_orphans` errors instead of returning a possibly too long list: on any read error while walking, if nothing in `from` matches (unmounted drive) or if `from` and `to` overlap. The config and log file are never deleted.
- **Errors**: per-file failures don't abort the run. They are appended to `./audio-conv.log` and the task is marked as failed in the UI. `Log` opens the file on the first write (no empty log file) and writes each entry in a single, mutex-guarded write; the sync write from async code is accepted as entries are short.
- **UI** (`ui.rs`): `MsgQueue` is a `Rc<RefCell<Vec<Msg>>>` (not a channel), drained by the UI loop every `UPDATE_INTERVAL_MILLIS`. Rendering uses `ratatui` with the crossterm backend; the initial clear goes through the backend directly because `Terminal::clear` queries the cursor position.
- **Config** (`context/`): `config_file.rs` has the YAML types (`ConfigFile` …, with `json_schema`; only `Transcode`, `AudioExtension`, `BitrateType` (part of `Context` via `matches`) and `TranscodeMatchFile` (taken by `compile_matches`) are re-exported from `context`, `ConfigFile` and `json_schema` stay under `context::config_file`), `cli.rs` the clap definition parsed into a typed `Args`, `mod.rs` merges both in `load` and compiles the matches. Clap args override the YAML config file (`audio-conv.yaml` in the current dir by default). Paths from the config file are resolved relative to the config file's directory, CLI paths relative to the cwd. Each match entry compiles `glob`, `regex` and `extensions` into case-insensitive byte regexes; with no matches configured, the default is `\.(flac|wav)$` → opus 160 kbit/s VBR. `example.audio-conv.yaml` is embedded into the binary via `include_bytes!` for `init`, so keep it in sync with the config format. `audio-conv.schema.json` (JSON Schema draft-07 for editor support, referenced by the `yaml-language-server` modeline in the example via its GitLab raw URL on `main`) is generated from the config types with `schemars`; doc comments on them become the descriptions, `tests/schema.rs` fails if the file is outdated and validates the example against it. The schema denies unknown fields, serde itself still ignores them.
- **Tags** (`tag.rs`): registers the custom GStreamer tags `musicbrainz-artistid` / `musicbrainz-albumartistid`, whose merge function joins multiple values with commas.

## Conventions

- Conventional commit prefixes (`chore:`, `docs:`, `build:` …). Dependency upgrades are committed as `chore: update deps`.
- Releases: `chore: bump version (vX.Y.Z)` bumps the version in `Cargo.toml`/`Cargo.lock` and adds a `CHANGELOG.md` entry; the commit is tagged `vX.Y.Z`.
- `use` statements for external crates (including `std`) start with `::` (e.g. `use ::anyhow::Result;`); `crate::`/`super::` paths stay as they are. rustfmt has no option for this, so keep it by hand.
- Use glib via `gstreamer::glib` instead of adding `glib` as a direct dependency, so its version can't drift from the one gstreamer uses.
- GStreamer enum properties (e.g. opusenc `bitrate-type`, lamemp3enc `target`, flacenc `quality`) must be set with `set_property_from_str`. Passing a string through `gmake`/`property()` compiles but panics at runtime when the pipeline is built.
