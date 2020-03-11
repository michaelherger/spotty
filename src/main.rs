#[cfg(debug_assertions)]
#[macro_use] extern crate log;
#[macro_use]
extern crate serde_json;

use futures::sync::mpsc::UnboundedReceiver;
use futures::{Async, Future, Poll, Stream};
use sha1::{Digest, Sha1};

#[cfg(debug_assertions)]
use std::env;
use std::fs::File;
use std::io::{self, stderr, Write};
use std::mem;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use std::time::Instant;
use tokio_core::reactor::{Handle, Core};
use tokio_io::IoStream;

use librespot::core::authentication::{get_credentials, Credentials};
use librespot::core::cache::Cache;
use librespot::core::config::{ConnectConfig, DeviceType, SessionConfig};
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;

use librespot::connect::discovery::{discovery, DiscoveryStream};
use librespot::connect::spirc::{Spirc, SpircTask};
use librespot::playback::audio_backend::{self};
use librespot::playback::config::{Bitrate, PlayerConfig};
use librespot::playback::mixer::{self, MixerConfig};
use librespot::playback::player::{Player, PlayerEvent};

mod lms;
use lms::LMS;

const VERSION: &'static str = concat!(env!("CARGO_PKG_NAME"), " v", env!("CARGO_PKG_VERSION"));

#[cfg(debug_assertions)]
const DEBUGMODE: bool = true;
#[cfg(not(debug_assertions))]
const DEBUGMODE: bool = false;

#[cfg(target_os="windows")]
const NULLDEVICE: &'static str = "NUL";
#[cfg(not(target_os="windows"))]
const NULLDEVICE: &'static str = "/dev/null";

fn device_id(name: &str) -> String {
    hex::encode(Sha1::digest(name.as_bytes()))
}

fn usage(program: &str, opts: &getopts::Options) -> String {
    println!("{}", VERSION.to_string());

    let brief = format!("Usage: {} [options]", program);
    opts.usage(&brief)
}

#[cfg(debug_assertions)]
fn setup_logging(verbose: bool) {
    let mut builder = env_logger::Builder::new();
    match env::var("RUST_LOG") {
        Ok(config) => {
            builder.parse_filters(&config);
            builder.init();

            if verbose {
                warn!("`--verbose` flag overidden by `RUST_LOG` environment variable");
            }
        }
        Err(_) => {
            if verbose {
                builder.parse_filters("mdns=info,librespot=debug,spotty=info");
            } else {
                builder.parse_filters("mdns=error,librespot=warn,spotty=error");
            }
            builder.init();
        }
    }
}

#[derive(Clone)]
struct Setup {
    cache: Option<Cache>,
    player_config: PlayerConfig,
    session_config: SessionConfig,
    connect_config: ConnectConfig,
    credentials: Option<Credentials>,
    enable_discovery: bool,
    zeroconf_port: u16,

    authenticate: bool,

    get_token: bool,
    save_token: Option<String>,
    client_id: Option<String>,
    scope: Option<String>,

    single_track: Option<String>,
    start_position: u32,
    lms: LMS
}

