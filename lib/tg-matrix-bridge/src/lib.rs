use crate::bridge_utils::{get_tg_bot, get_to_tg_data, BmMxData};
use std::sync::Arc;

use crate::matrix_handlers::mx_to_tg;
use crate::tg_handlers::tg_to_mx;
use bridge_utils::Bridge;
use matrix_sdk::ruma::events::room::message::SyncRoomMessageEvent;
use matrix_sdk::ruma::events::SyncMessageLikeEvent;
use matrix_sdk::Client;

use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::update_listeners::webhooks;

pub mod bridge_utils;
pub mod db;
pub mod matrix_handlers;
pub mod tg_handlers;

pub async fn dispatch(client: Arc<Client>, bridges: Arc<Vec<Bridge>>, webhook_url: Arc<String>) {
	let bot = get_tg_bot().await;
	let url = url::Url::parse(&format!(
		"{webhook_url}{}",
		bot.clone().into_inner().token()
	))
	.unwrap();
	let addr = ([0, 0, 0, 0], 8443).into();
	let listener = webhooks::axum(bot.clone(), webhooks::Options::new(addr, url))
		.await
		.unwrap();

	let bot_to_matrix = bot.clone();
	let bridges_to_matrix = Arc::clone(&bridges);
	client.add_event_handler(
		|ev: SyncRoomMessageEvent, room: matrix_sdk::Room, client: Client| async move {
			if ev.sender().as_str() == client.user_id().unwrap().as_str() {
				return;
			}
			let Some(bridge) = bridges_to_matrix
				.iter()
				.find(|b| b.mx_id == room.room_id().as_str())
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
	let client_to_tg = client.clone();
	let bridges_to_tg = bridges.clone();
	let tg_update_handler =
		teloxide::types::Update::filter_message().branch(teloxide::dptree::endpoint(tg_to_mx));
	let err_handler = teloxide::error_handlers::LoggingErrorHandler::new();
	Dispatcher::builder(bot, tg_update_handler)
		.dependencies(teloxide::dptree::deps![client_to_tg, bridges_to_tg])
		.build()
		.dispatch_with_listener(listener, err_handler)
		.await;
}
