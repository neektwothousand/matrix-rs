use std::fs::File;

use anyhow::bail;
use matrix_sdk::{
	ruma::{
		events::{
			room::message::{Relation, RoomMessageEventContent},
			AnyMessageLikeEvent, AnyTimelineEvent, OriginalSyncMessageLikeEvent,
		},
		EventId,
	},
	Room,
};
use teloxide::{
	payloads::{SendDocumentSetters, SendMessageSetters},
	prelude::Requester,
	types::{InputFile, MessageId},
	Bot,
};

use crate::{
	db::BridgedMessage,
	utils::{update_bridged_messages, BM_FILE_PATH},
};

async fn get_matrix_reply(
	matrix_event: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: Room,
) -> anyhow::Result<AnyMessageLikeEvent> {
	let Some(relation) = &matrix_event.content.relates_to else {
		bail!("");
	};
	let Relation::Reply { in_reply_to: reply } = relation else {
		bail!("");
	};
	let reply_event = room.event(&reply.event_id).await?;
	let AnyTimelineEvent::MessageLike(reply_message) = reply_event.event.deserialize()? else {
		bail!("");
	};

	Ok(reply_message)
}

fn find_bm(matrix_reply: AnyMessageLikeEvent) -> anyhow::Result<MessageId> {
	let id = EventId::parse(matrix_reply.event_id())?;
	let bm_file_path = &*BM_FILE_PATH;
	let bridged_messages: Vec<BridgedMessage> =
		match serde_json::from_reader(File::open(bm_file_path)?) {
			Ok(bm) => bm,
			Err(e) => bail!("{}", e),
		};
	let Some(bridged_message) = bridged_messages.iter().find(|bm| bm.matrix_id == id) else {
		bail!("message not found");
	};
	Ok(bridged_message.telegram_id.1)
}

pub async fn matrix_text_tg(
	tg_chat_id: i64,
	text: String,
	matrix_event: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: Room,
	bot: &Bot,
	preview: bool,
) -> anyhow::Result<()> {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	let reply_to_id = if let Ok(matrix_reply) = get_matrix_reply(matrix_event, room).await {
		let find_bm_result = find_bm(matrix_reply);
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
			update_bridged_messages(matrix_event.event_id.clone(), (t_msg.chat.id, t_msg.id))?;
		}
		Err(e) => eprintln!("{:?}", e),
	};
	Ok(())
}

pub async fn matrix_file_tg(
	tg_chat_id: i64,
	file: Vec<u8>,
	file_name: String,
	caption: &str,
	bot: &Bot,
) {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	match bot
		.send_document(chat_id, InputFile::memory(file).file_name(file_name))
		.caption(caption)
		.await
	{
		Ok(_) => (),
		Err(e) => eprintln!("{:?}", e),
	}
}
