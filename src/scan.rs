use crate::config::{Config, Transcode};
use ::anyhow::{Context, Error, Result};
use ::std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ConversionArgs {
	pub rel_from_path: PathBuf,
	pub transcode: Transcode,
}

pub fn get_conversion_args(config: &Config) -> impl Iterator<Item = Result<ConversionArgs>> + '_ {
	walkdir::WalkDir::new(&config.from)
		.into_iter()
		.filter_map(|e| e.ok())
		.filter(|e| e.file_type().is_file())
		.map(move |e| -> Result<Option<ConversionArgs>> {
			let from_bytes = e.path().as_os_str().as_encoded_bytes();

			let transcode = config
				.matches
				.iter()
				.filter(|m| m.regexes.iter().any(|regex| regex.is_match(from_bytes)))
				.map(|m| m.to.clone())
				.next();
			let transcode = if let Some(transcode) = transcode {
				transcode
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

			let mut to = config.to.join(rel_path);
			to.set_extension(transcode.extension());

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

			if is_newer {
				Ok(Some(ConversionArgs {
					rel_from_path: rel_path.to_path_buf(),
					transcode,
				}))
			} else {
				Ok(None)
			}
		})
		.filter_map(|e| e.transpose())
}
