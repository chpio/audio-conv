# Changelog

## Unreleased

* log the directories in "from" that can't be read instead of skipping them silently

## 1.5.0

* add `--delete` to log the files in "to" that don't belong to a file in "from", and `--force`
  to delete them
* include the tests in the crates.io package

## 1.4.1

* fix the commented `regex` example in the config written by `init`, it was invalid YAML
* improve the `--help` texts
* upgrade dependencies

## 1.4.0

* fix transcoding to "opus", "flac" and "mp3", v1.3.2 crashed on every transcode
* fix "copy" copying files again on every run
* "copy_audio" encoding format added, copies the first audio stream without transcoding it into a
  "m4a", "mka", "ogg", "opus", "webm", "mp3" or "flac" file, e.g. to extract the audio of video
  files
* only the first audio stream of a file gets converted, video streams are no longer decoded, and
  files without an audio stream fail with "No audio stream found"
* JSON schema for the config file added for editor support, configs written by `init` reference it
* dependencies upgraded

## 1.3.2

* dependencies upgraded

## 1.3.1

* dependencies upgraded

## 1.3.0

* allow multiple values for the tags "musicbrainz-artistid" and "musicbrainz-albumartistid"
* fix "from", "to" & "config" cli argument processing

## 1.2.2

* dependencies upgraded

## 1.2.1

* dependencies upgraded

## 1.2.0

* "copy" encoding format added
* "jobs" cli argument added, that lets you set the number of concurrent transcodes

## 1.1.0

* "flac" encoding format added
* resampling quality set to highest/"10"
