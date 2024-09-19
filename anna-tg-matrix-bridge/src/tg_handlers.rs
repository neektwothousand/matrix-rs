use std::fs::File;
use std::sync::Arc;

use anyhow::{bail, Context};
use matrix_sdk::ruma::events::room::message::{
	AddMentions, FileMessageEventContent, ForwardThread, ImageMessageEventContent, MessageType,
	VideoMessageEventContent,
};
use matrix_sdk::ruma::events::OriginalMessageLikeEvent;
use matrix_sdk::ruma::OwnedEventId;
use teloxide::Bot;
use teloxide::{prelude::Requester, types::Message};

use matrix_sdk::{
	ruma::{events::room::message::RoomMessageEventContent, RoomId},
	Client,
};

use crate::db::BridgedMessage;
use crate::utils::{get_user_name, update_bridged_messages, Bridge, BRIDGES};

fn find_bm(tg_reply: &Message, matrix_chat_id: &str) -> anyhow::Result<OwnedEventId> {
	let bm_file_path = format!("bridged_messages/{}.mpk", matrix_chat_id);
	let bridged_messages: Vec<BridgedMessage> =
		match rmp_serde::from_read(File::open(bm_file_path)?) {
			Ok(bm) => bm,
			Err(e) => bail!("{}", e),
		};
	let Some(bridged_message) = bridged_messages
		.iter()
		.find(|t| t.telegram_id == (tg_reply.chat.id, tg_reply.id))
	else {
		bail!("message not found");
	};
	Ok(bridged_message.matrix_id.clone())
}

pub async fn tg_text_handler(
	t_msg: Message,
	_bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&t_msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let text = t_msg.text().unwrap();
	let to_matrix_text = format!("{}: {text}", user);
	let bridge: &Bridge = BRIDGES
		.iter()
		.find(|b| b.telegram_chat.id == t_msg.chat.id.0)
		.context("chat isn't bridged")?;
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(bridge.matrix_chat.id)?)
		.unwrap();
	let message = if let Some(reply) = t_msg.reply_to_message() {
		let find_bm_result = find_bm(reply, bridge.matrix_chat.id);
		if let Ok(matrix_event_id) = find_bm_result {
			let original_message: OriginalMessageLikeEvent<RoomMessageEventContent> = matrix_room
				.event(&matrix_event_id)
				.await?
				.event
				.deserialize_as()?;
			RoomMessageEventContent::text_plain(to_matrix_text).make_reply_to(
				&original_message,
				ForwardThread::No,
				AddMentions::No,
			)
		} else {
			log::info!("{:?}", find_bm_result);
			RoomMessageEventContent::text_plain(to_matrix_text)
		}
	} else {
		RoomMessageEventContent::text_plain(to_matrix_text)
	};
	let sent_mt_msg = matrix_room.send(message).await?;
	update_bridged_messages(
		sent_mt_msg.event_id,
		(t_msg.chat.id, t_msg.id),
		matrix_room.room_id().as_str(),
	)?;
	anyhow::Ok(())
}

pub async fn tg_file_handler(
	t_msg: Message,
	bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&t_msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let (file_id, content_type) = if let Some(document) = &t_msg.document() {
		(&document.file.id, mime::APPLICATION_OCTET_STREAM)
	} else if let Some(photo) = &t_msg.photo() {
		(&photo.last().unwrap().file.id, mime::IMAGE_JPEG)
	} else if let Some(animation) = &t_msg.animation() {
		(&animation.file.id, "video/mp4".parse::<mime::Mime>()?)
	} else if let Some(sticker) = &t_msg.sticker() {
		let content_type = if sticker.is_video() {
			"video/webm".parse::<mime::Mime>()?
		} else {
			"image/webp".parse::<mime::Mime>()?
		};
		(&sticker.file.id, content_type)
	} else {
		eprintln!("unknown message type: {:?}", &t_msg);
		return Ok(());
	};
	let file_path = bot.get_file(file_id).await?.path;
	let file_url = format!(
		"https://api.telegram.org/file/bot{}/{file_path}",
		bot.token()
	);
	let bridge: &Bridge = BRIDGES
		.iter()
		.find(|b| b.telegram_chat.id == t_msg.chat.id.0)
		.context("chat isn't bridged")?;
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(bridge.matrix_chat.id)?)
		.unwrap();
	let sender = format!("from {}:", user);
	let message = RoomMessageEventContent::text_plain(sender);
	matrix_room.send(message).await?;

	let document = reqwest::get(file_url).await?.bytes().await?;
	let mxc_uri = matrix_client
		.media()
		.upload(&content_type, document.to_vec())
		.await?
		.content_uri;

	let file_message = match content_type.type_().as_str() {
		"image" => {
			let event_content = ImageMessageEventContent::new(
				"tg_image".to_string(),
				matrix_sdk::ruma::events::room::MediaSource::Plain(mxc_uri),
			);
			RoomMessageEventContent::new(MessageType::Image(event_content))
		}
		"video" => {
			let event_content = VideoMessageEventContent::new(
				"tg_video".to_string(),
				matrix_sdk::ruma::events::room::MediaSource::Plain(mxc_uri),
			);
			RoomMessageEventContent::new(MessageType::Video(event_content))
		}
		_ => {
			let event_content = FileMessageEventContent::new(
				"tg_document".to_string(),
				matrix_sdk::ruma::events::room::MediaSource::Plain(mxc_uri),
			);
			RoomMessageEventContent::new(MessageType::File(event_content))
		}
	};
	let sent_mt_msg = matrix_room.send(file_message).await?;
	update_bridged_messages(
		sent_mt_msg.event_id,
		(t_msg.chat.id, t_msg.id),
		matrix_room.room_id().as_str(),
	)?;

	if let Some(caption) = t_msg.caption() {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await?;
	}

	Ok(())
}
