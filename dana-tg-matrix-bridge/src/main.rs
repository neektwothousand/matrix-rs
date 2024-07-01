use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma;

use ruma::events::{
	room::message::{MessageType, SyncRoomMessageEvent},
	SyncMessageLikeEvent,
};

use serde::{Deserialize, Serialize};

use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::requests::Requester;
use teloxide::Bot;

use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TG_CHAT_ID: i64 = -1001402125530;
const MATRIX_CHAT_ID: &str = "!vUWLFTSVVBjhMouZpF:matrix.org";

async fn send_to_tg(text: String, bot: Bot) {
	let chat_id = teloxide::types::ChatId(TG_CHAT_ID);
	bot.send_message(chat_id, text).await.unwrap();
}

async fn send_to_matrix(text: &str, room: matrix_sdk::Room) {
	let text = format!("telegram:\n{}", text);
	let message = RoomMessageEventContent::text_plain(text);
	room.send(message).await.unwrap();
}

async fn tg_msg_handler(text: &str) {
	let matrix_client = get_matrix_client().await;
	matrix_client.sync_once(SyncSettings::default()).await.unwrap();
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(MATRIX_CHAT_ID).unwrap())
		.unwrap();
	send_to_matrix(text, matrix_room).await;
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
		.await
		.unwrap();

	// First we need to log in.
	let login_builder = client.matrix_auth().login_username(u, &user.password);

	let dana_device_id_file_str = "dana_device_id";
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
	client
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let matrix_client = get_matrix_client().await;
	let matrix_client_id = matrix_client.clone().user_id().unwrap().to_string();
	matrix_client.add_event_handler(
		|ev: SyncRoomMessageEvent, _room: matrix_sdk::Room, _client: matrix_sdk::Client| async move {
			if ev.sender().as_str() == matrix_client_id {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = &ev {
				if let MessageType::Text(text) = original_message.content.msgtype.clone() {
					let text = format!("{}: {}", ev.sender().as_str(), text.body);
					send_to_tg(text, get_tg_bot().await).await;
				}
			}
		},
	);
	let tg_update_handler = teloxide::types::Update::filter_message().branch(
		teloxide::dptree::endpoint(|msg: teloxide::types::Message| async move {
			let Some(text) = msg.text() else {
				anyhow::bail!("");
			};
			let Some(user) = msg.from() else {
				anyhow::bail!("");
			};
			let text = format!("{}: {text}", user.first_name);
			tg_msg_handler(&text).await;
			anyhow::Ok(())
	}));

	let bot = get_tg_bot().await;
	tokio::spawn(async move {
		Dispatcher::builder(bot, tg_update_handler)
		.build()
		.dispatch()
		.await;
	});
	loop {
		let client_sync = matrix_client.sync(SyncSettings::default()).await;
		let Err(ref _e) = client_sync else {
			eprintln!("dana http error");
			continue;
		};
	}
}
