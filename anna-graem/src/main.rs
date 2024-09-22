use std::sync::Arc;
use ruma::events::{room::message::SyncRoomMessageEvent, SyncMessageLikeEvent};
use matrix_sdk::{
	config::SyncSettings,
	ruma,
	Client,
};
use serde::{Deserialize, Serialize};
use teloxide::{
	dispatching::{Dispatcher, UpdateFilterExt},
	update_listeners::webhooks,
};
use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt},
};
use anna_graem::{
	tg_handlers::tg_to_mx,
	matrix_handlers::mx_to_tg,
	utils::{get_tg_bot, get_tg_webhook_link, get_to_tg_data, BmMxData, BRIDGES},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	simple_logger::SimpleLogger::new().env().init().unwrap();
	
	let bot = Arc::new(get_tg_bot().await);
	let url = url::Url::parse(&get_tg_webhook_link(bot.token()))?;
	let addr = ([0, 0, 0, 0], 8443).into();
	let listener = webhooks::axum(bot.clone(), webhooks::Options::new(addr, url)).await?;

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
			let Some(bridge) = BRIDGES
				.iter()
				.find(|b| b.matrix_chat.id == room.room_id().as_str())
			else {
				return;
			};
			if let SyncMessageLikeEvent::Original(_) = ev.clone() {
				let original_ev = ev.as_original().unwrap();
				let from_mx_data = BmMxData {
					mx_event: original_ev,
					room,
					mx_msg_type: &original_ev.content.msgtype,
				};
				let Ok(to_tg_data) =
					get_to_tg_data(&from_mx_data, bot_to_matrix, client, bridge).await
				else {
					return;
				};
				if let Err(e) = mx_to_tg(to_tg_data, from_mx_data).await {
					log::error!("{e}");
				}
			}
		},
	);
	tokio::spawn(async move {
		let tg_update_handler =
			teloxide::types::Update::filter_message().branch(teloxide::dptree::endpoint(tg_to_mx));
		let err_handler = teloxide::error_handlers::LoggingErrorHandler::new();
		Dispatcher::builder(bot, tg_update_handler)
			.dependencies(teloxide::dptree::deps![client])
			.build()
			.dispatch_with_listener(listener, err_handler)
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
