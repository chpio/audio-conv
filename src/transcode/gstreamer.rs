use super::error::{GBoxErrorWrapper, GErrorMessage};
use crate::{
	context::{self, AudioExtension, Transcode},
	ui,
};
use ::anyhow::{Context, Error, Result};
use ::futures::{pin_mut, prelude::*};
use ::gstreamer::{Element, element_error, glib, prelude::*};
use ::gstreamer_base::prelude::*;
use ::std::{
	path::Path,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};
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

/// Posts `err` as an error message on the bus, the bus stream recovers the original `err`.
fn post_error(element: &Element, err: Error, msg: &str) {
	let details = gstreamer::Structure::builder("error-details")
		.field("error", GBoxErrorWrapper::new(err))
		.build();

	element_error!(
		element,
		gstreamer::LibraryError::Failed,
		("{}", msg),
		details: details
	);
}

/// Builds the elements between the audio pad of the decodebin/parsebin and the filesink:
/// resampling and conversion, the encoder and the muxer/tagger.
pub fn dest_elements(transcode: &Transcode) -> Result<Vec<Element>> {
	let mut dest_elems = Vec::new();

	if !matches!(transcode, Transcode::CopyAudio { .. }) {
		dest_elems.push(gmake(
			"audioresample",
			&[
				// quality from 0 to 10
				("quality", &10i32),
			],
		)?);
		// `audioconvert` converts audio format, bitdepth, ...
		dest_elems.push(gmake("audioconvert", &[])?);
	}

	match transcode {
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
					context::BitrateType::Vbr => "vbr",
					context::BitrateType::Cbr => "cbr",
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
							context::BitrateType::Vbr => &false,
							context::BitrateType::Cbr => &true,
						},
					),
				],
			)?;
			encoder.set_property_from_str("target", "bitrate");

			dest_elems.push(encoder);
			dest_elems.push(gmake("id3v2mux", &[])?);
		}

		Transcode::CopyAudio { extension } => {
			let muxer = match extension {
				AudioExtension::M4a => "mp4mux",
				AudioExtension::Mka => "matroskamux",
				AudioExtension::Ogg | AudioExtension::Opus => "oggmux",
				AudioExtension::Webm => "webmmux",
				// mp3 and flac need no container, these elements only write the tags
				AudioExtension::Mp3 => "id3v2mux",
				AudioExtension::Flac => "flactag",
			};
			dest_elems.push(gmake(muxer, &[])?);
		}

		Transcode::Copy => {
			return Err(Error::msg(
				"`copy` copies the whole file and needs no gstreamer elements",
			));
		}
	}

	Ok(dest_elems)
}

pub async fn transcode_gstreamer(
	from_path: &Path,
	to_path: &Path,
	transcode: Transcode,
	task_id: usize,
	queue: &ui::MsgQueue,
) -> Result<()> {
	let file_src: Element = gmake("filesrc", &[("location", &from_path)])?;

	// `copy_audio` needs the audio stream only demuxed and parsed, everything else decoded
	let src_bin: Element = if let Transcode::CopyAudio { .. } = transcode {
		gmake("parsebin", &[])?
	} else {
		let decodebin: Element = gmake("decodebin", &[])?;

		// expose video streams (e.g. from video files or embedded cover art) undecoded, they are
		// ignored anyway and decoding them would only waste cpu time
		let select_result_type = glib::Type::from_name("GstAutoplugSelectResult")
			.context("Could not get type `GstAutoplugSelectResult`")?;
		decodebin.connect("autoplug-select", false, move |args| {
			let factory = args[3]
				.get::<gstreamer::ElementFactory>()
				.expect("autoplug-select: 4. arg is not an ElementFactory");
			let is_video_decoder = factory.has_type(
				gstreamer::ElementFactoryType::DECODER
					| gstreamer::ElementFactoryType::MEDIA_VIDEO
					| gstreamer::ElementFactoryType::MEDIA_IMAGE,
			);
			let result = if is_video_decoder { "expose" } else { "try" };
			glib::EnumClass::with_type(select_result_type)
				.and_then(|class| class.to_value_by_nick(result))
		});

		decodebin
	};

	// only the first audio stream gets transcoded, all of them would write into the same file
	let audio_linked = Arc::new(AtomicBool::new(false));

	let src_elems: &[&Element] = &[&file_src, &src_bin];

	let pipeline = gstreamer::Pipeline::new();

	pipeline.add_many(src_elems)?;
	Element::link_many(src_elems)?;

	// downgrade pipeline RC to a weak RC to break the reference cycle
	let pipeline_weak = pipeline.downgrade();

	let to_path_clone = to_path.to_owned();
	let audio_linked_clone = audio_linked.clone();
	src_bin.connect_pad_added(move |src_bin, src_pad| {
		let insert_sink = || -> Result<()> {
			let pipeline = match pipeline_weak.upgrade() {
				Some(pipeline) => pipeline,
				None => {
					// pipeline already destroyed... ignoring
					return Ok(());
				}
			};

			// parsebin may expose pads before their caps are fixed, fall back to the possible caps
			let caps = src_pad
				.current_caps()
				.unwrap_or_else(|| src_pad.query_caps(None));
			let is_audio = caps.structure(0).map(|s| {
				let name = s.name();
				name.starts_with("audio/")
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

			if audio_linked_clone.swap(true, Ordering::SeqCst) {
				// not the first audio pad... ignoring
				return Ok(());
			}

			let mut dest_elems = dest_elements(&transcode)?;

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

			// muxers only have request pads, `compatible_pad` requests them as needed
			let sink_pad = dest_elems
				.first()
				.unwrap()
				.compatible_pad(src_pad, Some(&caps))
				.with_context(|| match &transcode {
					Transcode::CopyAudio { extension } => format!(
						"The audio stream can't be stored in a .{} file without transcoding \
							 it, caps: {}",
						extension.as_str(),
						caps
					),
					_ => format!("Could not link the audio stream, caps: {}", caps),
				})?;
			src_pad.link(&sink_pad)?;

			Ok(())
		};

		if let Err(err) = insert_sink() {
			post_error(src_bin, err, "Failed to insert sink");
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
						let no_audio = !audio_linked.load(Ordering::SeqCst);

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

						// without an audio stream all pads are unlinked, the demuxer fails with a
						// cryptic "not-linked" error
						let err = if no_audio {
							err.context("No audio stream found")
						} else {
							err
						};

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

			let dur = src_bin
				.query_duration::<ClockTime>()
				.map(|time| time.nseconds());

			let ratio = dur.and_then(|dur| {
				if dur == 0 {
					return None;
				}

				let pos = src_bin
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

	if !audio_linked.load(Ordering::SeqCst) {
		return Err(Error::msg("No audio stream found"));
	}

	Ok(())
}
