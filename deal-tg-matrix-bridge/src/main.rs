use std::time::Duration;

use serde::{Serialize, Deserialize};

use matrix_sdk::ruma::{self, events::room::message::TextMessageEventContent};
use matrix_sdk::config::SyncSettings;
use ruma::events::{
	room::message::{SyncRoomMessageEvent, MessageType}, SyncMessageLikeEvent,
};

use teloxide::Bot;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn send_to_tg(text: TextMessageEventContent, room: matrix_sdk::Room) {
	
}

async fn get_tg_bot() -> teloxide::Bot {
	let token = std::fs::read_to_string("tg_token").unwrap();
	Bot::new(token)
}

async fn get_matrix_client() -> matrix_sdk::Client {
	#[derive(Serialize, Deserialize)]
	struct User {
		name: String,
		password: String,
	}
	let user: User = serde_yaml::from_reader(std::fs::File::open("dana.yaml").unwrap()).unwrap();

	let u = <&ruma::UserId>::try_from(<String as AsRef<str>>::as_ref(&user.name)).unwrap();
	let client = matrix_sdk::Client::builder()
		.sqlite_store("dana_sqlite_store", None)
		.server_name(u.server_name())
		.build()
		.await.unwrap();

	// First we need to log in.
	let login_builder = client.matrix_auth().login_username(u, &user.password);

	let dana_device_id_file_str = "dana_device_id";
	if let Ok(mut f) = File::open(dana_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await.unwrap();

		login_builder.device_id(&device_id_str).send().await.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(dana_device_id_file_str).await.unwrap();
		f.write_all(response.device_id.as_bytes()).await.unwrap();
	}
	client
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let matrix_client = get_matrix_client().await;
	let matrix_client_id = matrix_client.clone().user_id().unwrap().to_string();
	matrix_client.add_event_handler(
		|ev: SyncRoomMessageEvent, room: matrix_sdk::Room, _client: matrix_sdk::Client| async move {
			if ev.sender().as_str() == matrix_client_id {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = ev {
				if let (MessageType::Text(text), room) =
					(original_message.content.msgtype.clone(), room.clone()) {
						send_to_tg(text, room).await;
				}
			}
		},
	);

	tokio::spawn(async move { loop {
		let sync_settings = SyncSettings::default().timeout(Duration::from_secs(60));
		let Err(_err) = matrix_client.sync(sync_settings).await else {
			eprintln!("dana http error");
			continue;
		};
	}});

	let tg_bot = get_tg_bot().await;
	todo!();
	Ok(())
}
