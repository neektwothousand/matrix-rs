use matrix_sdk::{
	config::SyncSettings,
	ruma::{
		events::room::{member::StrippedRoomMemberEvent, message::SyncRoomMessageEvent},
		UserId,
	},
	Client,
	room::Room,
	ruma::events::{room::message::*, *},
};
use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	#[derive(Serialize, Deserialize)]
	struct User {
		name: String,
		password: String,
	}
	let user: User = serde_yaml::from_reader(std::fs::File::open("dana.yaml")?)?;

	let u = <&UserId>::try_from(<String as AsRef<str>>::as_ref(&user.name))?;
	let client = Client::builder()
		.sqlite_store("dana_sqlite_store", None)
		.server_name(u.server_name())
		.build()
		.await?;

	// First we need to log in.
	let login_builder = client.matrix_auth().login_username(u, &user.password);

	let dana_device_id_file_str = "dana_device_id";
	if let Ok(mut f) = File::open(dana_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await?;

		login_builder.device_id(&device_id_str).send().await?;
	} else {
		let response = login_builder.send().await?;
		let mut f = File::create(dana_device_id_file_str).await?;
		f.write_all(response.device_id.as_bytes()).await?;
	}

	client.add_event_handler(
		|ev: SyncRoomMessageEvent, room: Room, _client: Client| async move {
			if ev.sender().as_str() == user.name {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = ev {
				if let (MessageType::Text(text), room) =
					(original_message.content.msgtype.clone(), room.clone())
				{
					use dana_zane::commands;
					let _ = commands::match_command(&room, &text, &original_message).await;
					let _ = commands::match_text(&room, &text, &original_message).await;
				};
			}
		},
	);

	for room in client.invited_rooms() {
		client.join_room_by_id(room.room_id()).await.unwrap();
	}

	loop {
		let sync_settings = SyncSettings::default().timeout(Duration::from_secs(60));
		let Err(err) = client.sync(sync_settings).await else {
			continue;
		};
		eprintln!("{:?}", err);
	}
}
