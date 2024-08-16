use teloxide::{
	payloads::{SendDocumentSetters, SendMessageSetters},
	prelude::Requester,
	types::InputFile,
	Bot,
};

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
