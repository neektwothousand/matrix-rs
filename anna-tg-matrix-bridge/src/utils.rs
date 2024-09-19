use std::{fs::File, path::Path, sync::LazyLock};

use anyhow::{bail, Context};

use matrix_sdk::{
	ruma::{events::room::message::MessageType, OwnedEventId},
	Client,
};

use teloxide::{
	types::{ChatId, Message, MessageId},
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