fn setup(args: &[String]) -> Setup {
    let mut opts = getopts::Options::new();
    opts.optopt(
        "c",
        "cache",
        "Path to a directory where files will be cached.",
        "CACHE",
    ).optflag("", "disable-audio-cache", "(Only here fore compatibility with librespot - audio cache is disabled by default).")
        .optflag("", "enable-audio-cache", "Enable caching of the audio data.")
        .reqopt("n", "name", "Device name", "NAME")
        .optopt(
            "b",
            "bitrate",
            "Bitrate (96, 160 or 320). Defaults to 320.",
            "BITRATE",
        )
        .optflag("v", "verbose", "Enable verbose output")
        .optopt("u", "username", "Username to sign in with", "USERNAME")
        .optopt("p", "password", "Password", "PASSWORD")
        .optopt("", "ap-port", "Connect to AP with specified port. If no AP with that port are present fallback AP will be used. Available ports are usually 80, 443 and 4070", "AP_PORT")
        .optflag("", "disable-discovery", "Disable discovery mode")
        .optopt(
            "",
            "zeroconf-port",
            "The port the internal server advertised over zeroconf uses.",
            "ZEROCONF_PORT",
        )
        .optflag(
            "",
            "enable-volume-normalisation",
            "Play all tracks at the same volume",
        )
        .optflag("", "pass-through", "Pass raw OGG stream to output")
        .optopt("", "player-mac", "MAC address of the Squeezebox to be controlled", "MAC")
        .optopt("", "lms", "hostname and port of Logitech Media Server instance (eg. localhost:9000)", "LMS")
        .optopt("", "lms-auth", "Authentication data to access Logitech Media Server", "LMSAUTH")
        .optopt("", "single-track", "Play a single track ID and exit.", "ID")
        .optopt("", "start-position", "Position (in seconds) where playback should be started. Only valid with the --single-track option.", "STARTPOSITION")
        .optflag("a", "authenticate", "Authenticate given username and password. Make sure you define a cache folder to store credentials.")
        .optflag("t", "get-token", "Get oauth token to be used with the web API etc. and print it to the console.")
        .optopt("T", "save-token", "Get oauth token to be used with the web API etc. and store it in the given file.", "TOKENFILE")
        .optopt("i", "client-id", "A Spotify client_id to be used to get the oauth token. Required with the --get-token request.", "CLIENT_ID")
        .optopt("", "scope", "The scopes you want to have access to with the oauth token.", "SCOPE")
        .optflag("x", "check", "Run quick internal check");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            writeln!(
                stderr(),
                "error: {}\n{}",
                f.to_string(),
                usage(&args[0], &opts)
            )
            .unwrap();
            exit(1);
        }
    };

    if matches.opt_present("check") {
        println!("ok {}", VERSION.to_string());

        let capabilities = json!({
            "version": env!("CARGO_PKG_VERSION").to_string(),
            "lms-auth": true,
            "volume-normalisation": true,
            "debug": DEBUGMODE,
            "ogg-direct": true,
            "save-token": true,
            "podcasts": true,
            "zeroconf-port": true
        });

        println!("{}", capabilities.to_string());
        exit(1);
    }

    #[cfg(debug_assertions)]
    {
        let verbose = matches.opt_present("verbose");
        setup_logging(verbose);
    }

    let use_audio_cache = matches.opt_present("enable-audio-cache") && !matches.opt_present("disable-audio-cache");

    let cache = matches
        .opt_str("c")
        .map(|cache_location| Cache::new(PathBuf::from(cache_location), use_audio_cache));

    let zeroconf_port = matches
        .opt_str("zeroconf-port")
        .map(|port| port.parse::<u16>().unwrap())
        .unwrap_or(0);

    let name = matches.opt_str("name").unwrap();
    let credentials = {
        let cached_credentials = cache.as_ref().and_then(Cache::credentials);

        let password = |username: &String| -> String {
            write!(stderr(), "Password for {}: ", username).unwrap();
            stderr().flush().unwrap();
            rpassword::read_password().unwrap()
        };

        get_credentials(
            matches.opt_str("username"),
            matches.opt_str("password"),
            cached_credentials,
            password,
        )
    };

    let authenticate = matches.opt_present("authenticate");

    let enable_discovery = !matches.opt_present("disable-discovery");

    let start_position = matches.opt_str("start-position")
        .unwrap_or("0".to_string())
        .parse::<f32>().unwrap_or(0.0);

    let session_config = {
        let device_id = device_id(&name);

        SessionConfig {
            user_agent: VERSION.to_string(),
            device_id: device_id,
            proxy: None,
            ap_port: matches
                .opt_str("ap-port")
                .map(|port| port.parse::<u16>().expect("Invalid port")),
        }
    };

    let pass_through = matches.opt_present("pass-through");

    let player_config = {
        let bitrate = matches
            .opt_str("b")
            .as_ref()
            .map(|bitrate| Bitrate::from_str(bitrate).expect("Invalid bitrate"))
            .unwrap_or(Bitrate::Bitrate320);

        PlayerConfig {
            bitrate: bitrate,
            normalisation: matches.opt_present("enable-volume-normalisation"),
            normalisation_pregain: PlayerConfig::default().normalisation_pregain,
            pass_through: pass_through,
            lms_connect_mode: !matches.opt_present("single-track")
        }
    };

    let connect_config = {
        ConnectConfig {
            name: name,
            device_type: DeviceType::Speaker,
            volume: 0x8000 as u16,
            linear_volume: true,
            autoplay: false
        }
    };

    let client_id = matches.opt_str("client-id")
        .unwrap_or(format!("{}", include_str!("client_id.txt")));

    let save_token = matches.opt_str("save-token").unwrap_or("".to_string());

    let lms = LMS::new(matches.opt_str("lms"), matches.opt_str("player-mac"), matches.opt_str("lms-auth"));

    Setup {
        cache: cache,
        session_config: session_config,
        player_config: player_config,
        connect_config: connect_config,
        credentials: credentials,
        authenticate: authenticate,
        enable_discovery: enable_discovery,
        zeroconf_port: zeroconf_port,

        get_token: matches.opt_present("get-token") || save_token.as_str().len() != 0,
        save_token: if save_token.as_str().len() == 0 { None } else { Some(save_token) },

        client_id: if client_id.as_str().len() == 0 { None } else { Some(client_id) },
        scope: matches.opt_str("scope"),

        single_track: matches.opt_str("single-track"),
        start_position: (start_position * 1000.0) as u32,

        lms: lms
    }
}

