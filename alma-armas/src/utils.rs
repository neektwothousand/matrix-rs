use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{anyhow, Context};
use matrix_sdk::ruma::events::relation::Annotation;
use matrix_sdk::ruma::events::room::message::{
	AddMentions, ForwardThread, ImageMessageEventContent, MessageType, RoomMessageEvent,
	RoomMessageEventContent, VideoMessageEventContent,
};
use matrix_sdk::Room;
use mime::Mime;
use rand::seq::SliceRandom;
use serde_json::from_str;

pub mod db;
use db::{Booru, BooruPost, BooruRoom, DanbooruPost, GelbooruSource, User};

pub fn read_users() -> Result<Vec<User>, Box<dyn Error>> {
	let path = "alma-armas/db/users";

	// Open the file in read-only mode with buffer.
	let file = File::open(path)?;
	let reader = BufReader::new(file);

	let u = serde_json::from_reader(reader)?;

	Ok(u)
}

pub fn read_booru() -> Result<Vec<Booru>, Box<dyn Error>> {
	let path = "alma-armas/db/booru";

	// Open the file in read-only mode with buffer.
	let file = File::open(path)?;
	let reader = BufReader::new(file);

	let u = serde_json::from_reader(reader)?;

	Ok(u)
}

pub async fn get_booru_posts(url: &str) -> Result<Option<Vec<BooruPost>>, Box<dyn Error>> {
	let req_client = reqwest::Client::builder()
		.user_agent("dorothy-rs")
		.build()?;
	let booru_response = req_client.get(url).send().await?;
	let response_string = booru_response.text().await?;

	if response_string.is_empty() {
		return Ok(None);
	}

	if let Ok(gelbooru) = from_str::<GelbooruSource>(&response_string) {
		if let Some(posts) = gelbooru.post {
			return Ok(Some(posts));
		}
	}
	if let Ok(posts) = from_str::<Vec<BooruPost>>(&response_string) {
		return Ok(Some(posts));
	}
	match from_str::<DanbooruPost>(&response_string) {
		Ok(danbooru_post) => {
			let Some(original) = danbooru_post
				.media_asset
				.variants
				.iter()
				.find(|v| v.r#type == "original")
			else {
				return Ok(None);
			};
			let file_url = original.url.clone();
			let sample_url = if let Some(sample) = danbooru_post
				.media_asset
				.variants
				.iter()
				.find(|v| v.r#type == "sample")
			{
				sample.url.clone()
			} else {
				String::new()
			};

			let booru_post = BooruPost {
				id: danbooru_post.id,
				file_url,
				sample_url,
				rating: danbooru_post.rating,
				tags: danbooru_post.tag_string,
				md5: Some(danbooru_post.md5),
				hash: None,
			};
			Ok(Some(vec![booru_post]))
		}
		Err(_) => Ok(None),
	}
}

pub async fn send_feed_post(room: &Room, booru_post: BooruPost, caption: &str) -> anyhow::Result<()> {
	let file_url = url::Url::parse(&booru_post.file_url)?;
	let extension = Path::new(file_url.path()).extension().context("ext")?;
	let extension_str = extension.to_str().context("ext")?;
	let content_type = match extension_str {
		"jpg" | "jpeg" => "image/jpeg".parse::<Mime>()?,
		"png" => "image/png".parse::<Mime>()?,
		"mp4" => "video/mp4".parse::<Mime>()?,
		_ => return Err(anyhow!("invalid ext")),
	};
	let client = room.client();
	let event_id = match content_type.essence_str() {
		"image/jpeg" | "image/png" => {
			let body = file_url.path_segments().context("cannot be a base")?.last().context("")?;

			let request = reqwest::get(file_url.clone()).await?;
			let data = request.bytes().await?;
			let mxc_url = client
				.media()
				.upload(&content_type, data.to_vec())
				.await.context("can't upload")?
				.content_uri;

			let image_message = ImageMessageEventContent::plain(body.to_string(), mxc_url);
			let room_message = RoomMessageEventContent::new(MessageType::Image(image_message));
			room.send(room_message).await?.event_id
		}
		"video/mp4" => {
			let body = file_url.path_segments().context("")?.last().context("")?;

			let request = reqwest::get(file_url.clone()).await?;
			let data = request.bytes().await?;
			let mxc_url = client
				.media()
				.upload(&content_type, data.to_vec())
				.await?
				.content_uri;

			let video_message = VideoMessageEventContent::plain(body.to_string(), mxc_url);
			let room_message = RoomMessageEventContent::new(MessageType::Video(video_message));
			room.send(room_message).await?.event_id
		}
		_ => return Err(anyhow!("")),
	};

	let timeline_event = loop {
		match room.event(&event_id).await {
			Ok(event) => break event,
			Err(_) => continue,
		}
	};
	let original_message = timeline_event
		.event
		.deserialize_as::<RoomMessageEvent>()
		?;
	let forward_thread = ForwardThread::No;
	let add_mentions = AddMentions::No;
	let text_content = RoomMessageEventContent::text_plain(caption).make_reply_to(
		original_message.as_original().context("")?,
		forward_thread,
		add_mentions,
	);
	let event_id = room.send(text_content).await?.event_id;

	use matrix_sdk::ruma::events::reaction;
	for key in ["✅", "❤️", "❌"] {
		let annotation = Annotation::new(event_id.clone(), key.to_string());
		let reaction_content = reaction::ReactionEventContent::new(annotation);
		let res = room.send(reaction_content).await;
		if res.is_err() {
			eprintln!("{:?}", res);
		}
	}
	Ok(())
}

pub async fn get_booru_post_tags(
	booru_post: &BooruPost,
	booru_room: Option<&BooruRoom>,
) -> Vec<String> {
	let mut hashtags: Vec<String> = vec![];
	if let Some(booru_room) = booru_room {
		for tag in &booru_room.tags {
			let tag_match = booru_post
				.tags
				.split(' ')
				.filter(|&booru_tag| booru_tag == tag)
				.collect::<Vec<&str>>()
				.pop();
			if let Some(tag) = tag_match {
				hashtags.push("#".to_owned() + tag);
			}
		}
	}
	let mut booru_tags_split: Vec<&str> = booru_post.tags.split(' ').collect();
	let mut booru_tags: Vec<String> = vec![];
	for booru_tag in booru_tags_split.drain(..) {
		booru_tags.push("#".to_owned() + booru_tag);
	}
	let mut rng = rand::thread_rng();
	booru_tags.shuffle(&mut rng);
	for booru_tag in booru_tags.iter() {
		if !hashtags.contains(&booru_tag.to_string()) {
			hashtags.push(booru_tag.to_string());
		}
		if hashtags.len() == 3 {
			break;
		}
	}
	hashtags
}
