use crate::{config::Config, log::Log};

/// Everything shared while running, as `Arc<Context>`.
#[derive(Debug)]
pub struct Context {
	pub config: Config,
	pub log: Log,
}

impl Context {
	pub fn new(config: Config) -> Context {
		let log = Log::new(config.log_path.clone());
		Context { config, log }
	}
}
