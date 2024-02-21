use alma_armas::{
	get_booru_post_tags, get_booru_posts, read_booru, send_feed_post,
	utils::db::{Booru, BooruRoom},
};
use matrix_sdk::{
	config::SyncSettings,
	ruma::{RoomId, UserId},
	Client,
};
use serde::Deserialize;
use std::{
	io::{BufRead, Write},
	thread::sleep,
	time::Duration,
};
use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt},
};

async fn has_md5(feed: &str, md5: &str) -> bool {
	let path = format!("alma-armas/db/{feed}");
	std::fs::create_dir_all(path.clone()).unwrap();
	let md5_file = std::fs::File::options()
		.create(true)
		.write(true)
		.read(true)
		.open(&format!("{path}/md5"))
		.unwrap();
	let md5_reader = std::io::BufReader::new(md5_file);
	for line in md5_reader.lines() {
		if line.unwrap().trim() == md5 {
			return true;
		}
	}
	false
}

async fn write_md5(feed: &str, md5: &str) {
	let path = format!("alma-armas/db/{feed}");
	let mut md5_file = std::fs::File::options()
		.append(true)
		.open(&format!("{path}/md5"))
		.unwrap();
	writeln!(&mut md5_file, "{}", md5).unwrap();
}

async fn read_lastid(feed: &str, tag: &str, rating: &str, website: &str) -> u64 {
	let path = format!("alma-armas/db/{feed}/{tag}/{rating}/{website}/");
	std::fs::create_dir_all(path.clone()).unwrap();
	let lastid_file = std::fs::File::options()
		.create(true)
		.write(true)
		.read(true)
		.open(&format!("{path}/lastid"))
		.unwrap();
	let mut lastid_reader = std::io::BufReader::new(lastid_file);
	let mut lastid_str = String::new();
	lastid_reader.read_line(&mut lastid_str).unwrap();
	lastid_str = lastid_str.trim().to_string();

	if lastid_str == "" {
		if website.contains("api") {
			return 8115133;
		} else {
			return 8685574;
		}
	} else {
		return lastid_str.parse::<u64>().unwrap();
	}
}

async fn write_lastid(feed: &str, tag: &str, rating: &str, website: &str, lastid: u64) {
	let path = format!("alma-armas/db/{feed}/{tag}/{rating}/{website}/");
	let mut lastid_file = std::fs::File::create(&format!("{path}/lastid")).unwrap();
	writeln!(&mut lastid_file, "{}", lastid).unwrap();
}

async fn send_posts_in_chat(
	client: &Client,
	base_req: &str,
	api_key: &str,
	booru: &Booru,
	room: &BooruRoom,
) {
	let amount = booru.amount;
	let lastid = read_lastid(
		&booru.name,
		&room.tags.join("_"),
		&booru.rating,
		&booru.website,
	)
	.await;

	let chat_req = format!("{base_req}+{}", room.tags.join("+"));
	let url = format!(
		"https://{}/{}&limit={amount}&tags={}+id:>{lastid}+sort:id:asc{api_key}",
		booru.website, "index.php?page=dapi&s=post&q=index&json=1", chat_req
	);

	let Some(booru_posts) = get_booru_posts(&url).await.unwrap() else {
		return;
	};
	let mut post_ids: Vec<u64> = vec![];
	for booru_post in booru_posts {
		let hash = match booru_post.clone().md5 {
			Some(md5) => md5,
			None => match booru_post.clone().hash {
				Some(md5) => md5,
				None => panic!("hash not found"),
			},
		};

		let mut hashtags = get_booru_post_tags(&booru_post, Some(room)).await;
		if let Some(ref replace) = booru.replace {
			let mut hashtags_sub: Vec<String> = vec![];
			for (pattern, sub) in replace {
				for tag in hashtags.drain(..) {
					if tag.contains(*&pattern) {
						hashtags_sub.push(tag.replace(pattern, sub));
					} else {
						hashtags_sub.push(tag);
					}
				}
			}
			hashtags = hashtags_sub;
		}

		let source_website = match booru.website.strip_prefix("api.") {
			Some(source_website) => source_website,
			None => &booru.website,
		};
		let source = format!(
			"https://{}/index.php?page=post&s=view&id={}",
			source_website, booru_post.id
		);
		let caption = format!(
			"{} — {source}",
			hashtags.join(" ").replace(
				['\\', '!', '\'', ':', '{', '}', '+', '~', '(', ')', '.', ',', '/', '-'],
				""
			)
		);

		let joined_room = client
			.get_room(&RoomId::parse(room.id.clone()).unwrap())
			.unwrap();
		if !has_md5(&booru.name, &hash).await {
			send_feed_post(&joined_room, booru_post.clone(), &caption).await;
			write_md5(&booru.name, &hash).await;
		}

		post_ids.push(booru_post.id);
	}
	let max_id = post_ids.iter().fold(0, |acc, id| acc.max(*id));
	write_lastid(
		&booru.name,
		&room.tags.join("_"),
		&booru.rating,
		&booru.website,
		max_id,
	)
	.await;
}

async fn send_posts_in_booru(client: &Client, booru: &Booru) {
	if !booru.active {
		return;
	}
	let mut base_req = format!("{}+", booru.rating);
	let api_key: String = if let Some(api_key) = &booru.api_key {
		api_key.to_string()
	} else {
		String::new()
	};
	base_req += &booru
		.blacklist
		.iter()
		.map(|e| "-".to_owned() + e)
		.collect::<Vec<String>>()
		.join("+");
	println!("start {}", booru.name);
	for chat in &booru.chats {
		send_posts_in_chat(&client, &base_req, &api_key, &booru, &chat).await;
	}
	println!("end {}", booru.name);
}

async fn send_posts(client: &Client) {
	let booru_list = read_booru().unwrap();
	let to_join = booru_list.iter().map(|booru| {
		return send_posts_in_booru(&client, &booru);
	});
	futures::future::join_all(to_join).await;
}

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
}
#[tokio::main]
async fn main() {
	let user: User = serde_yaml::from_reader(std::fs::File::open("alma.yaml").unwrap()).unwrap();
	let user_id = UserId::parse(&user.name).unwrap();
	let client = Client::builder()
		.sqlite_store("alma_sqlite_store", None)
		.server_name(user_id.server_name())
		.build()
		.await
		.unwrap();

	// First we need to log in.
	let login_builder = client
		.matrix_auth()
		.login_username(&user_id, &user.password);

	let alma_device_id_file_str = "alma_device_id";
	if let Ok(mut f) = File::open(alma_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await.unwrap();

		login_builder
			.device_id(&device_id_str)
			.send()
			.await
			.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(alma_device_id_file_str).await.unwrap();
		f.write_all(response.device_id.as_bytes()).await.unwrap();
	}
	client.sync_once(SyncSettings::default()).await.unwrap();

	loop {
		send_posts(&client).await;
		sleep(Duration::new(30, 0));
	}
}
