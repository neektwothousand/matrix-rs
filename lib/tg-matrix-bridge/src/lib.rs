#![allow(clippy::missing_errors_doc)]
use crate::bridge_utils::get_tg_bot;
use std::sync::Arc;

use crate::bridge_structs::Bridge;
use crate::tg_handlers::tg_to_mx;
use matrix_sdk::Client;

use teloxide::dispatching::Dispatcher;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::update_listeners::webhooks;

pub mod bridge_structs;
pub mod bridge_utils;
pub mod db;
pub mod matrix_handlers;
pub mod tg_handlers;
mod timer;

#[allow(clippy::missing_panics_doc)]
pub async fn dispatch(client: Arc<Client>, bridges: Arc<Vec<Bridge>>, webhook_url: Arc<String>) {
	let bot = get_tg_bot().await;
	let url =
		url::Url::parse(&format!("{webhook_url}{}", bot.clone().into_inner().token())).unwrap();
	let addr = ([0, 0, 0, 0], 8443).into();
	let listener = loop {
		match webhooks::axum(bot.clone(), webhooks::Options::new(addr, url.clone())).await {
			Ok(listener) => break listener,
			Err(teloxide::RequestError::Network(e)) => {
				if e.is_timeout() {
					continue;
				}
			}
			Err(e) => {
				log::error!("{e}");
				return;
			}
		}
	};

	let tg_update_handler =
		teloxide::types::Update::filter_message().branch(teloxide::dptree::endpoint(tg_to_mx));
	let err_handler = teloxide::error_handlers::LoggingErrorHandler::new();
	Box::pin(
		Dispatcher::builder(bot, tg_update_handler)
			.dependencies(teloxide::dptree::deps![client, bridges])
			.build()
			.dispatch_with_listener(listener, err_handler),
	)
	.await;
}
