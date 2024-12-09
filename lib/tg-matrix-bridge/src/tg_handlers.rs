use std::sync::Arc;

use anyhow::{
	bail,
	Context,
};
use matrix_sdk::ruma::events::{
	room::{
		message::{
			AddMentions,
			ForwardThread,
			ImageMessageEventContent,
			MessageType,
			OriginalRoomMessageEvent,
			VideoMessageEventContent,
		},
		MediaSource,
	},
	AnyMessageLikeEvent,
	AnyTimelineEvent,
};
use teloxide::{
	adaptors::Throttle,
	prelude::Requester,
	types::{
		MediaKind,
		Message,
		MessageKind,
	},
	Bot,
};

use crate::{
	bridge_structs::Bridge,
	bridge_utils::{
		get_bms,
		get_user_name,
		update_bridged_messages,
	},
};
use matrix_sdk::{
	ruma::{
		events::room::message::RoomMessageEventContent,
		OwnedEventId,
		RoomId,
	},
	Client,
	Room,
};

fn find_mx_event_id(tg_reply: &teloxide::types::Message, mx_chat: &str) -> Option<OwnedEventId> {
	let bms = get_bms(mx_chat)?;
	let bm = bms.iter().find(|t| t.telegram_id == (tg_reply.chat.id, tg_reply.id))?;
	Some(bm.matrix_id.clone())
}

async fn get_reply(msg: &Message, matrix_room: &Room) -> Option<AnyMessageLikeEvent> {
	let event_id = find_mx_event_id(msg, matrix_room.room_id().as_str())?;
	let kind = matrix_room.event(&event_id, None).await.ok()?.kind;
	let ev = match kind.raw().deserialize_as::<AnyTimelineEvent>().ok()? {
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
	//crate::timer::timer!();
	let user = get_user_name(&msg)?;
	let bot = <Throttle<Bot> as Clone>::clone(&bot).into_inner();
	let MessageKind::Common(ref msg_common) = msg.kind else {
		bail!("");
	};

	let bridge: &Bridge = {
		//crate::timer::timer!();
		bridges.iter().find(|b| b.tg_id == msg.chat.id.0).context("chat isn't bridged")?
	};

	let matrix_room =
		client.get_room(&RoomId::parse(&bridge.mx_id)?).context("can't get matrix room")?;

	let tg_file: Option<(String, mime::Mime)> = match msg_common.media_kind {
		MediaKind::Photo(ref m) => {
			let file_path = bot.get_file(&m.photo.last().unwrap().file.id).await?.path;
			let file_url = format!("https://api.telegram.org/file/bot{}/{file_path}", bot.token());
			Some((file_url, mime::IMAGE_JPEG))
		}
		MediaKind::Animation(ref m) => {
			let file_path = bot.get_file(&m.animation.file.id).await?.path;
			let file_url = format!("https://api.telegram.org/file/bot{}/{file_path}", bot.token());
			Some((file_url, "video/mp4".parse::<mime::Mime>()?))
		}
		MediaKind::Sticker(ref m) => {
			let mime = if m.sticker.is_video() {
				"video/webm".parse::<mime::Mime>()?
			} else {
				"image/webp".parse::<mime::Mime>()?
			};
			let file_path = bot.get_file(&m.sticker.file.id).await?.path;
			let file_url = format!("https://api.telegram.org/file/bot{}/{file_path}", bot.token());
			Some((file_url, mime))
		}
		MediaKind::Video(ref m) => {
			let file_path = bot.get_file(&m.video.file.id).await?.path;
			let file_url = format!("https://api.telegram.org/file/bot{}/{file_path}", bot.token());
			Some((file_url, "video/mp4".parse::<mime::Mime>()?))
		}
		MediaKind::Document(ref m) => {
			let file_path = bot.get_file(&m.document.file.id).await?.path;
			let file_url = format!("https://api.telegram.org/file/bot{}/{file_path}", bot.token());
			Some((file_url, mime::APPLICATION_OCTET_STREAM))
		}
		_ => None,
	};

	let mxc_uri = if tg_file.is_some() {
		let file_url = &tg_file.as_ref().unwrap().0;
		let file = reqwest::get(file_url).await?.bytes().await?.to_vec();
		let mime = &tg_file.as_ref().unwrap().1;
		Some(client.media().upload(mime, file, None).await?.content_uri)
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

	let caption = format!("(from {user}\n{})", msg.caption().unwrap_or(""));
	let message =
		match &msg_common.media_kind {
			MediaKind::Text(t) => {
				let text = format!("{}: {}", user, t.text);
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id, None).await?;
					let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
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
						caption,
						MediaSource::Plain(mxc_uri.unwrap()),
					);
					if let Some(event_id) = reply_owned_event_id {
						let event = matrix_room.event(&event_id, None).await?;
						let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
						RoomMessageEventContent::new(MessageType::Video(event_content))
							.make_reply_to(&msg, ForwardThread::No, AddMentions::No)
					} else {
						RoomMessageEventContent::new(MessageType::Video(event_content))
					}
				} else {
					let event_content = ImageMessageEventContent::new(
						caption,
						MediaSource::Plain(mxc_uri.unwrap()),
					);
					if let Some(event_id) = reply_owned_event_id {
						let event = matrix_room.event(&event_id, None).await?;
						let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
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
					caption,
					MediaSource::Plain(mxc_uri.unwrap()),
				);
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id, None).await?;
					let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
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
					caption,
					MediaSource::Plain(mxc_uri.unwrap()),
				);
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id, None).await?;
					let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
					RoomMessageEventContent::new(MessageType::Video(event_content)).make_reply_to(
						&msg,
						ForwardThread::No,
						AddMentions::No,
					)
				} else {
					RoomMessageEventContent::new(MessageType::Video(event_content))
				}
			}
			_ => bail!("unsupported media_kind"),
		};
	let sent_mx_msg = utils::matrix::send(matrix_room.clone().into(), message).await?;
	update_bridged_messages(
		sent_mx_msg.event_id,
		(msg.chat.id, msg.id),
		matrix_room.room_id().as_str(),
	)?;

	Ok(())
}
