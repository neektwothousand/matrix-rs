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
use teloxide::requests::Requester;
use teloxide::Bot;

use anna_tg_matrix_bridge::utils::{get_tg_bot, tg_photo_handler, tg_text_handler};
use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt},
};

const TG_CHAT_ID: i64 = -1001402125530;

async fn matrix_text_tg(text: String, bot: &Bot) {
	let chat_id = teloxide::types::ChatId(TG_CHAT_ID);
	match bot.send_message(chat_id, text).await {
		Ok(_) => (),
		Err(e) => eprintln!("{:?}", e),
	};
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	#[derive(Serialize, Deserialize)]
	struct User {
		name: String,
		password: String,
	}
	let user: User = serde_yaml::from_reader(std::fs::File::open("anna.yaml").unwrap()).unwrap();

	let u = matrix_sdk::ruma::UserId::parse(&user.name).unwrap();
	let matrix_client = matrix_sdk::Client::builder()
		.sqlite_store("anna_sqlite_store", None)
		.server_name(u.server_name())
		.build()
		.await
		.unwrap();

	// First we need to log in.
	let login_builder = matrix_client
		.matrix_auth()
		.login_username(u, &user.password);

	let dana_device_id_file_str = "anna_device_id";
	if let Ok(mut f) = File::open(dana_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await.unwrap();

		login_builder
			.device_id(&device_id_str)
			.send()
			.await
			.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(dana_device_id_file_str).await.unwrap();
		f.write_all(response.device_id.as_bytes()).await.unwrap();
	}

	let matrix_client_id = matrix_client.user_id().unwrap().to_string();
	let bot = Arc::new(get_tg_bot().await);
	let bot_to_matrix = Arc::clone(&bot);
	matrix_client.add_event_handler(
		|ev: SyncRoomMessageEvent, room: matrix_sdk::Room, _: Client| async move {
			if ev.sender().as_str() == matrix_client_id {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = ev.clone() {
				match &original_message.content.msgtype {
					MessageType::Text(text) => {
						let text = format!("{}: {}", ev.sender().as_str(), text.body);
						matrix_text_tg(text, &bot_to_matrix).await;
					}
					MessageType::Image(_)
					| MessageType::File(_)
					| MessageType::Audio(_)
					| MessageType::Video(_) => {
						let url = room
							.matrix_to_event_permalink(ev.event_id())
							.await
							.unwrap()
							.to_string();
						let image = format!("{}: (file)\n{}", ev.sender().as_str(), url);
						matrix_text_tg(image, &bot_to_matrix).await;
					}
					_ => (),
				}
			}
		},
	);

	std::thread::spawn(|| async {
		let tg_update_handler = teloxide::types::Update::filter_message()
			.branch(teloxide::dptree::endpoint(tg_text_handler))
			.branch(teloxide::dptree::endpoint(tg_photo_handler));
		Dispatcher::builder(bot, tg_update_handler)
			.build()
			.dispatch()
			.await;
	});

	println!("bridge started");

	loop {
		let client_sync = matrix_client.sync(SyncSettings::default()).await;
		if let Err(ref e) = client_sync {
			eprintln!("{:#?}", e);
		}
	}
}
