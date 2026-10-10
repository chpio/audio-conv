mod common;

use ::audio_conv::{
	context::{Context, Transcode, compile_matches},
	scan::{ConversionArgs, find_transcode, jobs, walk},
};
use ::std::{
	fs,
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};
use common::{compile, dirs};

fn touch(path: &Path, mtime: SystemTime) {
	common::touch(path).set_modified(mtime).unwrap();
}

fn scan(ctx: &Context) -> Vec<ConversionArgs> {
	let mut args = jobs(ctx, &walk(ctx).unwrap()).unwrap();
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
	let from = &d.ctx.from;
	touch(&from.join("a.flac"), hours_ago(1));
	touch(&from.join("artist/album/b.WAV"), hours_ago(1));
	touch(&from.join("artist/album/cover.jpg"), hours_ago(1));
	fs::create_dir(from.join("empty.flac")).unwrap();

	let args = scan(&d.ctx);

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
	touch(&d.ctx.from.join("a.flac"), hours_ago(2));
	touch(&d.ctx.to.join("a.opus"), hours_ago(1));

	assert!(scan(&d.ctx).is_empty());
}

#[test]
fn includes_targets_older_than_source() {
	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.ctx.from.join("a.flac"), hours_ago(1));
	touch(&d.ctx.to.join("a.opus"), hours_ago(2));

	assert_eq!(rel_paths(&scan(&d.ctx)), [PathBuf::from("a.flac")]);
}

#[test]
fn target_extension_depends_on_codec() {
	let d = dirs(compile("[{extensions: [flac], to: {codec: mp3}}]"));
	touch(&d.ctx.from.join("a.flac"), hours_ago(2));
	// a target with the extension of a different codec doesn't count
	touch(&d.ctx.to.join("a.opus"), hours_ago(1));

	assert_eq!(rel_paths(&scan(&d.ctx)), [PathBuf::from("a.flac")]);

	touch(&d.ctx.to.join("a.mp3"), hours_ago(1));

	assert!(scan(&d.ctx).is_empty());
}

#[test]
fn copy_keeps_extension() {
	let d = dirs(compile("[{extensions: [jpg], to: {codec: copy}}]"));
	touch(&d.ctx.from.join("cover.jpg"), hours_ago(2));
	touch(&d.ctx.to.join("cover.jpg"), hours_ago(1));

	assert!(scan(&d.ctx).is_empty());
}

#[test]
fn copy_audio_uses_configured_extension() {
	let d = dirs(compile(
		"[{extensions: [mkv], to: {codec: copy_audio, extension: m4a}}]",
	));
	touch(&d.ctx.from.join("video.mkv"), hours_ago(2));

	assert_eq!(rel_paths(&scan(&d.ctx)), [PathBuf::from("video.mkv")]);

	touch(&d.ctx.to.join("video.m4a"), hours_ago(1));

	assert!(scan(&d.ctx).is_empty());
}

#[test]
fn walk_includes_up_to_date_files() {
	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.ctx.from.join("a.flac"), hours_ago(2));
	touch(&d.ctx.to.join("a.opus"), hours_ago(1));

	let scan = walk(&d.ctx).unwrap();

	assert_eq!(rel_paths(&scan.files), [PathBuf::from("a.flac")]);
	assert!(jobs(&d.ctx, &scan).unwrap().is_empty());
}

#[test]
fn walk_logs_and_skips_unreadable_dirs() {
	use ::std::os::unix::fs::PermissionsExt;

	let d = dirs(compile_matches(&[]).unwrap());
	touch(&d.ctx.from.join("a.flac"), hours_ago(1));
	let unreadable = d.ctx.from.join("unreadable");
	touch(&unreadable.join("b.flac"), hours_ago(1));
	fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

	let readable = fs::read_dir(&unreadable).is_ok();
	let scan = walk(&d.ctx);
	fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o755)).unwrap();

	// root can read it anyway
	if !readable {
		let scan = scan.unwrap();
		assert_eq!(scan.errors, 1);
		assert_eq!(rel_paths(&scan.files), [PathBuf::from("a.flac")]);
		let log = fs::read_to_string(d.ctx.log.path()).unwrap();
		assert!(log.contains("unreadable"));
	}
}
