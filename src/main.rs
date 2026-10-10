use ::anyhow::{Context as _, Result};
use ::audio_conv::{
	context::{self, Context},
	delete, scan, tag, transcode, ui,
};
use ::futures::prelude::*;
use ::std::sync::Arc;
use ::tokio::task;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
	let (ctx, conv_args) = task::spawn_blocking(init)
		.await
		.context("Init task failed")??;
	let ctx = Arc::new(ctx);

	let deleted = task::spawn_blocking({
		let ctx = ctx.clone();
		move || delete::run(&ctx)
	})
	.await
	.context("Delete task failed")?
	.context("Not deleting any files")?;

	task::LocalSet::new()
		.run_until(async move {
			let (ui_queue, ui_fut) = ui::init(ctx.clone(), conv_args.len(), deleted);

			let main_handle = async move {
				task::spawn_local(main_loop(ctx, conv_args, ui_queue))
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

/// Loads the config, initializes GStreamer and scans `from`.
fn init() -> Result<(Context, Vec<scan::ConversionArgs>)> {
	let ctx = context::load().context("Could not get the config")?;

	gstreamer::init()?;
	gstreamer::tags::register::<tag::MbArtistId>();
	gstreamer::tags::register::<tag::MbAlbumArtistId>();

	let conv_args = scan::get_conversion_args(&ctx)
		.collect::<Result<Vec<_>>>()
		.context("Failed loading dir structure")?;

	Ok((ctx, conv_args))
}

async fn main_loop(
	ctx: Arc<Context>,
	conv_args: Vec<scan::ConversionArgs>,
	ui_queue: ui::MsgQueue,
) -> Result<()> {
	let concurrent_jobs = match ctx.jobs {
		Some(jobs) => jobs,
		None => std::thread::available_parallelism()
			.context("Could not get the number of logical cores, use \"jobs\" to set it")?
			.get(),
	};

	stream::iter(conv_args.into_iter().enumerate())
		.map(Ok)
		.try_for_each_concurrent(concurrent_jobs, |(i, args)| {
			let ctx = &ctx;
			let ui_queue = &ui_queue;

			async move {
				ui_queue.push(ui::Msg::TaskStart {
					id: i,
					args: args.clone(),
				});

				match transcode::transcode(ctx, &args, i, ui_queue).await {
					Ok(()) => ui_queue.push(ui::Msg::TaskEnd { id: i }),
					Err(err) => {
						let err = err.context(format!(
							"Transcoding failed for {}",
							args.rel_from_path.display()
						));

						// a single short write, fine to block on
						if let Err(log_err) = ctx.log.write(format_args!("{:?}", err)) {
							return Err(err.context(format!("{:#}", log_err)));
						}

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
