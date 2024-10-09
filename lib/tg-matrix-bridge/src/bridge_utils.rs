use std::{fs::File, path::Path};

use anyhow::bail;

use matrix_sdk::{
	media::MediaEventContent,
	ruma::{
		events::{
			room::message::{
				FileMessageEventContent, ImageMessageEventContent, MessageType, Relation,
				RoomMessageEventContent, VideoMessageEventContent,
			}, AnyMessageLikeEventContent,
		}, OwnedEventId,
	},
	Client,
};

use teloxide::{
	adaptors::{throttle::Limits, Throttle},
	payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters},
	prelude::{Requester, RequesterExt},
	types::{ChatId, InputFile, LinkPreviewOptions, Message, MessageId, ReplyParameters},
	Bot, RequestError,
};

use crate::{
	bridge_structs::{BmMxData, BmTgData, Bridge, GetMatrixMedia, TgMessageKind},
	db::BridgedMessage,
};

pub async fn get_matrix_media(
	client: Client,
	message_type: MessageType,
) -> anyhow::Result<(String, Vec<u8>, MessageType)> {
	let media = <(String, Vec<u8>, MessageType) as GetMatrixMedia>::get_media(
		client.clone(),
		message_type.clone(),
	)
	.await?;
	Ok(media)
}

pub async fn get_tg_bot() -> Throttle<teloxide::Bot> {
	let token = std::fs::read_to_string("tg_token").unwrap();
	Bot::new(token).throttle(Limits::default())
}

pub fn get_user_name(msg: &Message) -> anyhow::Result<String> {
	let name = if let Some(chat) = &msg.sender_chat {
		if chat.is_channel() {
			chat.title().unwrap().to_string()
		} else {
			bail!("chat isn't a channel, name not found")
		}
	} else if let Some(user) = &msg.from {
		user.full_name()
	} else {
		bail!("user doesn't have \"from\" field")
	};
	Ok(name)
}

pub fn update_bridged_messages(
	matrix_event_id: OwnedEventId,
	telegram_event_id: (ChatId, MessageId),
	matrix_chat_id: &str,
) -> anyhow::Result<()> {
	let bm_file_path = format!("bridged_messages/{}.mpk", matrix_chat_id);
	if !Path::new(&bm_file_path).exists() {
		File::create_new(&bm_file_path)?;
	}
	let mut bridged_messages: Vec<BridgedMessage> =
		match rmp_serde::decode::from_read(File::open(&bm_file_path)?) {
			Ok(bm) => bm,
			Err(e) => {
				log::debug!("{}:{}:{}", file!(), line!(), e);
				vec![]
			}
		};
	bridged_messages.push(BridgedMessage {
		matrix_id: matrix_event_id,
		telegram_id: (telegram_event_id.0, telegram_event_id.1),
	});
	bridged_messages.reverse();
	bridged_messages.truncate(1000);
	bridged_messages.reverse();
	rmp_serde::encode::write(&mut File::create(bm_file_path)?, &bridged_messages)?;
	Ok(())
}

pub fn get_bms(mx_chat: &str) -> Option<Vec<BridgedMessage>> {
	let bm_file_path = format!("bridged_messages/{}.mpk", mx_chat);
	let file = match File::open(bm_file_path) {
		Ok(f) => f,
		Err(e) => {
			log::error!("{}:{}", line!(), e);
			return None;
		}
	};
	match rmp_serde::from_read(file) {
		Ok(bms) => Some(bms),
		Err(e) => {
			log::error!("{}:{}", line!(), e);
			None
		}
	}
}



