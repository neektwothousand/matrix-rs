use std::{
	net::{
		SocketAddr,
		ToSocketAddrs,
		UdpSocket,
	},
	time::Duration,
};

use serde::Deserialize;

#[derive(Deserialize)]
struct FactorioAddrs {
	factorio_addrs: Vec<String>,
}

#[must_use]
pub fn factorio_check() -> Option<String> {
	let msg = [0x22, 0x00, 0x00, 0x02, 0x00, 0x07, 0x19, 0x36, 0x01, 0x00, 0xb3, 0x24, 0x43, 0x66];

	let socket = match UdpSocket::bind("0.0.0.0:0") {
		Ok(socket) => socket,
		Err(e) => {
			log::error!("{}", e);
			return None;
		}
	};
	let bot_data = match std::fs::read_to_string("bot_data.toml") {
		Ok(data) => data,
		Err(e) => {
			log::error!("{}", e);
			return None;
		}
	};
	let factorio_server_addrs: FactorioAddrs = match toml::from_str(&bot_data) {
		Ok(addrs) => addrs,
		Err(e) => {
			log::error!("{}", e);
			return None;
		}
	};

	let port = 34197u16;
	for addr in factorio_server_addrs.factorio_addrs {
		let Ok(mut to_socket_addrs) = (addr.clone(), port).to_socket_addrs() else {
			continue;
		};
		let Some(SocketAddr::V4(socket_addr)) = to_socket_addrs.next() else {
			continue;
		};

		let _ = socket.send_to(&msg, socket_addr);
		let _ = socket.set_read_timeout(Some(Duration::from_secs(30)));

		let mut buf = [0; 64];
		let status: Option<String> = match socket.recv(&mut buf) {
			Ok(_) => Some(socket_addr.to_string()),
			Err(_) => None,
		};
		if status.is_some() {
			return status;
		}
	}
	None
}
