use matrix_sdk::{
	config::SyncSettings,
	ruma::{events::room::message::RoomMessageEventContent, RoomId, UserId},
	Client,
};
use serde::Deserialize;
use std::{
	fs::{File, self},
	io::{Read, Write},
	path::Path, os::unix::net::{UnixStream, UnixListener},
};

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
	room_id: String,
}

const SOCKET: &str = "/tmp/discord-rs.sock";

fn read_stream(mut stream: UnixStream) -> String {
	let mut buf = vec![];
	stream.read_to_end(&mut buf).unwrap();

	String::from_utf8_lossy(buf.as_slice()).to_string()
}

async fn read_sock(user: &User, client: &Client) {
	let room_id = RoomId::parse(&user.room_id).unwrap();
	let joined_room = client.get_room(&room_id).unwrap();

	if Path::new(SOCKET).exists() {
		fs::remove_file(SOCKET).unwrap();
	}
	let unix_listener = UnixListener::bind(SOCKET).unwrap();
	for stream in unix_listener.incoming() {
		let sock_message = match stream {
			Ok(stream) => read_stream(stream),
			Err(err) => {
				eprintln!("{:?}", err);
				continue;
			}
		};
		let content = RoomMessageEventContent::text_plain(sock_message);
		match joined_room.send(content).await {
			Ok(_) => (),
			Err(err) => eprintln!("{:?}", err),
		}
	}
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let user: &'static mut User = Box::leak(Box::new(
		serde_yaml::from_reader(File::open("deal.yaml").unwrap()).unwrap(),
	));
	let user_id = UserId::parse(user.name.as_str()).unwrap();
	let client = Box::leak(Box::new(
		Client::builder()
			.sqlite_store("deal_sqlite_store", None)
			.server_name(user_id.server_name())
			.build()
			.await
			.unwrap(),
	));

	let login_builder = client.matrix_auth().login_username(&user_id, &user.password);

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
	client.sync_once(SyncSettings::default()).await?;

	let (_, client_sync) = tokio::join!(
		read_sock(user, client),
		client.sync(SyncSettings::default())
	);

	client_sync?;
	return Ok(());
}
