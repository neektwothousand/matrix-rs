use crate::bridge_structs::{BmMxData, BmTgData};
use crate::bridge_utils::{bot_send_request, get_bms, update_bridged_messages};
use anyhow::{bail, Context};
use matrix_sdk::ruma::events::{AnyMessageLikeEventContent, OriginalMessageLikeEvent};
use matrix_sdk::ruma::{EventId, OwnedEventId};
use serde_json::Value;
use teloxide::types::MessageId;
use teloxide::types::{LinkPreviewOptions, ReplyParameters};
use teloxide::ApiError;

fn find_tg_msg_id(reply: OwnedEventId, mx_chat: &str) -> Option<MessageId> {
	let bms = get_bms(mx_chat)?;
	let bm = bms.iter().find(|bm| bm.matrix_id == reply)?;
	Some(bm.telegram_id.1)
}

async fn get_reply(
	matrix_event: &OriginalMessageLikeEvent<AnyMessageLikeEventContent>,
	room: &matrix_sdk::Room,
) -> Option<OwnedEventId> {
	let timeline_event = match room.event(&matrix_event.event_id).await {
		Ok(timeline_event) => timeline_event,
		Err(e) => {
			log::debug!("{:?}", e);
			return None;
		}
	};
	let raw_json = match serde_json::from_str::<Value>(timeline_event.event.json().get()) {
		Ok(raw_json) => raw_json,
		Err(e) => {
			log::debug!("{:?}", e);
			return None;
		}
	};
	let reply_event_id = raw_json
		.get("content")?
		.get("m.relates_to")?
		.get("m.in_reply_to")?
		.get("event_id")?
		.as_str()?;
	let Ok(owned_event_id) = EventId::parse(reply_event_id) else {
		return None;
	};
	Some(owned_event_id)
}

pub async fn mx_to_tg(to_tg_data: BmTgData, from_mx_data: BmMxData<'_>) -> anyhow::Result<()> {
	let bot = to_tg_data.bot.clone().context("bot not found")?;
	let chat_id = to_tg_data.chat_id.context("chat not found")?;
	let null_id = -1i32;
	let matrix_chat_id = from_mx_data.room.room_id().as_str();
	let matrix_event = from_mx_data.mx_event;
	let reply_to_id = {
		if let Some(matrix_reply) = get_reply(matrix_event, &from_mx_data.room).await {
			find_tg_msg_id(matrix_reply, matrix_chat_id).unwrap_or(MessageId(null_id))
		} else {
			MessageId(null_id)
		}
	};
	let from_user = from_mx_data.mx_event.sender.localpart();
	let link_preview = LinkPreviewOptions {
		is_disabled: to_tg_data.is_preview_disabled,
		url: None,
		prefer_large_media: true,
		prefer_small_media: false,
		show_above_text: false,
	};
	let reply_params = ReplyParameters::new(reply_to_id).allow_sending_without_reply();
	let res = bot_send_request(
		bot.clone(),
		to_tg_data.clone(),
		chat_id,
		reply_params.clone(),
		link_preview.clone(),
		from_user.to_string(),
	)
	.await;
	let t_msg = match res {
		Ok(msg) => msg,
		Err(teloxide::RequestError::Api(ApiError::RequestEntityTooLarge)) => {
			let mut to_tg_data = to_tg_data;
			to_tg_data.message = "telegram sucks and cannot display this message"
				.to_string()
				.into_bytes();
			bot_send_request(
				bot,
				to_tg_data,
				chat_id,
				reply_params,
				link_preview,
				from_user.to_string(),
			)
			.await?
		}
		_ => bail!(""),
	};
	update_bridged_messages(
		from_mx_data.mx_event.event_id.clone(),
		(t_msg.chat.id, t_msg.id),
		matrix_chat_id,
	)?;
	Ok(())
}
