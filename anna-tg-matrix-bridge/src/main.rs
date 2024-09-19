use std::sync::Arc;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma;
use matrix_sdk::Client;

use ruma::events::{
	room::message::{MessageType, SyncRoomMessageEvent},
	SyncMessageLikeEvent,
};

use serde::{Deserialize, Serialize};
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};

use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt},
};

use anna_tg_matrix_bridge::matrix_handlers::{matrix_file_tg, matrix_text_tg};
use anna_tg_matrix_bridge::tg_handlers::{tg_file_handler, tg_text_handler};
use anna_tg_matrix_bridge::utils::{get_matrix_media, get_tg_bot, BRIDGES};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	simple_logger::SimpleLogger::new().env().init().unwrap();

	#[derive(Serialize, Deserialize)]
	struct User {
		name: String,
		password: String,
	}
	let user: User = serde_json::from_reader(std::fs::File::open("anna.json").unwrap()).unwrap();

	let u = matrix_sdk::ruma::UserId::parse(&user.name).unwrap();
	let client = matrix_sdk::Client::builder()
		.sqlite_store("anna_sqlite_store", None)
		.server_name(u.server_name())
		.build()
		.await
		.unwrap();
	let bot = Arc::new(get_tg_bot().await);
	let bot_to_matrix = Arc::clone(&bot);
	let matrix_client = client.clone();
	let login_builder = matrix_client
		.matrix_auth()
		.login_username(u, &user.password);

	let anna_device_id_file_str = "anna_device_id";
	if let Ok(mut f) = File::open(anna_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await.unwrap();

		login_builder
			.device_id(&device_id_str)
			.send()
			.await
			.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(anna_device_id_file_str).await.unwrap();
		f.write_all(response.device_id.as_bytes()).await.unwrap();
	}
	let matrix_client_id = client.user_id().unwrap().to_string();
	client.add_event_handler(
		|ev: SyncRoomMessageEvent, room: matrix_sdk::Room, client: Client| async move {
			if ev.sender().as_str() == matrix_client_id {
				return;
			}
			let mut tg_chat_id: i64 = 0;
			for bridge in BRIDGES.iter() {
				if room.room_id().as_str() == bridge.matrix_chat.id {
					tg_chat_id = bridge.telegram_chat.id;
					break;
				}
			}
			if tg_chat_id == 0 {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = ev.clone() {
				let message_type = &original_message.content.msgtype;
				if let MessageType::Text(text) = message_type {
					let text = format!("{}: {}", ev.sender().as_str(), text.body);
					let matrix_event = ev.as_original().unwrap();
					let disable_preview = false;
					if let Err(e) = matrix_text_tg(
						tg_chat_id,
						text,
						matrix_event,
						room,
						&bot_to_matrix,
						disable_preview,
					)
					.await
					{
						eprintln!("{:?}", e);
					};
				} else {
					match get_matrix_media(client, message_type.clone()).await {
						Ok(media) => {
							let (media, media_name) = media;
							let caption = ev.sender().as_str();
							matrix_file_tg(tg_chat_id, media_name, media, caption, &bot_to_matrix)
								.await;
						}
						Err(e) => eprintln!("{:?}", e),
					}
				}
			}
		},
	);
	tokio::spawn(async move {
		let tg_update_handler = teloxide::types::Update::filter_message()
			.branch(
				teloxide::dptree::filter(|msg: teloxide::types::Message| msg.text().is_some())
					.endpoint(tg_text_handler),
			)
			.branch(teloxide::dptree::endpoint(tg_file_handler));
		Dispatcher::builder(bot, tg_update_handler)
			.dependencies(teloxide::dptree::deps![client])
			.build()
			.dispatch()
			.await;
	});
	if matrix_client.user_id().is_some() {
		log::info!("matrix client logged in");
	}
	log::info!("tg dispatched");
	loop {
		let client_sync = matrix_client.sync(SyncSettings::default()).await;
		if let Err(ref e) = client_sync {
			eprintln!("{:?}", e);
		}
	}
}
