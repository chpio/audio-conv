use crate::context::{Context, Transcode, TranscodeMatch};
use ::anyhow::{Context as _, Error, Result};
use ::std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ConversionArgs {
	pub rel_from_path: PathBuf,
	pub transcode: Transcode,
}

impl ConversionArgs {
	pub fn to_path(&self, to_dir: &Path) -> PathBuf {
		let mut to_path = to_dir.join(&self.rel_from_path);
		if let Some(extension) = self.transcode.extension() {
			to_path.set_extension(extension);
		}
		to_path
	}
}

/// Result of walking `from` once, shared by the conversion (`jobs`) and `--delete`.
#[derive(Debug)]
pub struct Scan {
	/// Every matching file, whether it needs a conversion or not.
	pub files: Vec<ConversionArgs>,
	/// Entries in `from` that couldn't be read, they are logged and skipped.
	pub errors: usize,
}

/// Finds the matching files in `from`, doesn't touch `to`.
pub fn walk(ctx: &Context) -> Result<Scan> {
	let mut scan = Scan {
		files: Vec::new(),
		errors: 0,
	};

	for entry in walkdir::WalkDir::new(&ctx.from) {
		let entry = match entry {
			Ok(entry) => entry,
			Err(err) => {
				scan.errors += 1;
				let err = Error::new(err).context("Skipped an unreadable entry in \"from\"");
				ctx.log.write(format_args!("{:?}", err))?;
				continue;
			}
		};

		if !entry.file_type().is_file() {
			continue;
		}
		let Some(transcode) = find_transcode(&ctx.matches, entry.path()) else {
			continue;
		};

		let rel_path = entry.path().strip_prefix(&ctx.from).with_context(|| {
			format!(
				"Unable to get relative path for {} from {}",
				entry.path().display(),
				ctx.from.display()
			)
		})?;

		scan.files.push(ConversionArgs {
			rel_from_path: rel_path.to_path_buf(),
			transcode: transcode.clone(),
		});
	}

	Ok(scan)
}

/// The scanned files whose target is missing or older than the source.
pub fn jobs(ctx: &Context, scan: &Scan) -> Result<Vec<ConversionArgs>> {
	scan.files
		.iter()
		.filter_map(|args| {
			needs_conversion(ctx, args)
				.map(|needed| needed.then(|| args.clone()))
				.transpose()
		})
		.collect()
}

fn needs_conversion(ctx: &Context, args: &ConversionArgs) -> Result<bool> {
	let from = ctx.from.join(&args.rel_from_path);
	let from_mtime = from
		.metadata()
		.and_then(|md| md.modified())
		.with_context(|| format!("Unable to get mtime for \"from\" file {}", from.display()))?;

	let to = args.to_path(&ctx.to);
	match to.metadata().and_then(|md| md.modified()) {
		Ok(to_mtime) => Ok(to_mtime < from_mtime),
		Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
		Err(err) => Err(err)
			.with_context(|| format!("Unable to get mtime for \"to\" file {}", to.display())),
	}
}

/// Returns the transcode of the first match whose regexes match `path`.
pub fn find_transcode<'a>(matches: &'a [TranscodeMatch], path: &Path) -> Option<&'a Transcode> {
	let path_bytes = path.as_os_str().as_encoded_bytes();
	matches
		.iter()
		.find(|m| m.regexes.iter().any(|regex| regex.is_match(path_bytes)))
		.map(|m| &m.to)
}
