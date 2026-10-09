use ::anyhow::Error;
use ::gstreamer::glib::{self, Boxed};
use ::std::{error::Error as StdError, fmt, result::Result as StdResult, sync::Arc};

#[derive(Clone, Debug, Boxed)]
#[boxed_type(name = "GBoxErrorWrapper")]
pub struct GBoxErrorWrapper(Arc<Error>);

impl GBoxErrorWrapper {
	pub fn new(err: Error) -> Self {
		GBoxErrorWrapper(Arc::new(err))
	}
}

impl StdError for GBoxErrorWrapper {
	fn source(&self) -> Option<&(dyn StdError + 'static)> {
		self.0.source()
	}
}

impl fmt::Display for GBoxErrorWrapper {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> StdResult<(), fmt::Error> {
		self.0.fmt(f)
	}
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("Received error from {}: {} (debug: {:?})", src, error, debug)]
pub struct GErrorMessage {
	pub src: String,
	pub error: String,
	pub debug: Option<String>,
	pub source: glib::Error,
}