struct Main {
    cache: Option<Cache>,
    player_config: PlayerConfig,
    session_config: SessionConfig,
    connect_config: ConnectConfig,
    handle: Handle,

    discovery: Option<DiscoveryStream>,
    signal: IoStream<()>,

    spirc: Option<Spirc>,
    spirc_task: Option<SpircTask>,
    connect: Box<dyn Future<Item = Session, Error = io::Error>>,

    shutdown: bool,
    last_credentials: Option<Credentials>,
    auto_connect_times: Vec<Instant>,
    authenticate: bool,

    player_event_channel: Option<UnboundedReceiver<PlayerEvent>>,
    lms: LMS
}

impl Main {
    fn new(handle: Handle, setup: Setup) -> Main {
        let mut task = Main {
            handle: handle.clone(),
            cache: setup.cache,
            session_config: setup.session_config,
            player_config: setup.player_config,
            connect_config: setup.connect_config,

            connect: Box::new(futures::future::empty()),
            discovery: None,
            spirc: None,
            spirc_task: None,
            shutdown: false,
            last_credentials: None,
            auto_connect_times: Vec::new(),
            authenticate: setup.authenticate,
            signal: Box::new(tokio_signal::ctrl_c().flatten_stream()),

            player_event_channel: None,
            lms: setup.lms
        };

        if setup.enable_discovery {
            let config = task.connect_config.clone();
            let device_id = task.session_config.device_id.clone();

            task.discovery =
                Some(discovery(&handle, config, device_id, setup.zeroconf_port).unwrap());
        }

        if let Some(credentials) = setup.credentials {
            task.credentials(credentials);
        }

        task
    }

    fn credentials(&mut self, credentials: Credentials) {
        self.last_credentials = Some(credentials.clone());
        let config = self.session_config.clone();
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

            if let Some(Async::Ready(Some(creds))) =
                self.discovery.as_mut().map(|d| d.poll().unwrap())
            {
                if let Some(ref spirc) = self.spirc {
                    spirc.shutdown();
                }
                self.auto_connect_times.clear();
                self.credentials(creds);

                progress = true;
            }

            if let Async::Ready(ref mut session) = self.connect.poll().unwrap() {
                if self.authenticate {
                    if !self.shutdown {
                        if let Some(ref spirc) = self.spirc {
                            spirc.shutdown();
                        }

                        self.shutdown = true;

                        return Ok(Async::Ready(()));
                    }
                }
                else {
                    self.connect = Box::new(futures::future::empty());
                    let mixer_config = MixerConfig {
                        card: String::from("default"),
                        mixer: String::from("PCM"),
                        index: 0,
                    };

                    let mixer = (mixer::find(Some("softvol")).unwrap())(Some(mixer_config));

                    let player_config = self.player_config.clone();
                    let connect_config = self.connect_config.clone();

                    let audio_filter = mixer.get_audio_filter();
                    let backend = audio_backend::find(None).unwrap();
                    let device = Some(NULLDEVICE.to_string());
                    let (player, event_channel) =
                        Player::new(player_config, session.clone(), audio_filter, move || {
                            (backend)(device)
                        });

                    let (spirc, spirc_task) = Spirc::new(connect_config, session.clone(), player, mixer);
                    self.spirc = Some(spirc);
                    self.spirc_task = Some(spirc_task);
                    self.player_event_channel = Some(event_channel);
                }

                progress = true;
            }

            if let Async::Ready(Some(())) = self.signal.poll().unwrap() {
                if !self.shutdown {
                    if let Some(ref spirc) = self.spirc {
                        spirc.shutdown();
                    } else {
                        return Ok(Async::Ready(()));
                    }
                    self.shutdown = true;
                } else {
                    return Ok(Async::Ready(()));
                }

                progress = true;
            }

            let mut drop_spirc_and_try_to_reconnect = false;
            if let Some(ref mut spirc_task) = self.spirc_task {
                if let Async::Ready(()) = spirc_task.poll().unwrap() {
                    if self.shutdown {
                        return Ok(Async::Ready(()));
                    } else {
#[cfg(debug_assertions)]
                        warn!("Spirc shut down unexpectedly");
                        drop_spirc_and_try_to_reconnect = true;
                    }
                    progress = true;
                }
            }
            if drop_spirc_and_try_to_reconnect {
                self.spirc_task = None;
                while (!self.auto_connect_times.is_empty())
                    && ((Instant::now() - self.auto_connect_times[0]).as_secs() > 600)
                {
                    let _ = self.auto_connect_times.remove(0);
                }

                if let Some(credentials) = self.last_credentials.clone() {
                    if self.auto_connect_times.len() >= 5 {
#[cfg(debug_assertions)]
                        warn!("Spirc shut down too often. Not reconnecting automatically.");
                    } else {
                        self.auto_connect_times.push(Instant::now());
                        self.credentials(credentials);
                    }
                }
            }

            if let Some(ref mut player_event_channel) = self.player_event_channel {
                if let Async::Ready(Some(event)) = player_event_channel.poll().unwrap() {
                    self.lms.signal_event(event, self.handle.clone());
                }
            }

            if !progress {
                return Ok(Async::NotReady);
            }
        }
    }
}

