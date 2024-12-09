use std::{
	fs::File,
	io::{
		Read,
		Write,
	},
	sync::Arc,
	time::Duration,
};

use tokio::time::sleep;

use matrix_sdk::{
	ruma::events::room::message::RoomMessageEventContent,
	Room,
};

use anyhow::Context;

async fn send_update(room: &Room, user_ids: &Vec<u64>) -> anyhow::Result<()> {
	let reqwest_client = reqwest::Client::builder().user_agent("matrix-bot").build()?;
	for user_id in user_ids {
		let file_name = format!("anilist_{user_id}_createdAt");
		let last_created_at = {
			let file = File::options().read(true).open(&file_name);
			match file {
				Ok(mut file) => {
					let mut buf = String::new();
					file.read_to_string(&mut buf)?;
					buf.trim().parse::<u64>()?
				}
				Err(_) => 0u64,
			}
		};
		let query = format!(
			"{{
				Activity(userId: {user_id}, createdAt_greater: {last_created_at}) {{
					... on ListActivity {{
						siteUrl
						createdAt
						user {{ name }}
						status
						progress
						media {{
							title {{ userPreferred }}
						}}
					}}
				}}
			}}"
		);
		let json_request = serde_json::json!({"query": query});
		let url = "https://graphql.anilist.co/";
		let request = reqwest_client
			.post(url)
			.header("Content-Type", "application/json")
			.json(&json_request)
			.build()?;
		let response = match reqwest_client.execute(request).await {
			Ok(r) => r,
			Err(e) => {
				log::error!("{:?}", e);
				return Err(anyhow::anyhow!(e.to_string()));
			}
		};
		let response_json = response.json::<serde_json::Value>().await?;
		let activity = &response_json.get("data").context("")?.get("Activity").context("")?;
		let activity_created_at = {
			if let Some(activity) = activity.get("createdAt") {
				activity.as_u64().unwrap()
			} else {
				continue;
			}
		};

		if activity_created_at <= last_created_at {
			continue;
		}
		let user = &activity
			.get("user")
			.context("user not found")?
			.get("name")
			.context("user name not found")?
			.as_str()
			.unwrap();
		let activity_link =
			&activity.get("siteUrl").context("siteUrl not found")?.as_str().unwrap();
		let anime = &activity
			.get("media")
			.context("media not found")?
			.get("title")
			.context("media title not found")?
			.get("userPreferred")
			.context("userPreferred not found")?
			.as_str()
			.unwrap();
		let status = &activity.get("status").context("status not found")?.as_str().unwrap();
		let progress =
			&activity.get("progress").context("progress not found")?.as_str().unwrap_or_default();
		let result = format!("｢{user}｣ {activity_link}\n｢{anime}｣ {status} {progress}");
		if let Err(e) = room.send(RoomMessageEventContent::text_plain(result)).await {
			eprintln!("{e:?}");
			continue;
		}
		let mut file = File::options().write(true).create(true).truncate(true).open(&file_name)?;
		file.write_all(activity_created_at.to_string().as_bytes())?;
		sleep(Duration::from_secs(60)).await;
	}
	Ok(())
}

pub async fn check(room: Arc<Room>, user_ids: Vec<u64>) {
	loop {
		if let Err(e) = send_update(&room, &user_ids).await {
			log::error!("{:?}", e);
		};
		sleep(Duration::from_secs(60)).await;
	}
}
