// TODO: many items from tokio-core::io have been deprecated in favour of tokio-io
#![allow(deprecated)]

#[macro_use] extern crate log;
#[cfg(debug_assertions)]
extern crate env_logger;
extern crate futures;
extern crate getopts;
extern crate librespot;
extern crate tokio_core;
extern crate tokio_signal;

#[cfg(debug_assertions)]
use env_logger::LogBuilder;
use futures::{Future, Async, Poll, Stream};
#[cfg(debug_assertions)]
use std::env;
use std::io::{self, stderr, Write};
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use tokio_core::reactor::{Handle, Core};
use tokio_core::io::IoStream;
use std::mem;

use librespot::spirc::{Spirc, SpircTask};
use librespot::authentication::{get_credentials, Credentials};
use librespot::authentication::discovery::{discovery, DiscoveryStream};
use librespot::audio_backend;
use librespot::cache::Cache;
use librespot::player::Player;
use librespot::session::{Bitrate, Config, Session};
use librespot::mixer::{self, Mixer};
use librespot::util::SpotifyId;

use librespot::version;

fn usage(program: &str, opts: &getopts::Options) -> String {
    let brief = format!("Usage: {} [options]", program);
    opts.usage(&brief)
}

#[cfg(debug_assertions)]
fn setup_logging(verbose: bool) {
    let mut builder = LogBuilder::new();
    match env::var("RUST_LOG") {
        Ok(config) => {
            builder.parse(&config);
            builder.init().unwrap();

            if verbose {
                warn!("`--verbose` flag overidden by `RUST_LOG` environment variable");
            }
        }
        Err(_) => {
            if verbose {
                builder.parse("mdns=info,librespot=trace");
            } else {
                builder.parse("mdns=error,librespot=warn");
            }
            builder.init().unwrap();
        }
    }
}

#[derive(Clone)]
struct Setup {
    mixer: fn() -> Box<Mixer>,

	name: String,
    cache: Option<Cache>,
    config: Config,
    credentials: Option<Credentials>,
    enable_discovery: bool,

    single_track: Option<String>,
    start_position: u32,
}

fn setup(args: &[String]) -> Setup {
    let mut opts = getopts::Options::new();
    opts.optopt("c", "cache", "Path to a directory where files will be cached.", "CACHE")
        .reqopt("n", "name", "Device name", "NAME")
        .optopt("b", "bitrate", "Bitrate (96, 160 or 320). Defaults to 160", "BITRATE")
        .optopt("", "onstart", "Run PROGRAM when playback is about to begin.", "PROGRAM")
        .optopt("", "onstop", "Run PROGRAM when playback has ended.", "PROGRAM")
        .optopt("", "single-track", "Play a single track ID and exit.", "ID")
        .optopt("", "start-position", "Position (in ms) where playback should be started. Only valid with the --single-track option.", "STARTPOSITION")
        .optopt("u", "username", "Username to sign in with", "USERNAME")
        .optopt("p", "password", "Password", "PASSWORD")
        .optflag("", "disable-discovery", "Disable discovery mode")
        .optopt("", "mixer", "Mixer to use", "MIXER");

	#[cfg(debug_assertions)]
    opts.optflag("v", "verbose", "Enable verbose output");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            writeln!(stderr(), "error: {}\n{}", f.to_string(), usage(&args[0], &opts)).unwrap();
            exit(1);
        }
    };

	#[cfg(debug_assertions)]
	{
	    let verbose = matches.opt_present("verbose");
	    setup_logging(verbose);
	}

    info!("librespot {} ({}). Built on {}.",
             version::short_sha(),
             version::commit_date(),
             version::short_now());

    let mixer_name = matches.opt_str("mixer");
    let mixer = mixer::find(mixer_name.as_ref())
        .expect("Invalid mixer");

    let bitrate = matches.opt_str("b").as_ref()
        .map(|bitrate| Bitrate::from_str(bitrate).expect("Invalid bitrate"))
        .unwrap_or(Bitrate::Bitrate160);

    let name = matches.opt_str("name").unwrap();
    let device_id = librespot::session::device_id(&name);

    let cache = matches.opt_str("c").map(|cache_location| {
        Cache::new(PathBuf::from(cache_location))
    });

    let cached_credentials = cache.as_ref().and_then(Cache::credentials);

    let credentials = get_credentials(matches.opt_str("username"),
                                      matches.opt_str("password"),
                                      cached_credentials);

    let enable_discovery = !matches.opt_present("disable-discovery");
    
    let start_position = matches.opt_str("start-position")
    	.unwrap_or("0".to_string())
    	.parse().unwrap_or(0);
    
    let config = Config {
        user_agent: version::version_string(),
        device_id: device_id,
        bitrate: bitrate,
        onstart: matches.opt_str("onstart"),
        onstop: matches.opt_str("onstop"),
    };

    Setup {
		name: name,
        cache: cache,
        config: config,
        credentials: credentials,
        enable_discovery: enable_discovery,
        mixer: mixer,

        single_track: matches.opt_str("single-track"),
        start_position: start_position * 1000,
    }
}

