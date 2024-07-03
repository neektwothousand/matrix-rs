use std::io::{Read, Write};

use crate::types::{
	login::{LoginRequest, LoginResponse},
	room_events::{SendResponse, SendToDeviceRequest},
	sync::{SyncRequest, SyncResponse},
};

#[derive(Debug, Default, Clone)]
pub struct Api {
	pub homeserver: String,
	pub access_token: Option<String>,
}

fn get_txn_id() -> String {
	let mut buf = String::new();
	if let Ok(mut file) = std::fs::File::open("txn_id") {
		file.read_to_string(&mut buf).unwrap();
		buf = (buf.parse::<u64>().unwrap() + 1).to_string();
		let mut file = std::fs::File::create("txn_id").unwrap();
		file.write_all(buf.as_bytes()).unwrap();
	} else {
		let mut file = std::fs::File::create("txn_id").unwrap();
		buf = 0.to_string();
		file.write_all(buf.as_bytes()).unwrap();
	}
	buf
}

fn get_since() -> Option<String> {
	if let Ok(mut file) = std::fs::File::open("since") {
		let mut buf = String::new();
		file.read_to_string(&mut buf).unwrap();
		Some(buf)
	} else {
		None
	}
}

fn write_since(since: &str) {
	let mut file = std::fs::File::create("since").unwrap();
	file.write_all(since.as_bytes()).unwrap();
}

impl Api {
	pub fn login(&self, body: &LoginRequest) -> LoginResponse {
		let method = format!("https://{}/_matrix/client/v3/login", self.homeserver);
		let login_response: LoginResponse = serde_json::from_str(
			&ureq::post(&method)
				.set("Accept", "application/json")
				.set("Content-Type", "application/json")
				.send_string(&serde_json::to_string(&body).unwrap())
				.unwrap()
				.into_string()
				.unwrap(),
		)
		.unwrap();
		login_response
	}
	pub fn sync(&self, parameter: &SyncRequest) -> Option<SyncResponse> {
		let method = format!("https://{}/_matrix/client/v3/sync", self.homeserver);
		let header = "Authorization";
		let auth_value = format!("Bearer {}", self.access_token.clone().unwrap());
		let mut request = ureq::get(&method).set(header, &auth_value);
		if let Some(since) = &parameter.since {
			request = request.query("since", since);
		} else if let Some(since) = get_since() {
			request = request.query("since", &since);
		}
		if let Some(filter) = &parameter.filter {
			request = request.query("filter", filter);
		}
		let full_state = parameter.full_state.to_string();
		let timeout = parameter.timeout.to_string();
		let pairs = vec![
			("timeout", timeout.as_str()),
			("full_state", full_state.as_str()),
			("set_presence", parameter.set_presence.as_str()),
		];
		request = request.query_pairs(pairs);
		let response_str = request.call().ok()?.into_string().unwrap();
		eprintln!("{}", &response_str);
		let sync_response: SyncResponse = serde_json::from_str(&response_str).unwrap();
		write_since(&sync_response.next_batch);
		Some(sync_response)
	}
	pub fn send(&self, event_type: &str, room_id: &str, body: &str) -> SendResponse {
		let txn_id = get_txn_id();
		let method = format!(
			"https://{}/_matrix/client/v3/rooms/{room_id}/send/{event_type}/{txn_id}",
			self.homeserver
		);
		let header = "Authorization";
		let auth_value = format!("Bearer {}", self.access_token.clone().unwrap());
		let request = ureq::put(&method)
			.set(header, &auth_value)
			.set("Accept", "application/json")
			.set("Content-Type", "application/json");
		let send_response: SendResponse =
			serde_json::from_str(&request.send_string(body).unwrap().into_string().unwrap())
				.unwrap();
		send_response
	}
	pub fn send_to_device(&self, event_type: &str, body: &SendToDeviceRequest) {
		let txn_id = get_txn_id();
		let method = format!(
			"https://{}/_matrix/client/v3/sendToDevice/{event_type}/{txn_id}",
			self.homeserver
		);
		let header = "Authorization";
		let auth_value = format!("Bearer {}", self.access_token.clone().unwrap());
		let request = ureq::put(&method)
			.set(header, &auth_value)
			.set("Accept", "application/json")
			.set("Content-Type", "application/json");
		request
			.send_string(&serde_json::to_string(body).unwrap())
			.unwrap();
	}
}
