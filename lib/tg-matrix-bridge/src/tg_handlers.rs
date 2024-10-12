use std::sync::Arc;

use anyhow::{bail, Context};
use matrix_sdk::ruma::events::room::message::{
	AddMentions, ForwardThread, OriginalRoomMessageEvent,
};
use matrix_sdk::ruma::events::room::message::{
	ImageMessageEventContent, MessageType, VideoMessageEventContent,
};
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::AnyMessageLikeEvent;
use matrix_sdk::ruma::events::AnyTimelineEvent;
use teloxide::adaptors::Throttle;
use teloxide::types::{MediaKind, MessageKind};
use teloxide::Bot;
use teloxide::{prelude::Requester, types::Message};

use crate::bridge_structs::Bridge;
use crate::bridge_utils::{get_bms, get_user_name, update_bridged_messages};
use matrix_sdk::ruma::OwnedEventId;
use matrix_sdk::{
	ruma::{events::room::message::RoomMessageEventContent, RoomId},
	Client, Room,
};

fn find_mx_event_id(tg_reply: &teloxide::types::Message, mx_chat: &str) -> Option<OwnedEventId> {
	let bms = get_bms(mx_chat)?;
	let bm = bms
		.iter()
		.find(|t| t.telegram_id == (tg_reply.chat.id, tg_reply.id))?;
	Some(bm.matrix_id.clone())
}

async fn get_reply(msg: &Message, matrix_room: &Room) -> Option<AnyMessageLikeEvent> {
	let event_id = find_mx_event_id(msg, matrix_room.room_id().as_str())?;
	let raw_ev = matrix_room.event(&event_id).await.ok()?.event;
	let ev = match raw_ev.deserialize_as::<AnyTimelineEvent>().ok()? {
		AnyTimelineEvent::MessageLike(m) => m,
		_ => return None,
	};
	Some(ev)
}

pub async fn tg_to_mx(
	msg: Message,
	bot: Throttle<Bot>,
	client: Arc<Client>,
	bridges: Arc<Vec<Bridge>>,
) -> anyhow::Result<()> {
	let user = get_user_name(&msg)?;
	let bot = <Throttle<Bot> as Clone>::clone(&bot).into_inner();
	let MessageKind::Common(ref msg_common) = msg.kind else {
		bail!("");
	};

	let bridge: &Bridge = bridges
		.iter()
		.find(|b| b.tg_id == msg.chat.id.0)
		.context("chat isn't bridged")?;
	let matrix_room = client.get_room(&RoomId::parse(&bridge.mx_id)?).unwrap();

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

	let reply_owned_event_id = if let Some(msg_reply) = &msg_common.reply_to_message {
		if let Some(ev) = get_reply(msg_reply, &matrix_room).await {
			let event_id = match ev {
				AnyMessageLikeEvent::RoomMessage(ref m) => {
					m.as_original().context("redacted")?.event_id.clone()
				}
				AnyMessageLikeEvent::Sticker(ref m) => {
					m.as_original().context("redacted")?.event_id.clone()
				}
				ev => bail!("{:?}", ev),
			};
			Some(event_id)
		} else {
			None
		}
	} else {
		None
	};

	let message =
		match &msg_common.media_kind {
			MediaKind::Text(t) => {
				let text = format!("{}: {}", user, t.text);
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id).await?;
					let msg = event.event.deserialize_as::<OriginalRoomMessageEvent>()?;
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
				let text = format!("from {}:", user);
				let text_plain = RoomMessageEventContent::text_plain(text);
				matrix_room.send(text_plain).await?;
				if tg_file.unwrap().1.type_() == "video" {
					let event_content = VideoMessageEventContent::new(
						"tg_video".to_string(),
						MediaSource::Plain(mxc_uri.unwrap()),
					);
					if let Some(event_id) = reply_owned_event_id {
						let event = matrix_room.event(&event_id).await?;
						let msg = event.event.deserialize_as::<OriginalRoomMessageEvent>()?;
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
					if let Some(event_id) = reply_owned_event_id {
						let event = matrix_room.event(&event_id).await?;
						let msg = event.event.deserialize_as::<OriginalRoomMessageEvent>()?;
						RoomMessageEventContent::new(MessageType::Image(event_content))
							.make_reply_to(&msg, ForwardThread::No, AddMentions::No)
					} else {
						RoomMessageEventContent::new(MessageType::Image(event_content))
					}
				}
			}
			MediaKind::Animation(_) | MediaKind::Video(_) => {
				let text = format!("from {}:", user);
				let text_plain = RoomMessageEventContent::text_plain(text);
				matrix_room.send(text_plain).await?;
				let event_content = VideoMessageEventContent::new(
					"tg_video".to_string(),
					MediaSource::Plain(mxc_uri.unwrap()),
				);
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id).await?;
					let msg = event.event.deserialize_as::<OriginalRoomMessageEvent>()?;
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
				let text = format!("from {}:", user);
				let text_plain = RoomMessageEventContent::text_plain(text);
				matrix_room.send(text_plain).await?;
				let event_content = VideoMessageEventContent::new(
					"tg_document".to_string(),
					MediaSource::Plain(mxc_uri.unwrap()),
				);
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id).await?;
					let msg = event.event.deserialize_as::<OriginalRoomMessageEvent>()?;
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
	let sent_mx_msg = utils::matrix::send(matrix_room.clone().into(), message).await?;
	update_bridged_messages(
		sent_mx_msg.event_id,
		(msg.chat.id, msg.id),
		matrix_room.room_id().as_str(),
	)?;

	if let Some(caption) = msg.caption() {
		let message = RoomMessageEventContent::text_plain(caption);
		utils::matrix::send(matrix_room.into(), message).await?;
	}

	Ok(())
}
