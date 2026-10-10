use ::anyhow::{Context, Result};
use ::audio_conv::{config, scan, tag, transcode, ui};
use ::futures::prelude::*;
use ::std::{fmt::Write as FmtWrite, path::Path};
use ::tokio::{fs, io::AsyncWriteExt, task};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
	task::LocalSet::new()
		.run_until(async move {
			let (ui_queue, ui_fut) = ui::init();

			let main_handle = async move {
				task::spawn_local(main_loop(ui_queue))
					.await
					.context("Main task failed")??;
				Result::<_>::Ok(())
			};

			let ui_handle = async move {
				task::spawn_local(ui_fut)
					.await
					.context("Ui task failed")?
					.context("Ui failed")?;
				Result::<_>::Ok(())
			};

			future::try_join(main_handle, ui_handle).await?;
			Ok(())
		})
		.await
}

async fn main_loop(ui_queue: ui::MsgQueue) -> Result<()> {
	let (config, conv_args) = task::spawn_blocking(|| -> Result<_> {
		gstreamer::init()?;
		gstreamer::tags::register::<tag::MbArtistId>();
		gstreamer::tags::register::<tag::MbAlbumArtistId>();

		let config = config::config().context("Could not get the config")?;

		let conv_args = scan::get_conversion_args(&config)
			.collect::<Result<Vec<_>>>()
			.context("Failed loading dir structure")?;

		Ok((config, conv_args))
	})
	.await
	.context("Init task failed")??;

	let log_path = Path::new(".")
		.canonicalize()
		.context("Unable to canonicalize path to log file")?
		.join("audio-conv.log");

	ui_queue.push(ui::Msg::Init {
		task_len: conv_args.len(),
		log_path: log_path.clone(),
	});

	let concurrent_jobs = match config.jobs {
		Some(jobs) => jobs,
		None => std::thread::available_parallelism()
			.context("Could not get the number of logical cores, use \"jobs\" to set it")?
			.get(),
	};

	stream::iter(conv_args.into_iter().enumerate())
		.map(Ok)
		.try_for_each_concurrent(concurrent_jobs, |(i, args)| {
			let config = &config;
			let ui_queue = &ui_queue;
			let log_path = &log_path;

			async move {
				ui_queue.push(ui::Msg::TaskStart {
					id: i,
					args: args.clone(),
				});

				match transcode::transcode(config, &args, i, ui_queue).await {
					Ok(()) => ui_queue.push(ui::Msg::TaskEnd { id: i }),
					Err(err) => {
						let err = err.context(format!(
							"Transcoding failed for {}",
							args.rel_from_path.display()
						));

						let mut log_file = match fs::OpenOptions::new()
							.create(true)
							.append(true)
							.open(log_path)
							.await
						{
							Ok(log_file) => log_file,
							Err(fs_err) => {
								let err = err.context(fs_err).context("Unable to open log file");
								return Err(err);
							}
						};

						let mut err_str = String::new();
						if let Err(write_err) = writeln!(&mut err_str, "{:?}", err) {
							let err = err.context(format!(
								"Unable to format transcoding error for logging (write error: {})",
								write_err
							));
							return Err(err);
						}

						log_file
							.write_all(err_str.as_ref())
							.await
							.map_err(|fs_err| {
								err.context(format!(
									"Unable to write transcoding error to log file (fs error: {})",
									fs_err
								))
							})?;

						ui_queue.push(ui::Msg::TaskError { id: i });
					}
				}

				Result::<_>::Ok(())
			}
		})
		.await?;

	ui_queue.push(ui::Msg::Exit);

	Ok(())
}
