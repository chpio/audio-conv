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
	let (ctx, scan) = task::spawn_blocking(|| -> Result<_> {
		let ctx = context::load().context("Could not get the config")?;

		gstreamer::init()?;
		gstreamer::tags::register::<tag::MbArtistId>();
		gstreamer::tags::register::<tag::MbAlbumArtistId>();

		let scan = scan::walk(&ctx).context("Failed loading dir structure")?;

		Ok((ctx, scan))
	})
	.await
	.context("Init task failed")??;
	let ctx = Arc::new(ctx);
	let scan = Arc::new(scan);

	// independent: the jobs only check the targets of the scanned files, `--delete` only touches
	// the other files in `to`, and converting only starts after both
	let jobs = task::spawn_blocking({
		let (ctx, scan) = (ctx.clone(), scan.clone());
		move || scan::jobs(&ctx, &scan)
	})
	.map(|res| {
		res.context("Jobs task failed")?
			.context("Failed loading dir structure")
	});
	let deleted = task::spawn_blocking({
		let (ctx, scan) = (ctx.clone(), scan.clone());
		move || delete::run(&ctx, &scan)
	})
	.map(|res| {
		res.context("Delete task failed")?
			.context("Not deleting any files")
	});
	let (conv_args, deleted) = future::try_join(jobs, deleted).await?;

	task::LocalSet::new()
		.run_until(async move {
			let (ui_queue, ui_fut) =
				ui::init(ctx.clone(), conv_args.len(), deleted, scan.errors > 0);

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
