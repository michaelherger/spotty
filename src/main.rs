// TODO: many items from tokio-core::io have been deprecated in favour of tokio-io
#![allow(deprecated)]

#[cfg(debug_assertions)]
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
use tokio_core::reactor::{Handle, Core};
use tokio_core::io::IoStream;
use std::mem;

use librespot::spirc::{Spirc, SpircTask};
use librespot::authentication::{get_credentials, Credentials};
#[cfg(not(target_os="windows"))]
use librespot::authentication::discovery::{discovery, DiscoveryStream};
use librespot::audio_backend;
use librespot::cache::Cache;
use librespot::player::Player;
use librespot::session::{Bitrate, Config, Session};
use librespot::mixer;
use librespot::util::SpotifyId;

const VERSION: &'static str = concat!(env!("CARGO_PKG_NAME"), " v", env!("CARGO_PKG_VERSION")); 

fn usage(program: &str, opts: &getopts::Options) -> String {
	println!("{}", VERSION.to_string());

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
	name: String,
	cache: Option<Cache>,
	config: Config,
	credentials: Option<Credentials>,
	authenticate: bool,
	enable_discovery: bool,
	
	get_token: bool,
	client_id: Option<String>,
	scope: Option<String>,

	single_track: Option<String>,
	start_position: u32,
}

fn setup(args: &[String]) -> Setup {
	let mut opts = getopts::Options::new();
	opts.optopt("c", "cache", "Path to a directory where files will be cached.", "CACHE")
		.optflag("", "enable-audio-cache", "Enable caching of the audio data.")
		.optflag("", "disable-audio-cache", "(Only here fore compatibility with librespot - audio cache is disabled by default).")
		.reqopt("n", "name", "Device name", "NAME")
		.optopt("", "onstart", "Run PROGRAM when playback is about to begin.", "PROGRAM")
		.optopt("", "onstop", "Run PROGRAM when playback has ended.", "PROGRAM")
		.optopt("", "onchange", "Run PROGRAM when playback changes (new track, seeking etc.).", "PROGRAM")
		.optopt("", "single-track", "Play a single track ID and exit.", "ID")
		.optopt("", "start-position", "Position (in ms) where playback should be started. Only valid with the --single-track option.", "STARTPOSITION")
		.optopt("u", "username", "Username to sign in with", "USERNAME")
		.optopt("p", "password", "Password", "PASSWORD")
		.optflag("a", "authenticate", "Authenticate given username and password. Make sure you define a cache folder to store credentials.")
		.optflag("", "disable-discovery", "Disable discovery mode")
		.optflag("t", "get-token", "Get oauth token to be used with the web API etc.")
		.optopt("i", "client-id", "A Spotify client_id to be used to get the oauth token. Required with the --get-token request.", "CLIENT_ID")
		.optopt("", "scope", "The scopes you want to have access to with the oauth token.", "SCOPE")
		.optflag("x", "check", "Run quick internal check");

	#[cfg(debug_assertions)]
	opts.optflag("v", "verbose", "Enable verbose output");

	let matches = match opts.parse(&args[1..]) {
		Ok(m) => m,
		Err(f) => {
			writeln!(stderr(), "error: {}\n{}", f.to_string(), usage(&args[0], &opts)).unwrap();
			exit(1);
		}
	};
	
	if matches.opt_present("check") {
		println!("ok {}", VERSION.to_string());
		exit(1);
	}

	#[cfg(debug_assertions)]
	{
		let verbose = matches.opt_present("verbose");
		setup_logging(verbose);
	}

	let name = matches.opt_str("name").unwrap();
	let device_id = librespot::session::device_id(&name);
	
	let use_audio_cache = matches.opt_present("enable-audio-cache") && !matches.opt_present("disable-audio-cache");

	let cache = matches.opt_str("c").map(|cache_location| {
		Cache::new(PathBuf::from(cache_location), use_audio_cache)
	});

	let cached_credentials = cache.as_ref().and_then(Cache::credentials);

	let credentials = get_credentials(matches.opt_str("username"), matches.opt_str("password"), cached_credentials);

	let authenticate = matches.opt_present("authenticate");

#[cfg(not(target_os="windows"))]
	let enable_discovery = !matches.opt_present("disable-discovery");
#[cfg(target_os="windows")]
	let enable_discovery = false;
	
	let start_position = matches.opt_str("start-position")
		.unwrap_or("0".to_string())
		.parse().unwrap_or(0.0);
		
	let config = Config {
		user_agent: VERSION.to_string(),
		device_id: device_id,
		bitrate: Bitrate::Bitrate320,
		onstart: matches.opt_str("onstart"),
		onstop: matches.opt_str("onstop"),
		onchange: matches.opt_str("onchange"),
	};

	Setup {
		name: name,
		cache: cache,
		config: config,
		credentials: credentials,
		authenticate: authenticate,
		enable_discovery: enable_discovery,
		
		get_token: matches.opt_present("get-token"),
		client_id: matches.opt_str("client-id"),
		scope: matches.opt_str("scope"),

		single_track: matches.opt_str("single-track"),
		start_position: (start_position * 1000.0) as u32,
	}
}

