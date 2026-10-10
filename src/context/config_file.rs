use ::anyhow::{Context, Error, Result};
use ::schemars::JsonSchema;
use ::serde::{Deserialize, Serialize};
use ::std::path::{Path, PathBuf};

/// What the matched files are converted to.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "codec")]
#[schemars(deny_unknown_fields)]
pub enum Transcode {
	/// Transcodes to Opus in an Ogg container (`.opus`).
	#[serde(rename = "opus")]
	Opus {
		/// Bitrate in kbit/s.
		#[serde(default = "default_opus_bitrate")]
		#[schemars(range(min = 4, max = 650))]
		bitrate: u16,

		#[serde(default = "bitrate_type_vbr")]
		bitrate_type: BitrateType,
	},

	/// Transcodes to FLAC (`.flac`).
	#[serde(rename = "flac")]
	Flac {
		/// Effort spent for the compression, 0 (fastest) to 9 (highest compression).
		#[serde(default = "default_flac_compression")]
		#[schemars(range(max = 9))]
		compression: u8,
	},

	/// Transcodes to MP3 (`.mp3`).
	#[serde(rename = "mp3")]
	// cbr is restricted to the bitrates of MP3 frames, other values get silently rounded up
	#[schemars(extend(
		"if" = {
			"properties": { "bitrate_type": { "const": "cbr" } },
			"required": ["bitrate_type"]
		},
		"then" = {
			"properties": {
				"bitrate": {
					"enum": [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320]
				}
			}
		}
	))]
	Mp3 {
		/// Bitrate in kbit/s. `vbr` (average bitrate) takes any value, `cbr` one of 8, 16, 24, 32,
		/// 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256 or 320.
		#[serde(default = "default_mp3_bitrate")]
		#[schemars(range(min = 8, max = 320))]
		bitrate: u16,

		#[serde(default = "bitrate_type_vbr")]
		bitrate_type: BitrateType,
	},

	/// Copies the file without transcoding it, the extension is kept.
	#[serde(rename = "copy")]
	Copy,

	/// Copies the first audio stream without transcoding it, e.g. the audio of a video file.
	#[serde(rename = "copy_audio")]
	CopyAudio {
		/// Extension and container of the target file, the audio codec of the source has to be
		/// supported by it.
		extension: AudioExtension,
	},
}

impl Transcode {
	/// Extension of the target file, `None` keeps the extension of the source file.
	pub fn extension(&self) -> Option<&'static str> {
		match self {
			Transcode::Opus { .. } => Some("opus"),
			Transcode::Flac { .. } => Some("flac"),
			Transcode::Mp3 { .. } => Some("mp3"),
			Transcode::Copy => None,
			Transcode::CopyAudio { extension } => Some(extension.as_str()),
		}
	}
}

impl Default for Transcode {
	fn default() -> Self {
		Transcode::Opus {
			bitrate: default_opus_bitrate(),
			bitrate_type: bitrate_type_vbr(),
		}
	}
}

fn default_opus_bitrate() -> u16 {
	160
}

fn default_flac_compression() -> u8 {
	5
}

fn bitrate_type_vbr() -> BitrateType {
	BitrateType::Vbr
}

fn default_mp3_bitrate() -> u16 {
	256
}

/// Container for `copy_audio`, the audio codec of the source has to be supported by it.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
pub enum AudioExtension {
	/// MP4 audio, e.g. for AAC or ALAC
	#[serde(rename = "m4a")]
	M4a,
	/// Matroska audio, supports nearly every audio codec
	#[serde(rename = "mka")]
	Mka,
	/// Ogg, e.g. for Vorbis, Opus or FLAC
	#[serde(rename = "ogg")]
	Ogg,
	/// Ogg, for Opus
	#[serde(rename = "opus")]
	Opus,
	/// WebM, for Vorbis or Opus
	#[serde(rename = "webm")]
	Webm,
	/// MP3
	#[serde(rename = "mp3")]
	Mp3,
	/// FLAC
	#[serde(rename = "flac")]
	Flac,
}

impl AudioExtension {
	pub fn as_str(self) -> &'static str {
		match self {
			AudioExtension::M4a => "m4a",
			AudioExtension::Mka => "mka",
			AudioExtension::Ogg => "ogg",
			AudioExtension::Opus => "opus",
			AudioExtension::Webm => "webm",
			AudioExtension::Mp3 => "mp3",
			AudioExtension::Flac => "flac",
		}
	}
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub enum BitrateType {
	/// Constant bitrate
	#[serde(rename = "cbr")]
	Cbr,
	/// Variable bitrate
	#[serde(rename = "vbr")]
	Vbr,
}

/// audio-conv config file, `audio-conv.yaml` in the current directory by default.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(title = "audio-conv config", deny_unknown_fields)]
pub struct ConfigFile {
	/// Directory with the source files, relative to the config file. Overridden by `--from`.
	pub from: Option<PathBuf>,

	/// Directory the converted files are written to, relative to the config file. Overridden by
	/// `--to`.
	pub to: Option<PathBuf>,

	/// Rules which files are converted and how, the first matching rule wins. Without rules,
	/// `.flac` and `.wav` files are transcoded to Opus with 160 kbit/s.
	#[serde(default)]
	pub matches: Vec<TranscodeMatchFile>,

	/// Number of files converted at once, defaults to the number of logical cores. Overridden by
	/// `--jobs`.
	pub jobs: Option<usize>,
}

impl ConfigFile {
	/// Loads the config file, `None` if it doesn't exist.
	pub fn load(path: &Path) -> Result<Option<ConfigFile>> {
		let mut file = match std::fs::File::open(path) {
			Ok(file) => file,
			Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
			Err(err) => return Err(Error::new(err)),
		};
		let config: ConfigFile =
			serde_yaml_ng::from_reader(&mut file).context("Could not parse config file")?;
		Ok(Some(config))
	}
}

/// A rule matching files by `glob`, `regex` and/or `extensions` (case insensitive, a file has
/// to match any of them). Without any of them, `.flac` and `.wav` files are matched.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TranscodeMatchFile {
	/// Glob matched against the absolute path of the file, e.g. `**/*.flac`.
	pub glob: Option<String>,

	/// Regular expression matched against the absolute path of the file, e.g. `\.flac$`.
	pub regex: Option<String>,

	/// File extensions without the leading dot, e.g. `flac`.
	#[serde(default)]
	pub extensions: Vec<String>,

	pub to: Transcode,
}

/// JSON schema of the config file (`ConfigFile`), for editor support.
pub fn json_schema() -> String {
	let schema = ::schemars::generate::SchemaSettings::draft07()
		.into_generator()
		.into_root_schema_for::<ConfigFile>();
	let mut json = Vec::new();
	let formatter = ::serde_json::ser::PrettyFormatter::with_indent(b"\t");
	let mut serializer = ::serde_json::Serializer::with_formatter(&mut json, formatter);
	schema
		.serialize(&mut serializer)
		.expect("Failed serializing the schema");
	json.push(b'\n');
	String::from_utf8(json).expect("Serialized schema is not valid UTF-8")
}
