# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`audio-conv` is a CLI that walks a `from` directory, transcodes matching audio files (opus, flac, mp3, or plain copy) via GStreamer into a mirrored directory structure under `to`, and shows progress in a terminal UI. It requires GStreamer >= 1.16 with the base plugins at runtime; which source formats work depends on the installed GStreamer plugins.

## Commands

```bash
cargo build
cargo clippy
cargo fmt                  # hard tabs (.rustfmt.toml)
cargo run -- init          # writes example.audio-conv.yaml as ./audio-conv.yaml
cargo run -- -f <from> -t <to> [-j N] [-c config.yaml]
cargo outdated -R          # find outdated direct deps (cargo info/search don't work: crates-io is mirrored via kellnr)
```

There are no tests.

## Architecture

- **Runtime model** (`main.rs`): single-threaded tokio runtime inside a `LocalSet`. Two local tasks run concurrently: `main_loop` (the conversion work) and the UI future from `ui::init()`. GStreamer init, config parsing and the directory walk happen in `spawn_blocking`.
- **Job selection** (`get_conversion_args`): every file under `from` is matched against `config.matches` (first match wins, matching on the raw path bytes). A file is converted only if the target is missing or older than the source (mtime). Transcodes run with `try_for_each_concurrent`, limited by `jobs` (default: number of CPUs).
- **Transcoding**: output goes to `<target>.tmp` first and is renamed on success (`rm_file_on_err` deletes the tmp file on failure), so partial files never pass the mtime check. `Transcode::Copy` is a plain `fs::copy`; everything else builds a GStreamer pipeline `filesrc ! decodebin` and attaches `audioresample ! audioconvert ! <encoder> [! mux] ! filesink` dynamically in decodebin's `pad-added` callback (audio pads only). Errors inside that callback are wrapped in `GBoxErrorWrapper` (a glib `Boxed` type) and sent through `element_error!` so the bus stream can recover the original `anyhow::Error`. Progress is polled from decodebin position/duration and sent to the UI.
- **Errors**: per-file failures don't abort the run. They are appended to `./audio-conv.log` and the task is marked as failed in the UI.
- **UI** (`ui.rs`): `MsgQueue` is a `Rc<RefCell<Vec<Msg>>>` (not a channel), drained by the UI loop every `UPDATE_INTERVAL_MILLIS`. Rendering uses `ratatui` with the crossterm backend; the initial clear goes through the backend directly because `Terminal::clear` queries the cursor position.
- **Config** (`config.rs`): clap args override the YAML config file (`audio-conv.yaml` in the current dir by default). Paths from the config file are resolved relative to the config file's directory, CLI paths relative to the cwd. Each match entry compiles `glob`, `regex` and `extensions` into case-insensitive byte regexes; with no matches configured, the default is `\.(flac|wav)$` → opus 160 kbit/s VBR. `example.audio-conv.yaml` is embedded into the binary via `include_bytes!` for `init`, so keep it in sync with the config format.
- **Tags** (`tag.rs`): registers the custom GStreamer tags `musicbrainz-artistid` / `musicbrainz-albumartistid`, whose merge function joins multiple values with commas.

## Conventions

- Conventional commit prefixes (`chore:`, `docs:`, `build:` …). Dependency upgrades are committed as `chore: update deps`.
- Releases: `chore: bump version (vX.Y.Z)` bumps the version in `Cargo.toml`/`Cargo.lock` and adds a `CHANGELOG.md` entry; the commit is tagged `vX.Y.Z`.
- Use glib via `gstreamer::glib` instead of adding `glib` as a direct dependency, so its version can't drift from the one gstreamer uses.
- GStreamer enum properties (e.g. opusenc `bitrate-type`, lamemp3enc `target`, flacenc `quality`) must be set with `set_property_from_str`. Passing a string through `gmake`/`property()` compiles but panics at runtime when the pipeline is built.
