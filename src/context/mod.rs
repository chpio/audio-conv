mod cli;
pub mod config_file;

// part of `Context` (via `matches`) or of `compile_matches`, the other types only describe the
// config file
pub use config_file::{AudioExtension, BitrateType, Transcode, TranscodeMatchFile};

use config_file::ConfigFile;

use crate::log::Log;
use ::anyhow::{Context as _, Error, Result};
use ::globset::GlobBuilder;
use ::regex::bytes::{Regex, RegexBuilder};
use ::std::{
	io::Write,
	path::{Path, PathBuf},
};

/// Everything shared while running, as `Arc<Context>`: the config file merged with the command
/// line arguments (see `load`) and the log file.
#[derive(Debug)]
pub struct Context {
	/// Path of the config file, it doesn't have to exist.
	pub config_path: PathBuf,
	pub from: PathBuf,
	pub to: PathBuf,
	pub matches: Vec<TranscodeMatch>,
	pub jobs: Option<usize>,
	pub delete: Option<Delete>,
	/// `audio-conv.log` in the current directory.
	pub log: Log,
}

/// Deleting files in `to` that don't belong to a file in `from`, only given on the command line.
#[derive(Clone, Copy, Debug)]
pub struct Delete {
	pub mode: DeleteMode,
	/// Without it, the files are only logged.
	pub force: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteMode {
	/// Only files audio-conv would create with the current config.
	Converted,
	/// All files.
	All,
}

#[derive(Debug)]
pub struct TranscodeMatch {
	pub regexes: Vec<Regex>,
	pub to: Transcode,
}

/// Merges the config file with the command line arguments, which take precedence.
pub fn load() -> Result<Context> {
	let args = cli::parse();

	let current_dir = std::env::current_dir().context("Could not get current directory")?;

	let enforce_config_load = args.config.is_some();
	let config_path = current_dir.join(
		args.config
			.as_deref()
			.unwrap_or(Path::new("audio-conv.yaml")),
	);

	if args.init {
		std::fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&config_path)
			.and_then(|mut f| f.write_all(std::include_bytes!("../../example.audio-conv.yaml")))
			.with_context(|| format!("Unable to write config file to {}", config_path.display()))?;

		std::process::exit(0);
	}

	let config_dir = config_path
		.parent()
		.context("Could not get parent directory of the config file")?;

	let config_file = ConfigFile::load(&config_path)
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

	Ok(Context {
		from: args
			.from
			.map(|p| current_dir.join(p))
			.or_else(|| {
				config_file
					.as_ref()
					.and_then(|c| c.from.as_ref())
					.map(|p| config_dir.join(p))
			})
			.ok_or_else(|| Error::msg("\"from\" not configured"))?
			.canonicalize()
			.context("Could not canonicalize \"from\" path")?,
		to: args
			.to
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
		jobs: args
			.jobs
			.or_else(|| config_file.as_ref().and_then(|c| c.jobs)),
		delete: args.delete,
		log: Log::new(current_dir.join("audio-conv.log")),
		// last, `config_dir` borrows it
		config_path,
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