pub async fn get_to_tg_data<'a>(
	from_mx_data: &BmMxData<'a>,
	bot: Throttle<Bot>,
	client: Client,
	bridge: &Bridge,
) -> anyhow::Result<BmTgData> {
	let mut tg_data = BmTgData {
		bot: Some(bot),
		chat_id: Some(ChatId(bridge.tg_id)),
		..Default::default()
	};
	let message_type = &from_mx_data.mx_msg_type;
	let relates_to = match &from_mx_data.mx_event.content {
		AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent { relates_to, .. }) => {
			relates_to
		}
		_ => &None,
	};
	let is_reply = { matches!(&relates_to, Some(Relation::Reply { .. })) };
	match message_type {
		MessageType::Text(t) => {
			tg_data.message = {
				if is_reply {
					t.body.split_once("\n\n").unwrap().1.as_bytes().to_vec()
				} else {
					t.body.as_bytes().to_vec()
				}
			};
			tg_data.tg_message_kind = Some(TgMessageKind::Text);
			tg_data.preview = true;
		}
		MessageType::Image(i) => {
			let ec = ImageMessageEventContent::new(i.body.clone(), i.source.clone());
			let message = get_event_content_vec(ec, &client).await?;
			tg_data.message = message;
			tg_data.tg_message_kind = Some(TgMessageKind::Photo);
			tg_data.file_name = Some(i.body.clone());
		}
		MessageType::Video(v) => {
			let ec = VideoMessageEventContent::new(v.body.clone(), v.source.clone());
			let message = get_event_content_vec(ec, &client).await?;
			tg_data.message = message;
			tg_data.tg_message_kind = Some(TgMessageKind::Document);
			tg_data.file_name = Some(v.body.clone())
		}
		MessageType::File(f) => {
			let ec = FileMessageEventContent::new(f.body.clone(), f.source.clone());
			let message = get_event_content_vec(ec, &client).await?;
			tg_data.message = message;
			tg_data.tg_message_kind = Some(TgMessageKind::Document);
			tg_data.file_name = Some(f.body.clone())
		}
		t => bail!("unsupported type: {:?}", t),
	}
	Ok(tg_data)
}

async fn get_event_content_vec(
	ec: impl MediaEventContent,
	client: &Client,
) -> anyhow::Result<Vec<u8>> {
	let message = match client.media().get_file(ec, false).await {
		Ok(Some(message)) => message,
		Ok(None) => bail!("{}:couldn't get content vec", line!()),
		Err(e) => bail!("{}:couldn't get content vec: {}", line!(), e),
	};
	Ok(message)
}

pub async fn bot_send_request(
	bot: Throttle<Bot>,
	to_tg_data: BmTgData,
	chat_id: ChatId,
	reply_params: ReplyParameters,
	link_preview: LinkPreviewOptions,
	from_user: String,
) -> anyhow::Result<Message> {
	let file_name = to_tg_data.file_name.unwrap_or("unknown".to_string());
	loop {
		let res = match to_tg_data.tg_message_kind {
			Some(TgMessageKind::Text) => {
				let text = format!(
					"{from_user}: {}",
					String::from_utf8_lossy(&to_tg_data.message.clone())
				);
				bot.send_message(chat_id, text)
					.reply_parameters(reply_params.clone())
					.link_preview_options(link_preview.clone())
					.await
			}
			Some(TgMessageKind::Photo) => {
				let photo =
					InputFile::memory(to_tg_data.message.clone()).file_name(file_name.clone());
				bot.send_photo(chat_id, photo)
					.caption(from_user.clone())
					.reply_parameters(reply_params.clone())
					.await
			}
			Some(TgMessageKind::Document) => {
				let document =
					InputFile::memory(to_tg_data.message.clone()).file_name(file_name.clone());
				bot.send_document(chat_id, document)
					.caption(from_user.clone())
					.reply_parameters(reply_params.clone())
					.await
			}
			_ => bail!(""),
		};
		match res {
			Ok(message) => return Ok(message),
			Err(e) => match e {
				RequestError::Network(e) => {
					if e.is_timeout() {
						continue;
					} else {
						bail!("{:?}", e);
					}
				}
				RequestError::Io(e) => bail!("{:?}", e),
				RequestError::InvalidJson { source, raw } => bail!("{:?} {:?}", source, raw),
				RequestError::Api(e) => bail!("{:?}", e),
				RequestError::MigrateToChatId(e) => bail!("{:?}", e),
				RequestError::RetryAfter(e) => bail!("{:?}", e),
			},
		}
	}
}
