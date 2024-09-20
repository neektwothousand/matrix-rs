use anyhow::bail;
use matrix_sdk::{
	ruma::events::{room::message::RoomMessageEventContent, OriginalSyncMessageLikeEvent},
	Room,
};
use teloxide::{
	payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters},
	prelude::Requester,
	types::{InputFile, MessageId},
	Bot,
};

use crate::utils::{find_bm, get_matrix_reply, update_bridged_messages};

pub async fn matrix_text_tg(
	tg_chat_id: i64,
	text: String,
	matrix_event: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: Room,
	bot: &Bot,
	preview: bool,
) -> anyhow::Result<()> {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	let matrix_chat_id = room.room_id().to_string();
	let reply_to_id = if let Ok(matrix_reply) = get_matrix_reply(matrix_event, &room).await {
		let find_bm_result = find_bm(matrix_reply, &matrix_chat_id);
		if let Ok(id) = find_bm_result {
			id
		} else {
			log::info!("{:?}", find_bm_result);
			MessageId(-1i32)
		}
	} else {
		MessageId(-1i32)
	};
	match bot
		.send_message(chat_id, text)
		.reply_to_message_id(reply_to_id)
		.allow_sending_without_reply(true)
		.disable_web_page_preview(preview)
		.await
	{
		Ok(t_msg) => {
			update_bridged_messages(
				matrix_event.event_id.clone(),
				(t_msg.chat.id, t_msg.id),
				&matrix_chat_id,
			)?;
		}
		Err(e) => bail!("{:?}", e),
	};
	Ok(())
}

pub async fn matrix_photo_tg(
	tg_chat_id: i64,
	photo: Vec<u8>,
	file_name: String,
	caption: &str,
	matrix_event: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: Room,
	bot: &Bot,
) -> anyhow::Result<()> {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	let matrix_chat_id = room.room_id().to_string();
	let reply_to_id = if let Ok(matrix_reply) = get_matrix_reply(matrix_event, &room).await {
		let find_bm_result = find_bm(matrix_reply, &matrix_chat_id);
		if let Ok(id) = find_bm_result {
			id
		} else {
			log::info!("{:?}", find_bm_result);
			MessageId(-1i32)
		}
	} else {
		MessageId(-1i32)
	};
	match bot
		.send_photo(chat_id, InputFile::memory(photo).file_name(file_name))
		.reply_to_message_id(reply_to_id)
		.allow_sending_without_reply(true)
		.caption(caption)
		.await
	{
		Ok(t_msg) => {
			update_bridged_messages(
				matrix_event.event_id.clone(),
				(t_msg.chat.id, t_msg.id),
				&matrix_chat_id,
			)?;
		}
		Err(e) => bail!("{:?}", e),
	}
	Ok(())
}

pub async fn matrix_file_tg(
	tg_chat_id: i64,
	file: Vec<u8>,
	file_name: String,
	caption: &str,
	matrix_event: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: Room,
	bot: &Bot,
) -> anyhow::Result<()> {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	let matrix_chat_id = room.room_id().to_string();
	let reply_to_id = if let Ok(matrix_reply) = get_matrix_reply(matrix_event, &room).await {
		let find_bm_result = find_bm(matrix_reply, &matrix_chat_id);
		if let Ok(id) = find_bm_result {
			id
		} else {
			log::info!("{:?}", find_bm_result);
			MessageId(-1i32)
		}
	} else {
		MessageId(-1i32)
	};
	match bot
		.send_document(chat_id, InputFile::memory(file).file_name(file_name))
		.reply_to_message_id(reply_to_id)
		.allow_sending_without_reply(true)
		.caption(caption)
		.await
	{
		Ok(t_msg) => {
			update_bridged_messages(
				matrix_event.event_id.clone(),
				(t_msg.chat.id, t_msg.id),
				&matrix_chat_id,
			)?;
		}
		Err(e) => bail!("{:?}", e),
	}
	Ok(())
}
