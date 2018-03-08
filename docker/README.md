# How to ross compile spotty

If only I knew... Unfortunately I never managed to get all builds working in a Docker environment. But here we go anyway. The following targets should build successfully:

* x86_64-unknown-linux-musl
* i686-unknown-linux-musl
* aarch64-unknown-linux-gnu (eg. Rock64)
* arm-unknown-linux-gnueabihf (eg. Raspberry Pi 2+)

Build the docker image from the root of the project with the following command:

```
$ docker build -t spotty-cross -f docker/Dockerfile .
```

The resulting image can be used to build spotty for aforementioned platforms.

```
$ docker run -v /tmp/spotty-build:/build -v $PWD:/src spotty-cross
```


The compiled binaries will be located in a sub folder called `releases`.

If only one architecture is desired, cargo can be invoked directly with the appropriate options:

```
$ docker run -v /tmp/spotty-build:/build spotty-cross cargo build --release
$ docker run -v /tmp/spotty-build:/build spotty-cross cargo build --release --target arm-unknown-linux-gnueabihf
$ docker run -v /tmp/spotty-build:/build spotty-cross cargo build --release --target aarch64-unknown-linux-gnu
```
Resulting files could be found in /tmp/spotty-build and sub-folders.