struct Main {
	name: String,
    cache: Option<Cache>,
    config: Config,
    mixer: fn() -> Box<Mixer>,
    handle: Handle,

    discovery: Option<DiscoveryStream>,
    signal: IoStream<()>,

    spirc: Option<Spirc>,
    spirc_task: Option<SpircTask>,
    connect: Box<Future<Item=Session, Error=io::Error>>,

    player: Option<Player>,

    single_track: Option<String>,
    start_position: u32,
    single_track_playing: Option<futures::sync::oneshot::Receiver<()>>,

    shutdown: bool,
}

impl Main {
    fn new(handle: Handle,
           name: String,
           config: Config,
           cache: Option<Cache>,
           mixer: fn() -> Box<Mixer>,
           single_track: Option<String>,
           start_position: u32,
    ) -> Main
    {
        Main {
            handle: handle.clone(),
            name: name,
            cache: cache,
            config: config,
            mixer: mixer,

            connect: Box::new(futures::future::empty()),
            discovery: None,
            spirc: None,
            spirc_task: None,

            player: None,
            single_track: single_track,
            start_position: start_position,
            single_track_playing: None,
            
            shutdown: false,
            signal: tokio_signal::ctrl_c(&handle).flatten_stream().boxed(),
        }
    }

    fn discovery(&mut self) {
        let device_id = self.config.device_id.clone();
        let name = self.name.clone();

        self.discovery = Some(discovery(&self.handle, name, device_id).unwrap());
    }

    fn credentials(&mut self, credentials: Credentials) {
        let config = self.config.clone();
        let handle = self.handle.clone();

        let connection = Session::connect(config, credentials, self.cache.clone(), handle);

        self.connect = connection;
        self.spirc = None;
        let task = mem::replace(&mut self.spirc_task, None);
        if let Some(task) = task {
            self.handle.spawn(task);
        }
    }
}

impl Future for Main {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        loop {
            let mut progress = false;

            if let Some(Async::Ready(Some(creds))) = self.discovery.as_mut().map(|d| d.poll().unwrap()) {
                if let Some(ref spirc) = self.spirc {
                    spirc.shutdown();
                }
                self.credentials(creds);

                progress = true;
            }

            if let Async::Ready(session) = self.connect.poll().unwrap() {
                self.connect = Box::new(futures::future::empty());
                let mixer = (self.mixer)();

                let audio_filter = mixer.get_audio_filter();
				let backend = audio_backend::find(None).unwrap();
                let player = Player::new(session.clone(), audio_filter, move || {
                    (backend)(None)
                });

                self.player = Some(player.clone());

                let (spirc, spirc_task) = Spirc::new(self.name.clone(), session, player, mixer);
                self.spirc = Some(spirc);
                self.spirc_task = Some(spirc_task);

                progress = true;
            }

            if let Async::Ready(Some(())) = self.signal.poll().unwrap() {
                if !self.shutdown {
                    if let Some(ref spirc) = self.spirc {
                        spirc.shutdown();
                    }
                    self.shutdown = true;
                } else {
                    return Ok(Async::Ready(()));
                }

                progress = true;
            }

            if let Some(ref mut spirc_task) = self.spirc_task {
                if let Async::Ready(()) = spirc_task.poll().unwrap() {
                    if self.shutdown {
                        return Ok(Async::Ready(()));
                    } else {
                        panic!("Spirc shut down unexpectedly");
                    }
                }
            }
            
            if let Some(ref mut player) = self.player {
				if let Some(ref track_id) = self.single_track {
					if self.single_track_playing.is_none() {
						self.single_track_playing = Some(player.load( SpotifyId::from_base62(
							track_id.replace("spotty://", "")
							.replace("spotify://", "")
							.replace("track:", "")
							.as_str()
						), true, self.start_position ));
									
						self.single_track_playing.poll().unwrap();
						progress = true;
					}
					else {
						let result = self.single_track_playing.poll();
										
						if result.is_err() {
							if !self.shutdown {
								if let Some(ref spirc) = self.spirc {
									spirc.shutdown();
								}
								self.shutdown = true;
							} else {
								return Ok(Async::Ready(()));
							}

							progress = true;
						}
						else {
							if let Async::Ready(Some(())) = result.unwrap() {
								progress = true;
							}
						}
					}
				}
            }

            if !progress {
                return Ok(Async::NotReady);
            }
        }
    }
}

fn main() {
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let args: Vec<String> = std::env::args().collect();
    let Setup { name, config, cache, enable_discovery, credentials, mixer, single_track, start_position } = setup(&args);

    let mut task = Main::new(handle, name, config, cache, mixer, single_track, start_position);
    if enable_discovery {
        task.discovery();
    }
    if let Some(credentials) = credentials {
        task.credentials(credentials);
    }

    core.run(task).unwrap()
}

