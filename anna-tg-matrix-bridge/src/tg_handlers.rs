use std::sync::Arc;

use anyhow::Context;
use matrix_sdk::ruma::events::room::message::{
	FileMessageEventContent, ImageMessageEventContent, MessageType, VideoMessageEventContent,
};
use teloxide::Bot;
use teloxide::{prelude::Requester, types::Message};

use matrix_sdk::{
	ruma::{events::room::message::RoomMessageEventContent, RoomId},
	Client,
};

use crate::utils::{get_user_name, BRIDGES};

pub async fn tg_text_handler(
	msg: Message,
	_bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let text = msg.text().unwrap();
	let to_matrix_text = if let Some(reply) = msg.reply_to_message() {
		let mut chat_link = reply.chat.id.to_string();
		if let Some(reply_text) = reply.text() {
			format!("> {}\n{}: {text}", reply_text, user)
		} else {
			format!(
				"https://t.me/c/{}/{}\n{}:{text}",
				chat_link.drain(4..).as_str(),
				reply.id,
				user
			)
		}
	} else {
		format!("{}: {text}", user)
	};
	let mut matrix_chat_id = String::new();
	for bridge in BRIDGES.iter() {
		if msg.chat.id.0 == bridge.telegram_chat.id {
			matrix_chat_id = bridge.matrix_chat.id.to_string();
			break;
		}
	}
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(matrix_chat_id)?)
		.unwrap();
	let message = RoomMessageEventContent::text_plain(to_matrix_text);
	matrix_room.send(message).await?;
	anyhow::Ok(())
}

pub async fn tg_file_handler(
	msg: Message,
	bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let (file_id, content_type) = if let Some(document) = &msg.document() {
		(&document.file.id, mime::APPLICATION_OCTET_STREAM)
	} else if let Some(photo) = &msg.photo() {
		(&photo.last().unwrap().file.id, mime::IMAGE_JPEG)
	} else if let Some(animation) = &msg.animation() {
		(&animation.file.id, "video/mp4".parse::<mime::Mime>()?)
	} else if let Some(sticker) = &msg.sticker() {
		let content_type = if sticker.is_video() {
			"video/webm".parse::<mime::Mime>()?
		} else {
			"image/webp".parse::<mime::Mime>()?
		};
		(&sticker.file.id, content_type)
	} else {
		eprintln!("unknown message type: {:?}", &msg);
		return Ok(());
	};
	let file_path = bot.get_file(file_id).await.unwrap().path;
	let file_url = format!(
		"https://api.telegram.org/file/bot{}/{file_path}",
		bot.token()
	);
	let mut matrix_chat_id = String::new();
	for bridge in BRIDGES.iter() {
		if msg.chat.id.0 == bridge.telegram_chat.id {
			matrix_chat_id = bridge.matrix_chat.id.to_string();
			break;
		}
	}
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(matrix_chat_id)?)
		.context("")?;
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
				"tg_image".to_string(),
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
	matrix_room.send(file_message).await?;

	if let Some(caption) = msg.caption() {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await?;
	}

	Ok(())
}
