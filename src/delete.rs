use crate::{
	context::{Context, DeleteMode, Transcode},
	scan::{Scan, find_transcode},
};
use ::anyhow::{Context as _, Error, Result};
use ::std::{
	collections::HashSet,
	ffi::OsStr,
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

/// Result of `--delete`.
#[derive(Debug)]
pub struct Deleted {
	/// Deleted files, or with `force: false` the ones that would be deleted.
	pub files: usize,
	/// Files that couldn't be deleted.
	pub failed: usize,
	pub force: bool,
}

/// Logs or deletes (with `--force`) the files in `to` that don't belong to a file in `from`,
/// `None` without `--delete`.
pub fn run(ctx: &Context, scan: &Scan) -> Result<Option<Deleted>> {
	let Some(delete) = ctx.delete else {
		return Ok(None);
	};

	let Orphans {
		paths: orphans,
		total,
	} = find_orphans(ctx, scan, delete.mode)?;

	// guards against a part of `from` that is missing without a read error, e.g. an empty mount
	// point, its files in `to` would look like orphans
	if delete.force && orphans.len() * 100 > total * usize::from(delete.max_percent) {
		return Err(Error::msg(format!(
			"Would delete {} of the {} files in {}, more than --max-delete={}%. Check them by running \
			 --delete without --force and raise --max-delete if that's intended",
			orphans.len(),
			total,
			ctx.to.display(),
			delete.max_percent
		)));
	}

	let mut failed = 0;

	if !orphans.is_empty() {
		// written before deleting anything and line by line, so every deleted file is logged,
		// even if the run is interrupted
		ctx.log.write(match delete.force {
			true => "Deleting files without a source file:",
			false => "Files without a source file, use --force to delete them:",
		})?;

		for path in &orphans {
			if !delete.force {
				ctx.log
					.write(format_args!("Would delete {}", path.display()))?;
				continue;
			}
			// stops deleting files that couldn't be logged
			match remove(&ctx.to, path) {
				Ok(()) => ctx.log.write(format_args!("Deleted {}", path.display()))?,
				Err(err) => {
					failed += 1;
					ctx.log.write(format_args!("{:?}", err))?;
				}
			}
		}
	}

	Ok(Some(Deleted {
		files: orphans.len() - failed,
		failed,
		force: delete.force,
	}))
}

#[derive(Debug)]
pub struct Orphans {
	/// Files in `to` that don't belong to a matching file in `from`, sorted. The config and log
	/// file are never included.
	pub paths: Vec<PathBuf>,
	/// All files in `to`.
	pub total: usize,
}

/// Fails instead of returning a possibly too long list: if `scan` couldn't read all of `from`,
/// if reading `to` fails, if no file in `from` matches (e.g. an unmounted drive) and if `from`
/// and `to` overlap.
pub fn find_orphans(ctx: &Context, scan: &Scan, mode: DeleteMode) -> Result<Orphans> {
	if ctx.to.starts_with(&ctx.from) || ctx.from.starts_with(&ctx.to) {
		return Err(Error::msg(format!(
			"\"from\" ({}) and \"to\" ({}) overlap",
			ctx.from.display(),
			ctx.to.display()
		)));
	}

	if scan.errors > 0 {
		return Err(Error::msg(format!(
			"Parts of {} couldn't be read ({} errors), see the log file",
			ctx.from.display(),
			scan.errors
		)));
	}

	if scan.files.is_empty() {
		return Err(Error::msg(format!(
			"No matching files found in {}, is it mounted?",
			ctx.from.display()
		)));
	}

	let targets: HashSet<PathBuf> = scan
		.files
		.iter()
		.map(|args| args.to_path(&ctx.to))
		.collect();

	// compared by their parent dir, as the files in `to` are found below the canonical `to`
	let keep: HashSet<PathBuf> = [ctx.config_path.as_path(), ctx.log.path()]
		.into_iter()
		.filter_map(|path| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
		.collect();

	let extensions: HashSet<&OsStr> = ctx
		.matches
		.iter()
		.filter_map(|m| m.to.extension())
		.map(OsStr::new)
		// temporary files written while converting
		.chain([OsStr::new("tmp")])
		.collect();

	let mut orphans = Vec::new();
	let mut total = 0;
	for entry in walkdir::WalkDir::new(&ctx.to) {
		let entry = entry.context("Failed reading the \"to\" directory")?;
		let path = entry.path();
		if entry.file_type().is_dir() {
			continue;
		}
		total += 1;
		if targets.contains(path) || keep.contains(path) {
			continue;
		}

		let is_orphan = match mode {
			DeleteMode::All => true,
			DeleteMode::Converted => {
				path.extension()
					.is_some_and(|extension| extensions.contains(extension))
					|| {
						// `copy` keeps the path, so a matching path in `from` would be copied here
						let rel_path = path.strip_prefix(&ctx.to)?;
						matches!(
							find_transcode(&ctx.matches, &ctx.from.join(rel_path)),
							Some(Transcode::Copy)
						)
					}
			}
		};

		if is_orphan {
			orphans.push(path.to_path_buf());
		}
	}

	orphans.sort();
	Ok(Orphans {
		paths: orphans,
		total,
	})
}

/// Deletes `path` and the directories it leaves empty, up to `to`.
pub fn remove(to: &Path, path: &Path) -> Result<()> {
	fs::remove_file(path).with_context(|| format!("Failed deleting {}", path.display()))?;

	for dir in path.ancestors().skip(1) {
		if dir == to || !dir.starts_with(to) {
			break;
		}
		match fs::remove_dir(dir) {
			Ok(()) => {}
			Err(err) if err.kind() == ErrorKind::DirectoryNotEmpty => break,
			Err(err) => {
				return Err(err)
					.with_context(|| format!("Failed deleting empty directory {}", dir.display()));
			}
		}
	}

	Ok(())
}
