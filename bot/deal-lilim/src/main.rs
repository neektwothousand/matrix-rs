use anyhow::Context;
use interactive::commands::{match_command, match_text};
use matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent;
use matrix_sdk::ruma::events::room::message::{MessageType, SyncRoomMessageEvent};
use matrix_sdk::ruma::events::SyncMessageLikeEvent;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::{config::SyncSettings, ruma, Client, Room};
use serde::Deserialize;
use std::io::Write;
use std::sync::Arc;
use tg_matrix_bridge::bridge_structs::Bridge;

#[derive(Deserialize)]
struct LoginData {
	name: String,
	password: String,
}

#[derive(Deserialize)]
struct User {
	login_data: Vec<LoginData>,
	room_id: String,
	webhook_url: String,
	anilist_ids: Vec<u64>,
	bridges: Vec<Bridge>,
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

	let user: User = toml::from_str(&std::fs::read_to_string("bot_data.toml")?)?;

	let u = ruma::UserId::parse(&user.login_data[0].name)?;
	let bridge_client = Arc::new(
		Client::builder()
			.sqlite_store("matrix_bot_sqlite", None)
			.server_name(u.server_name())
			.build()
			.await?,
	);
	let login_builder = bridge_client
		.matrix_auth()
		.login_username(u, &user.login_data[0].password);
	utils::matrix::read_or_create_device_id("bridge", login_builder).await?;

	let u = ruma::UserId::parse(&user.login_data[1].name)?;
	let utils_client = Arc::new(
		Client::builder()
			.sqlite_store("matrix_bot2_sqlite", None)
			.server_name(u.server_name())
			.build()
			.await?,
	);
	let login_builder = utils_client
		.matrix_auth()
		.login_username(u, &user.login_data[1].password);
	utils::matrix::read_or_create_device_id("utils", login_builder).await?;

	utils_client.sync_once(SyncSettings::default()).await?;

	let room_id = RoomId::parse(user.room_id)?;
	let room = Arc::new(utils_client.get_room(&room_id).context("room not found")?);

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
	let bridges = Arc::new(user.bridges);
	let webhook_url = Arc::new(user.webhook_url);

	let bridge_client_dispatch = bridge_client.clone();
	tokio::spawn(async move {
		loop {
			let res = tokio::join!(
				tg_matrix_bridge::dispatch(
					bridge_client_dispatch.clone(),
					bridges.clone(),
					webhook_url.clone()
				),
				bridge_client.sync(SyncSettings::default()),
			);
			if let Err(res) = res.1 {
				log::debug!("{:?}", res);
			}
		}
	});

	// interactive
	utils_client.add_event_handler(
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
	utils_client.add_event_handler(
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
		let client_sync = utils_client.sync(SyncSettings::default()).await;
		if let Err(ref e) = client_sync {
			log::debug!("{:?}", e);
		}
	}
}
