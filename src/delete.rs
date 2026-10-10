use crate::{
	config::{Config, DeleteMode, Transcode},
	scan::{ConversionArgs, find_transcode},
};
use ::anyhow::{Context, Error, Result};
use ::std::{
	collections::HashSet,
	ffi::OsStr,
	fs,
	io::{ErrorKind, Write as _},
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
pub fn run(config: &Config) -> Result<Option<Deleted>> {
	let Some(delete) = config.delete else {
		return Ok(None);
	};

	let orphans = find_orphans(config, delete.mode)?;

	let mut failed = 0;

	if !orphans.is_empty() {
		// opened before deleting anything and written line by line, so every deleted file is
		// logged, even if the run is interrupted
		let mut log_file = fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(&config.log_path)
			.with_context(|| format!("Unable to open log file {}", config.log_path.display()))?;

		for path in &orphans {
			let written = if !delete.force {
				writeln!(log_file, "Would delete {}", path.display())
			} else {
				match remove(&config.to, path) {
					Ok(()) => writeln!(log_file, "Deleted {}", path.display()),
					Err(err) => {
						failed += 1;
						writeln!(log_file, "{:?}", err)
					}
				}
			};
			// stop deleting files that couldn't be logged
			written.with_context(|| {
				format!("Unable to write to log file {}", config.log_path.display())
			})?;
		}
	}

	Ok(Some(Deleted {
		files: orphans.len() - failed,
		failed,
		force: delete.force,
	}))
}

/// Files in `to` that don't belong to a matching file in `from`, sorted. The config and log file
/// are never returned.
///
/// Fails instead of returning a possibly too long list: if reading `from` or `to` fails, if no
/// file in `from` matches (e.g. an unmounted drive) and if `from` and `to` overlap.
pub fn find_orphans(config: &Config, mode: DeleteMode) -> Result<Vec<PathBuf>> {
	if config.to.starts_with(&config.from) || config.from.starts_with(&config.to) {
		return Err(Error::msg(format!(
			"\"from\" ({}) and \"to\" ({}) overlap",
			config.from.display(),
			config.to.display()
		)));
	}

	let mut targets = HashSet::new();
	for entry in walkdir::WalkDir::new(&config.from) {
		let entry = entry.context("Failed reading the \"from\" directory")?;
		if !entry.file_type().is_file() {
			continue;
		}
		let Some(transcode) = find_transcode(&config.matches, entry.path()) else {
			continue;
		};
		let args = ConversionArgs {
			rel_from_path: entry.path().strip_prefix(&config.from)?.to_path_buf(),
			transcode: transcode.clone(),
		};
		targets.insert(args.to_path(&config.to));
	}

	if targets.is_empty() {
		return Err(Error::msg(format!(
			"No matching files found in {}, is it mounted?",
			config.from.display()
		)));
	}

	// compared by their parent dir, as the files in `to` are found below the canonical `to`
	let keep: HashSet<PathBuf> = [&config.config_path, &config.log_path]
		.into_iter()
		.filter_map(|path| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
		.collect();

	let extensions: HashSet<&OsStr> = config
		.matches
		.iter()
		.filter_map(|m| m.to.extension())
		.map(OsStr::new)
		// temporary files written while converting
		.chain([OsStr::new("tmp")])
		.collect();

	let mut orphans = Vec::new();
	for entry in walkdir::WalkDir::new(&config.to) {
		let entry = entry.context("Failed reading the \"to\" directory")?;
		let path = entry.path();
		if entry.file_type().is_dir() || targets.contains(path) || keep.contains(path) {
			continue;
		}

		let is_orphan = match mode {
			DeleteMode::All => true,
			DeleteMode::Converted => {
				path.extension()
					.is_some_and(|extension| extensions.contains(extension))
					|| {
						// `copy` keeps the path, so a matching path in `from` would be copied here
						let rel_path = path.strip_prefix(&config.to)?;
						matches!(
							find_transcode(&config.matches, &config.from.join(rel_path)),
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
	Ok(orphans)
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
