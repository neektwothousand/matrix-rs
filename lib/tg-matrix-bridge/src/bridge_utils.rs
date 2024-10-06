use std::{fs::File, path::Path, sync::LazyLock};

use anyhow::{bail, Context};

use matrix_sdk::{
	media::MediaEventContent,
	ruma::{
		events::{
			room::message::{
				FileMessageEventContent, ImageMessageEventContent, MessageType, Relation,
				RoomMessageEventContent,
			},
			AnyMessageLikeEvent, AnyTimelineEvent, OriginalSyncMessageLikeEvent,
		},
		EventId, OwnedEventId,
	},
	Client, Room,
};

use serde::Deserialize;
use teloxide::{
	adaptors::{throttle::Limits, Throttle},
	payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters},
	prelude::{Requester, RequesterExt},
	types::{ChatId, InputFile, LinkPreviewOptions, Message, MessageId, ReplyParameters},
	Bot, RequestError,
};

use crate::db::BridgedMessage;

pub type MatrixMedia = (String, Vec<u8>, MessageType);

#[derive(Deserialize)]
pub struct Bridge {
	pub mx_id: String,
	pub tg_id: i64,
}

pub enum TgMessageKind {
	Text,
	Photo,
	Sticker,
	Document,
}
#[derive(Default)]
pub struct BmTgData {
	pub bot: Option<Throttle<Bot>>,
	pub chat_id: Option<ChatId>,
	pub message: Vec<u8>,
	pub tg_message_kind: Option<TgMessageKind>,
	pub file_name: Option<String>,
	pub preview: bool,
}
pub struct BmMxData<'a> {
	pub mx_event: &'a OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	pub mx_msg_type: &'a MessageType,
	pub room: Room,
}

pub static BM_FILE_PATH: LazyLock<&str> = LazyLock::new(|| "bridged_messages/");

pub trait GetMatrixMedia {
	fn get_media(
		client: Client,
		media: MessageType,
	) -> impl std::future::Future<Output = anyhow::Result<MatrixMedia>> + Send;
}

impl GetMatrixMedia for MatrixMedia {
	async fn get_media(client: Client, media: MessageType) -> anyhow::Result<MatrixMedia> {
		match media {
			MessageType::File(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				let value = serde_json::to_value(&m)?;
				let body = value.get("body").context(dbg!("body not found"))?;
				Ok((body.to_string(), media, MessageType::File(m)))
			}
			MessageType::Image(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body.clone(), media, MessageType::Image(m)))
			}
			MessageType::Audio(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body.clone(), media, MessageType::Audio(m)))
			}
			MessageType::Video(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body.clone(), media, MessageType::Video(m)))
			}
			_ => bail!(""),
		}
	}
}

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

fn get_bms(mx_chat: &str) -> Option<Vec<BridgedMessage>> {
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

pub fn find_mx_event_id(tg_reply: &Message, mx_chat: &str) -> Option<OwnedEventId> {
	let bms = get_bms(mx_chat)?;
	let bm = bms
		.iter()
		.find(|t| t.telegram_id == (tg_reply.chat.id, tg_reply.id))?;
	Some(bm.matrix_id.clone())
}

pub fn find_tg_msg_id(reply: AnyMessageLikeEvent, mx_chat: &str) -> Option<MessageId> {
	let bms = get_bms(mx_chat)?;
	let id = match EventId::parse(reply.event_id()) {
		Ok(id) => id,
		Err(e) => {
			log::error!("{}:{}", line!(), e);
			return None;
		}
	};
	let bm = bms.iter().find(|bm| bm.matrix_id == id)?;
	Some(bm.telegram_id.1)
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
	let message_type = &from_mx_data.mx_event.content.msgtype;
	let is_reply = {
		matches!(
			&from_mx_data.mx_event.content.relates_to,
			Some(Relation::Reply { .. })
		)
	};
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
		MessageType::File(f) => {
			let ec = FileMessageEventContent::new(f.body.clone(), f.source.clone());
			let message = get_event_content_vec(ec, &client).await?;
			tg_data.message = message;
			tg_data.tg_message_kind = Some(TgMessageKind::Document);
			tg_data.file_name = Some(f.body.clone())
		}
		_ => bail!(""),
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

pub async fn get_matrix_reply(
	matrix_event: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: &Room,
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
