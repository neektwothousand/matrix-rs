use std::{
	fs::File,
	path::Path,
	sync::{Arc, LazyLock},
};

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

use teloxide::{
	payloads::{SendDocumentSetters, SendMessageSetters, SendPhotoSetters},
	requests::Requester,
	types::{ChatId, InputFile, Message, MessageId},
	Bot,
};

use crate::db::BridgedMessage;

pub type MatrixMedia = (String, Vec<u8>, MessageType);

pub struct MatrixChat<'a> {
	pub id: &'a str,
}
pub struct TelegramChat {
	pub id: i64,
}
pub struct Bridge<'a> {
	pub matrix_chat: MatrixChat<'a>,
	pub telegram_chat: TelegramChat,
}

pub static BRIDGES: LazyLock<Vec<Bridge>> = LazyLock::new(|| {
	vec![
		// The Wired
		Bridge {
			matrix_chat: MatrixChat {
				id: "!vUWLFTSVVBjhMouZpF:matrix.org",
			},
			telegram_chat: TelegramChat {
				id: -1001402125530i64,
			},
		},
		// OTHERWORLD
		Bridge {
			matrix_chat: MatrixChat {
				id: "!6oZjqONVahFLOKTvut:matrix.archneek.me",
			},
			telegram_chat: TelegramChat {
				id: -1002152065322i64,
			},
		},
		// /d/egen
		Bridge {
			matrix_chat: MatrixChat {
				id: "!Lk2SLHrfW23HEhYGbA:matrix.archneek.me",
			},
			telegram_chat: TelegramChat {
				id: -1001621395690i64,
			},
		},
	]
});

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

pub async fn get_tg_bot() -> teloxide::Bot {
	let token = std::fs::read_to_string("tg_token").unwrap();
	Bot::new(token)
}

pub fn get_user_name(msg: &Message) -> anyhow::Result<String> {
	let name = if let Some(chat) = msg.sender_chat() {
		if chat.is_channel() {
			chat.title().unwrap().to_string()
		} else {
			bail!("chat isn't a channel, name not found")
		}
	} else if let Some(user) = msg.from() {
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
	rmp_serde::encode::write(&mut File::create(bm_file_path)?, &bridged_messages).unwrap();
	Ok(())
}

pub fn find_bm(reply: AnyMessageLikeEvent, mx_chat: &str) -> anyhow::Result<MessageId> {
	let id = EventId::parse(reply.event_id())?;
	let bm_file_path = format!("bridged_messages/{}.mpk", mx_chat);
	let bms: Vec<BridgedMessage> = match rmp_serde::decode::from_read(File::open(bm_file_path)?) {
		Ok(bms) => bms,
		Err(e) => bail!("{}", e),
	};
	let Some(bm) = bms.iter().find(|bm| bm.matrix_id == id) else {
		bail!("message not found");
	};
	Ok(bm.telegram_id.1)
}

pub enum TgMessageKind {
	Text,
	Photo,
	Sticker,
	Document,
}
#[derive(Default)]
pub struct ToTgData {
	pub bot: Option<Arc<Bot>>,
	pub chat_id: Option<ChatId>,
	pub message: Vec<u8>,
	pub tg_message_kind: Option<TgMessageKind>,
	pub file_name: Option<String>,
	pub preview: bool,
}
pub struct FromMxData<'a> {
	pub matrix_event: &'a OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	pub room: Room,
}
pub async fn get_to_tg_data<'a>(
	from_mx_data: &FromMxData<'a>,
	bot: Arc<Bot>,
	client: Client,
	bridge: &Bridge<'a>,
) -> anyhow::Result<ToTgData> {
	let mut tg_data = ToTgData {
		bot: Some(bot),
		chat_id: Some(ChatId(bridge.telegram_chat.id)),
		..Default::default()
	};
	let message_type = &from_mx_data.matrix_event.content.msgtype;
	match message_type {
		MessageType::Text(t) => {
			tg_data.message = t.body.as_bytes().to_vec();
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

pub async fn send_to_tg<'a>(
	to_tg_data: ToTgData,
	from_mx_data: FromMxData<'a>,
) -> anyhow::Result<()> {
	let null_id = -1i32;
	let matrix_reply = get_matrix_reply(from_mx_data.matrix_event, &from_mx_data.room).await;
	let matrix_chat_id = from_mx_data.room.room_id().as_str();
	let reply_to_id = if matrix_reply.is_ok() {
		let bm = find_bm(matrix_reply?, matrix_chat_id);
		bm.unwrap_or(MessageId(null_id))
	} else {
		MessageId(null_id)
	};
	let file_name = to_tg_data.file_name.unwrap_or("unknown".to_string());
	let t_msg: Message = match to_tg_data.tg_message_kind {
		Some(TgMessageKind::Text) => {
			to_tg_data
				.bot
				.context("bot not found")?
				.send_message(
					to_tg_data.chat_id.context("chat_id not found")?,
					String::from_utf8_lossy(&to_tg_data.message),
				)
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
				.disable_web_page_preview(to_tg_data.preview)
				.await?
		}
		Some(TgMessageKind::Photo) => {
			to_tg_data
				.bot
				.context("bot not found")?
				.send_photo(
					to_tg_data.chat_id.context("chat_id not found")?,
					InputFile::memory(to_tg_data.message).file_name(file_name),
				)
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
				.await?
		}
		Some(TgMessageKind::Document) => {
			to_tg_data
				.bot
				.context("bot not found")?
				.send_document(
					to_tg_data.chat_id.context("chat_id not found")?,
					InputFile::memory(to_tg_data.message).file_name(file_name),
				)
				.reply_to_message_id(reply_to_id)
				.allow_sending_without_reply(true)
				.await?
		}
		_ => bail!(""),
	};
	update_bridged_messages(
		from_mx_data.matrix_event.event_id.clone(),
		(t_msg.chat.id, t_msg.id),
		matrix_chat_id,
	)?;
	Ok(())
}
