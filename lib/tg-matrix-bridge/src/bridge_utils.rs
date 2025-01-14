use std::fs::File;
use std::path::Path;

use anyhow::bail;

use matrix_sdk::media::MediaEventContent;
use matrix_sdk::ruma::events::room::message::FileMessageEventContent;
use matrix_sdk::ruma::events::room::message::ImageMessageEventContent;
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::events::room::message::Relation;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::events::room::message::VideoMessageEventContent;
use matrix_sdk::ruma::events::AnyMessageLikeEventContent;
use matrix_sdk::ruma::OwnedEventId;
use matrix_sdk::Client;

use teloxide::adaptors::throttle::Limits;
use teloxide::adaptors::Throttle;
use teloxide::payloads::SendDocumentSetters;
use teloxide::payloads::SendMessageSetters;
use teloxide::payloads::SendPhotoSetters;
use teloxide::payloads::SendStickerSetters;
use teloxide::payloads::SendVideoSetters;
use teloxide::prelude::Requester;
use teloxide::prelude::RequesterExt;
use teloxide::types::ChatId;
use teloxide::types::InputFile;
use teloxide::types::LinkPreviewOptions;
use teloxide::types::Message;
use teloxide::types::MessageId;
use teloxide::types::ReplyParameters;
use teloxide::Bot;
use teloxide::RequestError;

use crate::bridge_structs::BmMxData;
use crate::bridge_structs::BmTgData;
use crate::bridge_structs::Bridge;
use crate::bridge_structs::GetMatrixMedia;
use crate::bridge_structs::TgMessageKind;
use crate::db::BridgedMessage;

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

#[allow(clippy::missing_panics_doc)]
pub async fn get_tg_bot() -> Throttle<teloxide::Bot> {
	let token = std::fs::read_to_string("tg_token").unwrap();
	Bot::new(token).throttle(Limits::default())
}

pub fn get_user_name(msg: &Message) -> anyhow::Result<String> {
	let name = if let Some(chat) = &msg.sender_chat {
		if chat.is_channel() {
			chat.title().unwrap_or_default().to_string()
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
	let bm_file_path = format!("bridged_messages/{matrix_chat_id}.mpk");
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

#[must_use]
pub fn get_bms(mx_chat: &str) -> Option<Vec<BridgedMessage>> {
	let bm_file_path = format!("bridged_messages/{mx_chat}.mpk");
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
		AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent {
			relates_to,
			..
		}) => relates_to,
		_ => &None,
	};
	let is_reply = { matches!(&relates_to, Some(Relation::Reply { .. })) };
	match message_type {
		MessageType::Text(t) => {
			tg_data.message = {
				if is_reply {
					match t.body.split_once("\n\n") {
						Some(split) => split.1.as_bytes().to_vec(),
						None => bail!("couldn't find newline split"),
					}
				} else {
					t.body.as_bytes().to_vec()
				}
			};
			tg_data.tg_message_kind = Some(TgMessageKind::Text);
			tg_data.is_preview_disabled = false;
		}
		MessageType::Image(i) => {
			let ec = ImageMessageEventContent::new(i.body.clone(), i.source.clone());
			tg_data.message = get_event_content_vec(ec, &client).await?;
			tg_data.tg_message_kind = Some(TgMessageKind::Photo);
			tg_data.caption = Some(i.body.clone());
		}
		MessageType::Video(v) => {
			let ec = VideoMessageEventContent::new(v.body.clone(), v.source.clone());
			let message = get_event_content_vec(ec, &client).await?;
			tg_data.message = message;
			tg_data.tg_message_kind = Some(TgMessageKind::Video);
			tg_data.caption = Some(v.body.clone());
		}
		MessageType::File(f) => {
			let ec = FileMessageEventContent::new(f.body.clone(), f.source.clone());
			let message = get_event_content_vec(ec, &client).await?;
			tg_data.message = message;
			tg_data.tg_message_kind = Some(TgMessageKind::Document);
			tg_data.caption = Some(f.body.clone());
		}
		t => bail!("unsupported type: {:?}", t),
	}
	Ok(tg_data)
}

async fn get_event_content_vec(
	ec: impl MediaEventContent,
	client: &Client,
) -> anyhow::Result<Vec<u8>> {
	let message = match client.media().get_file(&ec, false).await {
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
) -> Result<Message, teloxide::RequestError> {
	let caption = format!("(from: {from_user})\n{}", to_tg_data.caption.unwrap_or_default());
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
				let input_file = InputFile::memory(to_tg_data.message.clone());
				bot.send_photo(chat_id, input_file)
					.caption(&caption)
					.reply_parameters(reply_params.clone())
					.await
			}
			Some(TgMessageKind::Sticker) => {
				let input_file = InputFile::memory(to_tg_data.message.clone());
				bot.send_sticker(chat_id, input_file).reply_parameters(reply_params.clone()).await
			}
			Some(TgMessageKind::Video) => {
				let input_file = InputFile::memory(to_tg_data.message.clone());
				bot.send_video(chat_id, input_file)
					.caption(&caption)
					.reply_parameters(reply_params.clone())
					.await
			}
			Some(TgMessageKind::Document) => {
				let input_file = InputFile::memory(to_tg_data.message.clone());
				bot.send_document(chat_id, input_file)
					.caption(&caption)
					.reply_parameters(reply_params.clone())
					.await
			}
			None => unreachable!(""),
		};
		match res {
			Err(RequestError::Network(e)) if e.is_timeout() => {
				continue;
			}
			x => {
				return x;
			}
		}
	}
}
