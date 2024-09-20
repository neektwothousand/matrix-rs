use anyhow::{bail, Context};
use teloxide::{
	payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters},
	prelude::Requester,
	types::{InputFile, MessageId},
};

use crate::utils::{find_bm, get_matrix_reply, update_bridged_messages, FromMxData, TgMessageKind, ToTgData};

pub async fn mx_to_tg(
	to_tg_data: ToTgData,
	from_mx_data: FromMxData<'_>,
) -> anyhow::Result<()> {
	let null_id = -1i32;
	let matrix_reply = get_matrix_reply(from_mx_data.matrix_event, &from_mx_data.room).await;
	let matrix_chat_id = from_mx_data.room.room_id().as_str();
	let reply_to_id = if matrix_reply.is_ok() {
		let bm = find_bm(matrix_reply?, matrix_chat_id);
		bm.unwrap_or(MessageId(null_id))
	} else {
		MessageId(null_id)
	};
	let file_name = to_tg_data.file_name.unwrap_or("unknown".to_string());
	let t_msg: teloxide::types::Message = match to_tg_data.tg_message_kind {
		Some(TgMessageKind::Text) => {
			to_tg_data
				.bot
				.context("bot not found")?
				.send_message(
					to_tg_data.chat_id.context("chat_id not found")?,
					String::from_utf8_lossy(&to_tg_data.message),
				)
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
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
				.await?
		}
		_ => bail!(""),
	};
	update_bridged_messages(
		from_mx_data.matrix_event.event_id.clone(),
		(t_msg.chat.id, t_msg.id),
		matrix_chat_id,
	)?;
	Ok(())
}
