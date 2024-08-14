use std::{
	fs::{self, File},
	io::{Read, Write},
	os::unix::net::{UnixListener, UnixStream},
	path::Path,
};

use matrix_sdk::{
	config::SyncSettings,
	ruma::api::client::message::send_message_event,
	ruma::{events::room::message::RoomMessageEventContent, RoomId, UserId},
	Client, Room,
};
use serde::Deserialize;
use tokio::task::spawn;
use tokio::time::{sleep, Duration};

const DIS_SOCK: &str = "/tmp/dis-rs.sock";
const MUR_SOCK: &str = "/tmp/mur-rs.sock";

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
	room_id: String,
}

async fn delete_message(room: Room, res: send_message_event::v3::Response) {
	let event_id = res.event_id;
	sleep(Duration::from_secs(86400)).await;
	let res = room.redact(&event_id, None, None).await;
	if let Err(res) = res {
		eprintln!("redact:\n{:#?}", res);
	}
}

fn read_stream(mut stream: UnixStream) -> String {
	let mut buf = vec![];
	stream.read_to_end(&mut buf).unwrap();

	String::from_utf8_lossy(buf.as_slice()).to_string()
}

async fn read_sock(room: Room, socket: &str) {
	let unix_listener = UnixListener::bind(socket).unwrap();
	for stream in unix_listener.incoming() {
		let sock_message = match stream {
			Ok(stream) => read_stream(stream),
			Err(err) => {
				eprintln!("{:?}", err);
				continue;
			}
		};
		let room = room.clone();
		spawn(async move {
			println!("{}", &sock_message);
			let content = RoomMessageEventContent::text_plain(sock_message);
			match room.send(content).await {
				Ok(res) => {
					delete_message(room, res).await;
				}
				Err(err) => eprintln!("{:?}", err),
			}
		});
		tokio::task::yield_now().await;
	}
}

async fn socket_handler(room: Room) {
	let sockets = [DIS_SOCK, MUR_SOCK];

	for socket in sockets {
		if Path::new(socket).exists() {
			fs::remove_file(socket).unwrap();
		}
		spawn(read_sock(room.clone(), socket));
	}
}

async fn anilist_update(room: &Room) {
	let user_ids = [5752916, 6832539];
	for user_id in user_ids {
		let mut queries = vec![];
		queries.push(format!(
			"{{
				Activity(userId: {user_id}, sort: ID_DESC) {{
					... on ListActivity {{
						siteUrl id user {{ name }} status progress media {{
							title {{ userPreferred }}
						}}
					}}
				}}
			}}"
		));
		for query in queries {
			let json_request = serde_json::json!({
				"query": query
			});
			let url = "https://graphql.anilist.co/";
			let reqwest_client = reqwest::Client::builder()
				.user_agent("deal-lilim")
				.build()
				.unwrap();
			let request = reqwest_client
				.post(url)
				.header("Content-Type", "application/json")
				.json(&json_request)
				.build()
				.unwrap();
			let response = match reqwest_client.execute(request).await {
				Ok(r) => r,
				Err(e) => {
					eprintln!("{:?}", e);
					return;
				}
			};
			let response_json = response.json::<serde_json::Value>().await.unwrap();
			let activity = &response_json["data"]["Activity"];
			let activity_id = activity["id"].as_u64().unwrap();
			let file_name = format!("anilist_{user_id}_last_id");
			let anilist_last_id = {
				let file = File::options().read(true).open(&file_name);
				if file.is_err() {
					0u64
				} else {
					let mut buf = String::new();
					file.unwrap().read_to_string(&mut buf).unwrap();
					buf.parse::<u64>().unwrap()
				}
			};
			if activity_id == anilist_last_id {
				continue;
			} else {
				let mut file = File::options()
					.write(true)
					.create(true)
					.truncate(true)
					.open(file_name)
					.unwrap();
				file.write_all(activity_id.to_string().as_bytes()).unwrap();
			}
			let user = &activity["user"]["name"].as_str().unwrap();
			let activity_link = &activity["siteUrl"].as_str().unwrap();
			let anime = &activity["media"]["title"]["userPreferred"]
				.as_str()
				.unwrap();
			let status = &activity["status"].as_str().unwrap();
			let progress = &activity["progress"].as_str().unwrap_or_default();
			let result = format!("｢{user}｣ {activity_link}\n｢{anime}｣ {status} {progress}");
			room.send(RoomMessageEventContent::text_plain(result))
				.await
				.unwrap();
			sleep(Duration::from_secs(10)).await;
		}
	}
}

#[tokio::main]
async fn main() {
	let user: User = serde_yaml::from_reader(File::open("deal.yaml").unwrap()).unwrap();
	let user_id = UserId::parse(user.name.as_str()).unwrap();
	let client = Client::builder()
		.sqlite_store("deal_sqlite_store", None)
		.server_name(user_id.server_name())
		.build()
		.await
		.unwrap();

	let login_builder = client
		.matrix_auth()
		.login_username(&user_id, &user.password);

	let deal_device_id_file_str = "deal_device_id";
	if let Ok(mut f) = File::open(deal_device_id_file_str) {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).unwrap();
		login_builder
			.device_id(&device_id_str)
			.send()
			.await
			.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(deal_device_id_file_str).unwrap();
		f.write_all(response.device_id.as_bytes()).unwrap();
	}

	let room_id = RoomId::parse(&user.room_id).unwrap();

	client.sync_once(SyncSettings::default()).await.unwrap();
	let room = client.get_room(&room_id).unwrap();
	spawn(socket_handler(room.clone()));
	spawn(async move {
		let room = room.clone();
		loop {
			anilist_update(&room).await;
			sleep(Duration::from_secs(300)).await;
		}
	});

	loop {
		let client_sync = client.sync(SyncSettings::default()).await;
		let Err(ref _e) = client_sync else {
			eprintln!("deal http error");
			continue;
		};
	}
}
