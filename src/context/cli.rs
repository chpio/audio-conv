use super::{Delete, DeleteMode};
use ::clap::{Arg, ArgAction, builder::ValueParser};
use ::std::path::PathBuf;

/// The command line arguments, paths are as given (relative to the current directory).
#[derive(Debug)]
pub struct Args {
	pub config: Option<PathBuf>,
	pub from: Option<PathBuf>,
	pub to: Option<PathBuf>,
	pub jobs: Option<usize>,
	pub delete: Option<Delete>,
	/// `None` converts the files.
	pub command: Option<Command>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
	/// Writes an example config.
	Init,
}

pub fn parse() -> Args {
	let arg_matches = ::clap::Command::new("audio-conv")
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
		.arg(
			Arg::new("delete")
				.long("delete")
				.required(false)
				.num_args(0..=1)
				.require_equals(true)
				.default_missing_value("converted")
				.value_parser(["converted", "all"])
				.value_name("MODE")
				.help(
					"Logs the files in \"to\" that don't belong to a file in \"from\", only the ones \
					 audio-conv would create (\"converted\", the default) or all of them (\"all\")",
				),
		)
		.arg(
			Arg::new("force")
				.long("force")
				.action(ArgAction::SetTrue)
				.requires("delete")
				.help("Deletes the files found by --delete instead of only logging them"),
		)
		.subcommand(
			::clap::Command::new("init")
				.about("Writes an example config to audio-conv.yaml (or the --config path)"),
		)
		.get_matches();

	Args {
		config: arg_matches.get_one::<PathBuf>("config").cloned(),
		from: arg_matches.get_one::<PathBuf>("from").cloned(),
		to: arg_matches.get_one::<PathBuf>("to").cloned(),
		jobs: arg_matches.get_one::<usize>("jobs").copied(),
		delete: arg_matches.get_one::<String>("delete").map(|mode| Delete {
			mode: match mode.as_str() {
				"all" => DeleteMode::All,
				_ => DeleteMode::Converted,
			},
			force: arg_matches.get_flag("force"),
		}),
		command: match arg_matches.subcommand_name() {
			Some("init") => Some(Command::Init),
			_ => None,
		},
	}
}
