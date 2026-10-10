use ::audio_conv::log::Log;
use ::std::fs;

#[test]
fn creates_the_file_only_on_the_first_write() {
	let tmp = tempfile::tempdir().unwrap();
	let path = tmp.path().join("audio-conv.log");

	let log = Log::new(path.clone());

	assert!(!path.exists());

	log.write("first").unwrap();

	assert_eq!(fs::read_to_string(&path).unwrap(), "first\n");
}

#[test]
fn appends_to_an_existing_file() {
	let tmp = tempfile::tempdir().unwrap();
	let path = tmp.path().join("audio-conv.log");
	fs::write(&path, "earlier run\n").unwrap();

	let log = Log::new(path.clone());
	log.write("first").unwrap();
	log.write(format_args!("second {}", 2)).unwrap();

	assert_eq!(
		fs::read_to_string(&path).unwrap(),
		"earlier run\nfirst\nsecond 2\n"
	);
}

#[test]
fn fails_if_the_file_cant_be_opened() {
	let tmp = tempfile::tempdir().unwrap();

	let log = Log::new(tmp.path().join("missing/audio-conv.log"));

	assert!(log.write("first").is_err());
}
