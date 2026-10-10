use ::audio_conv::context::{
	AudioExtension, BitrateType, Transcode, TranscodeMatch, TranscodeMatchFile, compile_matches,
	config_file::ConfigFile,
};
use ::std::path::Path;

fn compile_one(yaml: &str) -> ::anyhow::Result<TranscodeMatch> {
	let m: TranscodeMatchFile = serde_yaml_ng::from_str(yaml).unwrap();
	let mut matches = compile_matches(&[m])?;
	assert_eq!(matches.len(), 1);
	Ok(matches.remove(0))
}

fn is_match(m: &TranscodeMatch, path: &str) -> bool {
	m.regexes
		.iter()
		.any(|regex| regex.is_match(path.as_bytes()))
}

#[test]
fn no_matches_fall_back_to_default() {
	let matches = compile_matches(&[]).unwrap();

	assert_eq!(matches.len(), 1);
	assert!(matches!(
		matches[0].to,
		Transcode::Opus {
			bitrate: 160,
			bitrate_type: BitrateType::Vbr
		}
	));
	assert!(is_match(&matches[0], "/music/a.flac"));
	assert!(is_match(&matches[0], "/music/a.WAV"));
	assert!(!is_match(&matches[0], "/music/a.mp3"));
}

#[test]
fn empty_match_uses_default_regex() {
	let m = compile_one("to: {codec: copy}").unwrap();

	assert!(matches!(m.to, Transcode::Copy));
	assert!(is_match(&m, "/music/a.flac"));
	assert!(!is_match(&m, "/music/a.mp3"));
}

#[test]
fn extensions_match_case_insensitive_at_end() {
	let m = compile_one("{extensions: [mp3, m4a], to: {codec: copy}}").unwrap();

	assert!(is_match(&m, "/music/a.mp3"));
	assert!(is_match(&m, "/music/a.M4A"));
	assert!(!is_match(&m, "/music/a.mp3.txt"));
	assert!(!is_match(&m, "/music/amp3"));
}

#[test]
fn extensions_are_escaped() {
	let m = compile_one("{extensions: [c++], to: {codec: copy}}").unwrap();

	assert!(is_match(&m, "/src/a.c++"));
	assert!(!is_match(&m, "/src/a.cc"));
}

#[test]
fn glob_matches_case_insensitive() {
	let m = compile_one("{glob: '**/*.ogg', to: {codec: copy}}").unwrap();

	assert!(is_match(&m, "/music/artist/a.OGG"));
	assert!(!is_match(&m, "/music/artist/a.ogg.bak"));
}

#[test]
fn regex_matches_case_insensitive() {
	let m = compile_one(r#"{regex: '/live/.*\.flac$', to: {codec: copy}}"#).unwrap();

	assert!(is_match(&m, "/music/Live/a.flac"));
	assert!(!is_match(&m, "/music/studio/a.flac"));
}

#[test]
fn glob_regex_and_extensions_are_combined() {
	let m =
		compile_one("{glob: '**/*.ogg', regex: '\\.mp3$', extensions: [m4a], to: {codec: copy}}")
			.unwrap();

	assert_eq!(m.regexes.len(), 3);
	assert!(is_match(&m, "/a.ogg"));
	assert!(is_match(&m, "/a.mp3"));
	assert!(is_match(&m, "/a.m4a"));
	assert!(!is_match(&m, "/a.flac"));
}

#[test]
fn invalid_regex_is_an_error() {
	assert!(compile_one("{regex: '(', to: {codec: copy}}").is_err());
}

#[test]
fn invalid_glob_is_an_error() {
	assert!(compile_one("{glob: '[', to: {codec: copy}}").is_err());
}

#[test]
fn codec_defaults() {
	let to = |yaml| serde_yaml_ng::from_str::<Transcode>(yaml).unwrap();

	assert!(matches!(
		to("codec: opus"),
		Transcode::Opus {
			bitrate: 160,
			bitrate_type: BitrateType::Vbr
		}
	));
	assert!(matches!(
		to("codec: mp3"),
		Transcode::Mp3 {
			bitrate: 256,
			bitrate_type: BitrateType::Vbr
		}
	));
	assert!(matches!(
		to("codec: flac"),
		Transcode::Flac { compression: 5 }
	));
}

#[test]
fn copy_audio_uses_configured_extension() {
	let to = serde_yaml_ng::from_str::<Transcode>("{codec: copy_audio, extension: m4a}").unwrap();

	assert!(matches!(
		to,
		Transcode::CopyAudio {
			extension: AudioExtension::M4a
		}
	));
	assert_eq!(to.extension(), Some("m4a"));
}

#[test]
fn copy_audio_needs_a_known_extension() {
	assert!(serde_yaml_ng::from_str::<Transcode>("codec: copy_audio").is_err());
	assert!(serde_yaml_ng::from_str::<Transcode>("{codec: copy_audio, extension: aac}").is_err());
}

#[test]
fn unknown_codec_is_an_error() {
	assert!(serde_yaml_ng::from_str::<Transcode>("codec: aac").is_err());
}

#[test]
fn example_config_is_valid() {
	let config: ConfigFile =
		serde_yaml_ng::from_slice(include_bytes!("../example.audio-conv.yaml")).unwrap();

	assert_eq!(config.from.as_deref(), Some(Path::new("./music")));
	assert_eq!(config.to.as_deref(), Some(Path::new("./converted")));

	let matches = compile_matches(&config.matches).unwrap();
	assert_eq!(matches.len(), 1);
	assert!(matches!(
		matches[0].to,
		Transcode::Opus {
			bitrate: 160,
			bitrate_type: BitrateType::Vbr
		}
	));
	assert!(is_match(&matches[0], "/music/a.flac"));
	assert!(is_match(&matches[0], "/music/a.wav"));
}
