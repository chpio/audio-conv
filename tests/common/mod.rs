use ::audio_conv::{
	context::{Context, TranscodeMatch, TranscodeMatchFile, compile_matches},
	log::Log,
};
use ::std::{
	fs::{self, File},
	path::Path,
};
use ::tempfile::TempDir;

/// Compiles matches given in the config file format, e.g. `[{extensions: [flac], to: {codec: mp3}}]`.
pub fn compile(yaml: &str) -> Vec<TranscodeMatch> {
	let matches: Vec<TranscodeMatchFile> = serde_yaml_ng::from_str(yaml).unwrap();
	compile_matches(&matches).unwrap()
}

/// A `Context` with empty `from` and `to` dirs in a temporary dir, deleted on drop.
pub struct Dirs {
	_tmp: TempDir,
	pub ctx: Context,
}

pub fn dirs(matches: Vec<TranscodeMatch>) -> Dirs {
	let tmp = tempfile::tempdir().unwrap();
	// canonical like the paths from the config
	let root = tmp.path().canonicalize().unwrap();
	let from = root.join("from");
	let to = root.join("to");
	fs::create_dir(&from).unwrap();
	fs::create_dir(&to).unwrap();
	Dirs {
		_tmp: tmp,
		ctx: Context {
			config_path: root.join("audio-conv.yaml"),
			from,
			to,
			matches,
			jobs: None,
			delete: None,
			log: Log::new(root.join("audio-conv.log")),
		},
	}
}

/// Creates an empty file, including its parent dirs.
pub fn touch(path: &Path) -> File {
	fs::create_dir_all(path.parent().unwrap()).unwrap();
	File::create(path).unwrap()
}
