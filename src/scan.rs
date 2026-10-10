use crate::config::{Config, Transcode, TranscodeMatch};
use ::anyhow::{Context, Error, Result};
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

pub fn get_conversion_args(config: &Config) -> impl Iterator<Item = Result<ConversionArgs>> + '_ {
	walkdir::WalkDir::new(&config.from)
		.into_iter()
		.filter_map(|e| e.ok())
		.filter(|e| e.file_type().is_file())
		.map(move |e| -> Result<Option<ConversionArgs>> {
			let transcode = if let Some(transcode) = find_transcode(&config.matches, e.path()) {
				transcode.clone()
			} else {
				return Ok(None);
			};

			let rel_path = e.path().strip_prefix(&config.from).with_context(|| {
				format!(
					"Unable to get relative path for {} from {}",
					e.path().display(),
					config.from.display()
				)
			})?;

			let args = ConversionArgs {
				rel_from_path: rel_path.to_path_buf(),
				transcode,
			};
			let to = args.to_path(&config.to);

			let is_newer = {
				let from_mtime = e
					.metadata()
					.map_err(Error::new)
					.and_then(|md| md.modified().map_err(Error::new))
					.with_context(|| {
						format!(
							"Unable to get mtime for \"from\" file {}",
							e.path().display()
						)
					})?;
				let to_mtime = to.metadata().and_then(|md| md.modified());
				match to_mtime {
					Ok(to_mtime) => to_mtime < from_mtime,
					Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
					Err(err) => {
						return Err(err).with_context(|| {
							format!("Unable to get mtime for \"to\" file {}", to.display())
						});
					}
				}
			};

			Ok(is_newer.then_some(args))
		})
		.filter_map(|e| e.transpose())
}

/// Returns the transcode of the first match whose regexes match `path`.
pub fn find_transcode<'a>(matches: &'a [TranscodeMatch], path: &Path) -> Option<&'a Transcode> {
	let path_bytes = path.as_os_str().as_encoded_bytes();
	matches
		.iter()
		.find(|m| m.regexes.iter().any(|regex| regex.is_match(path_bytes)))
		.map(|m| &m.to)
}
