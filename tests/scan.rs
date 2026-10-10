use ::audio_conv::{
	config::{Config, Transcode, TranscodeMatch, TranscodeMatchFile, compile_matches},
	scan::{ConversionArgs, find_transcode, get_conversion_args},
};
use ::std::{
	fs::{self, File},
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};
use ::tempfile::TempDir;

fn compile(yaml: &str) -> Vec<TranscodeMatch> {
	let matches: Vec<TranscodeMatchFile> = serde_yaml_ng::from_str(yaml).unwrap();
	compile_matches(&matches).unwrap()
}

struct Dirs {
	_tmp: TempDir,
	config: Config,
}

fn dirs(matches: Vec<TranscodeMatch>) -> Dirs {
	let tmp = tempfile::tempdir().unwrap();
	let from = tmp.path().join("from");
	let to = tmp.path().join("to");
	fs::create_dir(&from).unwrap();
	fs::create_dir(&to).unwrap();
	Dirs {
		_tmp: tmp,
		config: Config {
			from,
			to,
			matches,
			jobs: None,
		},
	}
}

fn touch(path: &Path, mtime: SystemTime) {
	fs::create_dir_all(path.parent().unwrap()).unwrap();
	File::create(path).unwrap().set_modified(mtime).unwrap();
}

fn scan(config: &Config) -> Vec<ConversionArgs> {
	let mut args = get_conversion_args(config)
		.collect::<::anyhow::Result<Vec<_>>>()
		.unwrap();
	args.sort_by(|a, b| a.rel_from_path.cmp(&b.rel_from_path));
	args
}

fn rel_paths(args: &[ConversionArgs]) -> Vec<PathBuf> {
	args.iter().map(|a| a.rel_from_path.clone()).collect()
}

fn hours_ago(hours: u64) -> SystemTime {
	SystemTime::now() - Duration::from_secs(hours * 60 * 60)
}

#[test]
fn first_match_wins() {
	let matches = compile(
		"[{extensions: [flac], to: {codec: flac}}, {extensions: [flac, wav], to: {codec: copy}}]",
	);

	assert!(matches!(
		find_transcode(&matches, Path::new("/a.flac")),
		Some(Transcode::Flac { .. })
	));
	assert!(matches!(
		find_transcode(&matches, Path::new("/a.wav")),
		Some(Transcode::Copy)
	));
	assert!(find_transcode(&matches, Path::new("/a.mp3")).is_none());
}

#[test]
fn finds_matching_files_recursively() {
	let d = dirs(compile_matches(&[]).unwrap());
	let from = &d.config.from;
	touch(&from.join("a.flac"), hours_ago(1));
	touch(&from.join("artist/album/b.WAV"), hours_ago(1));
	touch(&from.join("artist/album/cover.jpg"), hours_ago(1));
	fs::create_dir(from.join("empty.flac")).unwrap();

	let args = scan(&d.config);

	assert_eq!(
		rel_paths(&args),
		[PathBuf::from("a.flac"), PathBuf::from("artist/album/b.WAV")]
	);
	assert!(
		args.iter()
			.all(|a| matches!(a.transcode, Transcode::Opus { .. }))
	);
}

#[test]
fn skips_targets_newer_than_source() {
	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.config.from.join("a.flac"), hours_ago(2));
	touch(&d.config.to.join("a.opus"), hours_ago(1));

	assert!(scan(&d.config).is_empty());
}

#[test]
fn includes_targets_older_than_source() {
	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.config.from.join("a.flac"), hours_ago(1));
	touch(&d.config.to.join("a.opus"), hours_ago(2));

	assert_eq!(rel_paths(&scan(&d.config)), [PathBuf::from("a.flac")]);
}

#[test]
fn target_extension_depends_on_codec() {
	let d = dirs(compile("[{extensions: [flac], to: {codec: mp3}}]"));
	touch(&d.config.from.join("a.flac"), hours_ago(2));
	// a target with the extension of a different codec doesn't count
	touch(&d.config.to.join("a.opus"), hours_ago(1));

	assert_eq!(rel_paths(&scan(&d.config)), [PathBuf::from("a.flac")]);

	touch(&d.config.to.join("a.mp3"), hours_ago(1));

	assert!(scan(&d.config).is_empty());
}

#[test]
fn copy_keeps_extension() {
	let d = dirs(compile("[{extensions: [jpg], to: {codec: copy}}]"));
	touch(&d.config.from.join("cover.jpg"), hours_ago(2));
	touch(&d.config.to.join("cover.jpg"), hours_ago(1));

	assert!(scan(&d.config).is_empty());
}
