FROM rust:1

# nodejs is needed by actions/checkout and actions/cache, rust:1 doesn't ship it
RUN apt-get update \
	&& apt-get install -y --no-install-recommends \
		nodejs \
		libgstreamer1.0-dev \
		libgstreamer-plugins-base1.0-dev \
		gstreamer1.0-plugins-base \
		gstreamer1.0-plugins-good \
		gstreamer1.0-plugins-ugly \
	&& rm -rf /var/lib/apt/lists/* \
	&& rustup component add clippy rustfmt
