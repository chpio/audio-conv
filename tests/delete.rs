mod common;

use ::audio_conv::{
	context::{Context, Delete, DeleteMode, TranscodeMatch, compile_matches},
	delete::{find_orphans, remove, run},
	log::Log,
	scan::walk,
};
use ::std::{fs, path::PathBuf};
use common::{Dirs, compile, dirs, touch};

fn orphans(ctx: &Context, mode: DeleteMode) -> Vec<PathBuf> {
	find_orphans(ctx, &walk(ctx).unwrap(), mode)
		.unwrap()
		.into_iter()
		.map(|path| path.strip_prefix(&ctx.to).unwrap().to_path_buf())
		.collect()
}

/// `from/a.flac` with its target and some other files in `to`.
fn library(matches: Vec<TranscodeMatch>) -> Dirs {
	let d = dirs(matches);
	touch(&d.ctx.from.join("album/a.flac"));
	touch(&d.ctx.to.join("album/a.opus"));
	touch(&d.ctx.to.join("album/b.opus"));
	touch(&d.ctx.to.join("album/c.tmp"));
	touch(&d.ctx.to.join("album/cover.jpg"));
	touch(&d.ctx.to.join("album/d.mp3"));
	d
}

#[test]
fn converted_only_finds_files_audio_conv_creates() {
	let d = library(compile_matches(&[]).unwrap());

	assert_eq!(
		orphans(&d.ctx, DeleteMode::Converted),
		[PathBuf::from("album/b.opus"), PathBuf::from("album/c.tmp")]
	);
}

#[test]
fn all_finds_every_file_without_source() {
	let d = library(compile_matches(&[]).unwrap());

	assert_eq!(
		orphans(&d.ctx, DeleteMode::All),
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
		orphans(&d.ctx, DeleteMode::Converted),
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
	d.ctx.config_path = d.ctx.to.join("audio-conv.yaml");
	d.ctx.log = Log::new(d.ctx.to.join("audio-conv.log"));
	touch(&d.ctx.config_path);
	touch(d.ctx.log.path());

	let orphans = find_orphans(&d.ctx, &walk(&d.ctx).unwrap(), DeleteMode::All).unwrap();

	assert!(!orphans.contains(&d.ctx.config_path));
	assert!(!orphans.contains(&d.ctx.log.path().to_path_buf()));
}

#[test]
fn fails_without_matching_source_files() {
	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.ctx.from.join("cover.jpg"));
	touch(&d.ctx.to.join("a.opus"));

	assert!(find_orphans(&d.ctx, &walk(&d.ctx).unwrap(), DeleteMode::All).is_err());
}

#[test]
fn fails_if_from_and_to_overlap() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.ctx.to = d.ctx.from.join("album");

	assert!(find_orphans(&d.ctx, &walk(&d.ctx).unwrap(), DeleteMode::All).is_err());
}

#[test]
fn fails_if_from_is_not_fully_readable() {
	use ::std::os::unix::fs::PermissionsExt;

	let d = library(compile_matches(&[]).unwrap());
	let unreadable = d.ctx.from.join("unreadable");
	fs::create_dir(&unreadable).unwrap();
	fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

	let readable = fs::read_dir(&unreadable).is_ok();
	let res = find_orphans(&d.ctx, &walk(&d.ctx).unwrap(), DeleteMode::All);
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
	d.ctx.delete = delete(DeleteMode::Converted, false);
	let ctx = d.ctx;

	let deleted = run(&ctx, &walk(&ctx).unwrap()).unwrap().unwrap();

	assert_eq!((deleted.files, deleted.failed), (2, 0));
	assert!(ctx.to.join("album/b.opus").exists());
	let log = fs::read_to_string(ctx.log.path()).unwrap();
	assert!(log.contains("Would delete") && log.contains("b.opus") && log.contains("c.tmp"));
}

#[test]
fn run_with_force_deletes_and_logs() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.ctx.delete = delete(DeleteMode::Converted, true);
	let ctx = d.ctx;

	let deleted = run(&ctx, &walk(&ctx).unwrap()).unwrap().unwrap();

	assert_eq!((deleted.files, deleted.failed), (2, 0));
	assert!(!ctx.to.join("album/b.opus").exists());
	assert!(ctx.to.join("album/a.opus").exists());
	let log = fs::read_to_string(ctx.log.path()).unwrap();
	assert!(log.contains("Deleted") && log.contains("b.opus") && log.contains("c.tmp"));
}

#[test]
fn run_deletes_nothing_if_the_log_file_cant_be_opened() {
	let mut d = library(compile_matches(&[]).unwrap());
	d.ctx.delete = delete(DeleteMode::Converted, true);
	d.ctx.log = Log::new(d.ctx.from.join("missing/audio-conv.log"));
	let ctx = d.ctx;

	assert!(run(&ctx, &walk(&ctx).unwrap()).is_err());
	assert!(ctx.to.join("album/b.opus").exists());
}

#[test]
fn run_without_delete_does_nothing() {
	let d = library(compile_matches(&[]).unwrap());
	let ctx = d.ctx;

	assert!(run(&ctx, &walk(&ctx).unwrap()).unwrap().is_none());
	assert!(!ctx.log.path().exists());
}

#[test]
fn remove_deletes_emptied_dirs_up_to_to() {
	let d = dirs(compile_matches(&[]).unwrap());
	let to = &d.ctx.to;
	touch(&to.join("artist/album/a.opus"));
	touch(&to.join("artist/b.opus"));

	remove(to, &to.join("artist/album/a.opus")).unwrap();

	assert!(!to.join("artist/album").exists());
	assert!(to.join("artist/b.opus").exists());

	remove(to, &to.join("artist/b.opus")).unwrap();

	assert!(!to.join("artist").exists());
	assert!(to.exists());
}
