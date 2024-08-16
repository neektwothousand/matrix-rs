use std::sync::LazyLock;

use anyhow::bail;

use matrix_sdk::{ruma::events::room::message::MessageType, Client};

use teloxide::{types::Message, Bot};

pub type MatrixMedia = (String, Vec<u8>);

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
				Ok((m.body, media))
			}
			MessageType::Image(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			MessageType::Audio(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			MessageType::Video(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			_ => bail!(""),
		}
	}
}

pub async fn get_matrix_media(
	client: Client,
	message_type: MessageType,
) -> anyhow::Result<(String, Vec<u8>)> {
	let Ok(media) =
		<(String, Vec<u8>) as GetMatrixMedia>::get_media(client.clone(), message_type.clone())
			.await
	else {
		bail!("");
	};
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
