use std::time::Duration;
use std::str::SplitWhitespace;

use matrix_sdk::{
	config::SyncSettings,
	ruma::{events::room::message::SyncRoomMessageEvent, UserId},
	Client,
};
use serde::{Serialize, Deserialize};

use matrix_sdk::room::Room;
use matrix_sdk::room;
use matrix_sdk::ruma::events::{room::message::*, *};

use regex::Regex;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use async_minecraft_ping::ConnectionConfig;

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
}
#[derive(Serialize, Deserialize)]
struct McServer {
	room_id: String,
	server: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let user: User = serde_yaml::from_reader(std::fs::File::open("deal.yaml").unwrap()).unwrap();
	let user_id = UserId::parse(&user.name).unwrap();
	let client = Client::builder()
		.sqlite_store("deal_sqlite_store", None)
		.server_name(user_id.server_name())
		.build()
		.await?;

	// First we need to log in.
	let login_builder = client.login_username(&user_id, &user.password);

	let deal_device_id_file_str = "deal_device_id";
	if let Ok(mut f) = File::open(deal_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await?;

		login_builder.device_id(&device_id_str).send().await?;
	} else {
		let response = login_builder.send().await?;
		let mut f = File::create(deal_device_id_file_str).await?;
		f.write_all(response.device_id.as_bytes()).await?;
	}

	client.add_event_handler(handle_message_event);

	let enc = client.encryption();
	println!("{:#?}", enc.cross_signing_status().await);

	// Syncing is important to synchronize the client state with the server.
	// This method will never return.
	client.sync(SyncSettings::default()).await?;

	Ok(())
}

async fn match_command(command: &str, joined_room: &room::Joined, mut args: SplitWhitespace<'_>) -> Option<String> {
	match command {
		"!mccheck" => {
			let addr: String = if let Some(addr) = args.next() { addr.to_string() } else {
				let file_name = "mclist.json";
				let file_read = std::fs::File::open(file_name).unwrap();
				let mc_list: Vec<McServer> = match serde_json::from_reader(&file_read) {
					Ok(mc_list) => mc_list,
					Err(e) => {
						eprintln!("{:?}", e);
						return None;
					}
				};
				let Some(mc_server) = mc_list.iter().find(|mc| mc.room_id == joined_room.room_id().as_str()) else {
					return None;
				};
				mc_server.server.clone()
			};
			let reg = Regex::new(r#"^(.*?)(?::(\d+))?$"#).unwrap();
			let Some(captures) = reg.captures(&addr) else {
				return Some("invalid ip or port".to_string());
			};
			let addr = captures.get(1).unwrap().as_str();
			let port = captures
				.get(2)
				.and_then(|x| x.as_str().parse::<u16>().ok())
				.unwrap_or(25565);

			let config = ConnectionConfig::build(addr)
				.with_timeout(Duration::from_secs(1))
				.with_port(port);

			match config.connect().await {
				Ok(connection) => {
					match connection.status().await {
						Ok(status) => {
							return Some(format!(
								"Server online: {}/{} players",
								status.status.players.online,
								status.status.players.max
							));
						}
						Err(err) => {
							return Some(err.to_string());
						}
					}
				}
				Err(err) => {
					return Some(err.to_string());
				}
			}
		}
		"!mcset" => {
			let Some(addr) = args.next() else {
				return None;
			};
			let reg = Regex::new(r#"^(.*?)(?::(\d+))?$"#).unwrap();
			if let None = reg.captures(addr) {
				return Some("invalid ip or port".to_string());
			}
			let file_name = "mclist.json";
			let file_read = std::fs::File::open(file_name);
			let mut mc_list: Vec<McServer> = match file_read {
				Ok(file) => {
					serde_json::from_reader(&file).unwrap()
				}
				Err(_) => vec![],
			};
			for (x, _) in mc_list.iter().enumerate() {
				if mc_list[x].room_id == joined_room.room_id().as_str() {
					mc_list[x].server = addr.to_string();
					let mut file_truncate = std::fs::File::create(file_name).unwrap();
					serde_json::to_writer(&mut file_truncate, &mc_list).unwrap();
					return Some(format!("server {} set", addr));
				}
			}
			let mc = McServer {
				room_id: joined_room.room_id().as_str().to_string(),
				server: addr.to_string(),
			};
			mc_list.push(mc);
			let mut file_truncate = std::fs::File::create(file_name).unwrap();
			serde_json::to_writer(&mut file_truncate, &mc_list).unwrap();
			return Some(format!("server {} set", addr));
		}
		_ => return None,
	}
}

async fn handle_message_event(
	ev: SyncRoomMessageEvent,
	r: Room,
	_client: Client,
) {
	let (text, joined_room) = {
		if let SyncMessageLikeEvent::Original(o) = ev {
			if let (MessageType::Text(t), Room::Joined(joined_room)) = (o.content.msgtype, r) {
				(t.body, joined_room)
			} else {
				return;
			}
		} else {
			return;
		}
	};
	let mut args = text.split_whitespace();
	let command = args.next().unwrap();
	if let Some(send_text_plain) = match_command(command, &joined_room, args).await {
		let _ = joined_room
			.send(RoomMessageEventContent::text_plain(send_text_plain), None)
			.await;
	}
}