fn main() {
    if std::env::var("RUST_BACKTRACE").is_err() {
#[cfg(debug_assertions)]
        env::set_var("RUST_BACKTRACE", "full")
    }
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let args: Vec<String> = std::env::args().collect();
    let Setup {
        cache,
        session_config,
        player_config,
        connect_config,
        credentials,
        authenticate,
        enable_discovery,
        zeroconf_port,
        get_token,
        save_token,
        client_id,
        scope,
        single_track,
        start_position,
        lms
    } = setup(&args.clone());

    if let Some(ref track_id) = single_track {
        match credentials {
            Some(credentials) => {
                let backend = audio_backend::find(None).unwrap();

                let track = SpotifyId::from_uri(
                                    track_id.replace("spotty://", "spotify:")
                                    .replace("://", ":")
                                    .as_str());

                let session = core.run(Session::connect(session_config.clone(), credentials, cache.clone(), handle)).unwrap();

                let (player, _) = Player::new(player_config, session.clone(), None, move || (backend)(None));

                core.run(player.load(track.unwrap(), true, start_position)).unwrap();
            }
            None => {
                println!("Missing credentials");
            }
        }
    }
    else if authenticate && !enable_discovery {
        core.run(Session::connect(session_config.clone(), credentials.unwrap(), cache.clone(), handle)).unwrap();
        println!("authorized");
    }
    else if get_token {
        if let Some(client_id) = client_id {
            let session = core.run(Session::connect(session_config, credentials.unwrap(), cache.clone(), handle)).unwrap();
            let scope = scope.unwrap_or("user-read-private,playlist-read-private,playlist-read-collaborative,playlist-modify-public,playlist-modify-private,user-follow-modify,user-follow-read,user-library-read,user-library-modify,user-top-read,user-read-recently-played".to_string());
            let url = format!("hm://keymaster/token/authenticated?client_id={}&scope={}", client_id, scope);

            let result = core.run(Box::new(session.mercury().get(url).map(move |response| {
                let data = response.payload.first().expect("Empty payload");
                let token = String::from_utf8(data.clone()).unwrap();

                if let Some(save_token) = save_token {
                    let mut file = File::create(save_token.to_string()).expect("Can't create token file");
                    file.write(&token.clone().into_bytes()).expect("Can't write token file");
                }
                else {
                    println!("{}", token);
                }
            })));

            match result {
                Ok(_) => (),
                Err(e) => println!("error getting token {:?}", e),
            }
        }
        else {
            println!("Use --client-id to provide a CLIENT_ID");
        }
    }
    else {
        core.run(Main::new(handle, Setup {
            cache,
            session_config,
            player_config,
            connect_config,
            credentials,
            authenticate,
            enable_discovery,
            zeroconf_port,
            get_token,
            save_token,
            client_id,
            scope,
            single_track,
            start_position,
            lms
        })).unwrap()
    }
}