struct Main {
	name: String,
	cache: Option<Cache>,
	config: Config,
	handle: Handle,

#[cfg(not(target_os="windows"))]
	discovery: Option<DiscoveryStream>,
#[cfg(target_os="windows")]
	discovery: Option<String>,
	signal: IoStream<()>,

	spirc: Option<Spirc>,
	spirc_task: Option<SpircTask>,
	connect: Box<Future<Item=Session, Error=io::Error>>,

	player: Option<Player>,

	shutdown: bool,
	authenticate: bool,
}

impl Main {
	fn new(
		handle: Handle,
		name: String,
		config: Config,
		cache: Option<Cache>,
		authenticate: bool,
	) -> Main
	{
		Main {
			handle: handle.clone(),
			name: name,
			cache: cache,
			config: config,

			connect: Box::new(futures::future::empty()),
			discovery: None,
			spirc: None,
			spirc_task: None,

			player: None,
			
			shutdown: false,
			authenticate: authenticate,
			signal: tokio_signal::ctrl_c(&handle).flatten_stream().boxed(),
		}
	}

#[cfg(not(target_os="windows"))]
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

#[cfg(not(target_os="windows"))] {
			if let Some(Async::Ready(Some(creds))) = self.discovery.as_mut().map(|d| d.poll().unwrap()) {
				if let Some(ref spirc) = self.spirc {
					spirc.shutdown();
				}
				self.credentials(creds);

				progress = true;
			}
}

			if let Async::Ready(session) = self.connect.poll().unwrap() {
				if self.authenticate {
					if !self.shutdown {
						if let Some(ref spirc) = self.spirc {
							spirc.shutdown();
						}
						self.shutdown = true;
					} else {
						return Ok(Async::Ready(()));
					}
				}
				else {
					self.connect = Box::new(futures::future::empty());

					let mixer = (mixer::find(Some("softvol")).unwrap())();

					let audio_filter = mixer.get_audio_filter();
					let backend = audio_backend::find(None).unwrap();
					let player = Player::new(session.clone(), audio_filter, move || {
						(backend)(None)
					});

					self.player = Some(player.clone());

					let (spirc, spirc_task) = Spirc::new(self.name.clone(), session, player, mixer);
					self.spirc = Some(spirc);
					self.spirc_task = Some(spirc_task);
				}

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
	let Setup { name, config, cache, enable_discovery, credentials, authenticate, get_token, client_id, scope, single_track, start_position } = setup(&args);

	if let Some(ref track_id) = single_track {
		match credentials {
			Some(credentials) => {
				let backend = audio_backend::find(None).unwrap();

				let track = SpotifyId::from_base62(
									track_id.replace("spotty://", "")
									.replace("spotify://", "")
									.replace("spotify:", "")
									.replace("track:", "")
									.as_str());
							
				let session = core.run(Session::connect(config, credentials, cache.clone(), handle)).unwrap();

				let player = Player::new(session.clone(), None, move || (backend)(None));

				core.run(player.load(track, true, start_position)).unwrap();
			} 
			None => {
				println!("Missing credentials");
			}
		}
	}
	else if authenticate && !enable_discovery {
		core.run(Session::connect(config, credentials.unwrap(), cache.clone(), handle)).unwrap();
		println!("authorized");
	}
	else if get_token {
		if let Some(client_id) = client_id {
			let session = core.run(Session::connect(config, credentials.unwrap(), cache.clone(), handle)).unwrap();
			let scope = scope.unwrap_or("user-read-private,playlist-read-private,playlist-read-collaborative,playlist-modify-public,playlist-modify-private,user-follow-modify,user-follow-read,user-library-read,user-library-modify,user-top-read,user-read-recently-played".to_string());
			let url = format!("hm://keymaster/token/authenticated?client_id={}&scope={}", client_id, scope);
			core.run(session.mercury().get(url).map(move |response| {
				let data = response.payload.first().expect("Empty payload");
				let token = String::from_utf8(data.clone()).unwrap();
				println!("{}", token);
			}).boxed()).unwrap();
		}
		else {
			println!("Use --client-id to provide a CLIENT_ID");
		}
	}
	else {
		let mut task = Main::new(handle, name, config, cache, authenticate);
		if enable_discovery {
#[cfg(not(target_os="windows"))]
			task.discovery();
		}
		if let Some(credentials) = credentials {
			task.credentials(credentials);
		}

		core.run(task).unwrap()
	}
}

