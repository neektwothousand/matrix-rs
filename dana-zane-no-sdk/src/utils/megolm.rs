use std::fs;

use serde::{Deserialize, Serialize};
use vodozemac::megolm::{
	InboundGroupSession, InboundGroupSessionPickle, SessionConfig, SessionKey,
};

const STORE_PATH: &str = "store";

#[derive(Serialize, Deserialize)]
pub struct MegolmSession {
	pub session_config: SessionConfig,
	pub inbound_group_session: InboundGroupSessionPickle,
}

pub fn create_megolm_session(key: &SessionKey) -> MegolmSession {
	let session_config = SessionConfig::version_2();
	let inbound_group_session = InboundGroupSession::new(key, session_config).pickle();
	MegolmSession {
		session_config,
		inbound_group_session,
	}
}

pub fn save_megolm_session(room_id: &str, megolm_session: &MegolmSession) {
	fs::create_dir(STORE_PATH).unwrap();
	let writer = fs::File::create(format!("{}/{room_id}", STORE_PATH)).unwrap();
	ron::ser::to_writer(writer, megolm_session).unwrap();
}

pub fn get_megolm_session(room_id: &str) -> Option<MegolmSession> {
	let rdr = fs::File::open(format!("{}/{room_id}", STORE_PATH)).ok()?;

	ron::de::from_reader(rdr).ok()?
}
