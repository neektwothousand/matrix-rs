use std::fs::File;
use std::sync::Arc;

use anyhow::{bail, Context};
use matrix_sdk::ruma::events::room::message::{AddMentions, ForwardThread};
use matrix_sdk::ruma::events::room::message::{
	ImageMessageEventContent, MessageType, VideoMessageEventContent,
};
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::OriginalMessageLikeEvent;
use matrix_sdk::ruma::OwnedEventId;
use teloxide::types::{MediaKind, MessageKind};
use teloxide::Bot;
use teloxide::{prelude::Requester, types::Message};

use matrix_sdk::{
	ruma::{events::room::message::RoomMessageEventContent, RoomId},
	Client, Room,
};

use crate::db::BridgedMessage;
use crate::utils::{get_user_name, update_bridged_messages, Bridge, BRIDGES};

fn find_bm(tg_reply: &Message, matrix_chat_id: &str) -> anyhow::Result<OwnedEventId> {
	let bm_file_path = format!("bridged_messages/{}.mpk", matrix_chat_id);
	let bridged_messages: Vec<BridgedMessage> =
		match rmp_serde::from_read(File::open(bm_file_path)?) {
			Ok(bm) => bm,
			Err(e) => {
				log::error!("{}:{}", line!(), e);
				bail!("");
			}
		};
	let Some(bridged_message) = bridged_messages
		.iter()
		.find(|t| t.telegram_id == (tg_reply.chat.id, tg_reply.id))
	else {
		let e = "message not found";
		log::error!("{}:{}", line!(), e);
		bail!("");
	};
	Ok(bridged_message.matrix_id.clone())
}

async fn get_reply_to_message<'a>(
	msg: &Message,
	matrix_room: &Room,
) -> Option<OriginalMessageLikeEvent<RoomMessageEventContent>> {
	use matrix_sdk::ruma::events::AnyMessageLikeEvent;
	use matrix_sdk::ruma::events::AnyTimelineEvent;
	let Ok(event_id) = find_bm(msg, matrix_room.room_id().as_str()) else {
		return None;
	};

	let raw_ev = matrix_room.event(&event_id).await.ok()?.event;
	let ev = match raw_ev.deserialize_as::<AnyTimelineEvent>().ok()? {
		AnyTimelineEvent::MessageLike(m) => m,
		_ => return None,
	};

	let msg_like_event = match ev {
		AnyMessageLikeEvent::RoomMessage(m) => m,
		_ => return None,
	};
	msg_like_event.as_original().cloned()
}

