use std::{
	fs,
	io::Read,
	os::unix::net::{UnixListener, UnixStream},
	path::Path,
	sync::Arc,
	time::Duration,
};

use matrix_sdk::{
	ruma::{
		api::client::message::send_message_event, events::room::message::RoomMessageEventContent,
	},
	Room,
};

use tokio::time::sleep;

const DIS_SOCK: &str = "/tmp/dis-rs.sock";
const MUR_SOCK: &str = "/tmp/mur-rs.sock";

async fn delayed_redaction(room: &Room, res: send_message_event::v3::Response) {
	let event_id = res.event_id;
	sleep(Duration::from_secs(86400)).await;
	let res = room.redact(&event_id, None, None).await;
	if let Err(res) = res {
		eprintln!("redact:\n{:#?}", res);
	}
}

fn read_to_end_unix_stream(mut stream: UnixStream) -> Result<String, std::io::Error> {
	let mut buf = vec![];
	stream.read_to_end(&mut buf)?;

	Ok(String::from_utf8_lossy(buf.as_slice()).to_string())
}

async fn send_message(
	sock_message: Result<String, std::io::Error>,
	room: Arc<Room>,
) -> anyhow::Result<()> {
	let content = RoomMessageEventContent::text_plain(sock_message?);
	let res = crate::matrix::send(room.clone(), content).await?;
	delayed_redaction(&room, res).await;
	Ok(())
}

async fn read_sock(room: Arc<Room>, socket: &str) -> anyhow::Result<()> {
	let unix_listener = UnixListener::bind(socket)?;
	for stream in unix_listener.incoming() {
		let sock_message = match stream {
			Ok(stream) => read_to_end_unix_stream(stream),
			Err(err) => {
				eprintln!("{:?}", err);
				continue;
			}
		};
		let room = room.clone();
		tokio::spawn(send_message(sock_message, room));
		tokio::task::yield_now().await;
	}
	Ok(())
}

pub async fn socket_handler(room: Arc<Room>) {
	let sockets = [DIS_SOCK, MUR_SOCK];

	let mut thread_handles = vec![];
	for socket in sockets {
		if Path::new(socket).exists() {
			fs::remove_file(socket).unwrap();
		}
		let room = room.clone();
		thread_handles.push(read_sock(room, socket));
	}
	let s1 = thread_handles.pop().unwrap().await;
	log::error!("{:?}", s1);
	let s2 = thread_handles.pop().unwrap().await;
	log::error!("{:?}", s2);
}
