use crate::bridge_structs::BmMxData;
use crate::bridge_utils::{get_tg_bot, get_to_tg_data};
use std::sync::Arc;

use crate::bridge_structs::Bridge;
use crate::matrix_handlers::mx_to_tg;
use crate::tg_handlers::tg_to_mx;
use matrix_sdk::event_handler::RawEvent;
use matrix_sdk::media::MediaEventContent;
use matrix_sdk::ruma::events::room::message::{
	AddMentions, ForwardThread, ImageMessageEventContent, MessageType, RoomMessageEventContent,
};
use matrix_sdk::ruma::events::{
	AnyMessageLikeEvent, AnyMessageLikeEventContent, AnySyncMessageLikeEvent, AnyTimelineEvent,
	MessageLikeUnsigned, OriginalMessageLikeEvent,
};
use matrix_sdk::ruma::EventId;
use matrix_sdk::Client;

use serde_json::Value;
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::update_listeners::webhooks;

pub mod bridge_structs;
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
		|ev: AnySyncMessageLikeEvent, raw: RawEvent, room: matrix_sdk::Room, client: Client| async move {
			if ev.sender().as_str() == client.user_id().unwrap().as_str() {
				return;
			}
			let Some(bridge) = bridges_to_matrix
				.iter()
				.find(|b| b.mx_id == room.room_id().as_str())
			else {
				return;
			};
			let oc = ev.original_content().unwrap();
			let original_ev = OriginalMessageLikeEvent {
				content: oc.clone(),
				event_id: ev.event_id().into(),
				origin_server_ts: ev.origin_server_ts(),
				room_id: room.room_id().into(),
				sender: ev.sender().into(),
				unsigned: MessageLikeUnsigned::new(),
			};
			let room_message = if let AnyMessageLikeEventContent::Sticker(sticker) = oc {
				let body = sticker.body.clone();
				let Some(source) = sticker.source() else {
					log::error!("sticker source not found");
					return;
				};
				let event_content = ImageMessageEventContent::new(body, source);
				let image_info = Some(Box::new(sticker.info));
				let message_type = MessageType::Image(event_content.info(image_info));
				let room_message = RoomMessageEventContent::new(message_type);
				let raw_json_value: Value = serde_json::from_str(raw.get()).unwrap();
				let reply_event_id = match raw_json_value["content"].get("m.relates_to") {
					Some(relates_to) => {
						if let Some(in_reply_to) = relates_to.get("m.in_reply_to") {
							in_reply_to.get("event_id")
						} else {
							None
						}
					}
					None => None,
				};
				if let Some(reply_event_id) = reply_event_id {
					let event_id = EventId::parse(reply_event_id.as_str().unwrap()).unwrap();
					let raw_ev = room.event(&event_id).await.unwrap().event;
					let ev = match raw_ev.deserialize_as::<AnyTimelineEvent>().unwrap() {
						AnyTimelineEvent::MessageLike(m) => m,
						_ => return,
					};
					let msg_like_event = match ev {
						AnyMessageLikeEvent::RoomMessage(m) => m,
						_ => return,
					};
					let oc = msg_like_event.as_original().unwrap();
					room_message
						.clone()
						.make_reply_to(oc, ForwardThread::No, AddMentions::No);
				};
				room_message
			} else if let AnyMessageLikeEventContent::RoomMessage(room_message) = oc {
				room_message
			} else {
				return;
			};
			let from_mx_data = BmMxData {
				mx_event: &original_ev,
				room,
				mx_msg_type: &room_message.msgtype,
			};
			let Ok(to_tg_data) = get_to_tg_data(&from_mx_data, bot_to_matrix, client, bridge).await
			else {
				return;
			};
			if let Err(e) = mx_to_tg(to_tg_data, from_mx_data).await {
				log::error!("{e}");
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
