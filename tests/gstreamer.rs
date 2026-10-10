//! Needs GStreamer with the plugins for all codecs installed.

use ::audio_conv::{context::Transcode, transcode::dest_elements};
use ::gstreamer::{ClockTime, Element, MessageType, prelude::*};

fn transcode(yaml: &str) -> Transcode {
	serde_yaml_ng::from_str(yaml).unwrap()
}

fn elements(yaml: &str) -> Vec<Element> {
	gstreamer::init().unwrap();
	dest_elements(&transcode(yaml)).unwrap()
}

fn factory_names(elems: &[Element]) -> Vec<String> {
	elems
		.iter()
		.map(|e| e.factory().unwrap().name().to_string())
		.collect()
}

fn enum_nick(elem: &Element, property: &str) -> String {
	let value = elem.property_value(property);
	gstreamer::glib::EnumValue::from_value(&value)
		.unwrap()
		.1
		.nick()
		.to_owned()
}

/// Encodes a short test tone with `elems`, fails on any pipeline error.
fn encode_test_tone(elems: Vec<Element>) {
	let pipeline = gstreamer::Pipeline::new();
	let src = gstreamer::ElementFactory::make("audiotestsrc")
		.property("num-buffers", 20)
		.build()
		.unwrap();
	let sink = gstreamer::ElementFactory::make("fakesink").build().unwrap();

	let mut all = vec![src];
	all.extend(elems);
	all.push(sink);
	let all: Vec<_> = all.iter().collect();
	pipeline.add_many(&all).unwrap();
	Element::link_many(&all).unwrap();

	pipeline.set_state(gstreamer::State::Playing).unwrap();
	let msg = pipeline
		.bus()
		.unwrap()
		.timed_pop_filtered(
			ClockTime::from_seconds(10),
			&[MessageType::Eos, MessageType::Error],
		)
		.expect("pipeline timed out");
	pipeline.set_state(gstreamer::State::Null).unwrap();

	if let gstreamer::MessageView::Error(err) = msg.view() {
		panic!("pipeline failed: {} ({:?})", err.error(), err.debug());
	}
}

#[test]
fn opus() {
	let elems = elements("{codec: opus, bitrate: 96, bitrate_type: cbr}");

	assert_eq!(
		factory_names(&elems),
		["audioresample", "audioconvert", "opusenc", "oggmux"]
	);
	assert_eq!(elems[2].property::<i32>("bitrate"), 96_000);
	assert_eq!(enum_nick(&elems[2], "bitrate-type"), "cbr");

	encode_test_tone(elems);
}

#[test]
fn flac() {
	let elems = elements("{codec: flac, compression: 8}");

	assert_eq!(
		factory_names(&elems),
		["audioresample", "audioconvert", "flacenc"]
	);
	assert_eq!(enum_nick(&elems[2], "quality"), "8");

	encode_test_tone(elems);
}

#[test]
fn flac_compression_out_of_range() {
	gstreamer::init().unwrap();

	assert!(dest_elements(&transcode("{codec: flac, compression: 10}")).is_err());
}

#[test]
fn mp3() {
	let elems = elements("{codec: mp3, bitrate: 192, bitrate_type: cbr}");

	assert_eq!(
		factory_names(&elems),
		["audioresample", "audioconvert", "lamemp3enc", "id3v2mux"]
	);
	assert_eq!(elems[2].property::<i32>("bitrate"), 192);
	assert!(elems[2].property::<bool>("cbr"));
	assert_eq!(enum_nick(&elems[2], "target"), "bitrate");

	encode_test_tone(elems);
}

#[test]
fn copy_audio() {
	for (extension, muxer) in [
		("m4a", "mp4mux"),
		("mka", "matroskamux"),
		("ogg", "oggmux"),
		("opus", "oggmux"),
		("webm", "webmmux"),
		("mp3", "id3v2mux"),
		("flac", "flactag"),
	] {
		let elems = elements(&format!("{{codec: copy_audio, extension: {}}}", extension));

		assert_eq!(factory_names(&elems), [muxer], "extension {}", extension);
	}
}

#[test]
fn copy_has_no_elements() {
	gstreamer::init().unwrap();

	assert!(dest_elements(&Transcode::Copy).is_err());
}
