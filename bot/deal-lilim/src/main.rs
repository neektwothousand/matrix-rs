use anyhow::Context;
use interactive::commands::{match_command, match_text};
use matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent;
use matrix_sdk::ruma::events::room::message::{MessageType, SyncRoomMessageEvent};
use matrix_sdk::ruma::events::SyncMessageLikeEvent;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::{config::SyncSettings, ruma, Client, Room};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::io::Write;

#[derive(Serialize, Deserialize)]
struct User {
	name: String,
	password: String,
	room_id: String,
	anilist_ids: Vec<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	env_logger::Builder::from_default_env()
		.format(|buf, record| {
			writeln!(
				buf,
				"{} - {}:{} - [{}] {}",
				chrono::offset::Local::now().format("%Y-%m-%dT%H:%M:%S"),
				record.file().unwrap_or("unknown"),
				record.line().unwrap_or(0),
				record.level(),
				record.args()
			)
		})
		.init();

	let user: User = serde_json::from_reader(std::fs::File::open("bot_login_data.json")?)?;

	let u = ruma::UserId::parse(&user.name)?;
	let client = Arc::new(
		Client::builder()
			.sqlite_store("matrix_bot_sqlite", None)
			.server_name(u.server_name())
			.build()
			.await?,
	);

	let login_builder = client.matrix_auth().login_username(u, &user.password);

	utils::matrix::read_or_create_device_id(login_builder).await?;
	client.sync_once(SyncSettings::default()).await?;
	let room_id = RoomId::parse(user.room_id)?;
	let room = Arc::new(client.get_room(&room_id).context("room not found")?);

	let anilist_room = room.clone();
	tokio::spawn(async move {
		loop {
			let res = tokio::spawn(utils::anilist::anilist_update(
				anilist_room.clone(),
				user.anilist_ids.clone(),
			))
			.await;
			log::error!("{:?}", res);
		}
	});
	let socket_room = room.clone();
	tokio::spawn(async move {
		loop {
			let res =
				tokio::spawn(utils::socket_listeners::socket_handler(socket_room.clone())).await;
			log::error!("{:?}", res);
		}
	});
	let bridge_client = client.clone();
	tokio::spawn(async move {
		loop {
			let res = tokio::spawn(tg_matrix_bridge::dispatch(bridge_client.clone())).await;
			log::error!("{:?}", res);
		}
	});

	// interactive
	client.add_event_handler(
		move |ev: SyncRoomMessageEvent, room: Room, client: Client| async move {
			if ev.sender().as_str() == client.user_id().unwrap().as_str() {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = ev {
				if let (MessageType::Text(text), room) =
					(original_message.content.msgtype.clone(), room.clone())
				{
					let _ = match_command(&room, &text, &original_message).await;
					let _ = match_text(&room, &text, &original_message).await;
				};
			}
		},
	);

	// auto join
	client.add_event_handler(
		|ev: StrippedRoomMemberEvent, room: Room, client: Client| async move {
			if ev.state_key != client.user_id().unwrap() {
				return;
			}
			if let Err(err) = room.join().await {
				dbg!("{}", err);
			};
		},
	);

	log::info!("bot started");
	loop {
		let client_sync = client.sync(SyncSettings::default()).await;
		if let Err(ref e) = client_sync {
			eprintln!("{:?}", e);
		}
	}
}
