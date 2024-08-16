use std::sync::{Arc, LazyLock};

use anyhow::{bail, Context};

use matrix_sdk::{
	ruma::{
		events::room::message::{FileMessageEventContent, ImageMessageEventContent, MessageType, RoomMessageEventContent},
		RoomId,
	},
	Client,
};

use teloxide::{
	net::Download,
	payloads::{SendDocumentSetters, SendMessageSetters},
	requests::Requester,
	types::{InputFile, Message},
	Bot,
};

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

fn get_user_name(msg: &Message) -> anyhow::Result<String> {
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

pub async fn tg_text_handler(
	msg: Message,
	_bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let Some(text) = msg.text() else {
		bail!("");
	};
	let to_matrix_text = if let Some(reply) = msg.reply_to_message() {
		let mut chat_link = reply.chat.id.to_string();
		if let Some(reply_text) = reply.text() {
			format!("> {}\n{}: {text}", reply_text, user)
		} else {
			format!(
				"https://t.me/c/{}/{}\n{}:{text}",
				chat_link.drain(4..).as_str(),
				reply.id,
				user
			)
		}
	} else {
		format!("{}: {text}", user)
	};
	let mut matrix_chat_id = String::new();
	for bridge in BRIDGES.iter() {
		if msg.chat.id.0 == bridge.telegram_chat.id {
			matrix_chat_id = bridge.matrix_chat.id.to_string();
			break;
		}
	}
	tg_text_matrix(matrix_chat_id, &to_matrix_text, matrix_client).await;
	anyhow::Ok(())
}

pub async fn tg_file_handler(
	msg: Message,
	bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let Some(document) = msg.document() else {
		bail!("");
	};
	let document_id = &document.file.id;
	let document_path = bot.get_file(document_id).await.unwrap().path;
	let document_file_path = format!("/tmp/{}:{}:{}.jpg", user, msg.id, msg.chat.id);
	let mut document_dst = tokio::fs::File::create(&document_file_path).await.unwrap();
	bot.download_file(&document_path, &mut document_dst)
		.await
		.unwrap();
	let sender = format!("from {}:", user);
	let mut matrix_chat_id = String::new();
	for bridge in BRIDGES.iter() {
		if msg.chat.id.0 == bridge.telegram_chat.id {
			matrix_chat_id = bridge.matrix_chat.id.to_string();
			break;
		}
	}
	tg_document_matrix(
		matrix_chat_id.to_string(),
		document_file_path,
		sender,
		msg.caption(),
		matrix_client,
	)
	.await?;
	Ok(())
}
pub async fn tg_photo_handler(
	msg: Message,
	bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let user = match get_user_name(&msg) {
		Ok(user) => user,
		Err(e) => {
			eprintln!("{:?}", e);
			return Ok(());
		}
	};
	let Some(media_photo) = msg.photo() else {
		bail!("");
	};
	let photo_id = &media_photo.last().unwrap().file.id;
	let photo_path = bot.get_file(photo_id).await.unwrap().path;
	let photo_file_path = format!("/tmp/{}:{}:{}.jpg", user, msg.id, msg.chat.id);
	let mut photo_dst = tokio::fs::File::create(&photo_file_path).await.unwrap();
	bot.download_file(&photo_path, &mut photo_dst)
		.await
		.unwrap();
	let sender = format!("from {}:", user);
	let mut matrix_chat_id = String::new();
	for bridge in BRIDGES.iter() {
		if msg.chat.id.0 == bridge.telegram_chat.id {
			matrix_chat_id = bridge.matrix_chat.id.to_string();
			break;
		}
	}
	tg_photo_matrix(
		matrix_chat_id.to_string(),
		photo_file_path,
		sender,
		msg.caption(),
		matrix_client,
	)
	.await?;
	Ok(())
}

pub async fn matrix_text_tg(tg_chat_id: i64, text: String, bot: &Bot, preview: bool) {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	match bot
		.send_message(chat_id, text)
		.disable_web_page_preview(preview)
		.await
	{
		Ok(_) => (),
		Err(e) => eprintln!("{:?}", e),
	};
}

pub async fn matrix_file_tg(
	tg_chat_id: i64,
	file: Vec<u8>,
	file_name: String,
	caption: &str,
	bot: &Bot,
) {
	let chat_id = teloxide::types::ChatId(tg_chat_id);
	match bot
		.send_document(chat_id, InputFile::memory(file).file_name(file_name))
		.caption(caption)
		.await
	{
		Ok(_) => (),
		Err(e) => eprintln!("{:?}", e),
	}
}

async fn tg_text_matrix(matrix_chat_id: String, text: &str, matrix_client: matrix_sdk::Client) {
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(matrix_chat_id).unwrap())
		.unwrap();
	let message = RoomMessageEventContent::text_plain(text);
	matrix_room.send(message).await.unwrap();
}

async fn tg_photo_matrix(
	matrix_chat_id: String,
	photo_file_path: String,
	sender: String,
	caption: Option<&str>,
	matrix_client: matrix_sdk::Client,
) -> anyhow::Result<()> {
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(matrix_chat_id)?)
		.context("")?;
	let extension_str = std::path::Path::new(&photo_file_path)
		.extension()
		.context("")?;
	let content_type = match extension_str.to_str().unwrap() {
		"jpg" | "jpeg" => mime::IMAGE_JPEG,
		"png" => mime::IMAGE_PNG,
		_ => mime::APPLICATION_OCTET_STREAM,
	};
	let photo = std::fs::read(photo_file_path)?;
	let mxc_uri = matrix_client
		.media()
		.upload(&content_type, photo)
		.await
		.unwrap()
		.content_uri;
	let image_message = ImageMessageEventContent::new(
		"tg_photo".to_string(),
		matrix_sdk::ruma::events::room::MediaSource::Plain(mxc_uri),
	);
	let message = RoomMessageEventContent::text_plain(sender);
	matrix_room.send(message).await?;
	let message = RoomMessageEventContent::new(MessageType::Image(image_message));
	matrix_room.send(message).await?;

	if let Some(caption) = caption {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await?;
	}

	Ok(())
}

async fn tg_document_matrix(
	matrix_chat_id: String,
	document_file_path: String,
	sender: String,
	caption: Option<&str>,
	matrix_client: matrix_sdk::Client,
) -> anyhow::Result<()> {
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(matrix_chat_id)?)
		.context("")?;
	let extension_str = std::path::Path::new(&document_file_path)
		.extension()
		.context("")?;
	let content_type = match extension_str.to_str().unwrap() {
		"jpg" | "jpeg" => mime::IMAGE_JPEG,
		"png" => mime::IMAGE_PNG,
		_ => mime::APPLICATION_OCTET_STREAM,
	};
	let document = std::fs::read(document_file_path)?;
	let mxc_uri = matrix_client
		.media()
		.upload(&content_type, document)
		.await
		.unwrap()
		.content_uri;
	let document_message = FileMessageEventContent::new(
		"tg_document".to_string(),
		matrix_sdk::ruma::events::room::MediaSource::Plain(mxc_uri),
	);
	let message = RoomMessageEventContent::text_plain(sender);
	matrix_room.send(message).await?;
	let message = RoomMessageEventContent::new(MessageType::File(document_message));
	matrix_room.send(message).await?;

	if let Some(caption) = caption {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await?;
	}

	Ok(())
}
