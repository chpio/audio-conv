# audio-conv

Converts all audio files in a directory and writes them, with the same directory structure, to
another directory, e.g. to keep a lossy copy of a _FLAC_ music library for a phone.

- transcodes to _Opus_, _FLAC_ or _MP3_, or copies files as they are
- extracts the audio of video files, transcoded or as it is (`copy_audio`)
- only converts files whose converted version is missing or older than the source, so repeated
  runs only pick up new and changed files
- converts multiple files in parallel and shows the progress in the terminal
- failed files don't stop the run, their errors are written to _audio-conv.log_ in the current
  directory

## Dependencies

Requires _GStreamer_ version 1.16 or higher with the _base_ plugins. Transcoding to _FLAC_ or
_MP3_ and `copy_audio` into anything but _ogg_/_opus_ additionally need the _good_ plugins.

The supported source audio formats (or even other media that is able to contain audio) depend on
the installed _GStreamer_ plugins.

## Installation

Building needs `pkg-config` and the _GStreamer_ development files, e.g.
`libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev` on _Debian_/_Ubuntu_ or
`gstreamer1-devel gstreamer1-plugins-base-devel` on _Fedora_.

```bash
$ cargo install audio-conv
```

## Usage

_audio-conv_ is able to write an example config to your current directory:

```bash
$ audio-conv init
```

Now edit the generated _audio-conv.yaml_ file and convert your audio files by running:

```bash
$ audio-conv
```

The config file and its settings can be overridden on the command line:

```
-c, --config <FILE>         Path to the config file [default: audio-conv.yaml]
-f, --from <DIR>            Directory with the source files, overrides "from" of the config file
-t, --to <DIR>              Directory the converted files are written to, overrides "to" of the config file
-j, --jobs <N>              Number of files converted at once [default: number of logical cores]
    --delete[=<MODE>]       Logs the files in "to" that don't belong to a file in "from", only the ones audio-conv would create ("converted", the default) or all of them ("all") [possible values: converted, all]
    --force                 Deletes the files found by --delete instead of only logging them, run without it first to check them
    --max-delete <PERCENT>  With --force, nothing is deleted if it would be more than this share of the files in "to", e.g. because a part of "from" isn't mounted [default: 10]
```

Paths in the config file are relative to the config file, paths on the command line relative to
the current directory.

Files are only compared by their modification time, so changing the config doesn't convert
existing files again (delete them from the target directory for that), and files deleted from the
source directory stay in the target directory, unless `--delete` is used.

### Deleting files

`--delete` finds the files in the target directory that don't belong to a file in the source
directory, e.g. because the source file was deleted or doesn't match any more. It only writes them
to _audio-conv.log_, they are deleted only with `--delete --force`. Directories left empty are
deleted too. There is no config file setting for it.

- `--delete` only considers files _audio-conv_ creates with the current config: files with the
  extension of a configured codec (e.g. _.opus_), files a `copy` match would create and leftover
  temporary files (_.tmp_). Other files, e.g. covers or playlists you added, stay.
- `--delete=all` considers every file in the target directory.

Always run `--delete` without `--force` first and check the log: _audio-conv_ can't tell a
deleted source file from one that's just missing at the moment, e.g. on a drive or share mounted
inside the source directory that isn't mounted. Their converted files would be deleted.

As a safeguard, `--force` deletes nothing if that would be more than 10 % of the files in the target
directory, e.g. after a part of the source directory went missing. To delete more, e.g. the old
files with `--delete=all` after changing the codec, raise the limit with `--max-delete=<PERCENT>`.

The config and log file are never deleted. Nothing is deleted if the source directory contains no
matching file (e.g. an unmounted drive), can't be read completely, or if the source and target
directories overlap.

## Configuration

```yaml
from: ./music
to: ./converted
matches:
    - extensions: [flac, wav]
      to:
          codec: opus
          bitrate: 160
    - glob: "**/*.mp4"
      to:
          codec: copy_audio
          extension: m4a
```

Every file is checked against the `matches`, the first matching one decides how it gets
converted, files matching none are skipped. A match selects files by `extensions`, `glob` and/or
`regex` (case insensitive, matched against the absolute path), without any of them it matches
_.flac_ and _.wav_ files. Without any `matches`, _.flac_ and _.wav_ files are transcoded to
_Opus_ with 160 kbit/s.

| `codec`      | output file          | options                                                                                                                                                                    |
| ------------ | -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `opus`       | _.opus_              | `bitrate` in kbit/s (default 160), `bitrate_type`: `vbr` (default) or `cbr`                                                                                                |
| `flac`       | _.flac_              | `compression` from 0 (fastest) to 9 (smallest), default 5                                                                                                                  |
| `mp3`        | _.mp3_               | `bitrate` in kbit/s (default 256), `bitrate_type`: `vbr` (average bitrate, default) or `cbr`                                                                               |
| `copy`       | same as the source   | copies the file as it is                                                                                                                                                   |
| `copy_audio` | given by `extension` | copies the first audio stream without transcoding it into an `m4a`, `mka`, `ogg`, `opus`, `webm`, `mp3` or `flac` file, the source's audio codec has to be supported by it |

For video files (or other files with multiple streams) only the first audio stream is used.

### Editor support

There is a [JSON schema](audio-conv.schema.json) for the config file. Editors using the
_yaml-language-server_ (e.g. _VS Code_ with the _YAML_ extension) offer completion, documentation
and validation with it. Configs written by `audio-conv init` already reference it, for other
configs add this line at the top:

```yaml
# yaml-language-server: $schema=https://gitlab.com/chpio/audio-conv/-/raw/main/audio-conv.schema.json
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