pub async fn tg_to_mx(msg: Message, bot: Arc<Bot>, client: Client) -> anyhow::Result<()> {
	let user = get_user_name(&msg)?;
	let MessageKind::Common(ref msg_common) = msg.kind else {
		bail!("");
	};

	let bridge: &Bridge = BRIDGES
		.iter()
		.find(|b| b.telegram_chat.id == msg.chat.id.0)
		.context("chat isn't bridged")?;
	let matrix_room = client
		.get_room(&RoomId::parse(bridge.matrix_chat.id)?)
		.unwrap();
	let reply_to_message = if let Some(msg) = &msg_common.reply_to_message {
		get_reply_to_message(&msg, &matrix_room).await
	} else {
		None
	};

	let tg_file: Option<(String, mime::Mime)> = match msg_common.media_kind {
		MediaKind::Photo(ref m) => {
			let file_path = bot.get_file(&m.photo.last().unwrap().file.id).await?.path;
			let file_url = format!(
				"https://api.telegram.org/file/bot{}/{file_path}",
				bot.token()
			);
			Some((file_url, mime::IMAGE_JPEG))
		}
		MediaKind::Animation(ref m) => {
			let file_path = bot.get_file(&m.animation.file.id).await?.path;
			let file_url = format!(
				"https://api.telegram.org/file/bot{}/{file_path}",
				bot.token()
			);
			Some((file_url, "video/mp4".parse::<mime::Mime>()?))
		}
		MediaKind::Sticker(ref m) => {
			let mime = if m.sticker.is_video() {
				"video/webm".parse::<mime::Mime>()?
			} else {
				"image/webp".parse::<mime::Mime>()?
			};
			let file_path = bot.get_file(&m.sticker.file.id).await?.path;
			let file_url = format!(
				"https://api.telegram.org/file/bot{}/{file_path}",
				bot.token()
			);
			Some((file_url, mime))
		}
		MediaKind::Video(ref m) => {
			let file_path = bot.get_file(&m.video.file.id).await?.path;
			let file_url = format!(
				"https://api.telegram.org/file/bot{}/{file_path}",
				bot.token()
			);
			Some((file_url, "video/mp4".parse::<mime::Mime>()?))
		}
		MediaKind::Document(ref m) => {
			let file_path = bot.get_file(&m.document.file.id).await?.path;
			let file_url = format!(
				"https://api.telegram.org/file/bot{}/{file_path}",
				bot.token()
			);
			Some((file_url, mime::APPLICATION_OCTET_STREAM))
		}
		_ => None,
	};

	let mxc_uri = if tg_file.is_some() {
		let file_url = &tg_file.as_ref().unwrap().0;
		let file = reqwest::get(file_url).await?.bytes().await?.to_vec();
		let mime = &tg_file.as_ref().unwrap().1;
		Some(client.media().upload(mime, file).await?.content_uri)
	} else {
		None
	};

	let message =
		match &msg_common.media_kind {
			MediaKind::Text(t) => {
				let text = format!("{}: {}", user, t.text);

				if let Some(msg) = reply_to_message {
					RoomMessageEventContent::text_plain(text).make_reply_to(
						&msg,
						ForwardThread::No,
						AddMentions::No,
					)
				} else {
					RoomMessageEventContent::text_plain(text)
				}
			}
			MediaKind::Photo(_) | MediaKind::Sticker(_) => {
				if tg_file.unwrap().1.type_() == "video" {
					let event_content = VideoMessageEventContent::new(
						"tg_video".to_string(),
						MediaSource::Plain(mxc_uri.unwrap()),
					);
					if let Some(msg) = reply_to_message {
						RoomMessageEventContent::new(MessageType::Video(event_content))
							.make_reply_to(&msg, ForwardThread::No, AddMentions::No)
					} else {
						RoomMessageEventContent::new(MessageType::Video(event_content))
					}
				} else {
					let event_content = ImageMessageEventContent::new(
						"tg_image".to_string(),
						MediaSource::Plain(mxc_uri.unwrap()),
					);
					if let Some(msg) = reply_to_message {
						RoomMessageEventContent::new(MessageType::Image(event_content))
							.make_reply_to(&msg, ForwardThread::No, AddMentions::No)
					} else {
						RoomMessageEventContent::new(MessageType::Image(event_content))
					}
				}
			}
			MediaKind::Animation(_) | MediaKind::Video(_) => {
				let event_content = VideoMessageEventContent::new(
					"tg_video".to_string(),
					MediaSource::Plain(mxc_uri.unwrap()),
				);
				if let Some(msg) = reply_to_message {
					RoomMessageEventContent::new(MessageType::Video(event_content)).make_reply_to(
						&msg,
						ForwardThread::No,
						AddMentions::No,
					)
				} else {
					RoomMessageEventContent::new(MessageType::Video(event_content))
				}
			}
			MediaKind::Document(_) => {
				let event_content = VideoMessageEventContent::new(
					"tg_document".to_string(),
					MediaSource::Plain(mxc_uri.unwrap()),
				);
				if let Some(msg) = reply_to_message {
					RoomMessageEventContent::new(MessageType::Video(event_content)).make_reply_to(
						&msg,
						ForwardThread::No,
						AddMentions::No,
					)
				} else {
					RoomMessageEventContent::new(MessageType::Video(event_content))
				}
			}
			_ => bail!("{}:unsupported media_kind", line!()),
		};
	let sent_mx_msg = matrix_room.send(message).await?;
	update_bridged_messages(
		sent_mx_msg.event_id,
		(msg.chat.id, msg.id),
		matrix_room.room_id().as_str(),
	)?;

	if let Some(caption) = msg.caption() {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await?;
	}

	Ok(())
}
