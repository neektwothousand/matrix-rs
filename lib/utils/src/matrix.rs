use anyhow::bail;
use matrix_sdk::{
	matrix_auth::LoginBuilder,
	ruma::{
		api::client::message::send_message_event::v3::Response,
		events::room::message::RoomMessageEventContent,
	},
	Room,
};
use std::{
	fs::File,
	io::{
		Read,
		Write,
	},
	sync::Arc,
};

pub async fn read_or_create_device_id(
	path: &str,
	login_builder: LoginBuilder,
) -> anyhow::Result<()> {
	let file_path = format!("{path}device_id");
	if let Ok(mut f) = File::open(&file_path) {
		let mut device_id = String::new();
		f.read_to_string(&mut device_id)?;

		login_builder.device_id(&device_id).send().await?;
	} else {
		let response = login_builder.send().await?;
		let mut f = File::create(&file_path)?;
		f.write_all(response.device_id.as_bytes())?;
	}
	Ok(())
}

pub async fn send(room: Arc<Room>, content: RoomMessageEventContent) -> anyhow::Result<Response> {
	loop {
		match room.send(content.clone()).await {
			Ok(response) => return Ok(response),
			Err(err) => match err {
				matrix_sdk::Error::Http(matrix_sdk::HttpError::Reqwest(_)) => continue,
				_ => {
					bail!("{:?}", err);
				}
			},
		}
	}
}
