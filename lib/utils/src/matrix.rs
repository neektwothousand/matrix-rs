use matrix_sdk::matrix_auth::LoginBuilder;
use std::fs::File;
use std::io::{Read, Write};

pub async fn read_or_create_device_id(login_builder: LoginBuilder) -> anyhow::Result<()> {
	if let Ok(mut f) = File::open("device_id") {
		let mut device_id = String::new();
		f.read_to_string(&mut device_id)?;

		login_builder.device_id(&device_id).send().await?;
	} else {
		let response = login_builder.send().await?;
		let mut f = File::create("device_id")?;
		f.write_all(response.device_id.as_bytes())?;
	}
	Ok(())
}
