use ::anyhow::{Context as _, Result};
use ::std::{
	fmt::Display,
	fs::{File, OpenOptions},
	io::Write,
	path::{Path, PathBuf},
	sync::Mutex,
};

/// The log file, opened on the first write, so it's only created if something gets logged.
#[derive(Debug)]
pub struct Log {
	path: PathBuf,
	file: Mutex<Option<File>>,
}

impl Log {
	pub fn new(path: PathBuf) -> Log {
		Log {
			path,
			file: Mutex::new(None),
		}
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	/// Appends `entry` and a newline, in a single write so entries from concurrent jobs don't mix.
	pub fn write(&self, entry: impl Display) -> Result<()> {
		let entry = format!("{entry}\n");

		let mut guard = self.file.lock().unwrap_or_else(|err| err.into_inner());
		let file = match guard.take() {
			Some(file) => file,
			None => OpenOptions::new()
				.create(true)
				.append(true)
				.open(&self.path)
				.with_context(|| format!("Unable to open log file {}", self.path.display()))?,
		};

		guard
			.insert(file)
			.write_all(entry.as_bytes())
			.with_context(|| format!("Unable to write to log file {}", self.path.display()))
	}
}
