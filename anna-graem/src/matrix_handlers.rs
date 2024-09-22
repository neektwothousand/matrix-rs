use anyhow::{bail, Context};
use teloxide::{
	payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters},
	prelude::Requester,
	types::{InputFile, MessageId},
};

use crate::utils::{
	find_tg_msg_id, get_matrix_reply, update_bridged_messages, BmMxData, BmTgData, TgMessageKind,
};

pub async fn mx_to_tg(to_tg_data: BmTgData, from_mx_data: BmMxData<'_>) -> anyhow::Result<()> {
	let null_id = -1i32;
	let matrix_reply = get_matrix_reply(from_mx_data.mx_event, &from_mx_data.room).await;
	let matrix_chat_id = from_mx_data.room.room_id().as_str();
	let reply_to_id = if matrix_reply.is_ok() {
		find_tg_msg_id(matrix_reply?, matrix_chat_id).unwrap_or(MessageId(null_id))
	} else {
		MessageId(null_id)
	};
	let file_name = to_tg_data.file_name.unwrap_or("unknown".to_string());
	let from_user = from_mx_data.mx_event.sender.localpart();
	let t_msg: teloxide::types::Message = match to_tg_data.tg_message_kind {
		Some(TgMessageKind::Text) => {
			let text = format!(
				"{from_user}: {}",
				String::from_utf8_lossy(&to_tg_data.message)
			);
			to_tg_data
				.bot
				.context("bot not found")?
				.send_message(to_tg_data.chat_id.context("chat_id not found")?, text)
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
				.disable_web_page_preview(to_tg_data.preview)
				.await?
		}
		Some(TgMessageKind::Photo) => {
			to_tg_data
				.bot
				.context("bot not found")?
				.send_photo(
					to_tg_data.chat_id.context("chat_id not found")?,
					InputFile::memory(to_tg_data.message).file_name(file_name),
				)
				.caption(from_user)
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
				.await?
		}
		Some(TgMessageKind::Document) => {
			to_tg_data
				.bot
				.context("bot not found")?
				.send_document(
					to_tg_data.chat_id.context("chat_id not found")?,
					InputFile::memory(to_tg_data.message).file_name(file_name),
				)
				.caption(from_user)
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
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
