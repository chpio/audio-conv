# audio-conv

Takes two paths, all audio files encountered in the first path are transcoded and stored in the
second path. The directory structure from the first path gets also copied to the second path.

- transcodes to _Opus_, _FLAC_ or _MP3_, or copies files as they are
- extracts the audio of video files, transcoded or as it is (`copy_audio`)
- only converts files whose converted version is missing or older than the source, so repeated
  runs only pick up new and changed files
- converts multiple files in parallel and shows the progress in the terminal
- failed files don't stop the run, their errors are written to _audio-conv.log_

## Dependencies

Requires _gstreamer_ version 1.16 or higher with the _base_ plugins. Transcoding to _FLAC_ or
_MP3_ and `copy_audio` into anything but _ogg_/_opus_ additionally need the _good_ plugins.

The supported source audio formats (or even other media that is able to contain audio) depend on
the installed _gstreamer_ plugins.

## Installation

```bash
$ cargo install audio-conv
```

## Usage

_audio-conv_ is able to write an example config to your current directory:

```bash
$ audio-conv init
```

Now you need to edit the generated _audio-conv.yaml_ file. And let it convert your audio files
by running it:

```bash
$ audio-conv
```

The config file and its settings can be overridden on the command line:

```
-c, --config <config>  Path to an audio-conv config file, defaults to "audio-conv.yaml"
-f, --from <from>      "from" directory path
-t, --to <to>          "to" directory path
-j, --jobs <jobs>      Allow N jobs/transcodes at once. Defaults to number of logical cores
```

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
