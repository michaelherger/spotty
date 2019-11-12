extern crate futures;
extern crate hyper;
extern crate tokio_core;

use std::str::FromStr;
use tokio_core::reactor::{Handle};

use futures::Future;
use hyper::{Method, Request, Uri, Client};
use hyper::header::{Authorization, ContentLength, ContentType};

use librespot::playback::player::PlayerEvent;

#[derive(Clone)]
pub struct LMS {
	base_url: Option<String>,
	player_mac: Option<String>,
	auth: Option<String>
}

#[allow(unused)]
impl LMS {
	pub fn new(base_url: Option<String>, player_mac: Option<String>, auth: Option<String>) -> LMS {
		LMS {
			base_url: Some(format!("http://{}/jsonrpc.js", base_url.unwrap_or("localhost:9000".to_string()))),
			player_mac: player_mac,
			auth: auth
		}
	}

	pub fn is_configured(&self) -> bool {
		if self.base_url != None {
			if self.player_mac != None {
				return true;
			}
		}

		return false;
	}

	pub fn signal_event(&self, event: PlayerEvent, handle: Handle) {
		let mut command = r#"["spottyconnect","change"]"#.to_string();

		match event {
			PlayerEvent::Changed {
				old_track_id,
				new_track_id,
			} => {
				#[cfg(debug_assertions)]
				info!("change: spotify:track:{} -> spotify:track:{}", old_track_id.to_base62(), new_track_id.to_base62());
				command = format!(r#"["spottyconnect","change","{}","{}"]"#, new_track_id.to_base62().to_string(), old_track_id.to_base62().to_string());
			}
			PlayerEvent::Started { track_id } => {
				#[cfg(debug_assertions)]
				info!("play spotify:track:{}", track_id.to_base62());
				command = format!(r#"["spottyconnect","start","{}"]"#, track_id.to_base62().to_string());
			}
			PlayerEvent::Stopped { track_id } => {
				#[cfg(debug_assertions)]
				info!("stop spotify:track:{}", track_id.to_base62());
				command = r#"["spottyconnect","stop"]"#.to_string();
			}
			PlayerEvent::Volume { volume } => {
				#[cfg(debug_assertions)]
				info!("volume {}", volume);
				// we're not using the volume here, as LMS will read player state anyway
				command = format!(r#"["spottyconnect","volume",{}]"#, volume.to_string());
			}
			PlayerEvent::Seek { position } => {
				#[cfg(debug_assertions)]
				info!("seek {}", position);
				// we're not implementing the seek event here, as it's going to read player state anyway
				command = r#"["spottyconnect","change"]"#.to_string();
			}
		}

		if !self.is_configured() {
			#[cfg(debug_assertions)]
			info!("LMS connection is not configured");
			return;
		}

		#[cfg(debug_assertions)]
		info!("Base URL to talk to LMS: {}", self.base_url.clone().unwrap());

		if let Some(ref base_url) = self.base_url {
			#[cfg(debug_assertions)]
			info!("Player MAC address to control: {}", self.player_mac.clone().unwrap());
			if let Some(ref player_mac) = self.player_mac {

				let client = Client::new(&handle);

				#[cfg(debug_assertions)]
				info!("Command to send to player: {}", command);

				let json = format!(r#"{{"id": 1,"method":"slim.request","params":["{}",{}]}}"#, player_mac, command);
				let uri = Uri::from_str(base_url).unwrap();
				let mut req = Request::new(Method::Post, uri);

				if let Some(ref auth) = self.auth {
					req.headers_mut().set(Authorization(format!("Basic {}", auth).to_owned()));
				}

				req.headers_mut().set_raw("X-Scanner", "1");
				req.headers_mut().set(ContentType::json());
				req.headers_mut().set(ContentLength(json.len() as u64));
				req.set_body(json);

				// ugh... just send that thing and don't care about the rest...
				let post = client.request(req).map(|_| ()).map_err(|_| ());
				handle.spawn(post);
			}
		}
	}
}
