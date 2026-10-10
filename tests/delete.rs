use ::audio_conv::{
	config::{Config, Delete, DeleteMode, TranscodeMatch, TranscodeMatchFile, compile_matches},
	context::Context,
	delete::{find_orphans, remove, run},
};
use ::std::{
	fs::{self, File},
	path::{Path, PathBuf},
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
	// canonical like the paths from the config
	let root = tmp.path().canonicalize().unwrap();
	let from = root.join("from");
	let to = root.join("to");
	fs::create_dir(&from).unwrap();
	fs::create_dir(&to).unwrap();
	Dirs {
		_tmp: tmp,
		config: Config {
			config_path: root.join("audio-conv.yaml"),
			log_path: root.join("audio-conv.log"),
			from,
			to,
			matches,
			jobs: None,
			delete: None,
		},
	}
}

fn touch(path: &Path) {
	fs::create_dir_all(path.parent().unwrap()).unwrap();
	File::create(path).unwrap();
}

fn orphans(config: &Config, mode: DeleteMode) -> Vec<PathBuf> {
	find_orphans(config, mode)
		.unwrap()
		.into_iter()
		.map(|path| path.strip_prefix(&config.to).unwrap().to_path_buf())
		.collect()
}

/// `from/a.flac` with its target and some other files in `to`.
fn library(matches: Vec<TranscodeMatch>) -> Dirs {
	let d = dirs(matches);
	touch(&d.config.from.join("album/a.flac"));
	touch(&d.config.to.join("album/a.opus"));
	touch(&d.config.to.join("album/b.opus"));
	touch(&d.config.to.join("album/c.tmp"));
	touch(&d.config.to.join("album/cover.jpg"));
	touch(&d.config.to.join("album/d.mp3"));
	d
}

#[test]
fn converted_only_finds_files_audio_conv_creates() {
	let d = library(compile_matches(&[]).unwrap());

	assert_eq!(
		orphans(&d.config, DeleteMode::Converted),
		[PathBuf::from("album/b.opus"), PathBuf::from("album/c.tmp")]
	);
}

#[test]
fn all_finds_every_file_without_source() {
	let d = library(compile_matches(&[]).unwrap());

	assert_eq!(
		orphans(&d.config, DeleteMode::All),
		[
			PathBuf::from("album/b.opus"),
			PathBuf::from("album/c.tmp"),
			PathBuf::from("album/cover.jpg"),
			PathBuf::from("album/d.mp3"),
		]
	);
}

#[test]
fn converted_includes_paths_a_copy_match_would_create() {
	let d = library(compile(
		"[{extensions: [flac], to: {codec: opus}}, {extensions: [jpg], to: {codec: copy}}]",
	));

	assert_eq!(
		orphans(&d.config, DeleteMode::Converted),
		[
			PathBuf::from("album/b.opus"),
			PathBuf::from("album/c.tmp"),
			PathBuf::from("album/cover.jpg"),
		]
	);
}

#[test]
fn keeps_config_and_log_file() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.config.config_path = d.config.to.join("audio-conv.yaml");
	d.config.log_path = d.config.to.join("audio-conv.log");
	touch(&d.config.config_path);
	touch(&d.config.log_path);

	let orphans = find_orphans(&d.config, DeleteMode::All).unwrap();

	assert!(!orphans.contains(&d.config.config_path));
	assert!(!orphans.contains(&d.config.log_path));
}

#[test]
fn fails_without_matching_source_files() {
	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.config.from.join("cover.jpg"));
	touch(&d.config.to.join("a.opus"));

	assert!(find_orphans(&d.config, DeleteMode::All).is_err());
}

#[test]
fn fails_if_from_and_to_overlap() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.config.to = d.config.from.join("album");

	assert!(find_orphans(&d.config, DeleteMode::All).is_err());
}

#[test]
fn fails_if_from_is_not_fully_readable() {
	use ::std::os::unix::fs::PermissionsExt;

	let d = library(compile_matches(&[]).unwrap());
	let unreadable = d.config.from.join("unreadable");
	fs::create_dir(&unreadable).unwrap();
	fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

	let readable = fs::read_dir(&unreadable).is_ok();
	let res = find_orphans(&d.config, DeleteMode::All);
	fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o755)).unwrap();

	// root can read it anyway
	if !readable {
		assert!(res.is_err());
	}
}

fn delete(mode: DeleteMode, force: bool) -> Option<Delete> {
	Some(Delete { mode, force })
}

#[test]
fn run_without_force_only_logs() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.config.delete = delete(DeleteMode::Converted, false);
	let ctx = Context::new(d.config);

	let deleted = run(&ctx).unwrap().unwrap();

	assert_eq!((deleted.files, deleted.failed), (2, 0));
	assert!(ctx.config.to.join("album/b.opus").exists());
	let log = fs::read_to_string(ctx.log.path()).unwrap();
	assert!(log.contains("Would delete") && log.contains("b.opus") && log.contains("c.tmp"));
}

#[test]
fn run_with_force_deletes_and_logs() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.config.delete = delete(DeleteMode::Converted, true);
	let ctx = Context::new(d.config);

	let deleted = run(&ctx).unwrap().unwrap();

	assert_eq!((deleted.files, deleted.failed), (2, 0));
	assert!(!ctx.config.to.join("album/b.opus").exists());
	assert!(ctx.config.to.join("album/a.opus").exists());
	let log = fs::read_to_string(ctx.log.path()).unwrap();
	assert!(log.contains("Deleted") && log.contains("b.opus") && log.contains("c.tmp"));
}

#[test]
fn run_deletes_nothing_if_the_log_file_cant_be_opened() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.config.delete = delete(DeleteMode::Converted, true);
	d.config.log_path = d.config.from.join("missing/audio-conv.log");
	let ctx = Context::new(d.config);

	assert!(run(&ctx).is_err());
	assert!(ctx.config.to.join("album/b.opus").exists());
}

#[test]
fn run_without_delete_does_nothing() {
	let d = library(compile_matches(&[]).unwrap());
	let ctx = Context::new(d.config);

	assert!(run(&ctx).unwrap().is_none());
	assert!(!ctx.log.path().exists());
}

#[test]
fn remove_deletes_emptied_dirs_up_to_to() {
	let d = dirs(compile_matches(&[]).unwrap());
	let to = &d.config.to;
	touch(&to.join("artist/album/a.opus"));
	touch(&to.join("artist/b.opus"));

	remove(to, &to.join("artist/album/a.opus")).unwrap();

	assert!(!to.join("artist/album").exists());
	assert!(to.join("artist/b.opus").exists());

	remove(to, &to.join("artist/b.opus")).unwrap();

	assert!(!to.join("artist").exists());
	assert!(to.exists());
}
