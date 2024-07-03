use dana_zane_no_sdk::{
	types::{
		login::{Identifier, LoginRequest},
		room_events::{RequestKeyInfo, RoomKeyRequest, ToDeviceEvent},
		sync::{ClientEventWithoutRoomId, SyncRequest, SyncResponse},
	},
	utils::{api::Api, megolm::get_megolm_session},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
	collections::HashMap,
	fs::{read_to_string, File},
	io::{Read, Write},
};
use vodozemac::megolm::{GroupSession, InboundGroupSession, MegolmMessage, SessionConfig};

#[derive(Deserialize, Clone)]
struct User {
	full_name: String,
	name: String,
	password: String,
	homeserver: String,
}

fn get_room_key_request(api: &Api, user: &User, room_id: &str) {
	let session_id = GroupSession::new(SessionConfig::version_2()).session_id();
	let request_key_info = RequestKeyInfo {
		algorithm: "m.megolm.v1.aes-sha2".to_string(),
		room_id: room_id.to_string(),
		sender_key: None,
		session_id,
	};

	use rand::distributions::{Alphanumeric, DistString};
	let request_id = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
	let device_id = read_to_string("device_id").unwrap();
	let room_key_request = RoomKeyRequest {
		action: "request".to_string(),
		body: Some(request_key_info),
		request_id,
		requesting_device_id: device_id.clone(),
	};

	let event_type = "m.room_key_request";
	let to_device_event = ToDeviceEvent {
		content: room_key_request,
		r#type: event_type.to_string(),
	};
	let mut device_map = HashMap::new();
	device_map.insert(device_id, to_device_event).unwrap();
	let mut messages_map = HashMap::new();
	messages_map
		.insert(user.full_name.clone(), device_map)
		.unwrap();
	use dana_zane_no_sdk::types::room_events::SendToDeviceRequest;
	let send_to_device_request = SendToDeviceRequest {
		messages: messages_map,
	};
	api.send_to_device(event_type, &send_to_device_request);
}

fn match_message(api: &Api, room_id: &str, message: &str) {
	match message {
		"!ping" => {
			let event_type = "m.room.message";
			let body = serde_json::json!({"body": "pong", "msgtype": "m.text"}).to_string();
			api.send(event_type, room_id, &body);
		}
		_ => (),
	}
}

pub struct EventType {
	room_encrypted: String,
	room_key: String,
	room_message: String,
	room_key_request: String,
}

impl Default for EventType {
	fn default() -> Self {
		EventType {
			room_key: "m.room.key".to_string(),
			room_key_request: "m.room.key_request".to_string(),
			room_message: "m.room.message".to_string(),
			room_encrypted: "m.room.encrypted".to_string(),
		}
	}
}

fn try_decrypt(
	api: &Api,
	user: &User,
	room_id: &str,
	event: &ClientEventWithoutRoomId,
) -> Option<()> {
	let event_type = event.content.get("type").unwrap();
	#[derive(Serialize)]
	struct EvType {
		room_key: String,
		room_key_request: String,
	}
	impl DeserializeOwned for EvType {}
	let event_type: EvType = serde_json::from_value(*event_type).unwrap();
	let evnt_type = match event_type {
		EvType{ room_key, .. } => room_key,
		EvType{ room_key_request, .. } => room_key_request,
	};
	
	let Some(alg) = event.content.get("algorithm") else {
		let body = event.content.get("body").unwrap();
		let message = body.as_str().unwrap();
		match_message(&api, room_id, message);
		return Some(());
	};
	if event_type == "m.room.encrypted" {
		let alg = alg.as_str().unwrap();
		if alg == "m.megolm.v1.aes-sha2" && event_type == "m.room.encrypted" {
			todo!()
		}
	}
	let alg = alg.as_str().unwrap();
	if alg == "m.megolm.v1.aes-sha2" && event_type == "m.room.encrypted" {
		let ciphertext = event.content.get("ciphertext").unwrap().as_str().unwrap();
		let meg_message = MegolmMessage::from_base64(ciphertext).unwrap();
		eprintln!("{:?}", meg_message);
		let megolm_session = get_megolm_session(room_id);

		let Some(megolm_session) = megolm_session else {
			room_key_request(&api, &user, room_id);
			return None;
		};
		let mut inbound_group_session =
			InboundGroupSession::from_pickle(megolm_session.inbound_group_session);
		let decrypted = inbound_group_session.decrypt(&meg_message);
		match decrypted {
			Ok(message) => {
				let message_str = String::from_utf8_lossy(&message.plaintext).to_string();
				match_message(&api, room_id, &message_str);
			}
			Err(err) => eprintln!("{:?}", err),
		}
		return Some(());
	} else if alg == "m.megolm.v1.aes-sha2" && event_type == "m.room.key" {
	}
	Some(())
}

fn iter_timeline(api: Api, user: User, sync_response: SyncResponse) {
	eprintln!("{:#?}", sync_response.to_device);
	let rooms = match &sync_response.rooms {
		Some(rooms) => rooms,
		None => return,
	};

	let joinmap = match &rooms.join {
		Some(joinmap) => joinmap,
		None => return,
	};

	for (room_id, room_data) in joinmap.iter() {
		let timeline = match &room_data.timeline {
			Some(timeline) => timeline,
			None => return,
		};

		for event in timeline.events.iter() {
			if event.sender == user.name {
				return;
			}
			try_decrypt(&api, &user, room_id, event);
		}
	}
}

fn polling(api: Api, user: User) {
	let mut sync_request = SyncRequest {
		set_presence: "online".to_string(),
		timeout: 300000,
		..Default::default()
	};
	loop {
		let Some(sync_response) = api.sync(&sync_request) else {
			continue;
		};
		sync_request.since = Some(sync_response.next_batch.clone());

		let api_clone = api.clone();
		let user_clone = user.clone();
		std::thread::spawn(move || {
			iter_timeline(api_clone, user_clone, sync_response);
		});
	}
}

fn main() {
	let pass = std::env::var("PASSWORD").unwrap();
	let full_userid = std::env::var("USERID").unwrap();
	let (userid, homeserver) = full_userid.split_once(':').unwrap();
	let user = User {
		full_name: full_userid.clone(),
		name: userid.to_string(),
		password: pass.clone(),
		homeserver: homeserver.to_string(),
	};

	let mut login_request = LoginRequest {
		identifier: Identifier {
			r#type: "m.id.user".to_string(),
			user: full_userid.clone(),
		},
		r#type: "m.login.password".to_string(),
		password: user.password.clone(),
		..Default::default()
	};
	if let Ok(mut device_id_file) = File::open("device_id") {
		let mut device_id_str = String::new();
		device_id_file.read_to_string(&mut device_id_str).unwrap();
		login_request.device_id = Some(device_id_str);
	}

	let mut api = Api {
		homeserver: user.homeserver.clone(),
		..Default::default()
	};
	let login_response = api.login(&login_request);
	api.access_token = Some(login_response.access_token.clone());
	println!("{:?}\n{:?}", &api, &login_response);
	let mut device_id_file = File::create("device_id").unwrap();
	device_id_file
		.write_all(login_response.device_id.as_bytes())
		.unwrap();
	polling(api, user);
}
