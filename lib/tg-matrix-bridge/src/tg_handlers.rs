use std::sync::Arc;

use anyhow::bail;
use anyhow::Context;
use matrix_sdk::ruma::events::room::message::AddMentions;
use matrix_sdk::ruma::events::room::message::FileMessageEventContent;
use matrix_sdk::ruma::events::room::message::ForwardThread;
use matrix_sdk::ruma::events::room::message::ImageMessageEventContent;
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::events::room::message::OriginalRoomMessageEvent;
use matrix_sdk::ruma::events::room::message::VideoMessageEventContent;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::AnyMessageLikeEvent;
use matrix_sdk::ruma::events::AnyTimelineEvent;
use teloxide::adaptors::Throttle;
use teloxide::prelude::Requester;
use teloxide::types::MediaKind;
use teloxide::types::Message;
use teloxide::types::MessageKind;
use teloxide::Bot;

use crate::bridge_structs::Bridge;
use crate::bridge_utils::get_bms;
use crate::bridge_utils::get_user_name;
use crate::bridge_utils::update_bridged_messages;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::OwnedEventId;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::Client;
use matrix_sdk::Room;

fn find_mx_event_id(tg_reply: &teloxide::types::Message, mx_chat: &str) -> Option<OwnedEventId> {
	let bms = get_bms(mx_chat)?;
	let bm = bms.iter().find(|t| t.telegram_id == (tg_reply.chat.id, tg_reply.id))?;
	Some(bm.matrix_id.clone())
}

async fn get_reply(msg: &Message, matrix_room: &Room) -> Option<AnyMessageLikeEvent> {
	let event_id = find_mx_event_id(msg, matrix_room.room_id().as_str())?;
	let kind = matrix_room.event(&event_id, None).await.ok()?.kind;
	let AnyTimelineEvent::MessageLike(ev) = kind.raw().deserialize_as::<AnyTimelineEvent>().ok()?
	else {
		return None;
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
			let Some(photo) = &m.photo.last() else {
				bail!("")
			};
			let file_path = bot.get_file(&photo.file.id).await?.path;
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

	let mxc_uri = if let Some(ref tg_file) = tg_file {
		let file_url = &tg_file.0;
		let mime = &tg_file.1;
		let file = reqwest::get(file_url).await?.bytes().await?.to_vec();
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
				let Some(tg_file) = tg_file else {
					bail!("")
				};
				let Some(mxc_uri) = mxc_uri else {
					bail!("")
				};
				if tg_file.1.type_() == "video" {
					let event_content =
						VideoMessageEventContent::new(caption, MediaSource::Plain(mxc_uri));
					if let Some(event_id) = reply_owned_event_id {
						let event = matrix_room.event(&event_id, None).await?;
						let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
						RoomMessageEventContent::new(MessageType::Video(event_content))
							.make_reply_to(&msg, ForwardThread::No, AddMentions::No)
					} else {
						RoomMessageEventContent::new(MessageType::Video(event_content))
					}
				} else {
					let event_content =
						ImageMessageEventContent::new(caption, MediaSource::Plain(mxc_uri));
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
				let Some(mxc_uri) = mxc_uri else {
					bail!("")
				};
				let event_content =
					VideoMessageEventContent::new(caption, MediaSource::Plain(mxc_uri));
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
				let Some(mxc_uri) = mxc_uri else {
					bail!("")
				};
				let event_content =
					FileMessageEventContent::new(caption, MediaSource::Plain(mxc_uri));
				if let Some(event_id) = reply_owned_event_id {
					let event = matrix_room.event(&event_id, None).await?;
					let msg = event.kind.raw().deserialize_as::<OriginalRoomMessageEvent>()?;
					RoomMessageEventContent::new(MessageType::File(event_content)).make_reply_to(
						&msg,
						ForwardThread::No,
						AddMentions::No,
					)
				} else {
					RoomMessageEventContent::new(MessageType::File(event_content))
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
