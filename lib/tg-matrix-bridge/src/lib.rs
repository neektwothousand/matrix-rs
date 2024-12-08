use crate::bridge_utils::get_tg_bot;
use std::sync::Arc;

use crate::{
	bridge_structs::Bridge,
	tg_handlers::tg_to_mx,
};
use matrix_sdk::Client;

use teloxide::{
	dispatching::{
		Dispatcher,
		UpdateFilterExt,
	},
	update_listeners::webhooks,
};

pub mod bridge_structs;
pub mod bridge_utils;
pub mod db;
pub mod matrix_handlers;
pub mod tg_handlers;
mod timer;

pub async fn dispatch(client: Arc<Client>, bridges: Arc<Vec<Bridge>>, webhook_url: Arc<String>) {
	let bot = get_tg_bot().await;
	let url =
		url::Url::parse(&format!("{webhook_url}{}", bot.clone().into_inner().token())).unwrap();
	let addr = ([0, 0, 0, 0], 8443).into();
	let listener = webhooks::axum(bot.clone(), webhooks::Options::new(addr, url)).await.unwrap();

	let tg_update_handler =
		teloxide::types::Update::filter_message().branch(teloxide::dptree::endpoint(tg_to_mx));
	let err_handler = teloxide::error_handlers::LoggingErrorHandler::new();
	Dispatcher::builder(bot, tg_update_handler)
		.dependencies(teloxide::dptree::deps![client, bridges])
		.build()
		.dispatch_with_listener(listener, err_handler)
		.await;
}
