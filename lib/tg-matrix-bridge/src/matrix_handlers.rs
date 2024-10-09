use crate::bridge_structs::{BmMxData, BmTgData};
use crate::bridge_utils::{bot_send_request, get_bms, update_bridged_messages};
use anyhow::{bail, Context};
use matrix_sdk::ruma::events::room::message::{Relation, RoomMessageEventContent};
use matrix_sdk::ruma::events::{
	AnyMessageLikeEvent, AnyMessageLikeEventContent, AnyTimelineEvent, OriginalMessageLikeEvent,
};
use matrix_sdk::ruma::EventId;
use teloxide::types::MessageId;
use teloxide::types::{LinkPreviewOptions, ReplyParameters};

fn find_tg_msg_id(reply: AnyMessageLikeEvent, mx_chat: &str) -> Option<MessageId> {
	let bms = get_bms(mx_chat)?;
	let id = match EventId::parse(reply.event_id()) {
		Ok(id) => id,
		Err(e) => {
			log::error!("{}:{}", line!(), e);
			return None;
		}
	};
	let bm = bms.iter().find(|bm| bm.matrix_id == id)?;
	Some(bm.telegram_id.1)
}
async fn get_reply(
	matrix_event: &OriginalMessageLikeEvent<AnyMessageLikeEventContent>,
	room: &matrix_sdk::Room,
) -> anyhow::Result<AnyMessageLikeEvent> {
	let relates_to = match &matrix_event.content {
		AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent { relates_to, .. }) => {
			relates_to.clone().context("m.relates_to not found")?
		}
		_ => bail!(""),
	};
	let reply = match &relates_to {
		Relation::Reply { in_reply_to } => in_reply_to,
		_ => bail!(""),
	};
	let reply_event = room.event(&reply.event_id).await?;
	let AnyTimelineEvent::MessageLike(reply_message) = reply_event.event.deserialize()? else {
		bail!("");
	};

	Ok(reply_message)
}

pub async fn mx_to_tg(to_tg_data: BmTgData, from_mx_data: BmMxData<'_>) -> anyhow::Result<()> {
	let bot = to_tg_data.bot.clone().context("bot not found")?;
	let chat_id = to_tg_data.chat_id.context("chat not found")?;
	let null_id = -1i32;
	let matrix_reply = get_reply(from_mx_data.mx_event, &from_mx_data.room).await;
	let matrix_chat_id = from_mx_data.room.room_id().as_str();
	let reply_to_id = if matrix_reply.is_ok() {
		find_tg_msg_id(matrix_reply?, matrix_chat_id).unwrap_or(MessageId(null_id))
	} else {
		MessageId(null_id)
	};
	let from_user = from_mx_data.mx_event.sender.localpart();
	let link_preview = LinkPreviewOptions {
		is_disabled: to_tg_data.preview,
		url: None,
		prefer_large_media: true,
		prefer_small_media: false,
		show_above_text: false,
	};
	let reply_params = ReplyParameters::new(reply_to_id).allow_sending_without_reply();
	let t_msg = bot_send_request(
		bot,
		to_tg_data,
		chat_id,
		reply_params,
		link_preview,
		from_user.to_string(),
	)
	.await?;
	update_bridged_messages(
		from_mx_data.mx_event.event_id.clone(),
		(t_msg.chat.id, t_msg.id),
		matrix_chat_id,
	)?;
	Ok(())
}
