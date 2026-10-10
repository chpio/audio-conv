use ::audio_conv::context::config_file::json_schema;
use ::std::{env, fs, path::Path};

const SCHEMA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/audio-conv.schema.json");

/// `audio-conv.schema.json` is generated from the config types, run the tests with
/// `UPDATE_SCHEMA=1` to update it.
#[test]
fn schema_file_is_up_to_date() {
	let schema = json_schema();

	if env::var_os("UPDATE_SCHEMA").is_some() {
		fs::write(SCHEMA_PATH, &schema).unwrap();
		return;
	}

	let file = fs::read_to_string(Path::new(SCHEMA_PATH)).unwrap_or_default();
	assert!(
		file == schema,
		"{} is outdated, run `UPDATE_SCHEMA=1 cargo test --test schema` to update it",
		SCHEMA_PATH
	);
}

fn is_valid(yaml: &str) -> bool {
	let schema: serde_json::Value = serde_json::from_str(&json_schema()).unwrap();
	let validator = jsonschema::validator_for(&schema).unwrap();
	let config: serde_json::Value = serde_yaml_ng::from_str(yaml).unwrap();
	validator.is_valid(&config)
}

#[test]
fn example_config_is_valid() {
	assert!(is_valid(include_str!("../example.audio-conv.yaml")));
}

#[test]
fn valid_configs() {
	assert!(is_valid("{}"));
	assert!(is_valid("{from: ./a, to: ./b, jobs: 4}"));
	assert!(is_valid(
		"matches:
  - {extensions: [flac], to: {codec: opus}}
  - {glob: '**/*.wav', to: {codec: opus, bitrate: 96, bitrate_type: cbr}}
  - {regex: '\\.ape$', to: {codec: flac, compression: 9}}
  - {to: {codec: mp3, bitrate: 100}}
  - {to: {codec: mp3, bitrate: 112, bitrate_type: cbr}}
  - {extensions: [jpg], to: {codec: copy}}
  - {extensions: [mp4], to: {codec: copy_audio, extension: m4a}}"
	));
}

#[test]
fn invalid_configs() {
	// unknown fields
	assert!(!is_valid("{form: ./a}"));
	assert!(!is_valid(
		"{matches: [{extension: [flac], to: {codec: opus}}]}"
	));
	assert!(!is_valid("{matches: [{to: {codec: opus, bitrat: 96}}]}"));
	// missing or unknown codec
	assert!(!is_valid("{matches: [{extensions: [flac]}]}"));
	assert!(!is_valid("{matches: [{to: {codec: aac}}]}"));
	// out of range
	assert!(!is_valid(
		"{matches: [{to: {codec: flac, compression: 10}}]}"
	));
	assert!(!is_valid("{matches: [{to: {codec: mp3, bitrate: 512}}]}"));
	// cbr only allows the bitrates of MP3 frames
	assert!(!is_valid(
		"{matches: [{to: {codec: mp3, bitrate: 100, bitrate_type: cbr}}]}"
	));
	// copy_audio needs a known extension
	assert!(!is_valid("{matches: [{to: {codec: copy_audio}}]}"));
	assert!(!is_valid(
		"{matches: [{to: {codec: copy_audio, extension: aac}}]}"
	));
}
