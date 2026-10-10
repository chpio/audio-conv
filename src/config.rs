use ::anyhow::{Context, Error, Result};
use ::clap::{ArgAction, builder::ValueParser};
use ::globset::GlobBuilder;
use ::regex::bytes::{Regex, RegexBuilder};
use ::schemars::JsonSchema;
use ::serde::{Deserialize, Serialize};
use ::std::{
	io::Write,
	path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Config {
	pub from: PathBuf,
	pub to: PathBuf,
	pub matches: Vec<TranscodeMatch>,
	pub jobs: Option<usize>,
}

#[derive(Debug)]
pub struct TranscodeMatch {
	pub regexes: Vec<Regex>,
	pub to: Transcode,
}

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

impl Default for Transcode {
	fn default() -> Self {
		Transcode::Opus {
			bitrate: default_opus_bitrate(),
			bitrate_type: bitrate_type_vbr(),
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

pub fn config() -> Result<Config> {
	use ::clap::{Arg, Command};

	let arg_matches = Command::new("audio-conv")
		.version(clap::crate_version!())
		.about(clap::crate_description!())
		.arg(
			Arg::new("config")
				.short('c')
				.long("config")
				.required(false)
				.value_parser(ValueParser::path_buf())
				.action(ArgAction::Set)
				.value_name("FILE")
				.help("Path to the config file [default: audio-conv.yaml]"),
		)
		.arg(
			Arg::new("from")
				.short('f')
				.long("from")
				.required(false)
				.value_parser(ValueParser::path_buf())
				.action(ArgAction::Set)
				.value_name("DIR")
				.help("Directory with the source files, overrides \"from\" of the config file"),
		)
		.arg(
			Arg::new("to")
				.short('t')
				.long("to")
				.required(false)
				.value_parser(ValueParser::path_buf())
				.action(ArgAction::Set)
				.value_name("DIR")
				.help(
					"Directory the converted files are written to, overrides \"to\" of the config file",
				),
		)
		.arg(
			Arg::new("jobs")
				.short('j')
				.long("jobs")
				.required(false)
				.value_parser(clap::value_parser!(usize))
				.action(ArgAction::Set)
				.value_name("N")
				.help("Number of files converted at once [default: number of logical cores]"),
		)
		.subcommand(
			Command::new("init")
				.about("Writes an example config to audio-conv.yaml (or the --config path)"),
		)
		.get_matches();

	let current_dir = std::env::current_dir().context("Could not get current directory")?;

	let config_path = arg_matches.get_one::<PathBuf>("config");
	let enforce_config_load = config_path.is_some();
	let config_path = config_path
		.map(AsRef::<Path>::as_ref)
		.unwrap_or_else(|| AsRef::<Path>::as_ref("audio-conv.yaml"));
	let config_path = current_dir.join(config_path);

	if let Some("init") = arg_matches.subcommand_name() {
		std::fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&config_path)
			.and_then(|mut f| f.write_all(std::include_bytes!("../example.audio-conv.yaml")))
			.with_context(|| format!("Unable to write config file to {}", config_path.display()))?;

		std::process::exit(0);
	}

	let config_dir = config_path
		.parent()
		.context("Could not get parent directory of the config file")?;

	let config_file = load_config_file(&config_path)
		.with_context(|| format!("Failed loading config file {}", config_path.display()))?;

	if enforce_config_load && config_file.is_none() {
		return Err(Error::msg(format!(
			"could not find config file \"{}\"",
			config_path.display()
		)));
	}

	let transcode_matches = compile_matches(
		config_file
			.as_ref()
			.map(|c| c.matches.as_slice())
			.unwrap_or_default(),
	)?;

	Ok(Config {
		from: {
			arg_matches
				.get_one::<PathBuf>("from")
				.map(|p| current_dir.join(p))
				.or_else(|| {
					config_file
						.as_ref()
						.and_then(|c| c.from.as_ref())
						.map(|p| config_dir.join(p))
				})
				.ok_or_else(|| Error::msg("\"from\" not configured"))?
				.canonicalize()
				.context("Could not canonicalize \"from\" path")?
		},
		to: arg_matches
			.get_one::<PathBuf>("to")
			.map(|p| current_dir.join(p))
			.or_else(|| {
				config_file
					.as_ref()
					.and_then(|c| c.to.as_ref())
					.map(|p| config_dir.join(p))
			})
			.ok_or_else(|| Error::msg("\"to\" not configured"))?
			.canonicalize()
			.context("Could not canonicalize \"to\" path")?,
		matches: transcode_matches,
		jobs: arg_matches
			.get_one("jobs")
			.copied()
			.or_else(|| config_file.as_ref().and_then(|c| c.jobs)),
	})
}

fn default_regex() -> Regex {
	RegexBuilder::new("\\.(flac|wav)$")
		.case_insensitive(true)
		.build()
		.expect("Failed compiling default match regex")
}

pub fn compile_matches(matches: &[TranscodeMatchFile]) -> Result<Vec<TranscodeMatch>> {
	if matches.is_empty() {
		return Ok(vec![TranscodeMatch {
			regexes: vec![default_regex()],
			to: Transcode::default(),
		}]);
	}

	matches.iter().map(compile_match).collect()
}

fn compile_match(m: &TranscodeMatchFile) -> Result<TranscodeMatch> {
	let glob = m.glob.iter().map(|glob| {
		let glob = GlobBuilder::new(glob)
			.case_insensitive(true)
			.build()
			.context("Failed building glob")?;
		let regex = Regex::new(glob.regex()).context("Failed compiling regex")?;
		Ok(regex)
	});

	let regex = m.regex.iter().map(|regex| {
		let regex = RegexBuilder::new(regex)
			.case_insensitive(true)
			.build()
			.context("Failed compiling regex")?;
		Ok(regex)
	});

	let extensions = m.extensions.iter().map(|ext| {
		let mut ext = regex::escape(ext);
		ext.insert_str(0, "\\.");
		ext.push('$');

		let regex = RegexBuilder::new(&ext)
			.case_insensitive(true)
			.build()
			.context("Failed compiling regex")?;
		Ok(regex)
	});

	let mut regexes = glob
		.chain(regex)
		.chain(extensions)
		.collect::<Result<Vec<_>>>()?;

	if regexes.is_empty() {
		regexes.push(default_regex());
	}

	Ok(TranscodeMatch {
		regexes,
		to: m.to.clone(),
	})
}

fn load_config_file(path: &Path) -> Result<Option<ConfigFile>> {
	let mut file = match std::fs::File::open(path) {
		Ok(file) => file,
		Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(err) => return Err(Error::new(err)),
	};
	let config: ConfigFile =
		serde_yaml_ng::from_reader(&mut file).context("Could not parse config file")?;
	Ok(Some(config))
}
