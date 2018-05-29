# Cross compilation environment for spotty

FROM debian:stretch

RUN dpkg --add-architecture arm64
RUN dpkg --add-architecture armhf
RUN dpkg --add-architecture armel
RUN dpkg --add-architecture i686
RUN apt-get update

RUN apt-get install -y curl git build-essential gcc-multilib musl-tools musl-dev musl
RUN apt-get install -y crossbuild-essential-armhf crossbuild-essential-armel crossbuild-essential-arm64

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin/:${PATH}"
RUN rustup target add x86_64-unknown-linux-musl
RUN rustup target add i686-unknown-linux-musl
RUN rustup target add aarch64-unknown-linux-gnu
#RUN rustup target add arm-unknown-linux-musleabihf
RUN rustup target add arm-unknown-linux-gnueabihf
RUN rustup target add arm-unknown-linux-gnueabi

RUN mkdir /.cargo && \
	echo '[target.aarch64-unknown-linux-gnu]\nlinker = "aarch64-linux-gnu-gcc"' > /.cargo/config && \
	echo '[target.arm-unknown-linux-gnueabihf]\nlinker = "arm-linux-gnueabihf-gcc"' >> /.cargo/config && \
	echo '[target.arm-unknown-linux-gnueabi]\nlinker = "arm-linux-gnueabi-gcc"' >> /.cargo/config
#	echo '[target.arm-unknown-linux-musleabihf]\nlinker = "arm-linux-gnueabihf-gcc"' >> /.cargo/config

RUN mkdir /build
ENV CARGO_TARGET_DIR /build
ENV CARGO_HOME /build/cache

RUN mkdir /src

WORKDIR /src
CMD ["/src/docker/docker-build.sh"]
