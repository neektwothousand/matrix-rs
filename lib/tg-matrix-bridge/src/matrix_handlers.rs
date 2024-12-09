use std::sync::Arc;

use crate::{
	bridge_structs::{
		BmMxData,
		BmTgData,
		Bridge,
	},
	bridge_utils::{
		bot_send_request,
		get_bms,
		get_tg_bot,
		get_to_tg_data,
		update_bridged_messages,
	},
};
use anyhow::{
	bail,
	Context,
};
use matrix_sdk::{
	event_handler::RawEvent,
	media::MediaEventContent,
	ruma::{
		events::{
			room::message::{
				AddMentions,
				ForwardThread,
				ImageMessageEventContent,
				MessageType,
				RoomMessageEventContent,
			},
			AnyMessageLikeEvent,
			AnyMessageLikeEventContent,
			AnySyncMessageLikeEvent,
			AnyTimelineEvent,
			MessageLikeUnsigned,
			OriginalMessageLikeEvent,
		},
		EventId,
		OwnedEventId,
	},
	Client,
};
use serde_json::Value;
use teloxide::{
	types::{
		LinkPreviewOptions,
		MessageId,
		ReplyParameters,
	},
	ApiError,
};

fn find_tg_msg_id(reply: &OwnedEventId, mx_chat: &str) -> Option<MessageId> {
	let bms = get_bms(mx_chat)?;
	let bm = bms.iter().find(|bm| bm.matrix_id == *reply)?;
	Some(bm.telegram_id.1)
}

async fn get_reply(
	matrix_event: &OriginalMessageLikeEvent<AnyMessageLikeEventContent>,
	room: &matrix_sdk::Room,
) -> Option<OwnedEventId> {
	let timeline_event = match room.event(&matrix_event.event_id, None).await {
		Ok(timeline_event) => timeline_event,
		Err(e) => {
			log::debug!("{:?}", e);
			return None;
		}
	};
	let raw_json = match serde_json::from_str::<Value>(timeline_event.kind.raw().json().get()) {
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
	//crate::timer::timer!();
	let bot = to_tg_data.bot.clone().context("bot not found")?;
	let chat_id = to_tg_data.chat_id.context("chat not found")?;
	let null_id = -1i32;
	let matrix_chat_id = from_mx_data.room.room_id().as_str();
	let matrix_event = from_mx_data.mx_event;
	let reply_to_id = {
		if let Some(matrix_reply) = get_reply(matrix_event, &from_mx_data.room).await {
			find_tg_msg_id(&matrix_reply, matrix_chat_id).unwrap_or(MessageId(null_id))
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
			to_tg_data.message =
				"telegram sucks and cannot display this message".to_string().into_bytes();
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

pub async fn client_event_handler(
	ev: AnySyncMessageLikeEvent,
	raw: RawEvent,
	room: matrix_sdk::Room,
	client: Client,
	bridges: Arc<Vec<Bridge>>,
) {
	let Some(client_id) = client.user_id() else {
		return;
	};
	if ev.sender().as_str() == client_id.as_str() {
		return;
	}
	let Some(bridge) = bridges.iter().find(|b| b.mx_id == room.room_id().as_str()) else {
		return;
	};
	let Some(oc) = ev.original_content() else {
		return;
	};
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
		let Ok(raw_json_value) = serde_json::from_str::<serde_json::Value>(raw.get()) else {
			return;
		};
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
			let Ok(event_id) = EventId::parse(reply_event_id.as_str().unwrap_or_default()) else {
				return;
			};
			let Ok(event) = room.event(&event_id, None).await else {
				return;
			};
			let Ok(AnyTimelineEvent::MessageLike(ev)) =
				event.kind.raw().deserialize_as::<AnyTimelineEvent>()
			else {
				return;
			};
			let AnyMessageLikeEvent::RoomMessage(msg_like_event) = ev else {
				return;
			};
			let Some(oc) = msg_like_event.as_original() else {
				return;
			};
			room_message.clone().make_reply_to(oc, ForwardThread::No, AddMentions::No);
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
	let Ok(to_tg_data) = get_to_tg_data(&from_mx_data, get_tg_bot().await, client, bridge).await
	else {
		return;
	};
	if let Err(e) = mx_to_tg(to_tg_data, from_mx_data).await {
		log::error!("{e}");
	}
}
