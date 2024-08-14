use std::io::Write;
use std::sync::Arc;

use anna_tg_matrix_bridge::utils::matrix_file_tg;
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

use anna_tg_matrix_bridge::utils::{
	get_matrix_media, get_tg_bot, matrix_text_tg, tg_photo_handler, tg_text_handler,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	#[derive(Serialize, Deserialize)]
	struct User {
		name: String,
		password: String,
	}
	let user: User = serde_yaml::from_reader(std::fs::File::open("anna.yaml").unwrap()).unwrap();

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
			if !std::path::Path::new("anna.log").exists() {
				std::fs::File::create("anna.log").unwrap();
			}
			let tg_chat_id = match room.room_id().as_str() {
				// The Wired
				"!vUWLFTSVVBjhMouZpF:matrix.org" => -1001402125530i64,
				// OTHERWORLD
				"!6oZjqONVahFLOKTvut:matrix.archneek.me" => -1002152065322i64,
				_ => return,
			};
			let mut log = std::fs::OpenOptions::new().append(true).open("anna.log").unwrap();
			log.write_all(format!("{:?}\n", ev.event_id()).as_bytes()).unwrap();
			if let SyncMessageLikeEvent::Original(original_message) = ev.clone() {
				let message_type = &original_message.content.msgtype;
				if let MessageType::Text(text) = message_type {
					let text = format!("{}: {}", ev.sender().as_str(), text.body);
					let disable_preview = false;
					matrix_text_tg(tg_chat_id, text, &bot_to_matrix, disable_preview).await;
				} else if let Ok(media) = get_matrix_media(client, message_type.clone()).await {
					let (media, media_name) = media;
					let caption = ev.sender().as_str();
					matrix_file_tg(tg_chat_id, media_name, media, caption, &bot_to_matrix).await;
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
			.branch(
				teloxide::dptree::filter(|msg: teloxide::types::Message| msg.photo().is_some())
					.endpoint(tg_photo_handler),
			);
		Dispatcher::builder(bot, tg_update_handler)
			.dependencies(teloxide::dptree::deps![client])
			.build()
			.dispatch()
			.await;
	});
	if matrix_client.user_id().is_some() {
		println!("matrix client logged in");
	}
	println!("tg dispatched");
	loop {
		let client_sync = matrix_client.sync(SyncSettings::default()).await;
		if let Err(ref e) = client_sync {
			eprintln!("{:?}", e);
		}
	}
}
