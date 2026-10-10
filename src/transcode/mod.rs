mod error;
mod gstreamer;

pub use gstreamer::dest_elements;

use crate::{
	config::{Config, Transcode},
	scan::ConversionArgs,
	ui,
};
use ::anyhow::{Context, Result};
use ::std::path::Path;
use ::tokio::fs;

pub async fn transcode(
	config: &Config,
	args: &ConversionArgs,
	task_id: usize,
	queue: &ui::MsgQueue,
) -> Result<()> {
	let from_path = config.from.join(&args.rel_from_path);
	let to_path = args.to_path(&config.to);

	fs::create_dir_all(
		to_path
			.parent()
			.with_context(|| format!("Could not get parent dir for {}", to_path.display()))?,
	)
	.await?;

	// encode into a tmp file first, then rename to actuall file name, that way we're writing
	// "whole" files to the intended file path, ignoring partial files in the mtime check
	let to_path_tmp = to_path.with_extension("tmp");

	rm_file_on_err(&to_path_tmp, async {
		match args.transcode {
			Transcode::Copy => {
				fs::copy(&from_path, &to_path_tmp).await.with_context(|| {
					format!(
						"Could not copy file from {} to {}",
						from_path.display(),
						to_path_tmp.display()
					)
				})?;
			}
			_ => {
				gstreamer::transcode_gstreamer(
					&from_path,
					&to_path_tmp,
					args.transcode.clone(),
					task_id,
					queue,
				)
				.await?
			}
		}

		fs::rename(&to_path_tmp, &to_path).await.with_context(|| {
			format!(
				"Could not rename temporary file {} to {}",
				to_path_tmp.display(),
				to_path.display()
			)
		})
	})
	.await
}

async fn rm_file_on_err<F, T>(path: &Path, f: F) -> Result<T>
where
	F: Future<Output = Result<T>>,
{
	match f.await {
		Err(err) => match fs::remove_file(path).await {
			Ok(()) => Err(err),
			Err(fs_err) if fs_err.kind() == std::io::ErrorKind::NotFound => Err(err),
			Err(fs_err) => {
				let err = err
					.context(fs_err)
					.context(format!("Removing file {} failed", path.display()));
				Err(err)
			}
		},
		res @ Ok(..) => res,
	}
}
