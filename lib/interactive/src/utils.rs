use matrix_sdk::{
	room::Room,
	ruma::{
		api::client::{
			media::create_content::v3::Response as MediaResponse,
			message::send_message_event::v3::Response,
		},
		events::{
			room::message::{
				AddMentions,
				ForwardThread,
				RoomMessageEventContent,
			},
			OriginalMessageLikeEvent,
		},
	},
	Error,
	HttpError,
};

use matrix_sdk::ruma::events::room::message::{
	FileMessageEventContent,
	ImageMessageEventContent,
	MessageType,
	TextMessageEventContent,
};

use mime::Mime;

pub struct SendMessage {
	pub room: Room,
	pub message: RoomMessageEventContent,
}

async fn upload(room: Room, media: (Mime, Vec<u8>)) -> Result<MediaResponse, HttpError> {
	room.client().media().upload(&media.0, media.1).await
}

impl SendMessage {
	pub async fn text(room: &Room, text: &str) -> Self {
		let message =
			RoomMessageEventContent::new(MessageType::Text(TextMessageEventContent::plain(text)));
		Self {
			room: room.clone(),
			message,
		}
	}
	pub async fn image(room: Room, media: (Mime, Vec<u8>)) -> Option<Self> {
		let mxc_uri = match upload(room.clone(), media).await {
			Ok(response) => response.content_uri,
			Err(err) => {
				eprintln!("{:?}", err);
				return None;
			}
		};
		let image_message = ImageMessageEventContent::plain(String::new(), mxc_uri);
		let message = RoomMessageEventContent::new(MessageType::Image(image_message));
		Some(Self {
			room,
			message,
		})
	}
	pub async fn file(room: Room, file_name: String, media: (Mime, Vec<u8>)) -> Option<Self> {
		let mxc_uri = match upload(room.clone(), media).await {
			Ok(response) => response.content_uri,
			Err(err) => {
				eprintln!("{:?}", err);
				return None;
			}
		};
		let file_message = FileMessageEventContent::plain(file_name, mxc_uri);
		let message = RoomMessageEventContent::new(MessageType::File(file_message));
		Some(Self {
			room,
			message,
		})
	}
	pub async fn reply(
		&self,
		original_message: &OriginalMessageLikeEvent<RoomMessageEventContent>,
	) -> Result<Response, Error> {
		let f = ForwardThread::Yes;
		let m = AddMentions::Yes;
		self.room.send(self.message.clone().make_reply_to(original_message, f, m)).await
	}
	pub async fn send(&self) -> Result<Response, Error> {
		self.room.send(self.message.clone()).await
	}
}
