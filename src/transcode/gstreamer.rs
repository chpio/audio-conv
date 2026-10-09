use super::error::{GBoxErrorWrapper, GErrorMessage};
use crate::{
	config::{self, Transcode},
	ui,
};
use ::anyhow::{Context, Error, Result};
use ::futures::{pin_mut, prelude::*};
use ::gstreamer::{Element, element_error, prelude::*};
use ::gstreamer_base::prelude::*;
use ::std::{path::Path, time::Duration};
use ::tokio::time::interval;

fn gmake<T: IsA<Element>>(factory_name: &str, properties: &[(&str, &dyn ToValue)]) -> Result<T> {
	let builder = gstreamer::ElementFactory::make(factory_name);
	let builder = properties.iter().fold(builder, |builder, (name, value)| {
		builder.property(name, value.to_value())
	});
	let res = builder
		.build()
		.with_context(|| format!("Could not make gstreamer Element \"{}\"", factory_name))?
		.downcast()
		.ok()
		.with_context(|| {
			format!(
				"Could not cast gstreamer Element \"{}\" into `{}`",
				factory_name,
				std::any::type_name::<T>()
			)
		})?;
	Ok(res)
}

pub async fn transcode_gstreamer(
	from_path: &Path,
	to_path: &Path,
	transcode: Transcode,
	task_id: usize,
	queue: &ui::MsgQueue,
) -> Result<()> {
	let file_src: Element = gmake("filesrc", &[("location", &from_path)])?;

	let decodebin: Element = gmake("decodebin", &[])?;

	let src_elems: &[&Element] = &[&file_src, &decodebin];

	let pipeline = gstreamer::Pipeline::new();

	pipeline.add_many(src_elems)?;
	Element::link_many(src_elems)?;

	// downgrade pipeline RC to a weak RC to break the reference cycle
	let pipeline_weak = pipeline.downgrade();

	let to_path_clone = to_path.to_owned();
	decodebin.connect_pad_added(move |decodebin, src_pad| {
		let insert_sink = || -> Result<()> {
			let pipeline = match pipeline_weak.upgrade() {
				Some(pipeline) => pipeline,
				None => {
					// pipeline already destroyed... ignoring
					return Ok(());
				}
			};

			let is_audio = src_pad.current_caps().and_then(|caps| {
				caps.structure(0).map(|s| {
					let name = s.name();
					name.starts_with("audio/")
				})
			});
			match is_audio {
				None => {
					return Err(Error::msg(format!(
						"Failed to get media type from pad {}",
						src_pad.name()
					)));
				}
				Some(false) => {
					// not audio pad... ignoring
					return Ok(());
				}
				Some(true) => {}
			}

			let resample: Element = gmake(
				"audioresample",
				&[
					// quality from 0 to 10
					("quality", &10i32),
				],
			)?;

			let mut dest_elems = vec![
				resample,
				// `audioconvert` converts audio format, bitdepth, ...
				gmake("audioconvert", &[])?,
			];

			match &transcode {
				Transcode::Opus {
					bitrate,
					bitrate_type,
				} => {
					let encoder: Element = gmake(
						"opusenc",
						&[(
							"bitrate",
							&i32::from(*bitrate)
								.checked_mul(1_000)
								.context("Bitrate overflowed")?,
						)],
					)?;
					// enum properties can't be set from a plain string value, they need to be
					// parsed from their nick
					encoder.set_property_from_str(
						"bitrate-type",
						match bitrate_type {
							config::BitrateType::Vbr => "vbr",
							config::BitrateType::Cbr => "cbr",
						},
					);

					dest_elems.push(encoder);
					dest_elems.push(gmake("oggmux", &[])?);
				}

				Transcode::Flac { compression } => {
					if *compression > 9 {
						return Err(Error::msg(format!(
							"Invalid flac compression {}, must be between 0 and 9",
							compression
						)));
					}

					let encoder: Element = gmake("flacenc", &[])?;
					encoder.set_property_from_str("quality", &compression.to_string());
					dest_elems.push(encoder);
				}

				Transcode::Mp3 {
					bitrate,
					bitrate_type,
				} => {
					let encoder: Element = gmake(
						"lamemp3enc",
						&[
							("bitrate", &i32::from(*bitrate)),
							(
								"cbr",
								match bitrate_type {
									config::BitrateType::Vbr => &false,
									config::BitrateType::Cbr => &true,
								},
							),
						],
					)?;
					encoder.set_property_from_str("target", "bitrate");

					dest_elems.push(encoder);
					dest_elems.push(gmake("id3v2mux", &[])?);
				}

				Transcode::Copy => {
					// already handled outside this fn
					unreachable!();
				}
			};

			let file_dest: gstreamer_base::BaseSink =
				gmake("filesink", &[("location", &to_path_clone)])?;
			file_dest.set_sync(false);
			dest_elems.push(file_dest.upcast());

			let dest_elem_refs: Vec<_> = dest_elems.iter().collect();
			pipeline.add_many(&dest_elem_refs)?;
			Element::link_many(&dest_elem_refs)?;

			for e in &dest_elems {
				e.sync_state_with_parent()?;
			}

			let sink_pad = dest_elems
				.first()
				.unwrap()
				.static_pad("sink")
				.expect("1. dest element has no sinkpad");
			src_pad.link(&sink_pad)?;

			Ok(())
		};

		if let Err(err) = insert_sink() {
			let details = gstreamer::Structure::builder("error-details")
				.field("error", GBoxErrorWrapper::new(err))
				.build();

			element_error!(
				decodebin,
				gstreamer::LibraryError::Failed,
				("Failed to insert sink"),
				details: details
			);
		}
	});

	let bus = pipeline.bus().context("Could not get bus for pipeline")?;

	pipeline
		.set_state(gstreamer::State::Playing)
		.context("Unable to set the pipeline to the `Playing` state")?;

	let stream_processor = async {
		bus.stream()
			.map::<Result<bool>, _>(|msg| {
				use ::gstreamer::MessageView;

				match msg.view() {
					MessageView::Eos(..) => {
						// we need to actively stop pulling the stream, that's because stream will
						// never end despite yielding an `Eos` message
						Ok(false)
					}
					MessageView::Error(err) => {
						let pipe_stop_res = pipeline.set_state(gstreamer::State::Null);

						let err: Error = err
							.details()
							.and_then(|details| {
								if details.name() != "error-details" {
									return None;
								}

								let err = details
									.get::<&GBoxErrorWrapper>("error")
									.unwrap()
									.clone()
									.into();
								Some(err)
							})
							.unwrap_or_else(|| {
								GErrorMessage {
									src: msg
										.src()
										.map(|s| String::from(s.path_string()))
										.unwrap_or_else(|| String::from("None")),
									error: err.error().to_string(),
									debug: err.debug().map(|gstring| gstring.into()),
									source: err.error(),
								}
								.into()
							});

						if let Err(pipe_err) = pipe_stop_res {
							let err = err.context(pipe_err).context(
								"Unable to set the pipeline to the `Null` state, after error",
							);
							Err(err)
						} else {
							Err(err)
						}
					}
					_ => Ok(true),
				}
			})
			.take_while(|e| {
				if let Ok(false) = e {
					futures::future::ready(false)
				} else {
					futures::future::ready(true)
				}
			})
			.try_for_each(|_| futures::future::ready(Ok(())))
			.await?;

		Result::<_>::Ok(())
	};
	pin_mut!(stream_processor);

	let mut progress_interval = interval(Duration::from_millis(ui::UPDATE_INTERVAL_MILLIS / 2));
	let progress_processor = async {
		use ::gstreamer::ClockTime;

		loop {
			progress_interval.tick().await;

			let dur = decodebin
				.query_duration::<ClockTime>()
				.map(|time| time.nseconds());

			let ratio = dur.and_then(|dur| {
				if dur == 0 {
					return None;
				}

				let pos = decodebin
					.query_position::<ClockTime>()
					.map(|time| time.nseconds());

				pos.map(|pos| {
					let ratio = pos as f64 / dur as f64;
					ratio.clamp(0.0, 1.0)
				})
			});

			if let Some(ratio) = ratio {
				queue.push(ui::Msg::TaskProgress { id: task_id, ratio });
			}
		}

		#[allow(unreachable_code)]
		Result::<_>::Ok(())
	};
	pin_mut!(progress_processor);

	future::try_select(stream_processor, progress_processor)
		.await
		.map_err(|err| err.factor_first().0)?;

	pipeline
		.set_state(gstreamer::State::Null)
		.context("Unable to set the pipeline to the `Null` state")?;

	Ok(())
}
